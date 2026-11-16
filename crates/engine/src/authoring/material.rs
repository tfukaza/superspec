use super::{
    Artifact, Diagnostic, Output, Request, Session,
    receiver::{self, Atlas, Chart},
};
use crate::{
    materialspec::program::{self, FieldValue, Inputs, Program},
    runtime::{
        dependencies::{Dependency, Node, Plan},
        store::ContentKey,
    },
};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, sync::Arc};
use superspec_geometry::image;
use superspec_model::instances::{IDENTITY, ImageResource, Instance, Model};
use superspec_types::{
    diagnostic::SourceLocation,
    material::{AlphaMode, LiveArgument, LiveOperation as LiveDescriptor, Material},
};

/// The baked maps of a receiver. Each is baked from only the outputs it reads, so an edit that
/// cannot change an output keeps the map baked from it, and a bake after a small edit evaluates
/// only the outputs that edit can reach.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Group {
    /// Base color, with opacity as alpha.
    Color,
    /// The relief normal: height, receiver normal and normal offset.
    Normal,
    /// Roughness and metalness.
    Properties,
    /// Emission.
    Emissive,
    /// Fields that live renderer operations read.
    Live,
    /// A preview of one field or channel: always every output, one map.
    Inspection,
}

impl Group {
    const MAPS: [Group; 5] = [
        Group::Color,
        Group::Normal,
        Group::Properties,
        Group::Emissive,
        Group::Live,
    ];

    fn name(self) -> &'static str {
        match self {
            Group::Color => "color",
            Group::Normal => "normal",
            Group::Properties => "properties",
            Group::Emissive => "emissive",
            Group::Live => "live",
            Group::Inspection => "inspection",
        }
    }

    /// The program outputs this group's map is baked from.
    fn outputs(self) -> program::OutputSet {
        use program::OutputSet as O;
        match self {
            Group::Color => O::BASE_COLOR.union(O::OPACITY),
            Group::Normal => O::HEIGHT.union(O::RECEIVER_NORMAL).union(O::NORMAL_OFFSET),
            Group::Properties => O::ROUGHNESS.union(O::METALNESS),
            Group::Emissive => O::EMISSIVE,
            Group::Live => O::NONE,
            Group::Inspection => O::ALL,
        }
    }
}

/// A set of groups.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Groups(u8);

impl Groups {
    const NONE: Self = Self(0);
    const MAPS: Self = Self(31);

    fn of(group: Group) -> Self {
        Self(1 << group as u8)
    }
    fn contains(self, group: Group) -> bool {
        self.0 & Self::of(group).0 != 0
    }
    fn with(self, group: Group) -> Self {
        Self(self.0 | Self::of(group).0)
    }
    fn without(self, group: Group) -> Self {
        Self(self.0 & !Self::of(group).0)
    }
    fn is_empty(self) -> bool {
        self.0 == 0
    }
    fn contains_all(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }
    fn iter(self) -> impl Iterator<Item = Group> {
        [
            Group::Color,
            Group::Normal,
            Group::Properties,
            Group::Emissive,
            Group::Live,
            Group::Inspection,
        ]
        .into_iter()
        .filter(move |group| self.contains(*group))
    }
    /// The groups whose per-texel data a bake of these groups evaluates. A preview of a field
    /// evaluates every output.
    fn rows(self) -> Self {
        if self.contains(Group::Inspection) {
            Self::MAPS
        } else {
            Self(self.0 & Self::MAPS.0)
        }
    }
    fn outputs(self) -> program::OutputSet {
        self.iter().fold(program::OutputSet::NONE, |set, group| {
            set.union(group.outputs())
        })
    }
}

/// An encoded map and its content identity.
pub(crate) struct BakedImage {
    id: String,
    resource: ImageResource,
}

/// One group's baked result.
pub(crate) enum BakedReceiver {
    /// The base color map; `blend` when any texel is translucent. Also the one map of an inspection.
    Color {
        image: BakedImage,
        blend: bool,
    },
    Normal(BakedImage),
    Properties(BakedImage),
    /// Present only when some texel emits: the strength that normalizes the map, and the map.
    Emissive(Option<(f64, BakedImage)>),
    Live {
        images: Vec<BakedImage>,
        operations: Vec<LiveDescriptor>,
    },
}

impl BakedReceiver {
    fn images(&self) -> Vec<&BakedImage> {
        match self {
            Self::Color { image, .. } | Self::Normal(image) | Self::Properties(image) => {
                vec![image]
            }
            Self::Emissive(emission) => emission.iter().map(|(_, image)| image).collect(),
            Self::Live { images, .. } => images.iter().collect(),
        }
    }

    /// Bytes this result retains, for artifact admission.
    fn bytes(&self) -> usize {
        self.images()
            .iter()
            .map(|image| image.resource.bytes.len() + image.id.len())
            .sum::<usize>()
            + 256
    }
}

/// The material of a receiver, assembled from its groups' maps exactly as a bake of the whole
/// receiver builds it.
fn assemble(
    groups: &BTreeMap<Group, Arc<BakedReceiver>>,
    inspecting: bool,
) -> Result<(Material, BTreeMap<String, ImageResource>), String> {
    let missing = || "A baked map is missing.".to_owned();
    let mut images = BTreeMap::new();
    let mut take = |baked: &BakedReceiver| {
        for image in baked.images() {
            images.insert(image.id.clone(), image.resource.clone());
        }
    };
    let Some(color) = groups.get(&if inspecting {
        Group::Inspection
    } else {
        Group::Color
    }) else {
        return Err(missing());
    };
    let BakedReceiver::Color { image, blend } = color.as_ref() else {
        return Err(missing());
    };
    take(color);
    let mut material = Material {
        color: "#ffffff".into(),
        repeat: 1.,
        roughness: 1.,
        metalness: 1.,
        texture: Some(image.id.clone()),
        alpha_mode: blend.then_some(AlphaMode::Blend),
        ..Default::default()
    };
    if inspecting {
        material.color = "#000000".into();
        material.emissive = Some("#ffffff".into());
        material.emissive_texture = Some(image.id.clone());
        material.metalness = 0.;
    } else {
        for group in [
            Group::Normal,
            Group::Properties,
            Group::Emissive,
            Group::Live,
        ] {
            let baked = groups.get(&group).ok_or_else(missing)?;
            take(baked);
            match baked.as_ref() {
                BakedReceiver::Normal(image) => {
                    material.normal_texture = Some(image.id.clone());
                    material.normal_scale = Some(1.);
                }
                BakedReceiver::Properties(image) => {
                    material.metallic_roughness_texture = Some(image.id.clone());
                }
                BakedReceiver::Emissive(Some((strength, image))) => {
                    material.emissive = Some("#ffffff".into());
                    material.emissive_strength = Some(*strength);
                    material.emissive_texture = Some(image.id.clone());
                }
                BakedReceiver::Emissive(None) => {}
                BakedReceiver::Live { operations, .. } => material.live = operations.clone(),
                BakedReceiver::Color { .. } => return Err(missing()),
            }
        }
    }
    material.id.clear();
    material.id = format!("studio/material/{}", hash(&json(&material)));
    Ok((material, images))
}

fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn node(id: String, implementation: &str, inputs: Vec<u8>, dependencies: Vec<Dependency>) -> Node {
    Node {
        id,
        implementation: implementation.into(),
        inputs,
        dependencies,
        source: None,
    }
}
fn dependency(role: &str, node: impl Into<String>) -> Dependency {
    Dependency {
        role: role.into(),
        node: node.into(),
    }
}
fn json(value: &impl serde::Serialize) -> Vec<u8> {
    serde_json::to_vec(value).expect("finite admitted authoring request")
}

/// What one material build produced.
enum Built {
    Output(Output),
    /// Encoded rows of one share of the requested bakes (see [`Distribution`]).
    Share(Vec<u8>),
}

/// One worker's part of a distributed bake: run `index` of `count` equal runs of consecutive
/// rows, for the bakes with these content keys.
struct Share<'a> {
    index: usize,
    count: usize,
    keys: &'a [ContentKey],
    /// The per-texel data wanted of each bake, parallel to `keys`.
    groups: &'a [Groups],
}

impl Session {
    pub(super) fn material(
        &mut self,
        request: &Request,
        revision: u64,
        assets: &dyn Fn(&str) -> Result<Arc<[u8]>, String>,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Output, Vec<Diagnostic>> {
        match self.material_with(request, revision, assets, cancelled, None)? {
            Built::Output(output) => Ok(output),
            Built::Share(_) => unreachable!("no share was requested"),
        }
    }

    /// Lets a host that can run several engine instances (browser workers) bake rows in
    /// parallel. While enabled, a build that needs a large uncached bake stops with a
    /// `BAKE_PENDING` diagnostic and [`Self::take_pending_bake`] describes the work; the host
    /// has other sessions compute [`Self::bake_share`]s, hands them to
    /// [`Self::provide_bake_share`] and builds again. A project of several materials reports
    /// every such bake of one build, so the host takes them until none is left before it builds
    /// again. Rows are the same bytes wherever they are evaluated, so the result is identical to
    /// an undistributed build. Disabling drops reported bakes and rows that were never used.
    pub fn distribute_bakes(&mut self, enabled: bool) {
        self.distribution.enabled = enabled;
        if !enabled {
            self.distribution.pending.clear();
            self.distribution.provided.clear();
        }
    }

    /// The next bake the last build stopped for, if any, in the order they were reported.
    pub fn take_pending_bake(&mut self) -> Option<PendingBake> {
        (!self.distribution.pending.is_empty()).then(|| self.distribution.pending.remove(0))
    }

    /// Evaluates one share of a pending bake. Runs in any session given the same assets; the
    /// result goes to [`Self::provide_bake_share`] of the session that reported the bake.
    pub fn bake_share(
        &mut self,
        pending: &PendingBake,
        index: usize,
        count: usize,
        assets: &dyn Fn(&str) -> Result<Arc<[u8]>, String>,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Vec<u8>, Vec<Diagnostic>> {
        let fail = |message: &str| vec![Diagnostic::message(&pending.request.entry, message)];
        if count == 0 || index >= count || count > 64 {
            return Err(fail(
                "A bake share needs an index below a count of at most 64.",
            ));
        }
        let keys = pending
            .keys
            .iter()
            .map(|key| parse_key(key))
            .collect::<Option<Vec<_>>>()
            .ok_or_else(|| fail("Invalid bake key."))?;
        // A description without masks (an older host) wants every map of every bake.
        let groups: Vec<Groups> = if pending.groups.is_empty() {
            vec![Groups::MAPS; keys.len()]
        } else {
            pending.groups.iter().map(|bits| Groups(*bits)).collect()
        };
        if groups.len() != keys.len() || groups.iter().any(|g| g.0 & !Groups::MAPS.0 != 0) {
            return Err(fail("Invalid bake groups."));
        }
        let revision = self.authoring_revision.get().unwrap_or(0);
        self.begin_authoring(revision, &pending.request.entry)?;
        let share = Share {
            index,
            count,
            keys: &keys,
            groups: &groups,
        };
        match self.material_with(&pending.request, revision, assets, cancelled, Some(share))? {
            Built::Share(bytes) => Ok(bytes),
            Built::Output(_) => unreachable!("a share was requested"),
        }
    }

    /// Accepts rows computed by [`Self::bake_share`] for a bake this session reported.
    pub fn provide_bake_share(&mut self, bytes: &[u8]) -> Result<(), String> {
        for (key, groups, total, rows) in read_shares(bytes)? {
            let provided = self.distribution.provided.entry(key).or_default();
            // Rows of another mask do not mix with these: they hold other data.
            if provided.slots.len() != total || provided.groups != groups {
                provided.groups = groups;
                provided.slots.clear();
                provided.slots.resize_with(total, || None);
            }
            for (ordinal, row) in rows {
                *provided
                    .slots
                    .get_mut(ordinal)
                    .ok_or("A provided row is outside its bake.")? = Some(row);
            }
        }
        Ok(())
    }

    fn material_with(
        &mut self,
        request: &Request,
        _revision: u64,
        assets: &dyn Fn(&str) -> Result<Arc<[u8]>, String>,
        cancelled: &dyn Fn() -> bool,
        share: Option<Share<'_>>,
    ) -> Result<Built, Vec<Diagnostic>> {
        let compiled = self.program(request)?;
        let fail = |message: String| vec![Diagnostic::message(&request.entry, message)];
        let receivers = if request.receivers.is_empty() {
            vec![receiver::swatch(request.domain)]
        } else {
            request.receivers.clone()
        };
        if receivers.len() > 256
            || receivers.iter().map(|r| r.indices.len()).sum::<usize>() > 98_304
        {
            return Err(fail(
                "Selected receivers exceed the authoring triangle allowance.".into(),
            ));
        }
        let mut nodes = Vec::new();
        let mut sources = request.modules.clone();
        sources.insert(request.entry.clone(), request.source.clone());
        let mut source_deps = Vec::new();
        for name in
            std::iter::once(&compiled.metadata().entry).chain(&compiled.metadata().dependencies)
        {
            let id = format!("source/{name}");
            let text = sources
                .get(name)
                .ok_or_else(|| fail(format!("Missing source module {name}.")))?;
            nodes.push(node(
                id.clone(),
                "material-source-v1",
                text.as_bytes().into(),
                vec![],
            ));
            source_deps.push(dependency(name, id));
        }
        nodes.push(node(
            "program".into(),
            "material-language-v1",
            request.entry.as_bytes().into(),
            source_deps,
        ));
        let mut image_bytes = BTreeMap::new();
        for name in compiled
            .metadata()
            .required_inputs
            .iter()
            .filter_map(|input| input.strip_prefix("image:"))
        {
            let bytes = assets(name).map_err(|e| fail(format!("{name}: {e}")))?;
            image_bytes.insert(name.to_owned(), bytes);
        }
        let mut input_deps = vec![dependency("program", "program")];
        for (name, bytes) in &image_bytes {
            let id = format!("image/{name}");
            nodes.push(node(id.clone(), "source-image-v1", bytes.to_vec(), vec![]));
            input_deps.push(dependency(&format!("image/{name}"), id));
        }
        nodes.push(node(
            "inputs".into(),
            "material-inputs-v1",
            json(&(&request.parameters, request.seed, &request.edges)),
            input_deps,
        ));
        // Every decoded image, without parameters: a map baked from some outputs depends on the
        // images, not on the parameters it never reads.
        nodes.push(node(
            "assets".into(),
            "material-assets-v1",
            Vec::new(),
            image_bytes
                .keys()
                .map(|name| dependency(&format!("image/{name}"), format!("image/{name}")))
                .collect(),
        ));
        let inspecting = request.inspection.is_some();
        let dependency_table = compiled.dependency_table();
        let live_fields = live_field_names(&compiled, request.inspection.as_deref());
        let compact = request.profile == super::Profile::Production;
        let mut targets = vec![];
        for (i, receiver) in receivers.iter().enumerate() {
            let layout = format!("layout/{i}");
            let bake = format!("bake/{i}");
            let mut identity = receiver.clone();
            identity.id.clear();
            identity.object_id.clear();
            nodes.push(node(
                layout.clone(),
                "receiver-atlas-v3",
                json(&(
                    identity,
                    request.resolution,
                    request.mapping,
                    request.domain,
                    request.gravity,
                    request.texel_density,
                )),
                vec![],
            ));
            nodes.push(node(
                bake.clone(),
                "material-receiver-v4",
                // Production bakes compress their PNG maps harder: same pixels, different bytes.
                json(&(
                    &request.inspection,
                    request.profile == super::Profile::Production,
                )),
                vec![
                    dependency("mapping", layout.clone()),
                    dependency("program", "program"),
                    dependency("inputs", "inputs"),
                ],
            ));
            targets.push(layout.clone());
            if inspecting {
                // A preview evaluates every output into one map: keyed by the whole request.
                targets.push(bake);
                continue;
            }
            // Each map is keyed by the inputs of only the outputs it is baked from.
            for group in Group::MAPS {
                let dependencies = group_dependencies(&dependency_table, group, &live_fields);
                let mut inputs_identity = group.name().as_bytes().to_vec();
                inputs_identity.push(u8::from(compact));
                match compiled.input_fingerprint(
                    &Inputs {
                        parameters: request.parameters.clone(),
                        seed: request.seed,
                        edges: request.edges.clone(),
                        ..Inputs::default()
                    },
                    &dependencies,
                ) {
                    Ok(fingerprint) => inputs_identity.extend(fingerprint),
                    // An invalid request never matches a retained map; building it reports why.
                    Err(error) => inputs_identity.extend(error.to_string().bytes()),
                }
                let id = format!("bake/{i}/{}", group.name());
                nodes.push(node(
                    id.clone(),
                    "material-group-v1",
                    inputs_identity,
                    vec![
                        dependency("mapping", layout.clone()),
                        dependency("program", "program"),
                        dependency("assets", "assets"),
                    ],
                ));
                targets.push(id);
            }
        }
        let dependencies_only = share.is_some().then(|| {
            // A share needs the program, inputs and layouts, but never bakes or caches a receiver.
            let nodes: Vec<Node> = nodes
                .iter()
                .filter(|node| !node.id.starts_with("bake/"))
                .cloned()
                .collect();
            let targets = (0..receivers.len())
                .map(|i| format!("layout/{i}"))
                .chain(["program".to_owned(), "inputs".to_owned()])
                .collect();
            Plan::new(nodes, targets)
        });
        let plan = Plan::new(nodes, targets).map_err(|e| fail(e.to_string()))?;
        let bake_keys: Vec<ContentKey> = (0..receivers.len())
            .map(|i| plan.key(&format!("bake/{i}")).expect("planned bake"))
            .collect();
        // The maps no retained artifact holds are the ones this build bakes. A build after a small
        // edit holds every map the edit cannot reach.
        let dirty: Vec<Groups> = (0..receivers.len())
            .map(|i| {
                if inspecting {
                    return if self.runtime.holds(&bake_keys[i]) {
                        Groups::NONE
                    } else {
                        Groups::of(Group::Inspection)
                    };
                }
                Group::MAPS
                    .into_iter()
                    .filter(|group| {
                        let key = plan
                            .key(&format!("bake/{i}/{}", group.name()))
                            .expect("planned map");
                        !self.runtime.holds(&key)
                    })
                    .fold(Groups::NONE, Groups::with)
            })
            .collect();
        if share.is_none() && self.distribution.enabled {
            let complete = |key: &ContentKey, wanted: Groups| {
                self.distribution.provided.get(key).is_some_and(|provided| {
                    provided.groups.contains_all(wanted.rows())
                        && provided.slots.iter().all(Option::is_some)
                })
            };
            let (keys, groups): (Vec<String>, Vec<u8>) = (0..receivers.len())
                .filter(|&i| !dirty[i].is_empty() && !complete(&bake_keys[i], dirty[i]))
                .map(|i| (format_key(&bake_keys[i]), dirty[i].rows().0))
                .unzip();
            // Small bakes finish sooner here than a round trip through other workers.
            let texels = request.resolution * request.resolution * receivers.len();
            if !keys.is_empty() && texels >= DISTRIBUTE_TEXELS {
                // A bake reported again (the host built before taking it) is listed once.
                self.distribution
                    .pending
                    .retain(|pending| pending.keys != keys || pending.groups != groups);
                self.distribution.pending.push(PendingBake {
                    request: request.clone(),
                    keys,
                    images: image_bytes.keys().cloned().collect(),
                    groups,
                });
                return Err(vec![Diagnostic {
                    code: "BAKE_PENDING".into(),
                    ..Diagnostic::message(&request.entry, "Material bake is being distributed.")
                }]);
            }
        }
        // Rows provided for these bakes are used once; a later build with other inputs has other keys.
        let mut provided: BTreeMap<usize, (Groups, Vec<Row>)> = BTreeMap::new();
        if share.is_none() {
            for (i, key) in bake_keys.iter().enumerate() {
                if let Some(rows) = self.distribution.provided.remove(key) {
                    if !rows.groups.contains_all(dirty[i].rows()) {
                        continue;
                    }
                    if let Some(slots) = rows.slots.into_iter().collect::<Option<Vec<Row>>>() {
                        provided.insert(i, (rows.groups, slots));
                    }
                }
            }
        }
        let run_plan = match &dependencies_only {
            Some(plan) => plan.as_ref().map_err(|e| fail(e.to_string()))?,
            None => &plan,
        };
        let mut evaluation_error = None;
        // Maps baked alongside the one a producer was asked for, until their own producers run.
        let mut baked_groups: BTreeMap<(usize, Group), Arc<BakedReceiver>> = BTreeMap::new();
        let mips = self.mips.clone();
        let complete = self
            .runtime
            .run(
                self.build_epoch(),
                run_plan,
                cancelled,
                |node, dependencies, stopped| match node.id.as_str() {
                    "program" => Ok((
                        Artifact::Program(compiled.clone()),
                        compiled.retained_bytes(),
                    )),
                    id if id.starts_with("image/") => {
                        // Decoding depends only on the encoded bytes, so parameter edits reuse it.
                        let image = image::decode(&image_bytes[&id["image/".len()..]])?;
                        // Filtered sampling may add mip levels of about the decoded size.
                        let bytes = image.rgba.len() * 2;
                        Ok((
                            Artifact::Image(Arc::new(program::Image {
                                width: image.width,
                                height: image.height,
                                rgba: image.rgba,
                            })),
                            bytes,
                        ))
                    }
                    "inputs" => {
                        let mut images = BTreeMap::new();
                        let mut count = 0usize;
                        for name in image_bytes.keys() {
                            let Artifact::Image(image) =
                                dependencies[&format!("image/{name}")].as_ref()
                            else {
                                unreachable!()
                            };
                            count += image.rgba.len();
                            if count > 64 * 1024 * 1024 {
                                return Err(
                                    "Decoded material images exceed the authoring allowance."
                                        .into(),
                                );
                            }
                            images.insert(name.clone(), image.clone());
                        }
                        Ok((
                            Artifact::Inputs(Arc::new(Inputs {
                                parameters: request.parameters.clone(),
                                seed: request.seed,
                                images,
                                edges: request.edges.clone(),
                                features: Vec::new(),
                                // One cache for the session: variants of a material filter
                                // their shared images once.
                                mips: mips.clone(),
                            })),
                            // The images are shared artifacts, counted where they are decoded.
                            json(&request.parameters).len() + 1024,
                        ))
                    }
                    id if id.starts_with("layout/") => {
                        let index = id[7..].parse::<usize>().unwrap();
                        let atlas = Atlas::new(
                            receivers[index].clone(),
                            request.resolution,
                            request.mapping,
                            request.domain,
                            request.gravity,
                            request.profile.max_resolution(),
                            request.texel_density,
                        )?;
                        let bytes = atlas.mesh.positions.len() * 88
                            + atlas.mesh.indices.len() * 4
                            + receivers[index].positions.len() * 8 * 3;
                        Ok((Artifact::Receiver(Arc::new(atlas)), bytes))
                    }
                    "assets" => {
                        let mut images = BTreeMap::new();
                        let mut count = 0usize;
                        for name in image_bytes.keys() {
                            let Artifact::Image(image) =
                                dependencies[&format!("image/{name}")].as_ref()
                            else {
                                unreachable!()
                            };
                            count += image.rgba.len();
                            if count > 64 * 1024 * 1024 {
                                return Err(
                                    "Decoded material images exceed the authoring allowance."
                                        .into(),
                                );
                            }
                            images.insert(name.clone(), image.clone());
                        }
                        Ok((
                            Artifact::Inputs(Arc::new(Inputs {
                                images,
                                mips: mips.clone(),
                                ..Inputs::default()
                            })),
                            1024,
                        ))
                    }
                    id if id.starts_with("bake/") => {
                        let Artifact::Program(program) = dependencies["program"].as_ref() else {
                            unreachable!()
                        };
                        let Artifact::Receiver(atlas) = dependencies["mapping"].as_ref() else {
                            unreachable!()
                        };
                        let rest = &id["bake/".len()..];
                        let (index, group) = match rest.split_once('/') {
                            Some((index, name)) => (
                                index.parse::<usize>().unwrap(),
                                Group::MAPS.into_iter().find(|g| g.name() == name).unwrap(),
                            ),
                            None => (rest.parse::<usize>().unwrap(), Group::Inspection),
                        };
                        if let Some(done) = baked_groups.remove(&(index, group)) {
                            let bytes = done.bytes();
                            return Ok((Artifact::Material(done), bytes));
                        }
                        // Evaluate every map this build needs of the receiver in one pass, so
                        // outputs the maps share are computed once.
                        let wanted = dirty[index].with(group);
                        let inputs = if inspecting {
                            let Artifact::Inputs(inputs) = dependencies["inputs"].as_ref() else {
                                unreachable!()
                            };
                            inputs.clone()
                        } else {
                            let Artifact::Inputs(assets) = dependencies["assets"].as_ref() else {
                                unreachable!()
                            };
                            Arc::new(Inputs {
                                parameters: request.parameters.clone(),
                                seed: request.seed,
                                images: assets.images.clone(),
                                edges: request.edges.clone(),
                                features: Vec::new(),
                                mips: assets.mips.clone(),
                            })
                        };
                        match bake_groups(
                            program,
                            &inputs,
                            atlas,
                            request.inspection.as_deref(),
                            request.profile.threads(),
                            compact,
                            wanted,
                            provided.remove(&index),
                            stopped,
                        ) {
                            Ok(results) => {
                                let mut requested = None;
                                for (made, value) in results {
                                    let value = Arc::new(value);
                                    if made == group {
                                        requested = Some(value);
                                    } else {
                                        baked_groups.insert((index, made), value);
                                    }
                                }
                                let value = requested.expect("the requested map was baked");
                                let bytes = value.bytes();
                                Ok((Artifact::Material(value), bytes))
                            }
                            Err(error) => {
                                let message = error.message.clone();
                                evaluation_error = Some(error);
                                Err(message)
                            }
                        }
                    }
                    _ => Ok((Artifact::Source, node.inputs.len())),
                },
            )
            .map_err(|e| {
                evaluation_error
                    .clone()
                    .map(|e| vec![e])
                    .unwrap_or_else(|| fail(e))
            })?;
        if let Some(share) = share {
            let Artifact::Program(program) = complete["program"].as_ref() else {
                unreachable!()
            };
            let Artifact::Inputs(inputs) = complete["inputs"].as_ref() else {
                unreachable!()
            };
            let mut bytes = Vec::new();
            for (i, key) in bake_keys.iter().enumerate() {
                let Some(position) = share.keys.iter().position(|wanted| wanted == key) else {
                    continue;
                };
                let Artifact::Receiver(atlas) = complete[&format!("layout/{i}")].as_ref() else {
                    unreachable!()
                };
                let groups = share.groups[position];
                let (total, rows) = bake_rows(
                    program,
                    inputs,
                    atlas,
                    request.inspection.as_deref(),
                    (share.index, share.count),
                    groups,
                    cancelled,
                )
                .map_err(|error| vec![error])?;
                write_share(&mut bytes, key, groups, total, &rows);
            }
            return Ok(Built::Share(bytes));
        }
        let mut model = Model {
            id: "studio/material".into(),
            name: "Material preview".into(),
            ..Default::default()
        };
        let mut layouts = Vec::with_capacity(receivers.len());
        for (i, receiver) in receivers.iter().enumerate() {
            let Artifact::Receiver(atlas) = complete[&format!("layout/{i}")].as_ref() else {
                unreachable!()
            };
            let mut maps = BTreeMap::new();
            let wanted: &[Group] = if inspecting {
                &[Group::Inspection]
            } else {
                &Group::MAPS
            };
            for &group in wanted {
                let id = if inspecting {
                    format!("bake/{i}")
                } else {
                    format!("bake/{i}/{}", group.name())
                };
                let Artifact::Material(baked) = complete[&id].as_ref() else {
                    unreachable!()
                };
                maps.insert(group, baked.clone());
            }
            let (material, images) = assemble(&maps, inspecting).map_err(&fail)?;
            layouts.push([atlas.width as f64, atlas.height as f64, atlas.density]);
            let mesh = format!(
                "studio/mesh/{}",
                hash(&json(&(
                    &atlas.mesh.positions,
                    &atlas.mesh.uvs,
                    &atlas.mesh.indices
                )))
            );
            model.meshes.insert(mesh.clone(), atlas.mesh.clone());
            model.images.extend(images);
            model
                .materials
                .insert(material.id.clone(), material.clone());
            model.instances.push(Instance {
                id: receiver.id.clone(),
                object_id: receiver.object_id.clone(),
                mesh,
                material: Some(material.id.clone()),
                transform: IDENTITY,
                source: SourceLocation {
                    line: 1,
                    column: 1,
                    offset: 0,
                    end: None,
                },
                visible: true,
                collidable: false,
            });
        }
        let mut metadata = serde_json::to_value(compiled.metadata()).unwrap();
        metadata["mapping"] = serde_json::to_value(request.mapping).unwrap();
        // Each receiver's texture size and detail, for hosts that report or budget them.
        metadata["atlases"] = receivers
            .iter()
            .zip(&layouts)
            .map(|(receiver, [width, height, density])| {
                serde_json::json!({
                    "id": receiver.id, "width": width, "height": height, "texelsPerMeter": density,
                })
            })
            .collect();
        Ok(Built::Output(Output { model, metadata }))
    }
}

#[cfg(test)]
thread_local! {
    /// The groups each bake evaluated on this thread, for tests of what an edit rebakes.
    static BAKED: std::cell::RefCell<Vec<Groups>> = const { std::cell::RefCell::new(Vec::new()) };
}
/// Bakes below this many requested texels are evaluated where they are requested.
const DISTRIBUTE_TEXELS: usize = 8192;

/// A session's state for baking rows in other sessions (see [`Session::distribute_bakes`]).
#[derive(Default)]
pub(crate) struct Distribution {
    enabled: bool,
    /// Bakes reported by builds and not yet taken by the host.
    pending: Vec<PendingBake>,
    /// Rows received so far for a reported bake, by row ordinal.
    provided: BTreeMap<ContentKey, ProvidedRows>,
}

/// Rows other sessions evaluated for one bake, with the data they carry.
#[derive(Default)]
struct ProvidedRows {
    groups: Groups,
    slots: Vec<Option<Row>>,
}

/// A bake other sessions can share: the material request, the content keys of its receivers'
/// uncached bakes and the images the request reads. `groups` says, per key, which maps' rows to
/// evaluate (a bit per [`Group`]); a build after a small edit needs only the maps the edit can
/// change. An absent list wants every map.
#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingBake {
    pub request: Request,
    pub keys: Vec<String>,
    pub images: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub groups: Vec<u8>,
}

fn format_key(key: &ContentKey) -> String {
    key.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn parse_key(text: &str) -> Option<ContentKey> {
    if text.len() != 64 || !text.is_ascii() {
        return None;
    }
    let mut key = [0u8; 32];
    for (slot, pair) in key.iter_mut().zip(text.as_bytes().chunks(2)) {
        *slot = u8::from_str_radix(std::str::from_utf8(pair).ok()?, 16).ok()?;
    }
    Some(key)
}

const SHARE_MAGIC: &[u8; 4] = b"SBR2";

/// Appends one bake's rows: key, the groups whose data the rows carry, total row count, then each
/// row with its ordinal. Numbers are little-endian and floats keep their bits, so a row is the
/// same wherever it is merged.
fn write_share(
    bytes: &mut Vec<u8>,
    key: &ContentKey,
    groups: Groups,
    total: usize,
    rows: &[(usize, Row)],
) {
    let word = |bytes: &mut Vec<u8>, value: usize| bytes.extend((value as u32).to_le_bytes());
    bytes.extend(SHARE_MAGIC);
    bytes.extend(key);
    bytes.push(groups.0);
    word(bytes, total);
    word(bytes, rows.len());
    for (ordinal, row) in rows {
        word(bytes, *ordinal);
        word(bytes, row.cell);
        word(bytes, row.live.len());
        bytes.push(u8::from(row.blend) | u8::from(row.emits) << 1);
        bytes.extend(row.strength.to_le_bytes());
        if groups.contains(Group::Color) {
            bytes.extend(&row.color);
        }
        if groups.contains(Group::Normal) {
            bytes.extend(&row.normal);
        }
        if groups.contains(Group::Properties) {
            bytes.extend(&row.roughness);
            bytes.extend(&row.metalness);
        }
        if groups.contains(Group::Emissive) && row.emits {
            for value in row.emitted.iter().flatten() {
                bytes.extend(value.to_le_bytes());
            }
        }
        for value in row.live.iter().flatten() {
            bytes.extend(value.to_le_bytes());
        }
    }
}

type Shares = Vec<(ContentKey, Groups, usize, Vec<(usize, Row)>)>;

fn read_shares(mut bytes: &[u8]) -> Result<Shares, String> {
    fn take<'a>(bytes: &mut &'a [u8], count: usize) -> Result<&'a [u8], String> {
        if bytes.len() < count {
            return Err("Truncated bake share.".into());
        }
        let (head, tail) = bytes.split_at(count);
        *bytes = tail;
        Ok(head)
    }
    fn word(bytes: &mut &[u8]) -> Result<usize, String> {
        Ok(u32::from_le_bytes(take(bytes, 4)?.try_into().unwrap()) as usize)
    }
    fn floats(bytes: &mut &[u8], count: usize) -> Result<Vec<f64>, String> {
        Ok(
            take(bytes, count.checked_mul(8).ok_or("Invalid bake share.")?)?
                .chunks_exact(8)
                .map(|chunk| f64::from_le_bytes(chunk.try_into().unwrap()))
                .collect(),
        )
    }
    let mut shares = Vec::new();
    while !bytes.is_empty() {
        if take(&mut bytes, 4)? != SHARE_MAGIC {
            return Err("This is not a bake share.".into());
        }
        let key: ContentKey = take(&mut bytes, 32)?.try_into().unwrap();
        let groups = Groups(take(&mut bytes, 1)?[0]);
        if groups.0 & !Groups::MAPS.0 != 0 {
            return Err("Invalid bake share groups.".into());
        }
        let total = word(&mut bytes)?;
        let count = word(&mut bytes)?;
        if total > 1 << 20 || count > total {
            return Err("Invalid bake share row count.".into());
        }
        let mut rows = Vec::with_capacity(count);
        for _ in 0..count {
            let ordinal = word(&mut bytes)?;
            let cell = word(&mut bytes)?;
            let live = word(&mut bytes)?;
            if cell > 4096 || live > 256 {
                return Err("Invalid bake share row.".into());
            }
            let flags = take(&mut bytes, 1)?[0];
            let strength = floats(&mut bytes, 1)?[0];
            let mut bytes_of = |wanted: bool, count: usize| -> Result<Vec<u8>, String> {
                Ok(if wanted {
                    take(&mut bytes, count)?.to_vec()
                } else {
                    Vec::new()
                })
            };
            let color = bytes_of(groups.contains(Group::Color), cell * 4)?;
            let normal = bytes_of(groups.contains(Group::Normal), cell * 4)?;
            let roughness = bytes_of(groups.contains(Group::Properties), cell)?;
            let metalness = bytes_of(groups.contains(Group::Properties), cell)?;
            let emits = flags & 2 != 0;
            let emitted = if groups.contains(Group::Emissive) && emits {
                floats(&mut bytes, cell * 3)?
                    .chunks_exact(3)
                    .map(|rgb| [rgb[0], rgb[1], rgb[2]])
                    .collect()
            } else {
                Vec::new()
            };
            let live = (0..live)
                .map(|_| floats(&mut bytes, cell))
                .collect::<Result<Vec<_>, _>>()?;
            rows.push((
                ordinal,
                Row {
                    cell,
                    color,
                    normal,
                    roughness,
                    metalness,
                    emitted,
                    live,
                    blend: flags & 1 != 0,
                    emits,
                    strength,
                },
            ));
        }
        shares.push((key, groups, total, rows));
    }
    Ok(shares)
}

fn byte(value: f64) -> u8 {
    (value.clamp(0., 1.) * 255.).round() as u8
}
/// Quantizes a unit-range value with an offset of up to `amount` half steps either way that
/// varies per texel and channel as integer-hashed white noise. Gentle relief otherwise lands in
/// wide 8-bit plateaus whose edges a glossy highlight shows as contour stripes; white noise has no
/// lattice structure to beat against screen pixels. Callers pass a smaller amount where the value
/// already changes by a step or more per texel, so busy relief stays as compressible as plain
/// rounding. It depends only on the atlas texel, so threading and the target (native or Wasm)
/// never change bytes.
fn dithered(value: f64, texel: [usize; 2], channel: usize, amount: f64) -> u8 {
    let mut h = (texel[0] as u32)
        .wrapping_mul(0x9E37_79B1)
        .wrapping_add((texel[1] as u32).wrapping_mul(0x85EB_CA77))
        .wrapping_add((channel as u32).wrapping_mul(0xC2B2_AE3D));
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^= h >> 12;
    h = h.wrapping_mul(0x297A_2D39);
    h ^= h >> 15;
    let noise = f64::from(h) / 4_294_967_296.;
    (value.clamp(0., 1.) * 255. + (noise - 0.5) * amount)
        .round()
        .clamp(0., 255.) as u8
}
fn srgb(value: f64) -> u8 {
    crate::materialspec::surface::color_byte(value).expect("finite checked material output")
}
fn inspection(
    value: &program::Outputs,
    field: &str,
    normal: [f64; 3],
    receiver: [f64; 3],
) -> Result<[f64; 3], String> {
    Ok(match field {
        "base_color" | "baseColor" => value.base_color,
        "roughness" => [value.roughness; 3],
        "metalness" => [value.metalness; 3],
        "height" => [0.5 + value.height * 50.; 3],
        "opacity" => [value.opacity; 3],
        "normal" => normal.map(|v| v * 0.5 + 0.5),
        "receiver_normal" | "receiverNormal" => value
            .receiver_normal
            .unwrap_or(receiver)
            .map(|v| v * 0.5 + 0.5),
        "emissive" => value.emissive,
        name => match value
            .fields
            .get(name)
            .ok_or_else(|| format!("Unknown inspection field {name}."))?
        {
            FieldValue::Scalar(v) => [*v; 3],
            FieldValue::Bool(v) => [if *v { 1. } else { 0. }; 3],
            FieldValue::Vec2(v) => [v[0], v[1], 0.],
            FieldValue::Vec3(v) => *v,
        },
    })
}

/// Live arguments that never vary become constants; the others are quantized four to a linear
/// RGBA texture on the atlas UVs, decoded as `byte / 255 * scale + offset`.
fn live_operations(
    program: &Program,
    fields: &[&str],
    values: &[Vec<f64>],
    pixels: usize,
    encode: &mut dyn FnMut(Vec<u8>) -> Result<String, Diagnostic>,
) -> Result<Vec<LiveDescriptor>, Diagnostic> {
    let mut arguments: BTreeMap<&str, LiveArgument> = BTreeMap::new();
    let mut varying = Vec::new();
    for (field, samples) in fields.iter().zip(values) {
        let (low, high) = samples
            .iter()
            .filter(|v| !v.is_nan())
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(low, high), v| {
                (low.min(*v), high.max(*v))
            });
        if !low.is_finite() {
            arguments.insert(field, LiveArgument::Constant { value: 0. });
        } else if high - low <= 1e-9 * (1. + high.abs()) {
            arguments.insert(field, LiveArgument::Constant { value: low });
        } else {
            varying.push((*field, samples, low, high));
        }
    }
    for group in varying.chunks(4) {
        let mut rgba = vec![0u8; pixels * 4];
        for (channel, (_, samples, low, high)) in group.iter().enumerate() {
            for (texel, value) in samples.iter().enumerate() {
                if !value.is_nan() {
                    rgba[texel * 4 + channel] =
                        (((value - low) / (high - low)).clamp(0., 1.) * 255.).round() as u8;
                }
            }
        }
        let texture = encode(rgba)?;
        for (channel, (field, _, low, high)) in group.iter().enumerate() {
            arguments.insert(
                field,
                LiveArgument::Texture {
                    texture: texture.clone(),
                    channel: channel as u8,
                    scale: high - low,
                    offset: *low,
                },
            );
        }
    }
    Ok(program
        .metadata()
        .live
        .iter()
        .map(|operation| LiveDescriptor {
            id: operation.id.clone(),
            stage: operation.stage.clone(),
            requires: operation.requires.clone(),
            arguments: operation
                .inputs
                .iter()
                .map(|(argument, field)| (argument.clone(), arguments[field.as_str()].clone()))
                .collect(),
        })
        .collect())
}

/// One texel row of one atlas chart. A row holds the data of the groups it was evaluated for and
/// leaves the other fields empty.
struct Row {
    /// Texels in the row.
    cell: usize,
    color: Vec<u8>,
    normal: Vec<u8>,
    roughness: Vec<u8>,
    metalness: Vec<u8>,
    /// Linear emission per texel; empty until a texel of the row emits, as most materials never do.
    emitted: Vec<[f64; 3]>,
    live: Vec<Vec<f64>>,
    blend: bool,
    emits: bool,
    strength: f64,
}

/// Rows a block evaluates together. Larger blocks reuse more neighbor heights; smaller ones bound
/// the working memory and the time between cancellation checks.
const BLOCK_ROWS: usize = 32;

/// One texel's evaluated center.
struct Center {
    triangle: usize,
    u: f64,
    v: f64,
    normal: [f64; 3],
    value: program::Outputs,
}
impl Center {
    /// Whether this texel samples exactly the point (triangle, u, v). Gutter texels repeat their
    /// chart's edge samples and a chart below the atlas's density repeats its few samples, so
    /// neighboring texels often do; their results are copied instead of evaluated again.
    fn samples(&self, triangle: usize, u: f64, v: f64) -> bool {
        self.triangle == triangle
            && self.u.to_bits() == u.to_bits()
            && self.v.to_bits() == v.to_bits()
    }
}

/// Where a texel's result comes from when a neighbor already sampled the same point.
#[derive(Clone, Copy)]
enum Repeat {
    /// The texel before it in the row.
    Left,
    /// The same column of the row above, when that row is the block's previous row.
    Above,
}

/// The texel of `chart` next to (x, y) whose center is the sample (triangle, u, v), if there is
/// one. Texel centers are one leg step apart, so the height one step along a leg is usually an
/// adjacent texel's own height; gutters, the far side of a chart's diagonal and projected
/// texels have no such neighbor.
fn neighbor_texel(
    atlas: &Atlas,
    chart: &Chart,
    [x, y]: [usize; 2],
    triangle: usize,
    [u, v]: [f64; 2],
) -> Option<[usize; 2]> {
    [[1i64, 0], [-1, 0], [0, 1], [0, -1]]
        .into_iter()
        .filter_map(|[dx, dy]| {
            let (nx, ny) = (x as i64 + dx, y as i64 + dy);
            (nx >= 0 && ny >= 0 && (nx as usize) < chart.width && (ny as usize) < chart.height)
                .then_some([nx as usize, ny as usize])
        })
        .find(|&[nx, ny]| {
            let (other, ou, ov) = atlas.texel(chart, nx, ny);
            other == triangle && (ou - u).abs() <= 1e-9 && (ov - v).abs() <= 1e-9
        })
}

/// Evaluates consecutive atlas rows for the groups in `groups`: the outputs those maps read, the
/// relief normal and any live arguments.
///
/// A normal needs the height one texel along each chart leg. Where that point is an adjacent
/// texel's center, the adjacent texel's height is used instead of evaluating the height graph
/// again, which is most of a bake's work otherwise. A neighbor row outside `rows` has its heights
/// evaluated here, at the same centers, so the result never depends on how rows are grouped
/// into blocks, threads or workers. Outputs no wanted group reads are never evaluated, and the
/// relief is derived only for the normal map (or a preview, which wants every group).
fn bake_block(
    evaluator: &mut program::Evaluator<'_>,
    atlas: &Atlas,
    rows: &[(usize, usize)],
    inspect: Option<&str>,
    live_fields: &program::FieldSelection,
    groups: Groups,
    entry: &str,
) -> Result<Vec<Row>, Diagnostic> {
    let failure = |message: String| Diagnostic::message(entry, message);
    let live_count = live_fields.len();
    let outputs = if inspect.is_some() {
        program::OutputSet::ALL
    } else {
        groups.outputs()
    };
    let wants = |group: Group| groups.contains(group);
    let relief = wants(Group::Normal);
    // Live field values of the samples of a row, one run of live_count per sample.
    let mut live: Vec<FieldValue> = Vec::new();
    let mut baked: Vec<Row> = Vec::with_capacity(rows.len());
    let mut centers: Vec<Vec<Center>> = Vec::with_capacity(rows.len());
    // Center heights by (chart, row): this block's rows, then neighbor rows as they are needed.
    let mut heights: BTreeMap<(usize, usize), Vec<f64>> = BTreeMap::new();
    // Height slopes of each row's texels, for texels that repeat a neighbor's sample.
    let mut slopes: Vec<Vec<[f64; 2]>> = Vec::with_capacity(rows.len());
    let above = |row: usize| row > 0 && rows[row - 1] == (rows[row].0, rows[row].1.wrapping_sub(1));
    for (row_number, &(chart_index, y)) in rows.iter().enumerate() {
        let chart = &atlas.charts[chart_index];
        let cell = chart.width;
        let sized = |group: Group, bytes: usize| {
            if wants(group) {
                vec![0; bytes]
            } else {
                Vec::new()
            }
        };
        let mut row = Row {
            cell,
            color: sized(Group::Color, cell * 4),
            normal: sized(Group::Normal, cell * 4),
            roughness: sized(Group::Properties, cell),
            metalness: sized(Group::Properties, cell),
            emitted: Vec::new(),
            live: vec![vec![f64::NAN; cell]; live_count],
            blend: false,
            emits: false,
            strength: 1.,
        };
        // Each row of texels has its own operation allowance; every sample keeps its step budget.
        evaluator.replenish(64_000_000);
        let mut row_centers: Vec<Center> = Vec::with_capacity(cell);
        let mut row_heights = Vec::with_capacity(cell);
        // The row's texels that sample a new point are evaluated together; the others copy a
        // neighbor's result, so which texels those are does not depend on any value.
        let texels: Vec<(usize, f64, f64)> = (0..cell).map(|x| atlas.texel(chart, x, y)).collect();
        let repeats: Vec<Option<Repeat>> = (0..cell)
            .map(|x| {
                let (triangle, u, v) = texels[x];
                let same = |other: (usize, f64, f64)| {
                    other.0 == triangle
                        && other.1.to_bits() == u.to_bits()
                        && other.2.to_bits() == v.to_bits()
                };
                if x > 0 && same(texels[x - 1]) {
                    Some(Repeat::Left)
                } else if above(row_number) && centers[row_number - 1][x].samples(triangle, u, v) {
                    Some(Repeat::Above)
                } else {
                    None
                }
            })
            .collect();
        let mut row_samples: Vec<program::Sample> = Vec::new();
        let mut sample_error = None;
        for x in 0..cell {
            if repeats[x].is_none() {
                let (triangle, u, v) = texels[x];
                match atlas.sample(triangle, u, v) {
                    Ok(sample) => row_samples.push(sample),
                    Err(message) => {
                        sample_error = Some(message);
                        break;
                    }
                }
            }
        }
        // A preview computes its one field; channel values are the same either way.
        let row_values = if inspect.is_some() {
            row_samples
                .iter()
                .map(|sample| evaluator.evaluate_channels(sample, inspect))
                .collect::<Result<Vec<_>, _>>()
        } else {
            evaluator.evaluate_row(&row_samples, outputs, live_fields, &mut live)
        }
        .map_err(Diagnostic::from)?;
        if let Some(message) = sample_error {
            return Err(failure(message));
        }
        let mut row_values = row_values.into_iter().enumerate();
        for x in 0..cell {
            let (triangle, u, v) = texels[x];
            let repeat = repeats[x];
            let (value, normal) = match repeat {
                Some(Repeat::Left) => {
                    for values in &mut row.live {
                        values[x] = values[x - 1];
                    }
                    let source: &Center = &row_centers[x - 1];
                    (source.value.clone(), source.normal)
                }
                Some(Repeat::Above) => {
                    for (values, source) in row.live.iter_mut().zip(&baked[row_number - 1].live) {
                        values[x] = source[x];
                    }
                    let source = &centers[row_number - 1][x];
                    (source.value.clone(), source.normal)
                }
                None => {
                    let (i, value) = row_values.next().expect("a value for each new sample");
                    let fields = &live[i * live_count..(i + 1) * live_count];
                    for (values, field) in row.live.iter_mut().zip(fields) {
                        values[x] = match field {
                            FieldValue::Scalar(v) => *v,
                            FieldValue::Bool(v) => f64::from(u8::from(*v)),
                            FieldValue::Vec2(v) => v[0],
                            FieldValue::Vec3(v) => v[0],
                        };
                    }
                    (value, row_samples[i].normal)
                }
            };
            if wants(Group::Properties) {
                row.roughness[x] = byte(value.roughness);
                row.metalness[x] = byte(value.metalness);
            }
            if wants(Group::Emissive) {
                row.strength = value.emissive.into_iter().fold(row.strength, f64::max);
                if value.emissive.iter().any(|v| *v > 0.) {
                    row.emits = true;
                    if row.emitted.is_empty() {
                        row.emitted = vec![[0.; 3]; cell];
                    }
                    row.emitted[x] = value.emissive;
                }
            }
            if wants(Group::Color) {
                row.blend |= value.opacity < 1.;
            }
            row_heights.push(value.height);
            row_centers.push(Center {
                triangle,
                u,
                v,
                normal,
                value,
            });
        }
        if relief {
            heights.insert((chart_index, y), row_heights);
        }
        centers.push(row_centers);
        baked.push(row);
    }
    if !relief && !wants(Group::Color) {
        return Ok(baked);
    }
    for (row_number, ((&(chart_index, y), row), row_centers)) in
        rows.iter().zip(&mut baked).zip(&centers).enumerate()
    {
        let chart = &atlas.charts[chart_index];
        evaluator.replenish(64_000_000);
        let mut previous: Option<[f64; 3]> = None;
        let mut row_slopes: Vec<[f64; 2]> = Vec::with_capacity(row_centers.len());
        for (x, center) in row_centers.iter().enumerate() {
            let (triangle, u, v) = (center.triangle, center.u, center.v);
            let value = &center.value;
            let mut direction = [0.; 3];
            if relief {
                let [step_u, step_v] = atlas.steps(triangle);
                let mut height_at = |target: [f64; 2]| -> Result<f64, Diagnostic> {
                    let Some([nx, ny]) = neighbor_texel(atlas, chart, [x, y], triangle, target)
                    else {
                        return evaluator
                            .evaluate_height(
                                &atlas
                                    .sample(triangle, target[0], target[1])
                                    .map_err(&failure)?,
                            )
                            .map_err(Diagnostic::from);
                    };
                    if !heights.contains_key(&(chart_index, ny)) {
                        // A neighbor row of another block: its center heights, as that block has them.
                        let mut row_heights: Vec<f64> = Vec::with_capacity(chart.width);
                        let mut before = None;
                        for other in 0..chart.width {
                            let (triangle, u, v) = atlas.texel(chart, other, ny);
                            let key = (triangle, u.to_bits(), v.to_bits());
                            let height = if before == Some(key) {
                                row_heights[other - 1]
                            } else {
                                evaluator
                                    .evaluate_height(
                                        &atlas.sample(triangle, u, v).map_err(&failure)?,
                                    )
                                    .map_err(Diagnostic::from)?
                            };
                            before = Some(key);
                            row_heights.push(height);
                        }
                        heights.insert((chart_index, ny), row_heights);
                    }
                    Ok(heights[&(chart_index, ny)][nx])
                };
                let [hu, hv] = if x > 0 && row_centers[x - 1].samples(triangle, u, v) {
                    row_slopes[x - 1]
                } else if above(row_number) && centers[row_number - 1][x].samples(triangle, u, v) {
                    slopes[row_number - 1][x]
                } else {
                    [height_at([u + step_u, v])?, height_at([u, v + step_v])?]
                };
                row_slopes.push([hu, hv]);
                direction = atlas
                    .normal(
                        triangle,
                        (hu - value.height) / step_u,
                        (hv - value.height) / step_v,
                    )
                    .map_err(&failure)?;
                if let Some(adjusted) = value.receiver_normal {
                    let [tangent, bitangent] = atlas.frame(triangle).map_err(&failure)?;
                    direction = program::orient_relief(
                        direction,
                        tangent,
                        bitangent,
                        center.normal,
                        adjusted,
                    )
                    .map_err(|message| failure(message.into()))?;
                }
                if let Some(offset) = value.normal_offset {
                    let [tangent, bitangent] = atlas.frame(triangle).map_err(&failure)?;
                    direction =
                        program::bend_shaded(direction, tangent, bitangent, center.normal, offset)
                            .map_err(|message| failure(message.into()))?;
                }
            }
            if wants(Group::Color) {
                let rgb = if let Some(field) = inspect {
                    inspection(value, field, direction, center.normal).map_err(&failure)?
                } else {
                    value.base_color
                };
                row.color[x * 4..x * 4 + 3].copy_from_slice(&rgb.map(srgb));
                row.color[x * 4 + 3] = byte(value.opacity);
            }
            if relief {
                let texel = [chart.x + x, chart.y + y];
                for (channel, v) in direction.into_iter().enumerate() {
                    // Full dither on plateaus (under a step per texel), none where relief already varies.
                    let step = previous.map_or(0., |p: [f64; 3]| (v - p[channel]).abs() * 127.5);
                    let amount = (1. - step).clamp(0., 1.);
                    row.normal[x * 4 + channel] = dithered(v * 0.5 + 0.5, texel, channel, amount);
                }
                previous = Some(direction);
                row.normal[x * 4 + 3] = 255;
            }
        }
        slopes.push(row_slopes);
    }
    Ok(baked)
}

/// Feature queries see this receiver's own creases and open edges, from the same world-space
/// positions the atlas samples; they are part of the receiver layout's cached identity.
fn receiver_features(program: &Program, atlas: &Atlas) -> Vec<program::Feature> {
    if !program
        .metadata()
        .required_inputs
        .iter()
        .any(|input| input == "features")
    {
        return Vec::new();
    }
    let receiver = &atlas.receiver;
    let mut features = program::receiver_features(&receiver.positions, &receiver.indices);
    features.extend(program::receiver_seams(
        &receiver.positions,
        &receiver.uvs,
        &receiver.indices,
    ));
    features.extend(receiver.contacts.iter().map(|contact| program::Feature {
        start: contact.start,
        end: contact.end,
        kind: program::FeatureKind::Contact,
        angle: 0.,
        normal: None,
    }));
    features.truncate(program::MAX_FEATURES);
    features
}

/// Live renderer arguments are fields; previews of a single field carry no live operations.
fn live_field_names<'a>(program: &'a Program, inspect: Option<&str>) -> Vec<&'a str> {
    let mut live_fields: Vec<&str> = Vec::new();
    if inspect.is_none() {
        for operation in &program.metadata().live {
            for field in operation.inputs.values() {
                if !live_fields.contains(&field.as_str()) {
                    live_fields.push(field);
                }
            }
        }
    }
    live_fields
}

/// What a map's bake reads from the request: the parameters, seed and source edges of the outputs
/// (and, for the live group, fields) it is baked from.
fn group_dependencies(
    table: &program::DependencyTable,
    group: Group,
    live_fields: &[&str],
) -> program::Dependencies {
    let outputs: &[&str] = match group {
        Group::Color => &["base_color", "opacity"],
        Group::Normal => &["height", "receiver_normal", "normal_offset"],
        Group::Properties => &["roughness", "metalness"],
        Group::Emissive => &["emissive"],
        Group::Live => &[],
        Group::Inspection => &[
            "base_color",
            "opacity",
            "height",
            "receiver_normal",
            "normal_offset",
            "roughness",
            "metalness",
            "emissive",
        ],
    };
    let mut found = program::Dependencies::default();
    for name in outputs {
        if let Some(dependencies) = table.outputs.get(*name) {
            found = found.union(dependencies);
        }
    }
    if group == Group::Live {
        for field in live_fields {
            if let Some(dependencies) = table.fields.get(*field) {
                found = found.union(dependencies);
            }
        }
    }
    found
}

/// Every chart row of an atlas as (chart, row), in the order rows are merged.
fn atlas_rows(atlas: &Atlas) -> Vec<(usize, usize)> {
    atlas
        .charts
        .iter()
        .enumerate()
        .flat_map(|(chart, placed)| (0..placed.height).map(move |y| (chart, y)))
        .collect()
}

/// Evaluates share `share.0` of `share.1` equal runs of consecutive rows for the data of `groups`,
/// with the total row count.
fn bake_rows(
    program: &Program,
    inputs: &Inputs,
    atlas: &Atlas,
    inspect: Option<&str>,
    share: (usize, usize),
    groups: Groups,
    cancelled: &dyn Fn() -> bool,
) -> Result<(usize, Vec<(usize, Row)>), Diagnostic> {
    let entry = program.metadata().entry.as_str();
    let failure = |message: String| Diagnostic::message(entry, message);
    let features = receiver_features(program, atlas);
    let live_fields = live_field_names(program, inspect);
    let live_selection = if groups.contains(Group::Live) {
        program.select_fields(&live_fields).map_err(&failure)?
    } else {
        program::FieldSelection::default()
    };
    let rows = atlas_rows(atlas);
    let mut evaluator = program
        .evaluator_with_features(inputs, &features)
        .map_err(Diagnostic::from)?;
    // Shares are consecutive rows, so only a share's first and last rows look outside it.
    let (first, last) = (
        rows.len() * share.0 / share.1,
        rows.len() * (share.0 + 1) / share.1,
    );
    let mut baked = Vec::with_capacity(last - first);
    for (block, block_rows) in rows[first..last].chunks(BLOCK_ROWS).enumerate() {
        if cancelled() {
            return Err(failure("Material authoring cancelled.".into()));
        }
        let start = first + block * BLOCK_ROWS;
        let block = bake_block(
            &mut evaluator,
            atlas,
            block_rows,
            inspect,
            &live_selection,
            groups,
            entry,
        )?;
        baked.extend(
            block
                .into_iter()
                .enumerate()
                .map(|(i, row)| (start + i, row)),
        );
    }
    Ok((rows.len(), baked))
}

/// Encodes one map of the atlas as PNG, identified by its content.
fn encode_map(
    atlas: &Atlas,
    rgba: Vec<u8>,
    compact: bool,
    cancelled: &dyn Fn() -> bool,
) -> Result<BakedImage, String> {
    let encode = if compact {
        image::encode_small_cancellable
    } else {
        image::encode_cancellable
    };
    let bytes = encode(
        &image::Image {
            width: atlas.width,
            height: atlas.height,
            rgba,
            mime: "image/png",
        },
        cancelled,
    )?;
    Ok(BakedImage {
        id: format!("studio/image/{}", hash(&bytes)),
        resource: ImageResource {
            mime: "image/png",
            bytes: bytes.into(),
            pixels: atlas.width * atlas.height,
        },
    })
}

/// One map's encoding, run after its pixels exist. It polls the stop test it is given.
type Encoding<'a> =
    Box<dyn FnOnce(&dyn Fn() -> bool) -> Result<BakedReceiver, Diagnostic> + Send + 'a>;

/// Bakes the maps of `wanted` from rows evaluated here (or `provided`, evaluated elsewhere), and
/// evaluates only the outputs those maps read. Every map equals the one a bake of all maps makes.
#[allow(clippy::too_many_arguments)]
fn bake_groups(
    program: &Program,
    inputs: &Inputs,
    atlas: &Atlas,
    inspect: Option<&str>,
    threads: usize,
    compact: bool,
    wanted: Groups,
    provided: Option<(Groups, Vec<Row>)>,
    cancelled: &dyn Fn() -> bool,
) -> Result<Vec<(Group, BakedReceiver)>, Diagnostic> {
    #[cfg(test)]
    BAKED.with(|baked| baked.borrow_mut().push(wanted));
    let failure = |message: String| Diagnostic::message(&program.metadata().entry, message);
    let pixels = atlas.width * atlas.height;
    if pixels > 4 * 1024 * 1024 {
        return Err(failure(
            "Material atlas exceeds its pixel allowance.".into(),
        ));
    }
    let live_fields = live_field_names(program, inspect);
    // The per-texel data the maps are baked from. Without live operations there is nothing to
    // evaluate for the live group.
    let mut data = wanted.rows();
    if live_fields.is_empty() {
        data = data.without(Group::Live);
    }
    let live_selection = if data.contains(Group::Live) {
        program.select_fields(&live_fields).map_err(&failure)?
    } else {
        program::FieldSelection::default()
    };
    let live_count = live_selection.len();
    let has = |group: Group| data.contains(group);
    let mut color = if has(Group::Color) {
        vec![0u8; pixels * 4]
    } else {
        Vec::new()
    };
    // Texels outside every chart are transparent; unused normals stay flat, so mip levels that
    // average across a chart's gutter never pull in an invalid direction.
    let mut normal = if has(Group::Normal) {
        [128u8, 128, 255, 0].repeat(pixels)
    } else {
        Vec::new()
    };
    let mut properties = if has(Group::Properties) {
        vec![0u8; pixels * 4]
    } else {
        Vec::new()
    };
    // Emission is rare and costs 24 bytes a texel, so its buffer exists only once a row emits.
    let mut emitted: Vec<[f64; 3]> = Vec::new();
    let mut blend = false;
    let mut emits = false;
    let mut emission_strength = 1f64;
    let mut live_values = vec![vec![f64::NAN; pixels]; live_count];
    let rows = atlas_rows(atlas);
    // Rows evaluated elsewhere are merged exactly like rows evaluated here; a set that does not
    // fit this atlas or lacks data is ignored and the rows are evaluated here instead.
    let mut provided = provided
        .filter(|(have, provided)| {
            have.contains_all(data)
                && provided.len() == rows.len()
                && provided.iter().zip(&rows).all(|(row, &(chart, _))| {
                    let cell = atlas.charts[chart].width;
                    row.cell == cell
                        && (!has(Group::Color) || row.color.len() == cell * 4)
                        && (!has(Group::Normal) || row.normal.len() == cell * 4)
                        && (!has(Group::Properties)
                            || (row.roughness.len() == cell && row.metalness.len() == cell))
                        && (!has(Group::Live)
                            || (row.live.len() == live_count
                                && row.live.iter().all(|values| values.len() == cell)))
                        && (!has(Group::Emissive) || !row.emits || row.emitted.len() == cell)
                })
        })
        .map(|(_, rows)| rows.into_iter());
    let features = if provided.is_some() || data.is_empty() {
        Vec::new()
    } else {
        receiver_features(program, atlas)
    };
    let evaluator = || {
        program
            .evaluator_with_features(inputs, &features)
            .map_err(Diagnostic::from)
    };
    let entry = program.metadata().entry.as_str();
    let threads = threads.max(1);
    let mut serial = if threads == 1 && provided.is_none() && !data.is_empty() {
        Some(evaluator()?)
    } else {
        None
    };
    // Rows are evaluated independently and written back in order, so threads never change bytes.
    let evaluated: &[(usize, usize)] = if data.is_empty() { &[] } else { &rows };
    for batch in evaluated.chunks(threads * BLOCK_ROWS) {
        if cancelled() {
            return Err(failure("Material authoring cancelled.".into()));
        }
        let results: Vec<Result<Row, Diagnostic>> = match &mut serial {
            None if provided.is_some() => provided
                .as_mut()
                .expect("provided rows")
                .by_ref()
                .take(batch.len())
                .map(Ok)
                .collect(),
            Some(evaluator) => match bake_block(
                evaluator,
                atlas,
                batch,
                inspect,
                &live_selection,
                data,
                entry,
            ) {
                Ok(rows) => rows.into_iter().map(Ok).collect(),
                Err(error) => vec![Err(error)],
            },
            None => std::thread::scope(|scope| {
                let handles: Vec<_> = batch
                    .chunks(batch.len().div_ceil(threads))
                    .map(|share| {
                        let live_selection = &live_selection;
                        let evaluator = &evaluator;
                        scope.spawn(move || -> Vec<Result<Row, Diagnostic>> {
                            let mut evaluator = match evaluator() {
                                Ok(evaluator) => evaluator,
                                Err(error) => {
                                    return share.iter().map(|_| Err(error.clone())).collect();
                                }
                            };
                            match bake_block(
                                &mut evaluator,
                                atlas,
                                share,
                                inspect,
                                live_selection,
                                data,
                                entry,
                            ) {
                                Ok(rows) => rows.into_iter().map(Ok).collect(),
                                Err(error) => vec![Err(error)],
                            }
                        })
                    })
                    .collect();
                handles
                    .into_iter()
                    .flat_map(|handle| handle.join().expect("bake rows do not panic"))
                    .collect()
            }),
        };
        for (&(chart, y), result) in batch.iter().zip(results) {
            let row = result?;
            if has(Group::Emissive) && row.emits && emitted.is_empty() {
                emitted = vec![[0.; 3]; pixels];
            }
            let chart = &atlas.charts[chart];
            let start = (chart.y + y) * atlas.width + chart.x;
            for x in 0..chart.width {
                let (texel, offset) = (start + x, (start + x) * 4);
                if has(Group::Color) {
                    color[offset..offset + 4].copy_from_slice(&row.color[x * 4..x * 4 + 4]);
                }
                if has(Group::Normal) {
                    normal[offset..offset + 4].copy_from_slice(&row.normal[x * 4..x * 4 + 4]);
                }
                if has(Group::Properties) {
                    properties[offset..offset + 4].copy_from_slice(&[
                        255,
                        row.roughness[x],
                        row.metalness[x],
                        255,
                    ]);
                }
                if has(Group::Emissive) && row.emits {
                    emitted[texel] = row.emitted[x];
                }
                if has(Group::Live) {
                    for (values, samples) in live_values.iter_mut().zip(&row.live) {
                        values[texel] = samples[x];
                    }
                }
            }
            if has(Group::Emissive) {
                emission_strength = emission_strength.max(row.strength);
                emits |= row.emits;
            }
            if has(Group::Color) {
                blend |= row.blend;
            }
        }
    }
    let mut encodings: Vec<(Group, Encoding<'_>)> = Vec::new();
    if wanted.contains(Group::Inspection) {
        encodings.push((
            Group::Inspection,
            Box::new(move |stopped| {
                let image = encode_map(atlas, color, compact, stopped)
                    .map_err(|message| failure_in(program, message))?;
                Ok(BakedReceiver::Color { image, blend })
            }),
        ));
    } else {
        for group in wanted.iter() {
            let encoding: Encoding<'_> = match group {
                Group::Color => {
                    let rgba = std::mem::take(&mut color);
                    Box::new(move |stopped| {
                        let image = encode_map(atlas, rgba, compact, stopped)
                            .map_err(|message| failure_in(program, message))?;
                        Ok(BakedReceiver::Color { image, blend })
                    })
                }
                Group::Normal => {
                    let rgba = std::mem::take(&mut normal);
                    Box::new(move |stopped| {
                        Ok(BakedReceiver::Normal(
                            encode_map(atlas, rgba, compact, stopped)
                                .map_err(|message| failure_in(program, message))?,
                        ))
                    })
                }
                Group::Properties => {
                    let rgba = std::mem::take(&mut properties);
                    Box::new(move |stopped| {
                        Ok(BakedReceiver::Properties(
                            encode_map(atlas, rgba, compact, stopped)
                                .map_err(|message| failure_in(program, message))?,
                        ))
                    })
                }
                Group::Emissive => {
                    let emitted = std::mem::take(&mut emitted);
                    Box::new(move |stopped| {
                        if !emits {
                            return Ok(BakedReceiver::Emissive(None));
                        }
                        let rgba = emitted
                            .into_iter()
                            .flat_map(|rgb| {
                                [
                                    srgb(rgb[0] / emission_strength),
                                    srgb(rgb[1] / emission_strength),
                                    srgb(rgb[2] / emission_strength),
                                    255,
                                ]
                            })
                            .collect();
                        let image = encode_map(atlas, rgba, compact, stopped)
                            .map_err(|message| failure_in(program, message))?;
                        Ok(BakedReceiver::Emissive(Some((emission_strength, image))))
                    })
                }
                Group::Live => {
                    let live_values = std::mem::take(&mut live_values);
                    let live_fields = live_fields.clone();
                    Box::new(move |stopped| {
                        let mut images = Vec::new();
                        let operations = live_operations(
                            program,
                            &live_fields,
                            &live_values,
                            pixels,
                            &mut |rgba| {
                                let image = encode_map(atlas, rgba, compact, stopped)
                                    .map_err(|message| failure_in(program, message))?;
                                let id = image.id.clone();
                                images.push(image);
                                Ok(id)
                            },
                        )?;
                        Ok(BakedReceiver::Live { images, operations })
                    })
                }
                Group::Inspection => unreachable!("handled above"),
            };
            encodings.push((group, encoding));
        }
    }
    encode_maps(encodings, threads, cancelled)
}

/// Named fields evaluated over a receiver's atlas: the data a GPU preview binds as textures for the
/// parts of a program only the CPU can evaluate (see `Program::shader_plan`).
#[derive(Clone, Debug)]
pub struct FieldBake {
    pub width: usize,
    pub height: usize,
    /// One entry per requested field, in order: `width * height` texels of four `f32` each (x, y,
    /// z, w; a scalar or bool is in x), rows from the atlas's upper edge. Texels no chart covers are zero.
    pub fields: Vec<Vec<f32>>,
}

impl Session {
    /// Evaluates `names` (fields of the request's program) at every texel of the request's
    /// receiver atlas, with the request's parameters, seed, edges and gravity. The atlas, its
    /// samples and the field values are the ones a full bake uses, so a field baked here is the
    /// value the CPU bake would have computed at the same texel, at 32-bit precision. A request
    /// of several receivers is refused: a texture belongs to one atlas.
    pub fn bake_fields(
        &mut self,
        request: &Request,
        revision: u64,
        names: &[String],
        assets: &dyn Fn(&str) -> Result<Arc<[u8]>, String>,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<FieldBake, Vec<Diagnostic>> {
        let program = self.program(request)?;
        self.begin_authoring(revision, &request.entry)?;
        let fail = |message: String| vec![Diagnostic::message(&request.entry, message)];
        if request.receivers.len() > 1 {
            return Err(fail("A field bake covers one receiver.".into()));
        }
        let receiver = request
            .receivers
            .first()
            .cloned()
            .unwrap_or_else(|| receiver::swatch(request.domain));
        let mut images = BTreeMap::new();
        for name in program
            .metadata()
            .required_inputs
            .iter()
            .filter_map(|input| input.strip_prefix("image:"))
        {
            let bytes = assets(name).map_err(|e| fail(format!("{name}: {e}")))?;
            let image = image::decode(&bytes).map_err(&fail)?;
            images.insert(
                name.to_owned(),
                Arc::new(program::Image {
                    width: image.width,
                    height: image.height,
                    rgba: image.rgba,
                }),
            );
        }
        let inputs = Inputs {
            parameters: request.parameters.clone(),
            seed: request.seed,
            images,
            edges: request.edges.clone(),
            features: Vec::new(),
            mips: self.mips.clone(),
        };
        let atlas = Atlas::new(
            receiver,
            request.resolution,
            request.mapping,
            request.domain,
            request.gravity,
            request.profile.max_resolution(),
            request.texel_density,
        )
        .map_err(&fail)?;
        let features = receiver_features(&program, &atlas);
        let wanted: Vec<&str> = names.iter().map(String::as_str).collect();
        let selection = program.select_fields(&wanted).map_err(&fail)?;
        let mut evaluator = program
            .evaluator_with_features(&inputs, &features)
            .map_err(|e| vec![Diagnostic::from(e)])?;
        let slots = selection.len();
        let mut fields = vec![vec![0f32; atlas.width * atlas.height * 4]; slots];
        let mut values = vec![FieldValue::Scalar(0.); slots];
        for chart in &atlas.charts {
            for y in 0..chart.height {
                if cancelled() {
                    return Err(fail("Field bake cancelled.".into()));
                }
                evaluator.replenish(64_000_000);
                let mut before: Option<(usize, u64, u64)> = None;
                for x in 0..chart.width {
                    let (triangle, u, v) = atlas.texel(chart, x, y);
                    let at = ((chart.y + y) * atlas.width + chart.x + x) * 4;
                    let key = (triangle, u.to_bits(), v.to_bits());
                    // A texel that samples its left neighbor's point repeats its values.
                    if before == Some(key) && x > 0 {
                        for field in fields.iter_mut() {
                            field.copy_within(at - 4..at, at);
                        }
                        continue;
                    }
                    before = Some(key);
                    let sample = atlas.sample(triangle, u, v).map_err(&fail)?;
                    evaluator
                        .evaluate_outputs(
                            &sample,
                            program::OutputSet::NONE,
                            &selection,
                            &mut values,
                        )
                        .map_err(|e| vec![Diagnostic::from(e)])?;
                    for (field, value) in fields.iter_mut().zip(&values) {
                        let channels: [f64; 4] = match value {
                            FieldValue::Scalar(a) => [*a, 0., 0., 0.],
                            FieldValue::Bool(a) => [f64::from(u8::from(*a)), 0., 0., 0.],
                            FieldValue::Vec2([a, b]) => [*a, *b, 0., 0.],
                            FieldValue::Vec3([a, b, c]) => [*a, *b, *c, 0.],
                        };
                        for (slot, channel) in channels.iter().enumerate() {
                            field[at + slot] = *channel as f32;
                        }
                    }
                }
            }
        }
        Ok(FieldBake {
            width: atlas.width,
            height: atlas.height,
            fields,
        })
    }
}

fn failure_in(program: &Program, message: String) -> Diagnostic {
    Diagnostic::message(&program.metadata().entry, message)
}

/// Runs the encodings, one after another on the calling thread, or at once on native threads when
/// the profile has them: maps are independent, so the bytes are the same either way. A cancelled
/// build stops the running encoders.
fn encode_maps(
    encodings: Vec<(Group, Encoding<'_>)>,
    threads: usize,
    cancelled: &dyn Fn() -> bool,
) -> Result<Vec<(Group, BakedReceiver)>, Diagnostic> {
    if threads <= 1 || encodings.len() <= 1 {
        let mut made = Vec::with_capacity(encodings.len());
        for (group, encoding) in encodings {
            made.push((group, encoding(cancelled)?));
        }
        return Ok(made);
    }
    let stop = std::sync::atomic::AtomicBool::new(false);
    let stopped = || stop.load(std::sync::atomic::Ordering::Relaxed);
    std::thread::scope(|scope| {
        let handles: Vec<_> = encodings
            .into_iter()
            .map(|(group, encoding)| {
                let stopped = &stopped;
                (group, scope.spawn(move || encoding(stopped)))
            })
            .collect();
        while !handles.iter().all(|(_, handle)| handle.is_finished()) {
            if cancelled() {
                stop.store(true, std::sync::atomic::Ordering::Relaxed);
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        let mut made = Vec::with_capacity(handles.len());
        let mut failed = None;
        for (group, handle) in handles {
            match handle.join().expect("map encoding does not panic") {
                Ok(baked) => made.push((group, baked)),
                Err(error) => {
                    failed.get_or_insert(error);
                }
            }
        }
        match failed {
            Some(error) => Err(error),
            None => Ok(made),
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::authoring::Request;
    use serde_json::json;
    use std::cell::Cell;

    const SOURCE: &str = "material 1;
param tint: scalar = 0.5 range(0, 1);
param shine: scalar = 0.2 range(0, 1);
param bump: scalar = 0.003 range(0, 0.01);
param glow: scalar = 0 range(0, 4);
let grain = noise(surface * 12, seed(\"g\"));
output base_color = vec3(tint * (0.4 + 0.6 * grain));
output roughness = shine + 0.3 * grain;
output metalness = 0;
output height = bump * grain;
output emissive = vec3(glow * 0.2);";

    fn request(parameters: serde_json::Value, seed: u32, resolution: usize) -> Request {
        Request {
            entry: "main.ms".into(),
            source: SOURCE.into(),
            parameters: serde_json::from_value(parameters).unwrap(),
            seed,
            resolution,
            ..Request::default()
        }
    }

    fn no_assets(name: &str) -> Result<Arc<[u8]>, String> {
        Err(format!("{name} is unavailable"))
    }

    fn groups(list: &[Group]) -> Groups {
        list.iter()
            .fold(Groups::NONE, |set, group| set.with(*group))
    }

    /// The build's output and the groups each bake it ran evaluated.
    fn build(session: &mut Session, revision: u64, request: &Request) -> (Output, Vec<Groups>) {
        BAKED.with(|baked| baked.borrow_mut().clear());
        let output = session
            .build(request, revision, &no_assets, &|| false)
            .unwrap_or_else(|e| panic!("{e:?}"));
        (
            output,
            BAKED.with(|baked| std::mem::take(&mut *baked.borrow_mut())),
        )
    }

    /// A build in a session that has never seen the request: every map is baked.
    fn fresh(request: &Request) -> Output {
        build(&mut Session::default(), 1, request).0
    }

    fn assert_same(a: &Output, b: &Output, what: &str) {
        assert_eq!(
            a.model.materials.keys().collect::<Vec<_>>(),
            b.model.materials.keys().collect::<Vec<_>>(),
            "{what}: material identities hash every map"
        );
        assert_eq!(a.model.images.len(), b.model.images.len(), "{what}");
        for (id, image) in &a.model.images {
            assert_eq!(&*image.bytes, &*b.model.images[id].bytes, "{what}: {id}");
        }
        assert_eq!(a.metadata["atlases"], b.metadata["atlases"], "{what}");
    }

    #[test]
    fn an_edit_bakes_only_the_maps_it_can_change_and_equals_a_fresh_bake() {
        let mut session = Session::default();
        let base = request(json!({}), 0, 32);
        let (first, baked) = build(&mut session, 1, &base);
        assert_eq!(
            baked,
            [Groups::MAPS],
            "a first bake evaluates every map once"
        );
        assert_same(&first, &fresh(&base), "first bake");
        use Group::*;
        let edits = [
            (json!({ "tint": 0.8 }), 0, vec![Color]),
            (json!({ "shine": 0.6 }), 0, vec![Properties]),
            (json!({ "bump": 0.006 }), 0, vec![Normal]),
            (json!({ "glow": 1.5 }), 0, vec![Emissive]),
            // Everything the program seeds follows the seed; emission does not.
            (json!({}), 5, vec![Color, Normal, Properties]),
            (
                json!({ "tint": 0.2, "bump": 0.001 }),
                0,
                vec![Color, Normal],
            ),
        ];
        let mut revision = 1;
        for (parameters, seed, expected) in edits {
            revision += 1;
            let edited = request(parameters.clone(), seed, 32);
            let (output, baked) = build(&mut session, revision, &edited);
            assert_eq!(baked, [groups(&expected)], "{parameters} seed {seed}");
            assert_same(
                &output,
                &fresh(&edited),
                &format!("{parameters} seed {seed}"),
            );
            // The same request again, and the unedited one, are served from retained maps.
            revision += 1;
            let (again, baked) = build(&mut session, revision, &edited);
            assert!(
                baked.is_empty(),
                "an equal request bakes nothing: {baked:?}"
            );
            assert_same(&again, &output, "repeat");
            revision += 1;
            let (back, baked) = build(&mut session, revision, &base);
            assert!(
                baked.is_empty(),
                "returning to retained maps bakes nothing: {baked:?}"
            );
            assert_same(&back, &first, "back to the base");
        }
    }

    #[test]
    fn explicitly_naming_a_default_keeps_every_map() {
        let mut session = Session::default();
        let (first, _) = build(&mut session, 1, &request(json!({}), 0, 32));
        let (named, baked) = build(
            &mut session,
            2,
            &request(json!({ "tint": 0.5, "shine": 0.2 }), 0, 32),
        );
        assert!(baked.is_empty(), "{baked:?}");
        assert_same(&named, &first, "defaults named explicitly");
    }

    #[test]
    fn failures_and_cancellation_publish_nothing_and_later_builds_recover() {
        let mut session = Session::default();
        let base = request(json!({}), 0, 32);
        let (first, _) = build(&mut session, 1, &base);
        let mut revision = 1;
        // Invalid requests fail with their diagnostic, even though every map they would need is
        // retained for some other request.
        for (parameters, fragment) in [
            (json!({ "tint": 5 }), "tint"),
            (json!({ "nope": 1 }), "nope"),
        ] {
            revision += 1;
            let errors = session
                .build(&request(parameters, 0, 32), revision, &no_assets, &|| false)
                .err()
                .expect("an invalid parameter fails");
            assert_eq!(errors[0].code, "MS_PARAMETER", "{errors:?}");
            assert!(errors[0].message.contains(fragment), "{errors:?}");
        }
        // Cancelling at every poll in turn never leaves a partial result behind.
        let edited = request(json!({ "tint": 0.9, "bump": 0.008 }), 3, 32);
        let expected = fresh(&edited);
        let mut cancelled_builds = 0;
        for limit in 0..40 {
            revision += 1;
            let polls = Cell::new(0);
            let outcome = session.build(&edited, revision, &no_assets, &|| {
                polls.set(polls.get() + 1);
                polls.get() > limit
            });
            match outcome {
                Ok(output) => assert_same(&output, &expected, "finished before the cancellation"),
                Err(errors) => {
                    cancelled_builds += 1;
                    assert!(
                        errors[0].message.contains("cancel")
                            || errors[0].message.contains("superseded"),
                        "{errors:?}"
                    );
                }
            }
        }
        assert!(cancelled_builds > 0, "some polls must cancel the build");
        revision += 1;
        let (recovered, _) = build(&mut session, revision, &edited);
        assert_same(&recovered, &expected, "after cancellations");
        revision += 1;
        let (unchanged, baked) = build(&mut session, revision, &base);
        assert!(
            baked.is_empty(),
            "the base maps survived every failure: {baked:?}"
        );
        assert_same(&unchanged, &first, "base after failures");
    }

    #[test]
    fn a_distributed_edit_asks_other_sessions_only_for_the_maps_it_changes() {
        let mut session = Session::default();
        session.distribute_bakes(true);
        let evaluate = |session: &mut Session| -> Vec<usize> {
            let pending = session.take_pending_bake().expect("a pending bake");
            let sizes = (0..3)
                .map(|index| {
                    let share = Session::default()
                        .bake_share(&pending, index, 3, &no_assets, &|| false)
                        .unwrap_or_else(|e| panic!("{e:?}"));
                    session.provide_bake_share(&share).unwrap();
                    share.len()
                })
                .collect();
            assert!(session.take_pending_bake().is_none());
            sizes
        };
        let base = request(json!({}), 0, 96);
        let stopped = session
            .build(&base, 1, &no_assets, &|| false)
            .err()
            .unwrap();
        assert_eq!(stopped[0].code, "BAKE_PENDING");
        let pending: PendingBake =
            serde_json::from_str(&serde_json::to_string(&session.distribution.pending[0]).unwrap())
                .unwrap();
        assert_eq!(pending.groups, [Groups::MAPS.0]);
        let full: usize = evaluate(&mut session).iter().sum();
        let (distributed, baked) = build(&mut session, 2, &base);
        assert_eq!(baked, [Groups::MAPS]);
        assert_same(&distributed, &fresh(&base), "distributed first bake");
        // A color-only edit reports only the color map, and the rows that come back carry only it.
        let edited = request(json!({ "tint": 0.9 }), 0, 96);
        let stopped = session
            .build(&edited, 3, &no_assets, &|| false)
            .err()
            .unwrap();
        assert_eq!(stopped[0].code, "BAKE_PENDING");
        assert_eq!(
            session.distribution.pending[0].groups,
            [Groups::of(Group::Color).0]
        );
        let partial: usize = evaluate(&mut session).iter().sum();
        assert!(
            partial * 2 < full,
            "color rows are a fraction of all rows: {partial} vs {full}"
        );
        let (output, baked) = build(&mut session, 4, &edited);
        assert_eq!(baked, [groups(&[Group::Color])]);
        assert_same(&output, &fresh(&edited), "distributed color edit");
    }
}

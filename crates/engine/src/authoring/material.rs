use super::{
    Artifact, Diagnostic, Output, Request, Session,
    receiver::{self, Atlas, Chart},
};
use crate::{
    materialspec::program::{self, FieldValue, Inputs, Program},
    runtime::{
        dependencies::{Dependency, Node, Plan},
        store::{ContentKey, Retention},
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
///
/// Errors are deterministic. A request whose parameters are invalid, or whose maps' fingerprints
/// cannot be formed, matches no retained map, so building it bakes (and fails with the parameter
/// diagnostic) whatever is retained. A map that fails to bake is never retained, and a retained
/// map was baked from the same inputs its outputs read, so no error can hide in it; when several
/// maps being baked would fail, the one evaluation pass reports its first failing sample (texel
/// order) and, within it, its first failing output (alphabetical order), exactly as a bake of
/// those outputs alone would. The limits on a sample's steps and a row's operations apply to the
/// outputs a build evaluates: a rebake that evaluates fewer outputs than a full bake can finish
/// where evaluating them all would exceed a limit.
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

/// Retention of kept field planes: a partition of its own, bounded apart from the build graph's
/// (which is the Session's one 128 MiB store), and dropped by `Session::clear` with the rest.
const FIELD_PLANES: Retention = Retention {
    name: "material-fields",
    bytes: 48 * 1024 * 1024,
    entries: 64,
};
/// Retention of field bakes (see [FieldBake]), by field: a GPU preview asks for the same fields
/// after every edit, and most edits cannot change them.
const FIELD_BAKES: Retention = Retention {
    name: "material-field-bakes",
    bytes: 32 * 1024 * 1024,
    entries: 64,
};
/// Fields cheaper than this are recomputed rather than kept.
const MIN_FIELD_STEPS: usize = 24;
/// Most a single bake adds to the plane partition (it evicts the least recently used beyond it).
const FIELD_CAPTURE_BYTES: usize = 24 * 1024 * 1024;

/// The fields one receiver's bake reads from earlier bakes and keeps for later ones.
#[derive(Default)]
struct FieldPlan {
    injected: Vec<Arc<BakedReceiver>>,
    /// Fields with no plane yet, costliest first: field index, components of its value (0 for a
    /// boolean) and the key its plane is kept under. A bake keeps as many as the run's budget allows.
    capture: Vec<(usize, usize, ContentKey)>,
    /// Kept planes that lack texels some earlier sample did not fill, with their keys: a bake that
    /// computes the field there completes a copy, which replaces the plane.
    completing: Vec<(ContentKey, Arc<BakedReceiver>)>,
}

/// The key of a field's plane: the layout whose samples it covers, the program and decoded images,
/// and the effective values of only the parameters, seed and edges the field reads.
fn field_plane_key(
    program: &Program,
    dependencies: &program::Dependencies,
    request: &Request,
    [layout, program_key, assets]: [&ContentKey; 3],
    field: usize,
) -> ContentKey {
    let fingerprint = program
        .input_fingerprint(
            &Inputs {
                parameters: request.parameters.clone(),
                seed: request.seed,
                edges: request.edges.clone(),
                ..Inputs::default()
            },
            dependencies,
        )
        .unwrap_or_else(|error| error.to_string().into_bytes());
    let mut digest = Sha256::new();
    for part in [
        b"material-field-v1".as_slice(),
        layout,
        program_key,
        assets,
        &(field as u32).to_le_bytes(),
        &fingerprint,
    ] {
        digest.update((part.len() as u64).to_le_bytes());
        digest.update(part);
    }
    digest.finalize().into()
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
    /// The values of one field at every texel, kept for later bakes (see [FieldPlan]).
    Plane(program::FieldPlane),
    /// One field of a field bake (see [FieldBake]): four `f32` per texel of the atlas.
    Texels(Vec<f32>),
}

impl BakedReceiver {
    fn images(&self) -> Vec<&BakedImage> {
        match self {
            Self::Color { image, .. } | Self::Normal(image) | Self::Properties(image) => {
                vec![image]
            }
            Self::Emissive(emission) => emission.iter().map(|(_, image)| image).collect(),
            Self::Live { images, .. } => images.iter().collect(),
            Self::Plane(_) | Self::Texels(_) => Vec::new(),
        }
    }

    /// Bytes this result retains, for artifact admission.
    fn bytes(&self) -> usize {
        match self {
            Self::Plane(plane) => return plane.bytes(),
            Self::Texels(texels) => return texels.len() * 4 + 64,
            _ => {}
        }
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
                BakedReceiver::Color { .. }
                | BakedReceiver::Plane(_)
                | BakedReceiver::Texels(_) => return Err(missing()),
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

/// Planned nodes, and the encoded images they read by name.
type SourceNodes = (Vec<Node>, BTreeMap<String, Arc<[u8]>>);

/// The nodes of a request's source modules, program, images and decoded images ("assets": every
/// image without parameters, since a map or field depends on the images, not on parameters it
/// never reads), with the encoded images by name. Every build and field bake of a request plans
/// these the same way, so they share the compiled program and the decoded images.
fn source_nodes(
    compiled: &Program,
    request: &Request,
    assets: &dyn Fn(&str) -> Result<Arc<[u8]>, String>,
) -> Result<SourceNodes, String> {
    let mut nodes = Vec::new();
    let mut sources = request.modules.clone();
    sources.insert(request.entry.clone(), request.source.clone());
    let mut source_deps = Vec::new();
    for name in std::iter::once(&compiled.metadata().entry).chain(&compiled.metadata().dependencies)
    {
        let id = format!("source/{name}");
        let text = sources
            .get(name)
            .ok_or_else(|| format!("Missing source module {name}."))?;
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
        let bytes = assets(name).map_err(|e| format!("{name}: {e}"))?;
        image_bytes.insert(name.to_owned(), bytes);
    }
    for (name, bytes) in &image_bytes {
        nodes.push(node(
            format!("image/{name}"),
            "source-image-v2",
            image_digest(bytes).to_vec(),
            vec![],
        ));
    }
    nodes.push(node(
        "assets".into(),
        "material-assets-v1",
        Vec::new(),
        image_bytes
            .keys()
            .map(|name| dependency(&format!("image/{name}"), format!("image/{name}")))
            .collect(),
    ));
    Ok((nodes, image_bytes))
}

/// SHA-256 of an encoded image. Every build keys its image nodes by their bytes, and hosts hand
/// the same allocation to build after build, so the digest of an allocation is computed once and
/// found again by its address for as long as the host keeps it (an address is only compared while
/// its entry holds the allocation, so it cannot be another image's).
fn image_digest(bytes: &Arc<[u8]>) -> [u8; 32] {
    type Known = Vec<(std::sync::Weak<[u8]>, [u8; 32])>;
    static DIGESTS: std::sync::Mutex<Known> = std::sync::Mutex::new(Vec::new());
    const KEPT: usize = 64;
    let Ok(mut known) = DIGESTS.lock() else {
        return Sha256::digest(bytes).into();
    };
    // Entries of images their hosts dropped would otherwise keep those allocations.
    known.retain(|(owner, _)| owner.strong_count() > 0);
    let found = known
        .iter()
        .position(|(owner, _)| std::ptr::addr_eq(owner.as_ptr(), Arc::as_ptr(bytes)));
    if let Some(at) = found {
        // Most recently used last.
        let entry = known.remove(at);
        let digest = entry.1;
        known.push(entry);
        return digest;
    }
    let digest: [u8; 32] = Sha256::digest(bytes).into();
    if known.len() == KEPT {
        known.remove(0);
    }
    known.push((Arc::downgrade(bytes), digest));
    digest
}

/// The node of receiver `index`'s atlas: everything its layout and samples depend on, apart from
/// the names the host gives the receiver.
fn layout_node(index: usize, receiver: &super::Receiver, request: &Request) -> Node {
    let mut identity = receiver.clone();
    identity.id.clear();
    identity.object_id.clear();
    node(
        format!("layout/{index}"),
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
    )
}

/// An `image/` node's artifact. Decoding depends only on the encoded bytes, so parameter edits
/// reuse it.
fn decoded_image(bytes: &[u8]) -> Result<(Artifact, usize), String> {
    let image = image::decode(bytes)?;
    // Filtered sampling may add mip levels of about the decoded size.
    let retained = image.rgba.len() * 2;
    Ok((
        Artifact::Image(Arc::new(program::Image {
            width: image.width,
            height: image.height,
            rgba: image.rgba,
        })),
        retained,
    ))
}

/// The `assets` node's artifact: inputs holding only the decoded images.
fn decoded_assets(
    image_bytes: &BTreeMap<String, Arc<[u8]>>,
    dependencies: &BTreeMap<String, Arc<Artifact>>,
    mips: &program::MipCache,
) -> Result<(Artifact, usize), String> {
    let mut images = BTreeMap::new();
    let mut count = 0usize;
    for name in image_bytes.keys() {
        let Artifact::Image(image) = dependencies[&format!("image/{name}")].as_ref() else {
            unreachable!()
        };
        count += image.rgba.len();
        if count > 64 * 1024 * 1024 {
            return Err("Decoded material images exceed the authoring allowance.".into());
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

/// A `layout/` node's artifact: the receiver's atlas.
fn receiver_layout(
    receiver: &super::Receiver,
    request: &Request,
) -> Result<(Artifact, usize), String> {
    let atlas = Atlas::new(
        receiver.clone(),
        request.resolution,
        request.mapping,
        request.domain,
        request.gravity,
        request.profile.max_resolution(),
        request.texel_density,
    )?;
    let bytes = atlas.mesh.positions.len() * 88
        + atlas.mesh.indices.len() * 4
        + receiver.positions.len() * 8 * 3;
    Ok((Artifact::Receiver(Arc::new(atlas)), bytes))
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
            self.distribution.reported.clear();
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
            // Rows of a bake this session is not waiting for (a superseded edit, or one already
            // built) are dropped: no later build would ask for them.
            if !self.distribution.reported.contains_key(&key) {
                continue;
            }
            let provided = self.distribution.provided.entry(key).or_default();
            // Rows of another mask do not mix with these: they hold other data.
            if provided.slots.len() != total || provided.groups != groups {
                provided.groups = groups;
                provided.slots.clear();
                provided.slots.resize_with(total, || None);
                provided.bytes = 0;
            }
            for (ordinal, row) in rows {
                let slot = provided
                    .slots
                    .get_mut(ordinal)
                    .ok_or("A provided row is outside its bake.")?;
                provided.bytes += row.bytes();
                if let Some(old) = slot.replace(row) {
                    provided.bytes -= old.bytes();
                }
            }
            // Over the budget, the rows of the oldest other reports go first.
            while self.distribution.provided_bytes() > PROVIDED_BYTES
                && self.distribution.forget_oldest(Some(&key))
            {}
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
        let (mut nodes, image_bytes) = source_nodes(&compiled, request, assets).map_err(&fail)?;
        let mut input_deps = vec![dependency("program", "program")];
        for name in image_bytes.keys() {
            input_deps.push(dependency(
                &format!("image/{name}"),
                format!("image/{name}"),
            ));
        }
        nodes.push(node(
            "inputs".into(),
            "material-inputs-v1",
            json(&(&request.parameters, request.seed, &request.edges)),
            input_deps,
        ));
        let inspecting = request.inspection.is_some();
        let dependency_table = compiled.dependency_table();
        let live_fields = live_field_names(&compiled, request.inspection.as_deref());
        let compact = request.profile == super::Profile::Production;
        let mut targets = vec![];
        for (i, receiver) in receivers.iter().enumerate() {
            let layout = format!("layout/{i}");
            let bake = format!("bake/{i}");
            nodes.push(layout_node(i, receiver, request));
            nodes.push(node(
                bake.clone(),
                "material-receiver-v5",
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
                    "material-group-v2",
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
                for key in keys.iter().filter_map(|key| parse_key(key)) {
                    self.distribution.report(key);
                }
                self.distribution.pending.push(PendingBake {
                    request: request.clone(),
                    keys,
                    images: image_bytes.keys().cloned().collect(),
                    groups,
                });
                // A host that never takes them does not grow the list without bound.
                let excess = self.distribution.pending.len().saturating_sub(MAX_PENDING);
                self.distribution.pending.drain(..excess);
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
                // This build bakes it, from these rows or here: it is no longer waited for.
                self.distribution.reported.remove(key);
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
        // Fields that an earlier bake kept for the samples of these receivers, and the expensive
        // fields this bake keeps for the next one. Planes are keyed by the inputs the field reads,
        // so an edit that cannot change a field finds its values; only serial previews keep them.
        let mut field_plans: Vec<FieldPlan> = Vec::new();
        if share.is_none() && !inspecting && request.profile.threads() == 1 {
            let field_names: Vec<&str> = compiled
                .metadata()
                .fields
                .iter()
                .map(|field| field.name.as_str())
                .collect();
            let steps = compiled.field_steps();
            let program_key = plan.key("program").expect("planned program");
            let assets_key = plan.key("assets").expect("planned assets");
            for (i, wanted) in dirty.iter().enumerate() {
                let mut candidates = std::collections::BTreeSet::new();
                for group in wanted.iter() {
                    let dependencies = group_dependencies(&dependency_table, group, &live_fields);
                    candidates.extend(dependencies.fields.iter().copied());
                    if group == Group::Live {
                        candidates.extend(
                            live_fields
                                .iter()
                                .filter_map(|name| field_names.iter().position(|n| n == name)),
                        );
                    }
                }
                candidates.retain(|field| steps.get(*field).is_some_and(|s| *s >= MIN_FIELD_STEPS));
                let layout_key = plan.key(&format!("layout/{i}")).expect("planned layout");
                let mut chosen = FieldPlan::default();
                let mut missing = Vec::new();
                for field in candidates {
                    let key = field_plane_key(
                        &compiled,
                        &dependency_table.fields[field_names[field]],
                        request,
                        [&layout_key, &program_key, &assets_key],
                        field,
                    );
                    match self.runtime.lookup(FIELD_PLANES, &key) {
                        Some(found) => {
                            if let Artifact::Material(baked) = found.as_ref() {
                                if let BakedReceiver::Plane(plane) = baked.as_ref() {
                                    chosen.injected.push(baked.clone());
                                    if plane.incomplete() {
                                        chosen.completing.push((key, baked.clone()));
                                    }
                                }
                            }
                        }
                        None => missing.push((steps[field], field, key)),
                    }
                }
                // The costliest fields first; the bake takes as many as the run's budget allows
                // once it knows the atlas the planes cover.
                missing.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
                for (_, field, key) in missing {
                    let components = match compiled.metadata().fields[field].kind {
                        program::Type::Scalar => 1,
                        program::Type::Vec2 => 2,
                        program::Type::Vec3 => 3,
                        _ => 0,
                    };
                    chosen.capture.push((field, components, key));
                }
                field_plans.push(chosen);
            }
        }
        // What every receiver of this run may still add to the plane partition.
        let mut capture_budget = FIELD_CAPTURE_BYTES;
        let mut captured_planes: Vec<(ContentKey, program::FieldPlane, bool)> = Vec::new();
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
                        decoded_image(&image_bytes[&id["image/".len()..]])
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
                        receiver_layout(&receivers[index], request)
                    }
                    "assets" => decoded_assets(&image_bytes, dependencies, &mips),
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
                            field_plans.get(index),
                            &mut capture_budget,
                            stopped,
                        ) {
                            Ok((results, planes)) => {
                                captured_planes.extend(planes);
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
        // Only a finished build keeps what it computed: a failed or cancelled one never reaches here.
        // A plane that holds nothing (its rows were evaluated one sample at a time, or nothing read
        // the field) is not kept, so a later bake tries again; a completed copy replaces its plane
        // only when it gained texels.
        for (key, plane, completes) in captured_planes {
            if plane.stored() == 0 || (completes && plane.added() == 0) {
                continue;
            }
            let bytes = plane.bytes();
            self.runtime.retain(
                FIELD_PLANES,
                key,
                Artifact::Material(Arc::new(BakedReceiver::Plane(plane))),
                bytes,
                completes,
            );
        }
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
    /// Fields each field bake on this thread had to evaluate (the others were kept).
    static FIELDS_BAKED: std::cell::RefCell<Vec<usize>> = const { std::cell::RefCell::new(Vec::new()) };
    /// Field planes (read, kept) of each bake that evaluated rows on this thread.
    static PLANES: std::cell::RefCell<Vec<(usize, usize)>> = const { std::cell::RefCell::new(Vec::new()) };
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
    /// The bakes this session reported and has not built since, with when each was reported.
    /// Rows are accepted only for these, so nothing can be parked under a key no build will ask for.
    reported: BTreeMap<ContentKey, u64>,
    /// Counts reports, to tell the oldest.
    clock: u64,
}

/// Bakes reported and waiting at most; reporting more forgets the oldest and their rows.
const MAX_REPORTED: usize = 512;
/// Pending descriptions kept for a host that does not take them; more drop the oldest.
const MAX_PENDING: usize = 256;
/// Bytes of provided rows kept at most; more drop the rows of the oldest reported bakes. One 2048 px
/// bake's rows are about 40 MiB.
const PROVIDED_BYTES: usize = 256 * 1024 * 1024;

impl Distribution {
    /// Notes that `key` was reported now, forgetting the oldest reports beyond the bound.
    fn report(&mut self, key: ContentKey) {
        self.clock += 1;
        self.reported.insert(key, self.clock);
        while self.reported.len() > MAX_REPORTED {
            self.forget_oldest(None);
        }
    }

    /// Forgets the oldest reported bake other than `keep`, with its rows; false when there is none.
    fn forget_oldest(&mut self, keep: Option<&ContentKey>) -> bool {
        let oldest = self
            .reported
            .iter()
            .filter(|(key, _)| Some(*key) != keep)
            .min_by_key(|(_, stamp)| **stamp)
            .map(|(key, _)| *key);
        match oldest {
            Some(key) => {
                self.reported.remove(&key);
                self.provided.remove(&key);
                true
            }
            None => false,
        }
    }

    fn provided_bytes(&self) -> usize {
        self.provided.values().map(|rows| rows.bytes).sum()
    }
}

/// Rows other sessions evaluated for one bake, with the data they carry.
#[derive(Default)]
struct ProvidedRows {
    groups: Groups,
    slots: Vec<Option<Row>>,
    /// Bytes the rows received so far hold.
    bytes: usize,
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
        // A row takes at least 21 bytes, and a share is one of at most 64 equal runs of a bake's
        // rows, so neither count can ask for more memory than the bytes received account for.
        if total > 1 << 20
            || count > total
            || count > bytes.len() / 21
            || total > 64 * (count + 1) + 64
        {
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
            // Evaluated rows hold finite values only; anything else would reach the encoders.
            if !(strength.is_finite() && strength >= 1.) {
                return Err("Invalid bake share emission strength.".into());
            }
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
            if emitted
                .iter()
                .flatten()
                .any(|v: &f64| !(v.is_finite() && *v >= 0. && *v <= strength))
                || live.iter().flatten().any(|v| !v.is_finite())
            {
                return Err("Invalid bake share values.".into());
            }
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

impl Row {
    /// Bytes this row holds.
    fn bytes(&self) -> usize {
        self.color.len()
            + self.normal.len()
            + self.roughness.len()
            + self.metalness.len()
            + self.emitted.len() * 24
            + self
                .live
                .iter()
                .map(|values| values.len() * 8)
                .sum::<usize>()
            + 128
    }
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
/// texels have no such neighbor. `texel` gives the sample of a texel of the chart, as
/// [`Atlas::texel`] does.
fn neighbor_texel(
    texel: impl Fn(usize, usize) -> (usize, f64, f64),
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
            let (other, ou, ov) = texel(nx, ny);
            other == triangle && (ou - u).abs() <= 1e-9 && (ov - v).abs() <= 1e-9
        })
}

/// The program field a preview shows, to evaluate with the outputs; none for an output channel.
fn inspected_fields(program: &Program, inspect: Option<&str>) -> program::FieldSelection {
    inspect
        .and_then(|name| program.select_fields(&[name]).ok())
        .unwrap_or_default()
}

/// The height output at each sample, evaluated together where the program allows.
fn heights_of(
    evaluator: &mut program::Evaluator<'_>,
    samples: &[program::Sample],
) -> Result<Vec<f64>, Diagnostic> {
    if samples.is_empty() {
        return Ok(Vec::new());
    }
    let mut unused = Vec::new();
    Ok(evaluator
        .evaluate_row(
            samples,
            program::OutputSet::HEIGHT,
            &program::FieldSelection::default(),
            &mut unused,
        )
        .map_err(Diagnostic::from)?
        .into_iter()
        .map(|outputs| outputs.height)
        .collect())
}

/// Field planes a block of rows reads and fills (see [FieldPlan]); empty for bakes that keep none.
#[derive(Default)]
struct FieldAccess<'a> {
    injected: &'a [&'a program::FieldPlane],
    capture: &'a mut [program::FieldPlane],
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
    inspected: &program::FieldSelection,
    groups: Groups,
    fields: &mut FieldAccess<'_>,
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
    let mut inspected_values: Vec<FieldValue> = Vec::new();
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
        let mut row_texels: Vec<usize> = Vec::new();
        let mut sample_error = None;
        for x in 0..cell {
            if repeats[x].is_none() {
                let (triangle, u, v) = texels[x];
                match atlas.sample(triangle, u, v) {
                    Ok(sample) => {
                        row_samples.push(sample);
                        row_texels.push((chart.y + y) * atlas.width + chart.x + x);
                    }
                    Err(message) => {
                        sample_error = Some(message);
                        break;
                    }
                }
            }
        }
        // A preview computes its one field with every output; channel values are the same either way.
        let mut row_values = evaluator
            .evaluate_row_with_planes(
                &row_samples,
                &row_texels,
                outputs,
                if inspect.is_some() {
                    inspected
                } else {
                    live_fields
                },
                if inspect.is_some() {
                    &mut inspected_values
                } else {
                    &mut live
                },
                fields.injected,
                &mut *fields.capture,
            )
            .map_err(Diagnostic::from)?;
        if let (Some(name), 1) = (inspect, inspected.len()) {
            for (value, field) in row_values.iter_mut().zip(&inspected_values) {
                value.fields.insert(name.to_owned(), field.clone());
            }
        }
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
        // The heights one step along each leg that no adjacent texel already has, for the texels
        // that need new slopes, are evaluated together; the texels below take them in order.
        let mut direct = Vec::new();
        // The adjacent texel, if any, at each of those two points, for each such texel in order.
        let mut neighbors: Vec<[Option<[usize; 2]>; 2]> = Vec::new();
        // The sample of a texel of this chart: this block's rows already hold theirs.
        let below = row_number + 1 < rows.len() && rows[row_number + 1] == (chart_index, y + 1);
        let texel_of = |nx: usize, ny: usize| -> (usize, f64, f64) {
            let held = if ny == y {
                Some(row_centers)
            } else if ny + 1 == y && above(row_number) {
                Some(&centers[row_number - 1])
            } else if ny == y + 1 && below {
                Some(&centers[row_number + 1])
            } else {
                None
            };
            match held {
                Some(held) => (held[nx].triangle, held[nx].u, held[nx].v),
                None => atlas.texel(chart, nx, ny),
            }
        };
        if relief {
            for (x, center) in row_centers.iter().enumerate() {
                let (triangle, u, v) = (center.triangle, center.u, center.v);
                if (x > 0 && row_centers[x - 1].samples(triangle, u, v))
                    || (above(row_number) && centers[row_number - 1][x].samples(triangle, u, v))
                {
                    continue;
                }
                let [step_u, step_v] = atlas.steps(triangle);
                let mut found = [None; 2];
                for (leg, target) in [[u + step_u, v], [u, v + step_v]].into_iter().enumerate() {
                    found[leg] = neighbor_texel(texel_of, chart, [x, y], triangle, target);
                    if found[leg].is_none() {
                        direct.push(
                            atlas
                                .sample(triangle, target[0], target[1])
                                .map_err(&failure)?,
                        );
                    }
                }
                neighbors.push(found);
            }
        }
        let mut neighbors = neighbors.into_iter();
        let mut direct_heights = heights_of(evaluator, &direct)?.into_iter();
        let mut previous: Option<[f64; 3]> = None;
        let mut row_slopes: Vec<[f64; 2]> = Vec::with_capacity(row_centers.len());
        for (x, center) in row_centers.iter().enumerate() {
            let (triangle, u, v) = (center.triangle, center.u, center.v);
            let value = &center.value;
            let mut direction = [0.; 3];
            if relief {
                let [step_u, step_v] = atlas.steps(triangle);
                let mut height_at = |neighbor: Option<[usize; 2]>| -> Result<f64, Diagnostic> {
                    let Some([nx, ny]) = neighbor else {
                        return Ok(direct_heights.next().expect("a height for each new point"));
                    };
                    if !heights.contains_key(&(chart_index, ny)) {
                        // A neighbor row of another block: its center heights, as that block has them.
                        let mut row_heights: Vec<f64> = Vec::with_capacity(chart.width);
                        let mut before = None;
                        let mut points = Vec::new();
                        let mut fresh = Vec::new();
                        for other in 0..chart.width {
                            let (triangle, u, v) = atlas.texel(chart, other, ny);
                            let key = (triangle, u.to_bits(), v.to_bits());
                            let new = before != Some(key);
                            fresh.push(new);
                            if new {
                                points.push(atlas.sample(triangle, u, v).map_err(&failure)?);
                            }
                            before = Some(key);
                        }
                        let mut computed = heights_of(evaluator, &points)?.into_iter();
                        for (other, new) in fresh.into_iter().enumerate() {
                            let height = if new {
                                computed.next().expect("a height for each new point")
                            } else {
                                row_heights[other - 1]
                            };
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
                    let [along_u, along_v] = neighbors.next().expect("neighbors of each new texel");
                    [height_at(along_u)?, height_at(along_v)?]
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
    let inspected = inspected_fields(program, inspect);
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
            &inspected,
            groups,
            &mut FieldAccess::default(),
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
    field_plan: Option<&FieldPlan>,
    capture_budget: &mut usize,
    cancelled: &dyn Fn() -> bool,
) -> Result<
    (
        Vec<(Group, BakedReceiver)>,
        Vec<(ContentKey, program::FieldPlane, bool)>,
    ),
    Diagnostic,
> {
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
    let inspected = inspected_fields(program, inspect);
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
    // Planes of fields kept from earlier bakes are read, and planes of fields this bake computes are
    // filled, by the serial evaluator only.
    let injected_planes: Vec<&program::FieldPlane> = field_plan
        .map(|plan| {
            plan.injected
                .iter()
                .filter_map(|baked| match baked.as_ref() {
                    BakedReceiver::Plane(plane) => Some(plane),
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default();
    // Planes are sized by this atlas. As many are filled as the run's budget still allows, and
    // none when a plane could not be kept or the rows cannot be evaluated together (only that
    // path fills planes).
    let mut captured: Vec<program::FieldPlane> = Vec::new();
    let mut captured_keys: Vec<(ContentKey, bool)> = Vec::new();
    if let (Some(evaluator), Some(plan)) = (&serial, field_plan) {
        let plane_bytes = pixels * 32;
        if plane_bytes + 64 <= FIELD_PLANES.bytes && evaluator.rows_evaluate_together() {
            for (field, components, key) in &plan.capture {
                if plane_bytes > *capture_budget {
                    break;
                }
                *capture_budget -= plane_bytes;
                captured.push(program::FieldPlane::new(*field, *components, pixels));
                captured_keys.push((*key, false));
            }
            for (key, kept) in &plan.completing {
                let BakedReceiver::Plane(plane) = kept.as_ref() else {
                    continue;
                };
                if plane_bytes > *capture_budget {
                    break;
                }
                *capture_budget -= plane_bytes;
                captured.push(plane.completing());
                captured_keys.push((*key, true));
            }
        }
    }
    #[cfg(test)]
    PLANES.with(|planes| {
        planes
            .borrow_mut()
            .push((injected_planes.len(), captured.len()))
    });
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
                &inspected,
                data,
                &mut FieldAccess {
                    injected: &injected_planes,
                    capture: &mut captured,
                },
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
                        let inspected = &inspected;
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
                                inspected,
                                data,
                                &mut FieldAccess::default(),
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
    let planes = captured_keys
        .into_iter()
        .zip(captured)
        .map(|((key, completes), plane)| (key, plane, completes))
        .collect();
    Ok((encode_maps(encodings, threads, cancelled)?, planes))
}

/// Fields one field bake returns at most.
const MAX_BAKED_FIELDS: usize = 16;
/// Bytes one field bake returns at most (16 per texel and field).
const BAKED_FIELD_BYTES: usize = 128 * 1024 * 1024;

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
        self.begin_authoring(revision, &request.entry)?;
        let program = self.program(request)?;
        let fail = |message: String| vec![Diagnostic::message(&request.entry, message)];
        if names.len() > MAX_BAKED_FIELDS {
            return Err(fail(format!(
                "A field bake covers at most {MAX_BAKED_FIELDS} fields."
            )));
        }
        if request.receivers.len() > 1 {
            return Err(fail("A field bake covers one receiver.".into()));
        }
        let receiver = request
            .receivers
            .first()
            .cloned()
            .unwrap_or_else(|| receiver::swatch(request.domain));
        // The program, decoded images and atlas are the artifacts a build of this request
        // plans, under the same keys: whichever runs first decodes and lays out for both.
        let (mut nodes, image_bytes) = source_nodes(&program, request, assets).map_err(&fail)?;
        nodes.push(layout_node(0, &receiver, request));
        let plan = Plan::new(
            nodes,
            vec!["program".into(), "assets".into(), "layout/0".into()],
        )
        .map_err(|e| fail(e.to_string()))?;
        let mips = self.mips.clone();
        let complete = self
            .runtime
            .run(
                self.build_epoch(),
                &plan,
                cancelled,
                |node, dependencies, _| match node.id.as_str() {
                    "program" => Ok((Artifact::Program(program.clone()), program.retained_bytes())),
                    id if id.starts_with("image/") => {
                        decoded_image(&image_bytes[&id["image/".len()..]])
                    }
                    "assets" => decoded_assets(&image_bytes, dependencies, &mips),
                    "layout/0" => receiver_layout(&receiver, request),
                    _ => Ok((Artifact::Source, node.inputs.len())),
                },
            )
            .map_err(&fail)?;
        let Artifact::Inputs(decoded) = complete["assets"].as_ref() else {
            unreachable!()
        };
        let Artifact::Receiver(atlas) = complete["layout/0"].as_ref() else {
            unreachable!()
        };
        let inputs = Inputs {
            parameters: request.parameters.clone(),
            seed: request.seed,
            images: decoded.images.clone(),
            edges: request.edges.clone(),
            features: Vec::new(),
            mips: decoded.mips.clone(),
        };
        let wanted: Vec<&str> = names.iter().map(String::as_str).collect();
        program.select_fields(&wanted).map_err(&fail)?;
        // The request is validated whether or not its fields are kept, so it fails the same way.
        program
            .evaluator(&inputs)
            .map_err(|e| vec![Diagnostic::from(e)])?;
        let pixels = atlas.width * atlas.height;
        if wanted.len().saturating_mul(pixels * 16) > BAKED_FIELD_BYTES {
            return Err(fail(format!(
                "These fields over this atlas exceed the {} MiB a field bake may return.",
                BAKED_FIELD_BYTES >> 20
            )));
        }
        // A field's texels are kept under what the field reads: the atlas, the program, the
        // decoded images and the effective values of only its own parameters, seed and edges.
        let table = program.dependency_table();
        let keys: Vec<ContentKey> = wanted
            .iter()
            .map(|name| {
                let field = program
                    .metadata()
                    .fields
                    .iter()
                    .position(|field| field.name == *name)
                    .expect("a selected field");
                let plane = field_plane_key(
                    &program,
                    &table.fields[*name],
                    request,
                    ["layout/0", "program", "assets"]
                        .map(|id| plan.key(id).expect("a planned node"))
                        .each_ref(),
                    field,
                );
                let mut digest = Sha256::new();
                digest.update(b"material-field-bake-v1");
                digest.update(plane);
                digest.finalize().into()
            })
            .collect();
        let mut fields: Vec<Option<Vec<f32>>> = keys
            .iter()
            .map(|key| {
                let kept = self.runtime.lookup(FIELD_BAKES, key)?;
                match kept.as_ref() {
                    Artifact::Material(baked) => match baked.as_ref() {
                        BakedReceiver::Texels(texels) if texels.len() == pixels * 4 => {
                            Some(texels.clone())
                        }
                        _ => None,
                    },
                    _ => None,
                }
            })
            .collect();
        #[cfg(test)]
        FIELDS_BAKED.with(|baked| {
            baked
                .borrow_mut()
                .push(fields.iter().filter(|field| field.is_none()).count())
        });
        // The fields no earlier bake kept, each once however often it is named.
        let mut missing: Vec<&str> = Vec::new();
        for (name, field) in wanted.iter().zip(&fields) {
            if field.is_none() && !missing.contains(name) {
                missing.push(name);
            }
        }
        if !missing.is_empty() {
            let features = receiver_features(&program, atlas);
            let selection = program.select_fields(&missing).map_err(&fail)?;
            let mut evaluator = program
                .evaluator_with_features(&inputs, &features)
                .map_err(|e| vec![Diagnostic::from(e)])?;
            let slots = selection.len();
            let mut computed = vec![vec![0f32; pixels * 4]; slots];
            let mut values = Vec::new();
            for chart in &atlas.charts {
                for y in 0..chart.height {
                    if cancelled() {
                        return Err(fail("Field bake cancelled.".into()));
                    }
                    evaluator.replenish(64_000_000);
                    // A texel that samples its left neighbor's point repeats its values; the
                    // others are evaluated together, as a bake's rows are.
                    let mut before: Option<(usize, u64, u64)> = None;
                    let mut fresh = Vec::with_capacity(chart.width);
                    let mut samples = Vec::new();
                    let mut sample_error = None;
                    for x in 0..chart.width {
                        let (triangle, u, v) = atlas.texel(chart, x, y);
                        let key = (triangle, u.to_bits(), v.to_bits());
                        let new = before != Some(key);
                        before = Some(key);
                        fresh.push(new);
                        if new {
                            match atlas.sample(triangle, u, v) {
                                Ok(sample) => samples.push(sample),
                                Err(message) => {
                                    sample_error = Some(message);
                                    break;
                                }
                            }
                        }
                    }
                    // Samples before one the atlas cannot place fail first, as in texel order.
                    evaluator
                        .evaluate_row(&samples, program::OutputSet::NONE, &selection, &mut values)
                        .map_err(|e| vec![Diagnostic::from(e)])?;
                    if let Some(message) = sample_error {
                        return Err(fail(message));
                    }
                    let mut next = 0;
                    for (x, new) in fresh.into_iter().enumerate() {
                        let at = ((chart.y + y) * atlas.width + chart.x + x) * 4;
                        if !new {
                            for field in computed.iter_mut() {
                                field.copy_within(at - 4..at, at);
                            }
                            continue;
                        }
                        let row_values = &values[next * slots..(next + 1) * slots];
                        next += 1;
                        for (field, value) in computed.iter_mut().zip(row_values) {
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
            // Only a finished bake keeps what it computed.
            for (name, texels) in missing.iter().zip(computed) {
                for ((wanted, key), field) in wanted.iter().zip(&keys).zip(fields.iter_mut()) {
                    if wanted == name && field.is_none() {
                        *field = Some(texels.clone());
                        self.runtime.retain(
                            FIELD_BAKES,
                            *key,
                            Artifact::Material(Arc::new(BakedReceiver::Texels(texels.clone()))),
                            texels.len() * 4 + 64,
                            false,
                        );
                    }
                }
            }
        }
        let fields: Vec<Vec<f32>> = fields
            .into_iter()
            .map(|field| field.expect("every field kept or computed"))
            .collect();
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

    const HEAVY: &str = "material 1;
param tint: scalar = 0.5 range(0, 1);
param shine: scalar = 0.2 range(0, 1);
param zoom: scalar = 12 range(1, 40);
let heavy = fbm(surface * zoom, seed(\"g\"), 3) + noise(surface * 5, seed(\"h\")) * 0.5 + sin(surface.x * 9) * cos(surface.y * 7) * 0.1;
let shade = clamp(heavy * tint, 0, 1);
output base_color = vec3(shade, shade * 0.5, 0.2);
output roughness = clamp(heavy * shine, 0, 1);
output metalness = 0;";

    fn heavy_request(parameters: serde_json::Value, seed: u32) -> Request {
        Request {
            source: HEAVY.into(),
            ..request(parameters, seed, 48)
        }
    }

    /// The (read, kept) planes of the bakes a build ran.
    fn planes_of(
        session: &mut Session,
        revision: u64,
        request: &Request,
    ) -> (Output, Vec<(usize, usize)>) {
        PLANES.with(|planes| planes.borrow_mut().clear());
        let (output, _) = build(session, revision, request);
        (
            output,
            PLANES.with(|planes| std::mem::take(&mut *planes.borrow_mut())),
        )
    }

    #[test]
    fn an_edit_reads_the_expensive_fields_it_cannot_change_and_equals_a_fresh_bake() {
        let mut session = Session::default();
        let base = heavy_request(json!({}), 0);
        let (first, planes) = planes_of(&mut session, 1, &base);
        assert_eq!(
            planes,
            [(0, 1)],
            "the costly field is kept by the first bake"
        );
        assert_same(&first, &fresh(&base), "first bake");
        // The field reads only the seed and `zoom`: edits of other parameters read its plane, and
        // an edit of `zoom` or the seed computes and keeps it again.
        let edits = [
            (json!({ "tint": 0.9 }), 0, (1, 0)),
            (json!({ "shine": 0.7 }), 0, (1, 0)),
            (json!({ "tint": 0.3, "shine": 0.4 }), 0, (1, 0)),
            (json!({ "zoom": 20 }), 0, (0, 1)),
            (json!({}), 4, (0, 1)),
        ];
        let mut revision = 1;
        for (parameters, seed, expected) in edits {
            revision += 1;
            let edited = heavy_request(parameters.clone(), seed);
            let (output, planes) = planes_of(&mut session, revision, &edited);
            assert_eq!(planes, [expected], "{parameters} seed {seed}");
            assert_same(
                &output,
                &fresh(&edited),
                &format!("{parameters} seed {seed}"),
            );
        }
        // Cancelling at every poll never publishes a partial plane: later builds still equal fresh ones.
        let target = heavy_request(json!({ "tint": 0.65, "zoom": 17 }), 2);
        let expected = fresh(&target);
        for limit in 0..30 {
            revision += 1;
            let polls = Cell::new(0);
            let _ = session.build(&target, revision, &no_assets, &|| {
                polls.set(polls.get() + 1);
                polls.get() > limit
            });
        }
        revision += 1;
        let (recovered, _) = planes_of(&mut session, revision, &target);
        assert_same(&recovered, &expected, "after cancellations");
        revision += 1;
        let (edited, planes) = planes_of(
            &mut session,
            revision,
            &heavy_request(json!({ "tint": 0.1, "zoom": 17 }), 2),
        );
        assert_eq!(planes, [(1, 0)], "the plane of the finished build is read");
        assert_same(
            &edited,
            &fresh(&heavy_request(json!({ "tint": 0.1, "zoom": 17 }), 2)),
            "after recovery",
        );
        // A cleared session keeps nothing, planes included.
        session.clear();
        revision += 1;
        let (_, planes) = planes_of(&mut session, revision, &target);
        assert_eq!(planes, [(0, 1)]);
    }

    #[test]
    fn a_partial_rebake_is_held_to_the_limits_of_the_outputs_it_evaluates() {
        // Each of color and roughness fits one sample's 16,384 steps; evaluating both would not.
        const SOURCE: &str = "material 1;
param a: bool = false;
param b: bool = true;
fn l1(x: scalar) -> scalar = x + x + x + x + x + x + x + x + x + x;
fn l2(x: scalar) -> scalar = l1(x) + l1(x) + l1(x) + l1(x) + l1(x) + l1(x) + l1(x) + l1(x) + l1(x) + l1(x);
fn l3(x: scalar) -> scalar = l2(x) + l2(x) + l2(x) + l2(x) + l2(x) + l2(x) + l2(x) + l2(x) + l2(x) + l2(x);
fn heavy(x: scalar) -> scalar = l3(x) + l3(x) + l3(x) + l3(x);
output base_color = vec3(select(a, heavy(surface.x) * 0.00001, 0.25));
output roughness = select(b, heavy(surface.y) * 0.00001, 0.5);
output metalness = 0;";
        let with = |parameters: serde_json::Value| Request {
            source: SOURCE.into(),
            ..request(parameters, 0, 32)
        };
        let mut session = Session::default();
        build(&mut session, 1, &with(json!({ "a": false, "b": true })));
        // Only color is dirty: roughness (which reads `b` alone) is kept, so the rebake evaluates
        // only color and succeeds.
        let both = with(json!({ "a": true, "b": true }));
        let (output, baked) = build(&mut session, 2, &both);
        assert_eq!(baked, [groups(&[Group::Color])]);
        assert!(!output.model.materials.is_empty());
        // The same request on a session that holds neither map evaluates both outputs in each
        // sample, which exceeds the step budget: limits apply to what a build evaluates.
        let errors = Session::default()
            .build(&both, 1, &no_assets, &|| false)
            .err()
            .expect("evaluating both outputs exceeds the step budget");
        assert_eq!(errors[0].code, "MS_LIMIT", "{errors:?}");
        // Errors do not depend on what is retained: a build that bakes a failing map reports that
        // map's error whether or not other maps are kept.
        let mut other = Session::default();
        build(&mut other, 1, &with(json!({ "a": false, "b": true })));
        let bad = with(json!({ "a": true, "b": true, "nope": 1 }));
        for session in [&mut session, &mut other, &mut Session::default()] {
            let errors = session
                .build(&bad, 9, &no_assets, &|| false)
                .err()
                .expect("an unknown parameter fails");
            assert_eq!(errors[0].code, "MS_PARAMETER", "{errors:?}");
        }
    }

    fn quad(id: &str, width: f64, height: f64) -> crate::authoring::Receiver {
        crate::authoring::Receiver {
            id: id.into(),
            object_id: id.into(),
            positions: vec![0., 0., 0., width, 0., 0., width, 0., height, 0., 0., height],
            normals: vec![0., -1., 0., 0., -1., 0., 0., -1., 0., 0., -1., 0.],
            uvs: vec![0., 0., 1., 0., 1., 1., 0., 1.],
            indices: vec![0, 1, 2, 0, 2, 3],
            object_to_world: None,
            contacts: Vec::new(),
        }
    }

    #[test]
    fn kept_planes_share_one_budget_per_run_sized_by_the_atlases() {
        // Four square 512 px atlases: a plane of each is 8 MiB, and a run adds 24 MiB at most.
        let request = Request {
            source: HEAVY.into(),
            receivers: vec![
                quad("a", 1., 1.),
                quad("b", 2., 2.),
                quad("c", 3., 3.),
                quad("d", 4., 4.),
            ],
            ..request(json!({}), 0, 512)
        };
        let mut session = Session::default();
        let (_, planes) = planes_of(&mut session, 1, &request);
        assert_eq!(planes.iter().map(|p| p.1).sum::<usize>(), 3, "{planes:?}");
        let (entries, bytes) = session.runtime.retained(FIELD_PLANES);
        assert_eq!(entries, 3);
        assert!(bytes <= FIELD_CAPTURE_BYTES + 3 * 64, "{bytes}");
    }

    #[test]
    fn a_program_whose_rows_cannot_be_evaluated_together_keeps_no_planes() {
        // The untaken branch makes the cost bound too large for rows; nothing would fill a plane.
        let source = "material 1;
fn l1(x: scalar) -> scalar = x + x + x + x + x + x + x + x + x + x;
fn l2(x: scalar) -> scalar = l1(x) + l1(x) + l1(x) + l1(x) + l1(x) + l1(x) + l1(x) + l1(x) + l1(x) + l1(x);
fn l3(x: scalar) -> scalar = l2(x) + l2(x) + l2(x) + l2(x) + l2(x) + l2(x) + l2(x) + l2(x) + l2(x) + l2(x);
fn l4(x: scalar) -> scalar = l3(x) + l3(x) + l3(x) + l3(x) + l3(x) + l3(x) + l3(x) + l3(x) + l3(x) + l3(x);
param tint: scalar = 0.5 range(0, 1);
let heavy = fbm(surface * 12, seed(\"g\"), 3) + noise(surface * 5, seed(\"h\")) * 0.5 + sin(surface.x * 9) * cos(surface.y * 7) * 0.1;
output base_color = vec3(clamp(heavy * tint, 0, 1), select(surface.x > 100, l4(surface.x), 0.25), 0.2);
output roughness = 0.5;";
        let with = |tint: f64| Request {
            source: source.into(),
            ..request(json!({ "tint": tint }), 0, 32)
        };
        let mut session = Session::default();
        for (revision, tint) in [(1, 0.5), (2, 0.7), (3, 0.9)] {
            let (output, planes) = planes_of(&mut session, revision, &with(tint));
            assert_eq!(planes, [(0, 0)], "tint {tint}");
            assert_same(&output, &fresh(&with(tint)), "unbounded rows");
        }
        assert_eq!(session.runtime.retained(FIELD_PLANES).0, 0);
    }

    #[test]
    fn a_plane_some_samples_did_not_fill_is_completed_by_a_later_bake() {
        // Color reads `heavy` on half the swatch; roughness reads it everywhere once `wide` is set.
        let source = "material 1;
param wide: bool = false;
param shine: scalar = 0.2 range(0, 1);
let heavy = fbm(surface * 12, seed(\"g\"), 3) + noise(surface * 5, seed(\"h\")) * 0.5 + sin(surface.x * 9) * cos(surface.y * 7) * 0.1;
output base_color = vec3(clamp(select(surface.x < 0.5, heavy, 0.3), 0, 1), 0.4, 0.2);
output roughness = clamp(select(wide, heavy, 0.5) * shine, 0, 1);";
        let with = |parameters: serde_json::Value| Request {
            source: source.into(),
            ..request(parameters, 0, 48)
        };
        let mut session = Session::default();
        let (_, planes) = planes_of(&mut session, 1, &with(json!({})));
        assert_eq!(planes, [(0, 1)]);
        // The half-filled plane is read and a copy completed with the other half.
        let wide = with(json!({ "wide": true }));
        let (output, planes) = planes_of(&mut session, 2, &wide);
        assert_eq!(planes, [(1, 1)]);
        assert_same(&output, &fresh(&wide), "completing bake");
        // The completed plane replaced it: the next edit only reads.
        let edited = with(json!({ "wide": true, "shine": 0.6 }));
        let (output, planes) = planes_of(&mut session, 3, &edited);
        assert_eq!(planes, [(1, 0)]);
        assert_same(&output, &fresh(&edited), "after completion");
        assert_eq!(session.runtime.retained(FIELD_PLANES).0, 1);
    }

    #[test]
    fn shares_are_kept_only_for_reported_bakes_and_within_bounds() {
        let mut session = Session::default();
        session.distribute_bakes(true);
        let first = request(json!({}), 0, 96);
        assert!(session.build(&first, 1, &no_assets, &|| false).is_err());
        let pending = session.take_pending_bake().unwrap();
        let share = Session::default()
            .bake_share(&pending, 0, 2, &no_assets, &|| false)
            .unwrap_or_else(|e| panic!("{e:?}"));
        // A session that reported nothing keeps nothing of it.
        let mut stranger = Session::default();
        stranger.distribute_bakes(true);
        stranger.provide_bake_share(&share).unwrap();
        assert!(stranger.distribution.provided.is_empty());
        // The reporting session keeps it until a build takes it.
        session.provide_bake_share(&share).unwrap();
        assert_eq!(session.distribution.provided.len(), 1);
        // Many superseded edits: reports, descriptions and rows stay bounded.
        for edit in 0..(MAX_REPORTED + 40) {
            let edited = request(json!({ "tint": edit as f64 / 2000. }), 1, 96);
            assert!(
                session
                    .build(&edited, 2 + edit as u64, &no_assets, &|| false)
                    .is_err()
            );
        }
        assert!(session.distribution.reported.len() <= MAX_REPORTED);
        assert!(session.distribution.pending.len() <= MAX_PENDING);
        assert!(
            session.distribution.provided.is_empty(),
            "the rows of the forgotten first report went with it"
        );
        session.provide_bake_share(&share).unwrap();
        assert!(
            session.distribution.provided.is_empty(),
            "a forgotten bake's rows are dropped"
        );
        // Disabling drops everything.
        session.distribute_bakes(false);
        assert!(
            session.distribution.reported.is_empty() && session.distribution.pending.is_empty()
        );
    }

    #[test]
    fn malformed_shares_are_refused_without_reserving_what_they_claim() {
        let key = [7u8; 32];
        // A header claiming a million rows in a few bytes.
        let mut claim = Vec::new();
        claim.extend(SHARE_MAGIC);
        claim.extend(key);
        claim.push(Groups::MAPS.0);
        claim.extend((1u32 << 20).to_le_bytes());
        claim.extend((1u32 << 20).to_le_bytes());
        assert!(read_shares(&claim).is_err());
        // A total far beyond what the share's own rows imply.
        let row = |emitted: f64, strength: f64| Row {
            cell: 1,
            color: vec![0; 4],
            normal: vec![0; 4],
            roughness: vec![0],
            metalness: vec![0],
            emitted: vec![[emitted, 0., 0.]],
            live: Vec::new(),
            blend: false,
            emits: true,
            strength,
        };
        let mut sparse = Vec::new();
        write_share(
            &mut sparse,
            &key,
            Groups::MAPS,
            1 << 20,
            &[(0, row(0.5, 1.))],
        );
        assert!(read_shares(&sparse).is_err());
        // Values no evaluation produces.
        for (emitted, strength) in [
            (f64::NAN, 1.),
            (f64::INFINITY, 1.),
            (0.5, f64::NAN),
            (-1., 1.),
            (0.5, 0.),
        ] {
            let mut bytes = Vec::new();
            write_share(
                &mut bytes,
                &key,
                Groups::MAPS,
                1,
                &[(0, row(emitted, strength))],
            );
            assert!(read_shares(&bytes).is_err(), "{emitted} {strength}");
        }
        let mut fine = Vec::new();
        write_share(&mut fine, &key, Groups::MAPS, 1, &[(0, row(0.5, 1.))]);
        assert_eq!(read_shares(&fine).unwrap().len(), 1);
    }

    #[test]
    fn a_field_bake_is_bounded() {
        let mut session = Session::default();
        let names: Vec<String> = (0..MAX_BAKED_FIELDS + 1)
            .map(|_| "heavy".to_owned())
            .collect();
        let errors = session
            .bake_fields(&heavy_request(json!({}), 0), 1, &names, &no_assets, &|| {
                false
            })
            .err()
            .expect("too many fields");
        assert!(errors[0].message.contains("at most"), "{errors:?}");
        let baked = session
            .bake_fields(
                &heavy_request(json!({}), 0),
                2,
                &names[..2],
                &no_assets,
                &|| false,
            )
            .unwrap_or_else(|e| panic!("{e:?}"));
        assert_eq!(baked.fields.len(), 2);
    }

    /// The fields a field bake evaluated, with its result.
    fn fields_of(
        session: &mut Session,
        revision: u64,
        request: &Request,
        names: &[&str],
    ) -> (FieldBake, usize) {
        FIELDS_BAKED.with(|baked| baked.borrow_mut().clear());
        let names: Vec<String> = names.iter().map(|name| (*name).to_owned()).collect();
        let baked = session
            .bake_fields(request, revision, &names, &no_assets, &|| false)
            .unwrap_or_else(|e| panic!("{e:?}"));
        let evaluated = FIELDS_BAKED.with(|baked| baked.borrow().iter().sum());
        (baked, evaluated)
    }

    #[test]
    fn a_field_bake_evaluates_only_the_fields_an_edit_can_change() {
        let mut session = Session::default();
        let base = heavy_request(json!({}), 0);
        let (first, evaluated) = fields_of(&mut session, 1, &base, &["heavy", "shade"]);
        assert_eq!(evaluated, 2);
        // Each field equals what the per-sample evaluator computes at the texel's sample.
        let atlas = Atlas::new(
            receiver::swatch(base.domain),
            base.resolution,
            base.mapping,
            base.domain,
            base.gravity,
            base.profile.max_resolution(),
            base.texel_density,
        )
        .unwrap();
        let program = session.program(&base).unwrap();
        let inputs = Inputs::default();
        let selection = program.select_fields(&["heavy", "shade"]).unwrap();
        let mut evaluator = program.evaluator(&inputs).unwrap();
        let mut checked = 0;
        for chart in &atlas.charts {
            for y in 0..chart.height {
                for x in 0..chart.width {
                    let (triangle, u, v) = atlas.texel(chart, x, y);
                    let sample = atlas.sample(triangle, u, v).unwrap();
                    let mut values = [FieldValue::Scalar(0.), FieldValue::Scalar(0.)];
                    evaluator.replenish(64_000_000);
                    evaluator
                        .evaluate_outputs(
                            &sample,
                            program::OutputSet::NONE,
                            &selection,
                            &mut values,
                        )
                        .unwrap();
                    let at = ((chart.y + y) * atlas.width + chart.x + x) * 4;
                    for (field, value) in first.fields.iter().zip(&values) {
                        let FieldValue::Scalar(value) = value else {
                            panic!("a scalar field")
                        };
                        assert_eq!(field[at].to_bits(), (*value as f32).to_bits());
                        assert_eq!(field[at + 1..at + 4], [0.; 3]);
                    }
                    checked += 1;
                }
            }
        }
        assert!(checked >= 48 * 48 / 2, "{checked} texels");
        // `heavy` reads the seed and `zoom`; `shade` also reads `tint`; nothing reads `shine`.
        for (revision, (parameters, seed, expected)) in [
            (json!({}), 0, 0),
            (json!({ "shine": 0.7 }), 0, 0),
            (json!({ "tint": 0.9 }), 0, 1),
            (json!({ "zoom": 20 }), 0, 2),
            (json!({ "zoom": 20 }), 5, 2),
            (json!({ "zoom": 20, "shine": 0.1 }), 5, 0),
        ]
        .into_iter()
        .enumerate()
        {
            let edited = heavy_request(parameters, seed);
            let (kept, evaluated) = fields_of(
                &mut session,
                revision as u64 + 2,
                &edited,
                &["heavy", "shade"],
            );
            assert_eq!(evaluated, expected, "edit {revision}");
            let (fresh, _) = fields_of(&mut Session::default(), 1, &edited, &["heavy", "shade"]);
            assert_eq!((kept.width, kept.height), (fresh.width, fresh.height));
            for (a, b) in kept.fields.iter().zip(&fresh.fields) {
                assert!(
                    a.iter().zip(b).all(|(a, b)| a.to_bits() == b.to_bits()),
                    "edit {revision}"
                );
            }
        }
        // A field named twice is evaluated once and returned twice; in another order it is kept.
        let other = heavy_request(json!({ "zoom": 30 }), 0);
        let (twice, evaluated) = fields_of(&mut session, 20, &other, &["heavy", "heavy"]);
        assert_eq!(evaluated, 2, "both slots lacked it");
        assert_eq!(twice.fields[0], twice.fields[1]);
        let (again, evaluated) = fields_of(&mut session, 21, &other, &["shade", "heavy"]);
        assert_eq!(evaluated, 1);
        assert_eq!(again.fields[1], twice.fields[0]);
        // An invalid parameter fails whatever is kept, and a failed bake keeps nothing.
        let invalid = heavy_request(json!({ "shine": 7 }), 0);
        let names = ["heavy".to_owned()];
        assert!(
            session
                .bake_fields(&invalid, 30, &names, &no_assets, &|| false)
                .is_err()
        );
        let cancelled = heavy_request(json!({ "zoom": 3 }), 0);
        assert!(
            session
                .bake_fields(&cancelled, 31, &names, &no_assets, &|| true)
                .is_err()
        );
        let (_, evaluated) = fields_of(&mut session, 32, &cancelled, &["heavy"]);
        assert_eq!(evaluated, 1, "the cancelled bake kept nothing");
        // A build of the same request shares the atlas and program with the field bake, and its
        // maps are what a fresh session builds.
        let (output, _) = build(&mut session, 33, &cancelled);
        assert_same(&output, &fresh(&cancelled), "build after field bakes");
    }
}

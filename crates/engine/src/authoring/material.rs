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
use superspec_model::instances::{IDENTITY, ImageResource, Instance, LocalMesh, Model};
use superspec_types::{
    diagnostic::SourceLocation,
    material::{AlphaMode, LiveArgument, LiveOperation as LiveDescriptor, Material},
};

pub(crate) struct BakedReceiver {
    pub mesh: Arc<LocalMesh>,
    pub material: Material,
    pub images: BTreeMap<String, ImageResource>,
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
    /// [`Self::provide_bake_share`] and builds again. Rows are the same bytes wherever they are
    /// evaluated, so the result is identical to an undistributed build.
    pub fn distribute_bakes(&mut self, enabled: bool) {
        self.distribution.enabled = enabled;
        if !enabled {
            self.distribution.pending = None;
            self.distribution.provided.clear();
        }
    }

    /// The bake the last build stopped for, if any.
    pub fn take_pending_bake(&mut self) -> Option<PendingBake> {
        self.distribution.pending.take()
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
        let revision = self.authoring_revision.get().unwrap_or(0);
        self.begin_authoring(revision, &pending.request.entry)?;
        let share = Share {
            index,
            count,
            keys: &keys,
        };
        match self.material_with(&pending.request, revision, assets, cancelled, Some(share))? {
            Built::Share(bytes) => Ok(bytes),
            Built::Output(_) => unreachable!("a share was requested"),
        }
    }

    /// Accepts rows computed by [`Self::bake_share`] for a bake this session reported.
    pub fn provide_bake_share(&mut self, bytes: &[u8]) -> Result<(), String> {
        for (key, total, rows) in read_shares(bytes)? {
            let slots = self.distribution.provided.entry(key).or_default();
            if slots.len() != total {
                slots.clear();
                slots.resize_with(total, || None);
            }
            for (ordinal, row) in rows {
                *slots
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
        let mut targets = vec![];
        for (i, receiver) in receivers.iter().enumerate() {
            let layout = format!("layout/{i}");
            let bake = format!("bake/{i}");
            let mut identity = receiver.clone();
            identity.id.clear();
            identity.object_id.clear();
            nodes.push(node(
                layout.clone(),
                "receiver-atlas-v2",
                json(&(
                    identity,
                    request.resolution,
                    request.mapping,
                    request.domain,
                    request.gravity,
                )),
                vec![],
            ));
            nodes.push(node(
                bake.clone(),
                "material-receiver-v3",
                // Production bakes compress their PNG maps harder: same pixels, different bytes.
                json(&(
                    &request.inspection,
                    request.profile == super::Profile::Production,
                )),
                vec![
                    dependency("mapping", layout),
                    dependency("program", "program"),
                    dependency("inputs", "inputs"),
                ],
            ));
            targets.push(bake);
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
        if share.is_none() && self.distribution.enabled {
            let complete = |key: &ContentKey| {
                self.distribution
                    .provided
                    .get(key)
                    .is_some_and(|rows| rows.iter().all(Option::is_some))
            };
            let missing: Vec<String> = bake_keys
                .iter()
                .filter(|key| !self.runtime.holds(key) && !complete(key))
                .map(format_key)
                .collect();
            // Small bakes finish sooner here than a round trip through other workers.
            let texels = request.resolution * request.resolution * receivers.len();
            if !missing.is_empty() && texels >= DISTRIBUTE_TEXELS {
                // Rows for other bakes were never used (their inputs changed); only this
                // request's partial rows are worth keeping. Rows for a later material of the
                // same project stay until that material is built.
                self.distribution
                    .provided
                    .retain(|key, _| bake_keys.contains(key));
                self.distribution.pending = Some(PendingBake {
                    request: request.clone(),
                    keys: missing,
                    images: image_bytes.keys().cloned().collect(),
                });
                return Err(vec![Diagnostic {
                    code: "BAKE_PENDING".into(),
                    ..Diagnostic::message(&request.entry, "Material bake is being distributed.")
                }]);
            }
        }
        // Rows provided for these bakes are used once; a later build with other inputs has other keys.
        let mut provided: BTreeMap<ContentKey, Vec<Row>> = BTreeMap::new();
        if share.is_none() {
            for key in &bake_keys {
                if let Some(rows) = self.distribution.provided.remove(key) {
                    if let Some(rows) = rows.into_iter().collect::<Option<Vec<Row>>>() {
                        provided.insert(*key, rows);
                    }
                }
            }
        }
        let run_plan = match &dependencies_only {
            Some(plan) => plan.as_ref().map_err(|e| fail(e.to_string()))?,
            None => &plan,
        };
        let mut evaluation_error = None;
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
                        let bytes = image.rgba.len();
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
                            images.insert(name.clone(), image.as_ref().clone());
                        }
                        Ok((
                            Artifact::Inputs(Arc::new(Inputs {
                                parameters: request.parameters.clone(),
                                seed: request.seed,
                                images,
                                edges: request.edges.clone(),
                                features: Vec::new(),
                                mips: Default::default(),
                            })),
                            // Filtered sampling may add mip levels of at most 3/4 the decoded bytes.
                            count * 2 + json(&request.parameters).len() + 1024,
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
                        )?;
                        let bytes = atlas.mesh.positions.len() * 88
                            + atlas.mesh.indices.len() * 4
                            + receivers[index].positions.len() * 8 * 3;
                        Ok((Artifact::Receiver(Arc::new(atlas)), bytes))
                    }
                    id if id.starts_with("bake/") => {
                        let Artifact::Program(program) = dependencies["program"].as_ref() else {
                            unreachable!()
                        };
                        let Artifact::Inputs(inputs) = dependencies["inputs"].as_ref() else {
                            unreachable!()
                        };
                        let Artifact::Receiver(atlas) = dependencies["mapping"].as_ref() else {
                            unreachable!()
                        };
                        let index = id[5..].parse::<usize>().unwrap();
                        match bake(
                            program,
                            inputs,
                            atlas,
                            request.inspection.as_deref(),
                            request.profile.threads(),
                            request.profile == super::Profile::Production,
                            provided.remove(&bake_keys[index]),
                            stopped,
                        ) {
                            Ok(value) => {
                                let bytes = value
                                    .images
                                    .values()
                                    .map(|image| image.bytes.len())
                                    .sum::<usize>()
                                    + value.mesh.positions.len() * 88;
                                Ok((Artifact::Material(Arc::new(value)), bytes))
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
                if !share.keys.contains(key) {
                    continue;
                }
                let Artifact::Receiver(atlas) = complete[&format!("layout/{i}")].as_ref() else {
                    unreachable!()
                };
                let (total, rows) = bake_rows(
                    program,
                    inputs,
                    atlas,
                    request.inspection.as_deref(),
                    (share.index, share.count),
                    cancelled,
                )
                .map_err(|error| vec![error])?;
                write_share(&mut bytes, key, total, &rows);
            }
            return Ok(Built::Share(bytes));
        }
        let mut model = Model {
            id: "studio/material".into(),
            name: "Material preview".into(),
            ..Default::default()
        };
        for (i, receiver) in receivers.iter().enumerate() {
            let Artifact::Material(baked) = complete[&format!("bake/{i}")].as_ref() else {
                unreachable!()
            };
            let mesh = format!(
                "studio/mesh/{}",
                hash(&json(&(
                    &baked.mesh.positions,
                    &baked.mesh.uvs,
                    &baked.mesh.indices
                )))
            );
            model.meshes.insert(mesh.clone(), baked.mesh.clone());
            model.images.extend(baked.images.clone());
            model
                .materials
                .insert(baked.material.id.clone(), baked.material.clone());
            model.instances.push(Instance {
                id: receiver.id.clone(),
                object_id: receiver.object_id.clone(),
                mesh,
                material: Some(baked.material.id.clone()),
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
        Ok(Built::Output(Output { model, metadata }))
    }
}

/// Bakes below this many requested texels are evaluated where they are requested.
const DISTRIBUTE_TEXELS: usize = 8192;

/// A session's state for baking rows in other sessions (see [`Session::distribute_bakes`]).
#[derive(Default)]
pub(crate) struct Distribution {
    enabled: bool,
    pending: Option<PendingBake>,
    /// Rows received so far for a reported bake, by row ordinal.
    provided: BTreeMap<ContentKey, Vec<Option<Row>>>,
}

/// A bake other sessions can share: the material request, the content keys of its receivers'
/// uncached bakes and the images the request reads.
#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingBake {
    pub request: Request,
    pub keys: Vec<String>,
    pub images: Vec<String>,
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

const SHARE_MAGIC: &[u8; 4] = b"SBR1";

/// Appends one bake's rows: key, total row count, then each row with its ordinal. Numbers are
/// little-endian and floats keep their bits, so a row is the same wherever it is merged.
fn write_share(bytes: &mut Vec<u8>, key: &ContentKey, total: usize, rows: &[(usize, Row)]) {
    let word = |bytes: &mut Vec<u8>, value: usize| bytes.extend((value as u32).to_le_bytes());
    bytes.extend(SHARE_MAGIC);
    bytes.extend(key);
    word(bytes, total);
    word(bytes, rows.len());
    for (ordinal, row) in rows {
        word(bytes, *ordinal);
        word(bytes, row.roughness.len());
        word(bytes, row.live.len());
        bytes.push(u8::from(row.blend) | u8::from(row.emits) << 1);
        bytes.extend(row.strength.to_le_bytes());
        bytes.extend(&row.color);
        bytes.extend(&row.normal);
        bytes.extend(&row.roughness);
        bytes.extend(&row.metalness);
        if row.emits {
            for value in row.emitted.iter().flatten() {
                bytes.extend(value.to_le_bytes());
            }
        }
        for value in row.live.iter().flatten() {
            bytes.extend(value.to_le_bytes());
        }
    }
}

type Shares = Vec<(ContentKey, usize, Vec<(usize, Row)>)>;

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
            let color = take(&mut bytes, cell * 4)?.to_vec();
            let normal = take(&mut bytes, cell * 4)?.to_vec();
            let roughness = take(&mut bytes, cell)?.to_vec();
            let metalness = take(&mut bytes, cell)?.to_vec();
            let emits = flags & 2 != 0;
            let emitted = if emits {
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
        shares.push((key, total, rows));
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

/// One texel row of one atlas chart.
struct Row {
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

/// Evaluates consecutive atlas rows: channels, the relief normal and any live arguments.
///
/// A normal needs the height one texel along each chart leg. Where that point is an adjacent
/// texel's center, the adjacent texel's height is used instead of evaluating the height graph
/// again, which is most of a bake's work otherwise. A neighbor row outside `rows` has its heights
/// evaluated here, at the same centers, so the result never depends on how rows are grouped
/// into blocks, threads or workers.
fn bake_block(
    evaluator: &mut program::Evaluator<'_>,
    atlas: &Atlas,
    rows: &[(usize, usize)],
    inspect: Option<&str>,
    live_fields: &program::FieldSelection,
    entry: &str,
) -> Result<Vec<Row>, Diagnostic> {
    let failure = |message: String| Diagnostic::message(entry, message);
    let live_count = live_fields.len();
    let mut live = vec![FieldValue::Scalar(0.); live_count];
    let mut baked: Vec<Row> = Vec::with_capacity(rows.len());
    let mut centers: Vec<Vec<Center>> = Vec::with_capacity(rows.len());
    // Center heights by (chart, row): this block's rows, then neighbor rows as they are needed.
    let mut heights: BTreeMap<(usize, usize), Vec<f64>> = BTreeMap::new();
    for &(chart_index, y) in rows {
        let chart = &atlas.charts[chart_index];
        let cell = chart.width;
        let mut row = Row {
            color: vec![0; cell * 4],
            normal: vec![0; cell * 4],
            roughness: vec![0; cell],
            metalness: vec![0; cell],
            emitted: Vec::new(),
            live: vec![vec![f64::NAN; cell]; live_count],
            blend: false,
            emits: false,
            strength: 1.,
        };
        // Each row of texels has its own operation allowance; every sample keeps its step budget.
        evaluator.replenish(64_000_000);
        let mut row_centers = Vec::with_capacity(cell);
        let mut row_heights = Vec::with_capacity(cell);
        for x in 0..cell {
            let (triangle, u, v) = atlas.texel(chart, x, y);
            let sample = atlas.sample(triangle, u, v).map_err(&failure)?;
            // Fields are computed only when inspected; channel values are the same either way.
            let value = if live_count == 0 {
                evaluator.evaluate_channels(&sample, inspect)
            } else {
                evaluator.evaluate_selected(&sample, live_fields, &mut live)
            }
            .map_err(Diagnostic::from)?;
            row.roughness[x] = byte(value.roughness);
            row.metalness[x] = byte(value.metalness);
            row.strength = value.emissive.into_iter().fold(row.strength, f64::max);
            row.blend |= value.opacity < 1.;
            if value.emissive.iter().any(|v| *v > 0.) {
                row.emits = true;
                if row.emitted.is_empty() {
                    row.emitted = vec![[0.; 3]; cell];
                }
                row.emitted[x] = value.emissive;
            }
            for (values, field) in row.live.iter_mut().zip(&live) {
                values[x] = match field {
                    FieldValue::Scalar(v) => *v,
                    FieldValue::Bool(v) => f64::from(u8::from(*v)),
                    FieldValue::Vec2(v) => v[0],
                    FieldValue::Vec3(v) => v[0],
                };
            }
            row_heights.push(value.height);
            row_centers.push(Center {
                triangle,
                u,
                v,
                normal: sample.normal,
                value,
            });
        }
        heights.insert((chart_index, y), row_heights);
        centers.push(row_centers);
        baked.push(row);
    }
    for ((&(chart_index, y), row), row_centers) in rows.iter().zip(&mut baked).zip(&centers) {
        let chart = &atlas.charts[chart_index];
        evaluator.replenish(64_000_000);
        let mut previous: Option<[f64; 3]> = None;
        for (x, center) in row_centers.iter().enumerate() {
            let (triangle, u, v) = (center.triangle, center.u, center.v);
            let value = &center.value;
            let [step_u, step_v] = atlas.steps(triangle);
            let mut height_at = |target: [f64; 2]| -> Result<f64, Diagnostic> {
                let Some([nx, ny]) = neighbor_texel(atlas, chart, [x, y], triangle, target) else {
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
                    let mut row_heights = Vec::with_capacity(chart.width);
                    for other in 0..chart.width {
                        let (triangle, u, v) = atlas.texel(chart, other, ny);
                        row_heights.push(
                            evaluator
                                .evaluate_height(&atlas.sample(triangle, u, v).map_err(&failure)?)
                                .map_err(Diagnostic::from)?,
                        );
                    }
                    heights.insert((chart_index, ny), row_heights);
                }
                Ok(heights[&(chart_index, ny)][nx])
            };
            let hu = height_at([u + step_u, v])?;
            let hv = height_at([u, v + step_v])?;
            let mut direction = atlas
                .normal(
                    triangle,
                    (hu - value.height) / step_u,
                    (hv - value.height) / step_v,
                )
                .map_err(&failure)?;
            if let Some(adjusted) = value.receiver_normal {
                let [tangent, bitangent] = atlas.frame(triangle).map_err(&failure)?;
                direction =
                    program::orient_relief(direction, tangent, bitangent, center.normal, adjusted)
                        .map_err(|message| failure(message.into()))?;
            }
            if let Some(offset) = value.normal_offset {
                let [tangent, bitangent] = atlas.frame(triangle).map_err(&failure)?;
                direction =
                    program::bend_shaded(direction, tangent, bitangent, center.normal, offset)
                        .map_err(|message| failure(message.into()))?;
            }
            let rgb = if let Some(field) = inspect {
                inspection(value, field, direction, center.normal).map_err(&failure)?
            } else {
                value.base_color
            };
            row.color[x * 4..x * 4 + 3].copy_from_slice(&rgb.map(srgb));
            row.color[x * 4 + 3] = byte(value.opacity);
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

/// Every chart row of an atlas as (chart, row), in the order rows are merged.
fn atlas_rows(atlas: &Atlas) -> Vec<(usize, usize)> {
    atlas
        .charts
        .iter()
        .enumerate()
        .flat_map(|(chart, placed)| (0..placed.height).map(move |y| (chart, y)))
        .collect()
}

/// Evaluates share `share.0` of `share.1` equal runs of consecutive rows, with the total row count.
fn bake_rows(
    program: &Program,
    inputs: &Inputs,
    atlas: &Atlas,
    inspect: Option<&str>,
    share: (usize, usize),
    cancelled: &dyn Fn() -> bool,
) -> Result<(usize, Vec<(usize, Row)>), Diagnostic> {
    let entry = program.metadata().entry.as_str();
    let failure = |message: String| Diagnostic::message(entry, message);
    let features = receiver_features(program, atlas);
    let live_fields = live_field_names(program, inspect);
    let live_selection = program.select_fields(&live_fields).map_err(&failure)?;
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

#[allow(clippy::too_many_arguments)]
fn bake(
    program: &Program,
    inputs: &Inputs,
    atlas: &Atlas,
    inspect: Option<&str>,
    threads: usize,
    compact: bool,
    provided: Option<Vec<Row>>,
    cancelled: &dyn Fn() -> bool,
) -> Result<BakedReceiver, Diagnostic> {
    let failure = |message: String| Diagnostic::message(&program.metadata().entry, message);
    let pixels = atlas.width * atlas.height;
    if pixels > 4 * 1024 * 1024 {
        return Err(failure(
            "Material atlas exceeds its pixel allowance.".into(),
        ));
    }
    let mut color = vec![0u8; pixels * 4];
    // Texels outside every chart are transparent; unused normals stay flat, so mip levels that
    // average across a chart's gutter never pull in an invalid direction.
    let mut normal = [128u8, 128, 255, 0].repeat(pixels);
    let mut properties = vec![0u8; pixels * 4];
    // Emission is rare and costs 24 bytes a texel, so its buffer exists only once a row emits.
    let mut emitted: Vec<[f64; 3]> = Vec::new();
    let mut blend = false;
    let mut emits = false;
    let mut emission_strength = 1f64;
    let live_fields = live_field_names(program, inspect);
    let mut live_values = vec![vec![f64::NAN; pixels]; live_fields.len()];
    let live_selection = program.select_fields(&live_fields).map_err(&failure)?;
    let rows = atlas_rows(atlas);
    // Rows evaluated elsewhere are merged exactly like rows evaluated here; a set that does not
    // fit this atlas is ignored and the rows are evaluated here instead.
    let mut provided = provided
        .filter(|provided| {
            provided.len() == rows.len()
                && provided.iter().zip(&rows).all(|(row, &(chart, _))| {
                    let cell = atlas.charts[chart].width;
                    row.color.len() == cell * 4
                        && row.normal.len() == cell * 4
                        && row.roughness.len() == cell
                        && row.metalness.len() == cell
                        && row.live.len() == live_fields.len()
                        && row.live.iter().all(|values| values.len() == cell)
                        && (!row.emits || row.emitted.len() == cell)
                })
        })
        .map(Vec::into_iter);
    let features = if provided.is_some() {
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
    let mut serial = if threads == 1 && provided.is_none() {
        Some(evaluator()?)
    } else {
        None
    };
    // Rows are evaluated independently and written back in order, so threads never change bytes.
    for batch in rows.chunks(threads * BLOCK_ROWS) {
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
            Some(evaluator) => {
                match bake_block(evaluator, atlas, batch, inspect, &live_selection, entry) {
                    Ok(rows) => rows.into_iter().map(Ok).collect(),
                    Err(error) => vec![Err(error)],
                }
            }
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
            if row.emits && emitted.is_empty() {
                emitted = vec![[0.; 3]; pixels];
            }
            let chart = &atlas.charts[chart];
            let start = (chart.y + y) * atlas.width + chart.x;
            for x in 0..chart.width {
                let (texel, offset) = (start + x, (start + x) * 4);
                color[offset..offset + 4].copy_from_slice(&row.color[x * 4..x * 4 + 4]);
                normal[offset..offset + 4].copy_from_slice(&row.normal[x * 4..x * 4 + 4]);
                properties[offset..offset + 4].copy_from_slice(&[
                    255,
                    row.roughness[x],
                    row.metalness[x],
                    255,
                ]);
                if row.emits {
                    emitted[texel] = row.emitted[x];
                }
                for (values, samples) in live_values.iter_mut().zip(&row.live) {
                    values[texel] = samples[x];
                }
            }
            emission_strength = emission_strength.max(row.strength);
            blend |= row.blend;
            emits |= row.emits;
        }
    }
    let mut images = BTreeMap::new();
    let mut encode = |rgba: Vec<u8>| -> Result<String, Diagnostic> {
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
        )
        .map_err(&failure)?;
        let id = format!("studio/image/{}", hash(&bytes));
        images.insert(
            id.clone(),
            ImageResource {
                mime: "image/png",
                bytes: bytes.into(),
                pixels,
            },
        );
        Ok(id)
    };
    let texture = encode(color)?;
    let mut material = Material {
        color: "#ffffff".into(),
        repeat: 1.,
        roughness: 1.,
        metalness: 1.,
        texture: Some(texture.clone()),
        alpha_mode: blend.then_some(AlphaMode::Blend),
        ..Default::default()
    };
    if inspect.is_some() {
        material.color = "#000000".into();
        material.emissive = Some("#ffffff".into());
        material.emissive_texture = Some(texture);
        material.metalness = 0.;
    } else {
        material.normal_texture = Some(encode(normal)?);
        material.normal_scale = Some(1.);
        material.metallic_roughness_texture = Some(encode(properties)?);
        if emits {
            let emissive = emitted
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
            material.emissive = Some("#ffffff".into());
            material.emissive_strength = Some(emission_strength);
            material.emissive_texture = Some(encode(emissive)?);
        }
        material.live = live_operations(program, &live_fields, &live_values, pixels, &mut encode)?;
    }
    material.id.clear();
    material.id = format!("studio/material/{}", hash(&json(&material)));
    Ok(BakedReceiver {
        mesh: atlas.mesh.clone(),
        material,
        images,
    })
}

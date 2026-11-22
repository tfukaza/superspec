mod lods;

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
    /// The intrinsic fields among `capture`, by field index, with the key of the field's
    /// raster, which no receiver's layout enters.
    intrinsic: BTreeMap<usize, ContentKey>,
}

/// The intrinsic rasters a build reads and adds to, by key. They start as copies of the kept
/// ones, every receiver of the build reads and fills the same copies, and a build that finishes
/// keeps the ones that changed.
type Rasters = BTreeMap<ContentKey, WorkingRaster>;

#[derive(Default)]
struct WorkingRaster {
    raster: IntrinsicRaster,
    /// Whether a second layout has asked for the field's values. Mapping a receiver onto the
    /// lattice costs a pass over its samples, and most fields are only ever baked for one
    /// layout, so the first layout leaves just this mark (an empty raster) and the raster is
    /// filled from the second on.
    shared: bool,
    /// Whether the build changed what is kept: values, or the mark.
    changed: bool,
}

/// Retention of intrinsic rasters (see [IntrinsicRaster]).
const INTRINSIC_RASTERS: Retention = Retention {
    name: "material-intrinsic",
    bytes: 64 * 1024 * 1024,
    // Most entries are the marks of fields one layout has baked, which hold nothing.
    entries: 512,
};
/// Bytes one intrinsic raster holds at most; values beyond are simply not kept.
const INTRINSIC_RASTER_BYTES: usize = 24 * 1024 * 1024;
/// Lattice points along a raster tile's edge.
const RASTER_TILE: i64 = 64;

/// A point of the lattice an intrinsic raster is indexed by: the bits of the sample's footprint
/// (the lattice step), and the whole numbers `i`, `j` with `surface = ((i, j) + 0.5) * step`
/// exactly.
type LatticePoint = (u64, i64, i64);

/// The values an intrinsic field takes at positions of the material's own domain, kept apart
/// from any receiver: the field reads nothing of a sample but its surface position and
/// footprint, so two samples with the same bits of both have the same value, on whatever
/// receiver and at whatever texel they fall. The raster is keyed by the program, the decoded
/// images, the field and the parameters and seed it reads; inside, values are found by lattice
/// point, in tiles. A sample that is not exactly on its footprint's lattice is never looked up or
/// kept, so reading the raster cannot change a baked value.
#[derive(Clone, Default)]
pub(crate) struct IntrinsicRaster {
    /// By footprint bits and tile; each tile holds the four numbers of a plane texel for
    /// `RASTER_TILE` squared points, NaN in the fourth where it holds none.
    tiles: BTreeMap<LatticePoint, Box<[f64]>>,
    /// Bytes this raster holds at most; [`INTRINSIC_RASTER_BYTES`] where zero.
    cap: usize,
    /// Points given a value.
    points: usize,
}

/// Retention of the rasters of intrinsic outputs of a layered stack's materials (see
/// [IntrinsicRaster]): the base's values by position of its own domain, which a resized
/// receiver, another receiver and another stack over the same base read instead of evaluating.
const INTRINSIC_OUTPUTS: Retention = Retention {
    name: "material-intrinsic-outputs",
    bytes: if cfg!(target_arch = "wasm32") {
        96 * 1024 * 1024
    } else {
        512 * 1024 * 1024
    },
    entries: 256,
};
/// Bytes one raster of an intrinsic output holds at most: the lattice points of a 1024 px
/// atlas, or a quarter of a 2048 px one, natively; a third of that in Wasm.
const INTRINSIC_OUTPUT_BYTES: usize = if cfg!(target_arch = "wasm32") {
    40 * 1024 * 1024
} else {
    128 * 1024 * 1024
};
/// Retention of the planes of layered stacks, apart from the build graph's store (a 2048 px
/// atlas's base color alone is 96 MiB): a layer or base edit, or a layer added or removed, finds
/// the planes it cannot change here.
const LAYER_PLANES: Retention = Retention {
    name: "material-layer-planes",
    bytes: if cfg!(target_arch = "wasm32") {
        192 * 1024 * 1024
    } else {
        1536 * 1024 * 1024
    },
    entries: 1024,
};

impl IntrinsicRaster {
    /// The lattice point `sample` is exactly on, if it is on one.
    fn point(sample: &program::Sample) -> Option<LatticePoint> {
        let step = sample.footprint;
        if !(step.is_finite() && step > 0.) {
            return None;
        }
        let along = |position: f64| {
            let index = (position / step - 0.5).round();
            ((index + 0.5) * step == position && index.abs() < 1e12).then_some(index as i64)
        };
        Some((
            step.to_bits(),
            along(sample.surface[0])?,
            along(sample.surface[1])?,
        ))
    }
    fn slot((step, i, j): LatticePoint) -> (LatticePoint, usize) {
        let tile = (step, i.div_euclid(RASTER_TILE), j.div_euclid(RASTER_TILE));
        let within = j.rem_euclid(RASTER_TILE) * RASTER_TILE + i.rem_euclid(RASTER_TILE);
        (tile, within as usize * 4)
    }
    fn get(&self, point: LatticePoint) -> Option<[f64; 4]> {
        let (tile, at) = Self::slot(point);
        let values = &self.tiles.get(&tile)?[at..at + 4];
        (!values[3].is_nan()).then(|| [values[0], values[1], values[2], values[3]])
    }
    fn put(&mut self, point: LatticePoint, value: [f64; 4]) {
        let (tile, at) = Self::slot(point);
        let cap = if self.cap == 0 {
            INTRINSIC_RASTER_BYTES
        } else {
            self.cap
        };
        if !self.tiles.contains_key(&tile) && self.bytes() >= cap {
            return;
        }
        let values = self.tiles.entry(tile).or_insert_with(|| {
            [0., 0., 0., f64::NAN]
                .repeat((RASTER_TILE * RASTER_TILE) as usize)
                .into()
        });
        if values[at + 3].is_nan() {
            self.points += 1;
        }
        values[at..at + 4].copy_from_slice(&value);
    }
    fn bytes(&self) -> usize {
        self.tiles.len() * ((RASTER_TILE * RASTER_TILE) as usize * 32 + 64) + 64
    }
}

/// The texels of an atlas whose samples are exactly on their footprint's lattice, with their
/// lattice points. A chart whose first inner texel is not is skipped whole, so a receiver that
/// does not qualify costs a sample a chart.
fn lattice_points(atlas: &Atlas) -> Vec<(usize, LatticePoint)> {
    let mut points = Vec::new();
    let point = |chart: &Chart, x: usize, y: usize| {
        let (triangle, u, v) = atlas.texel(chart, x, y);
        IntrinsicRaster::point(&atlas.sample(triangle, u, v).ok()?)
    };
    for chart in &atlas.charts {
        let inner = receiver::GUTTER.min(chart.width - 1).min(chart.height - 1);
        if point(chart, inner, inner).is_none() {
            continue;
        }
        for y in 0..chart.height {
            for x in 0..chart.width {
                if let Some(found) = point(chart, x, y) {
                    points.push(((chart.y + y) * atlas.width + chart.x + x, found));
                }
            }
        }
    }
    points
}

/// The key of a field's plane: the layout whose samples it covers, the program and decoded images,
/// the effective values of only the parameters, seed and edges the field reads, and the content
/// of the external planes it reads (`planes`: the receiver's bound planes by input name).
fn field_plane_key(
    program: &Program,
    dependencies: &program::Dependencies,
    request: &Request,
    [layout, program_key, assets]: [&ContentKey; 3],
    planes: &[(String, ContentKey)],
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
    // Fields that read no external input keep the key they always had.
    for (name, key) in planes {
        if dependencies.externals.contains(name) {
            digest.update((name.len() as u64).to_le_bytes());
            digest.update(name.as_bytes());
            digest.update(key);
        }
    }
    digest.finalize().into()
}

/// The planes a request binds to the external inputs `program` reads, for each receiver: input
/// name, and the node of the plane. Inputs the request binds nothing to read their fallback. A
/// request that binds none holds nothing here, so builds without planes allocate as they always
/// did.
#[derive(Default)]
struct BoundPlanes {
    receivers: Vec<Vec<(String, String)>>,
    /// The request's other materials that planes are baked from, by their name in the request.
    sources: BTreeMap<String, Source>,
    /// Every plane node of the build, by id: the entry's and, in a layered build, the ones the
    /// materials of the stack read from each other.
    nodes: BTreeMap<String, PlaneNode>,
}

/// One plane node: the receiver it covers, the input name it is read as, and its source.
struct PlaneNode {
    index: usize,
    name: String,
    bound: Bound,
    /// A layered stack's plane: the planes its value reads, by input name and node id.
    reads: Vec<(String, String)>,
}

impl PlaneNode {
    /// Whether this is a plane of a layered stack, which no entry program's key enters.
    fn layered(id: &str) -> bool {
        id.contains('@')
    }
}

/// Where a bound plane's values come from.
enum Bound {
    /// An encoded plane a host supplied.
    Asset(Arc<[u8]>),
    /// A value (field or output) of one of the request's other materials, baked in this build.
    Material { material: String, value: String },
}

/// Another material of the request, evaluated over the same receivers to make planes.
struct Source {
    program: Arc<Program>,
    /// The request as that material's own: its entry, source, parameters and seed.
    request: Request,
    table: program::DependencyTable,
    /// In a layered build, the material of the stack and the value whose plane each input of
    /// this one reads (`base.<channel>`: the channel of the one beneath that last changed it).
    /// Its other inputs read the request's planes of the receiver by name.
    bindings: BTreeMap<String, (String, String)>,
    /// The receiver feature each named feature it measures to follows (see `Layer::follow`).
    follow: BTreeMap<String, program::Follow>,
}

impl std::ops::Index<usize> for BoundPlanes {
    type Output = [(String, String)];
    fn index(&self, receiver: usize) -> &Self::Output {
        self.receivers.get(receiver).map_or(&[], Vec::as_slice)
    }
}

/// The id of the node of a plane of receiver `index` read as input `name`: from an asset or a
/// value the entry binds by name, or (`producer`) a value of a material of a layered stack, which
/// several materials of the stack read under one name.
fn plane_id(index: usize, name: &str, producer: Option<&str>) -> String {
    match producer {
        None => format!("plane/{index}/{name}"),
        Some(material) => format!("plane/{index}/{name}@{material}"),
    }
}

/// A layered request's stack: the materials it bakes planes of and which plane each input reads.
#[derive(Default)]
struct Chain {
    /// The base and then the layers, lowest first, by their material name in the build, with
    /// their requests and the features they follow.
    materials: Vec<(
        String,
        program::MaterialRequest,
        BTreeMap<String, program::Follow>,
    )>,
    /// For the composite entry, the material and value each input reads: `base.<channel>` the
    /// channel of the material that last changed it, `live.<k>` a field of the base that its
    /// live operations read.
    entry: BTreeMap<String, (String, String)>,
    /// For each material of the stack, the material and channel each of its `base.<channel>`
    /// inputs reads.
    reads: BTreeMap<String, BTreeMap<String, (String, String)>>,
    /// The base's live operations, their inputs renamed to the composite's fields that read the
    /// base's fields through planes.
    live: Vec<program::LiveOperation>,
}

/// Values of one material a plane bake holds at most: a plane of exact values is 32 bytes a
/// texel, so one receiver's planes of one material stay within 16 times that.
const MAX_SOURCE_VALUES: usize = 16;
/// Bytes one baked plane may hold: an atlas of the largest admitted size (2048 px square).
const PLANE_NODE_BYTES: usize = 2048 * 2048 * 32 + 64;

impl Session {
    fn bound_planes(
        &mut self,
        program: &Program,
        request: &Request,
        receivers: &[super::Receiver],
        assets: &dyn Fn(&str) -> Result<Arc<[u8]>, String>,
        chain: Option<&Chain>,
    ) -> Result<BoundPlanes, Vec<Diagnostic>> {
        let externals = &program.metadata().externals;
        if externals.is_empty() || (request.planes.is_empty() && chain.is_none()) {
            return Ok(BoundPlanes::default());
        }
        let fail = |message: String| vec![Diagnostic::message(&request.entry, message)];
        let mut bound = BoundPlanes::default();
        // The stack's materials, each reading the planes of the ones beneath it.
        for (name, wanted, follow) in chain.map_or(&[][..], |chain| &chain.materials) {
            let mut source = self.plane_source(request, wanted)?;
            source.bindings = chain
                .and_then(|chain| chain.reads.get(name))
                .cloned()
                .unwrap_or_default();
            source.follow = follow.clone();
            bound.sources.insert(name.clone(), source);
        }
        for (index, receiver) in receivers.iter().enumerate() {
            let mut planes = Vec::new();
            for external in externals {
                if let Some((producer, value)) =
                    chain.and_then(|chain| chain.entry.get(&external.name))
                {
                    let id = chain_plane(
                        &mut bound,
                        request,
                        receiver,
                        index,
                        &external.name,
                        (producer, value),
                        assets,
                    )
                    .map_err(&fail)?;
                    planes.push((external.name.clone(), id));
                    continue;
                }
                let source = request
                    .planes
                    .get(&receiver.id)
                    .and_then(|named| named.get(&external.name));
                let id = plane_id(index, &external.name, None);
                match source {
                    None => continue,
                    Some(super::PlaneSource::Asset(asset)) => {
                        let bytes = assets(asset).map_err(|e| fail(format!("{asset}: {e}")))?;
                        bound.nodes.insert(
                            id.clone(),
                            PlaneNode {
                                index,
                                name: external.name.clone(),
                                bound: Bound::Asset(bytes),
                                reads: Vec::new(),
                            },
                        );
                    }
                    Some(super::PlaneSource::Material { material, value }) => {
                        if !bound.sources.contains_key(material) {
                            let wanted = request.materials.get(material).ok_or_else(|| {
                                fail(format!(
                                    "External input '{}' names the material '{material}', which the request does not list.",
                                    external.name
                                ))
                            })?;
                            let source = self.plane_source(request, wanted)?;
                            bound.sources.insert(material.clone(), source);
                        }
                        let source = &bound.sources[material];
                        if source_value(&source.program, &source.table, value).is_none() {
                            return Err(fail(format!(
                                "Material '{material}' has no field '{value}', and no such output a plane can hold."
                            )));
                        }
                        bound.nodes.insert(
                            id.clone(),
                            PlaneNode {
                                index,
                                name: external.name.clone(),
                                bound: Bound::Material {
                                    material: material.clone(),
                                    value: value.clone(),
                                },
                                reads: Vec::new(),
                            },
                        );
                    }
                }
                planes.push((external.name.clone(), id));
            }
            if chain.is_none() && planes.len() > MAX_SOURCE_VALUES {
                return Err(fail(format!(
                    "A receiver binds at most {MAX_SOURCE_VALUES} planes."
                )));
            }
            bound.receivers.push(planes);
        }
        // Every plane of one material of a stack over a receiver reads all the planes any of
        // them reads, so one pass bakes them all: a cold build evaluates each material once.
        let mut union: BTreeMap<(usize, String), Vec<(String, String)>> = BTreeMap::new();
        for (id, plane) in &bound.nodes {
            if let (true, Bound::Material { material, .. }) = (PlaneNode::layered(id), &plane.bound)
            {
                let reads = union.entry((plane.index, material.clone())).or_default();
                for read in &plane.reads {
                    if !reads.contains(read) {
                        reads.push(read.clone());
                    }
                }
            }
        }
        for (id, plane) in bound.nodes.iter_mut() {
            if let (true, Bound::Material { material, .. }) = (PlaneNode::layered(id), &plane.bound)
            {
                let mut reads = union[&(plane.index, material.clone())].clone();
                reads.sort();
                plane.reads = reads;
            }
        }
        Ok(bound)
    }

    /// What the session keeps of material bakes beside its build graph, by partition: entries
    /// and bytes of kept field planes, intrinsic rasters of fields and of a stack's outputs, and
    /// the planes of layered stacks. For hosts and benches that report memory.
    pub fn retained_material(&self) -> BTreeMap<&'static str, (usize, usize)> {
        [
            FIELD_PLANES,
            INTRINSIC_RASTERS,
            INTRINSIC_OUTPUTS,
            LAYER_PLANES,
        ]
        .into_iter()
        .map(|partition| (partition.name, self.runtime.retained(partition)))
        .collect()
    }

    /// Another material of the request's modules, compiled as its own request.
    fn plane_source(
        &mut self,
        request: &Request,
        wanted: &program::MaterialRequest,
    ) -> Result<Source, Vec<Diagnostic>> {
        let fail = |message: String| vec![Diagnostic::message(&request.entry, message)];
        let text = if wanted.entry == request.entry {
            Some(&request.source)
        } else {
            request.modules.get(&wanted.entry)
        };
        let own = Request {
            entry: wanted.entry.clone(),
            source: text
                .ok_or_else(|| fail(format!("Missing source module {}.", wanted.entry)))?
                .clone(),
            parameters: wanted.parameters.clone(),
            seed: wanted.seed,
            planes: BTreeMap::new(),
            materials: BTreeMap::new(),
            layers: Vec::new(),
            ..request.clone()
        };
        let program = self.program(&own)?;
        let table = program.dependency_table();
        Ok(Source {
            program,
            request: own,
            table,
            bindings: BTreeMap::new(),
            follow: BTreeMap::new(),
        })
    }

    /// A layered request as the build bakes it: a composite entry that reads every channel of
    /// the stack as the plane of the material that last changed it, the stack's materials (the
    /// base under `stack.base`, the layers under `stack.1`, `stack.2`, ...) and which plane each
    /// of their `base.<channel>` inputs reads. `None` for a request without layers.
    fn layered(
        &mut self,
        request: &Request,
    ) -> Result<Option<(Request, Chain, Vec<Arc<Program>>)>, Vec<Diagnostic>> {
        if request.layers.is_empty() {
            return Ok(None);
        }
        let fail = |message: String| vec![Diagnostic::message(&request.entry, message)];
        if request.layers.len() > MAX_LAYERS {
            return Err(fail(format!(
                "A slot composes at most {MAX_LAYERS} layers over its base."
            )));
        }
        if request.inspection.is_some() {
            return Err(fail(
                "A layered material is inspected through its base and each layer on its own."
                    .into(),
            ));
        }
        let base = self.program(request)?;
        let metadata = base.metadata();
        if metadata.kind != program::RecipeKind::Material
            || metadata
                .outputs
                .iter()
                .any(|output| output == "receiver_normal")
        {
            return Err(fail(
                "The base of a layered slot is a material (defineMaterial) without a receiver normal."
                    .into(),
            ));
        }
        let mut modules = request.modules.clone();
        modules.insert(request.entry.clone(), request.source.clone());
        let mut chain = Chain::default();
        // The material that last changed each channel, from the base up.
        let mut latest: BTreeMap<String, String> = BTreeMap::new();
        for (channel, _) in STACK_CHANNELS {
            if metadata.outputs.iter().any(|output| output == channel) {
                latest.insert(channel.to_owned(), STACK_BASE.to_owned());
            }
        }
        chain.materials.push((
            STACK_BASE.to_owned(),
            program::MaterialRequest {
                entry: request.entry.clone(),
                parameters: request.parameters.clone(),
                seed: request.seed,
            },
            BTreeMap::new(),
        ));
        let mut layers = Vec::with_capacity(request.layers.len());
        for (k, layer) in request.layers.iter().enumerate() {
            let name = format!("stack.{}", k + 1);
            let text = modules
                .get(&layer.entry)
                .ok_or_else(|| fail(format!("Missing source module {}.", layer.entry)))?;
            let program = self.program(&Request {
                entry: layer.entry.clone(),
                source: text.clone(),
                parameters: layer.parameters.clone(),
                seed: layer.seed,
                layers: Vec::new(),
                ..request.clone()
            })?;
            let own = program.metadata();
            if own.kind != program::RecipeKind::Layer || !own.live.is_empty() {
                return Err(vec![Diagnostic::message(
                    &layer.entry,
                    "A slot's layer is a weathering layer (defineLayer) without live operations.",
                )]);
            }
            let reads = own
                .externals
                .iter()
                .filter_map(|external| {
                    let channel = external.name.strip_prefix(program::BASE_PLANE)?;
                    let producer = latest.get(channel)?.clone();
                    Some((external.name.clone(), (producer, channel.to_owned())))
                })
                .collect();
            chain.reads.insert(name.clone(), reads);
            for output in &own.outputs {
                latest.insert(output.clone(), name.clone());
            }
            chain
                .materials
                .push((name, layer_request(layer), layer.follow.clone()));
            layers.push(program);
        }
        let mut text = String::from("material 1;\n");
        // A slot is drawn as its base is: a base without an opacity stays opaque whatever opacity
        // a layer gives (its layers still read the opacity beneath them), so the composite has
        // an opacity only where the base states one.
        let base_opacity = metadata.outputs.iter().any(|output| output == "opacity");
        for (channel, fallback) in STACK_CHANNELS {
            if channel == "opacity" && !base_opacity {
                continue;
            }
            if let Some(producer) = latest.get(channel) {
                let input = format!("{}{channel}", program::BASE_PLANE);
                text.push_str(&format!(
                    "output {channel} = external(\"{input}\", {fallback});\n"
                ));
                chain
                    .entry
                    .insert(input, (producer.clone(), channel.to_owned()));
            }
        }
        // The base's live operations read its fields; the composite reads them as planes, so
        // the live maps are the base's (layers change no live input).
        let mut live_fields: Vec<&str> = Vec::new();
        let mut live = metadata.live.clone();
        for operation in &mut live {
            for field in operation.inputs.values_mut() {
                let at = match live_fields
                    .iter()
                    .position(|known| *known == field.as_str())
                {
                    Some(at) => at,
                    None => {
                        let kind = metadata.fields.iter().find(|known| known.name == *field);
                        let fallback = match kind.map(|known| known.kind) {
                            Some(program::Type::Scalar) => "0",
                            Some(program::Type::Vec2) => "vec2(0)",
                            Some(program::Type::Vec3) => "vec3(0)",
                            _ => {
                                return Err(fail(format!(
                                    "Live input field '{field}' of a layered slot's base is not a number or a vector."
                                )));
                            }
                        };
                        let at = live_fields.len();
                        let base_field = metadata
                            .fields
                            .iter()
                            .find(|known| known.name == *field)
                            .map(|known| known.name.as_str())
                            .expect("a field of the base");
                        live_fields.push(base_field);
                        text.push_str(&format!(
                            "let live_{at} = external(\"live.{at}\", {fallback});\n"
                        ));
                        chain
                            .entry
                            .insert(format!("live.{at}"), (STACK_BASE.to_owned(), field.clone()));
                        at
                    }
                };
                *field = format!("live_{at}");
            }
        }
        if !live.is_empty() {
            // The composite's program is keyed by its text: say which operations it carries.
            let described: Vec<_> = live
                .iter()
                .map(|operation| (&operation.id, operation.version, &operation.inputs))
                .collect();
            text.push_str(&format!(
                "// live operations: {}\n",
                serde_json::to_string(&described).unwrap()
            ));
        }
        chain.live = live;
        let composite = Request {
            entry: COMPOSITE_ENTRY.into(),
            source: text,
            modules,
            parameters: BTreeMap::new(),
            seed: 0,
            layers: Vec::new(),
            ..request.clone()
        };
        let mut programs = vec![base];
        programs.extend(layers);
        Ok(Some((composite, chain, programs)))
    }

    /// The composite a layered request bakes, compiled with the base's live operations. The
    /// composite is the engine's own text, so what fails in it is reported on the slot's base
    /// module (`slot`): a stack whose channels and live inputs exceed what one program reads.
    fn composite(
        &mut self,
        composite: &Request,
        chain: &Chain,
        slot: &str,
    ) -> Result<Arc<Program>, Vec<Diagnostic>> {
        let attribute = |errors: Vec<Diagnostic>| -> Vec<Diagnostic> {
            errors
                .into_iter()
                .map(|error| {
                    if error.source_name != COMPOSITE_ENTRY {
                        return error;
                    }
                    Diagnostic {
                        code: error.code,
                        ..Diagnostic::message(
                            slot,
                            format!(
                                "The layered slot cannot compose its base and layers: {}",
                                error.message
                            ),
                        )
                    }
                })
                .collect()
        };
        let mut program = self.program(composite).map_err(attribute)?;
        if !chain.live.is_empty() {
            let live = program.with_live(chain.live.clone()).map_err(|message| {
                attribute(vec![Diagnostic::message(COMPOSITE_ENTRY, message)])
            })?;
            program = Arc::new(live);
        }
        Ok(program)
    }

    /// Checks a layered slot as its build does before it bakes anything: the stack, the
    /// composite it bakes and each layer's parameters. Hosts that compose the layers themselves
    /// (a GPU preview) report the same errors as a bake.
    pub(super) fn check_layered(&mut self, request: &Request) -> Result<(), Vec<Diagnostic>> {
        let Some((composite, chain, programs)) = self.layered(request)? else {
            return Ok(());
        };
        self.composite(&composite, &chain, &request.entry)?;
        for (program, layer) in programs[1..].iter().zip(&request.layers) {
            let inputs = Inputs {
                parameters: layer.parameters.clone(),
                seed: layer.seed,
                ..Inputs::default()
            };
            program
                .input_fingerprint(&inputs, &program::Dependencies::default())
                .map_err(|error| vec![Diagnostic::from(error)])?;
        }
        Ok(())
    }
}

/// Layers a slot composes over its base at most.
const MAX_LAYERS: usize = 8;
/// The base's material name in a layered build.
const STACK_BASE: &str = "stack.base";
/// The entry of a layered build's composite (see `Session::layered`).
const COMPOSITE_ENTRY: &str = program::LAYERED_COMPOSITE;
/// Channels a layered stack passes up, with the value of each where nothing gives it.
const STACK_CHANNELS: [(&str, &str); 7] = [
    ("base_color", "vec3(0.5)"),
    ("roughness", "1"),
    ("metalness", "0"),
    ("height", "0"),
    ("opacity", "1"),
    ("emissive", "vec3(0)"),
    ("normal_offset", "vec3(0)"),
];

fn layer_request(layer: &super::Layer) -> program::MaterialRequest {
    program::MaterialRequest {
        entry: layer.entry.clone(),
        parameters: layer.parameters.clone(),
        seed: layer.seed,
    }
}

/// A receiver's named features as a layer that follows `follow` sees them: each of the layer's
/// names measures to the receiver's feature it follows, and the receiver's other names stay.
fn followed(
    features: &[program::NamedFeature],
    follow: &BTreeMap<String, program::Follow>,
) -> Result<program::NamedFeatures, String> {
    program::NamedFeatures::new(&program::followed_features(features, follow))
}

/// The node of the plane of `value` (a channel, or a field the base's live operations read) of
/// the stack's material `producer` over receiver `index`, read as input `name`, with the nodes of
/// the planes that value reads, made where not made yet. Returns its id.
fn chain_plane(
    bound: &mut BoundPlanes,
    request: &Request,
    receiver: &super::Receiver,
    index: usize,
    name: &str,
    (producer, value): (&str, &str),
    assets: &dyn Fn(&str) -> Result<Arc<[u8]>, String>,
) -> Result<String, String> {
    let id = plane_id(index, name, Some(producer));
    if bound.nodes.contains_key(&id) {
        return Ok(id);
    }
    let source = &bound.sources[producer];
    let (_, _, read) = source_value(&source.program, &source.table, value)
        .ok_or_else(|| format!("Material '{producer}' has no output {value}."))?;
    let inputs: Vec<String> = read.externals.iter().cloned().collect();
    let mut reads = Vec::new();
    for input in inputs {
        let below = bound.sources[producer].bindings.get(&input).cloned();
        let found = match below {
            Some((beneath, channel)) => Some(chain_plane(
                bound,
                request,
                receiver,
                index,
                &input,
                (&beneath, &channel),
                assets,
            )?),
            None => match request
                .planes
                .get(&receiver.id)
                .and_then(|named| named.get(&input))
            {
                None => None,
                Some(super::PlaneSource::Asset(asset)) => {
                    let id = plane_id(index, &input, None);
                    if !bound.nodes.contains_key(&id) {
                        let bytes = assets(asset).map_err(|e| format!("{asset}: {e}"))?;
                        bound.nodes.insert(
                            id.clone(),
                            PlaneNode {
                                index,
                                name: input.clone(),
                                bound: Bound::Asset(bytes),
                                reads: Vec::new(),
                            },
                        );
                    }
                    Some(id)
                }
                Some(super::PlaneSource::Material { .. }) => {
                    return Err(format!(
                        "Input '{input}' of a layered stack reads an asset plane or the surface beneath, not another material."
                    ));
                }
            },
        };
        if let Some(found) = found {
            reads.push((input, found));
        }
    }
    bound.nodes.insert(
        id.clone(),
        PlaneNode {
            index,
            name: name.to_owned(),
            bound: Bound::Material {
                material: producer.to_owned(),
                value: value.to_owned(),
            },
            reads,
        },
    );
    Ok(id)
}

/// What a plane baked from `value` of a material holds: the field's index, or the output's
/// name; the components of a value; and what the value reads.
fn source_value<'a>(
    program: &Program,
    table: &'a program::DependencyTable,
    value: &str,
) -> Option<(Option<usize>, usize, &'a program::Dependencies)> {
    let fields = &program.metadata().fields;
    if let Some(index) = fields.iter().position(|field| field.name == value) {
        let components = match fields[index].kind {
            program::Type::Scalar => 1,
            program::Type::Vec2 => 2,
            program::Type::Vec3 => 3,
            _ => return None,
        };
        return Some((Some(index), components, table.fields.get(value)?));
    }
    let components = match value {
        "base_color" | "emissive" | "normal_offset" => 3,
        "roughness" | "metalness" | "height" | "opacity" => 1,
        _ => return None,
    };
    Some((None, components, table.outputs.get(value)?))
}

/// An output of an evaluation, as a plane holds it.
fn output_of(result: &program::Outputs, value: &str) -> [f64; 3] {
    match value {
        "base_color" => result.base_color,
        "emissive" => result.emissive,
        "normal_offset" => result.normal_offset.unwrap_or_default(),
        "roughness" => [result.roughness, 0., 0.],
        "metalness" => [result.metalness, 0., 0.],
        "height" => [result.height, 0., 0.],
        _ => [result.opacity, 0., 0.],
    }
}

/// Adds the nodes of receiver `index`'s bound planes, with the nodes of the materials they are
/// baked from (program and decoded images, under that material's name) where not planned yet.
fn plane_nodes(
    bound: &BoundPlanes,
    index: usize,
    assets: &dyn Fn(&str) -> Result<Arc<[u8]>, String>,
    nodes: &mut Vec<Node>,
    image_bytes: &mut BTreeMap<String, Arc<[u8]>>,
    layered: bool,
) -> Result<(), String> {
    for (id, plane) in bound.nodes.iter().filter(|(_, plane)| plane.index == index) {
        let name = &plane.name;
        let mut dependencies = vec![dependency("mapping", format!("layout/{index}"))];
        // A layered build's composite entry changes with the channels of its stack, which a
        // plane of the stack never depends on.
        if !layered {
            dependencies.push(dependency("program", "program"));
        }
        let mut inputs = name.as_bytes().to_vec();
        inputs.push(0);
        let implementation = match &plane.bound {
            Bound::Asset(bytes) => {
                inputs.extend(image_digest(bytes));
                "material-plane-v1"
            }
            Bound::Material { material, value } => {
                let source = &bound.sources[material];
                let (program_id, assets_id) =
                    (format!("program:{material}"), format!("assets:{material}"));
                if !nodes.iter().any(|node| node.id == program_id) {
                    let (planned, images) = source_nodes(&source.program, &source.request, assets)?;
                    for mut planned in planned {
                        match planned.id.as_str() {
                            "program" => planned.id = program_id.clone(),
                            "assets" => planned.id = assets_id.clone(),
                            _ => {}
                        }
                        if !nodes.iter().any(|node| node.id == planned.id) {
                            nodes.push(planned);
                        }
                    }
                    image_bytes.extend(images);
                }
                // Keyed by what the value reads of that material's request, like a map.
                let (_, _, read) = source_value(&source.program, &source.table, value)
                    .expect("a checked source value");
                inputs.extend(value.as_bytes());
                inputs.push(0);
                match source.program.input_fingerprint(
                    &Inputs {
                        parameters: source.request.parameters.clone(),
                        seed: source.request.seed,
                        edges: source.request.edges.clone(),
                        ..Inputs::default()
                    },
                    read,
                ) {
                    Ok(fingerprint) => inputs.extend(fingerprint),
                    Err(error) => inputs.extend(error.to_string().bytes()),
                }
                dependencies.push(dependency("source", program_id));
                dependencies.push(dependency("source-assets", assets_id));
                if PlaneNode::layered(id) {
                    // What the layer follows is part of what it reads; the planes beneath are
                    // dependencies, so an edit below bakes this again and one above never does.
                    inputs.push(0);
                    inputs.extend(json(&source.follow));
                    for (input, beneath) in &plane.reads {
                        dependencies.push(dependency(&format!("plane/{input}"), beneath.clone()));
                    }
                    "material-layer-plane-v1"
                } else {
                    "material-plane-bake-v1"
                }
            }
        };
        nodes.push(node(id.clone(), implementation, inputs, dependencies));
    }
    Ok(())
}

/// What one block of rows of an output plane bake found, in the order the planes take it.
#[derive(Default)]
struct OutputRows {
    /// Texels of new sample points, each with a value of every wanted output in `values`.
    sets: Vec<usize>,
    values: Vec<[f64; 3]>,
    /// Points one step along a chart leg that are no texel's center: texel and leg, each with a
    /// value of every wanted output in `leg_values`.
    legs: Vec<(usize, usize)>,
    leg_values: Vec<[f64; 3]>,
    /// Texels that repeat the texel before them in their row, in order.
    repeats: Vec<usize>,
    /// Values for the intrinsic rasters: wanted slot, lattice point, value.
    kept: Vec<(usize, LatticePoint, [f64; 3])>,
    /// Points read from the rasters, and points evaluated.
    held: usize,
    evaluated: usize,
}

/// Evaluates `outputs` at `points` (texel or leg index for the planes, and sample), taking the
/// values of samples every raster holds from the rasters and adding to `out` the values of the
/// wanted outputs for each point, in order.
#[allow(clippy::too_many_arguments)]
fn output_points(
    evaluator: &mut program::Evaluator<'_>,
    points: &[(usize, program::Sample)],
    outputs: program::OutputSet,
    values: &[&str],
    externals: &[&program::Plane],
    rasters: &[Option<&IntrinsicRaster>],
    out: &mut Vec<[f64; 3]>,
    kept: &mut Vec<(usize, LatticePoint, [f64; 3])>,
    counts: &mut [usize; 2],
) -> Result<(), Diagnostic> {
    let any_raster = rasters.iter().any(Option::is_some);
    let every_raster = !rasters.is_empty() && rasters.iter().all(Option::is_some);
    let mut lattice: Vec<Option<LatticePoint>> = Vec::with_capacity(points.len());
    let mut held: Vec<bool> = Vec::with_capacity(points.len());
    let mut samples = Vec::with_capacity(points.len());
    let mut texels = Vec::with_capacity(points.len());
    for (texel, sample) in points {
        let point = if any_raster {
            IntrinsicRaster::point(sample)
        } else {
            None
        };
        let found = every_raster
            && point.is_some_and(|point| {
                rasters
                    .iter()
                    .all(|raster| raster.is_some_and(|raster| raster.get(point).is_some()))
            });
        lattice.push(point);
        held.push(found);
        if !found {
            samples.push(sample.clone());
            texels.push(*texel);
        }
    }
    counts[0] += points.len() - samples.len();
    counts[1] += samples.len();
    let results = if samples.is_empty() {
        Vec::new()
    } else {
        let mut scratch = Vec::new();
        evaluator
            .fill_row(
                &samples,
                &texels,
                outputs,
                &program::FieldSelection::default(),
                &mut scratch,
                externals,
                &mut [],
            )
            .map_err(Diagnostic::from)?
    };
    let mut results = results.iter();
    for (point, found) in lattice.into_iter().zip(held) {
        if found {
            let point = point.expect("a held point is on the lattice");
            for raster in rasters {
                let value = raster.expect("every raster").get(point).expect("held");
                out.push([value[0], value[1], value[2]]);
            }
            continue;
        }
        let result = results.next().expect("a result for each evaluated point");
        for (slot, value) in values.iter().enumerate() {
            let value = output_of(result, value);
            out.push(value);
            if let (Some(point), Some(raster)) = (point, rasters.get(slot).copied().flatten()) {
                if raster.get(point).is_none() {
                    kept.push((slot, point, value));
                }
            }
        }
    }
    Ok(())
}

/// Evaluates the rows of an output plane bake (see [`bake_output_planes`]).
#[allow(clippy::too_many_arguments)]
fn output_rows(
    evaluator: &mut program::Evaluator<'_>,
    atlas: &Atlas,
    rows: &[(usize, usize)],
    outputs: program::OutputSet,
    values: &[&str],
    externals: &[&program::Plane],
    rasters: &[Option<&IntrinsicRaster>],
    entry: &str,
) -> Result<OutputRows, Diagnostic> {
    let failure = |message: String| Diagnostic::message(entry, message);
    let pixels = atlas.width * atlas.height;
    let mut found = OutputRows::default();
    let mut counts = [0; 2];
    for &(chart_index, y) in rows {
        let chart = &atlas.charts[chart_index];
        evaluator.replenish_row(chart.width);
        let mut before: Option<(usize, u64, u64)> = None;
        let mut fresh = Vec::with_capacity(chart.width);
        let mut points = Vec::with_capacity(chart.width);
        for x in 0..chart.width {
            let (triangle, u, v) = atlas.texel(chart, x, y);
            let key = (triangle, u.to_bits(), v.to_bits());
            let texel = (chart.y + y) * atlas.width + chart.x + x;
            fresh.push(before != Some(key));
            if before != Some(key) {
                points.push((texel, atlas.sample(triangle, u, v).map_err(&failure)?));
                found.sets.push(texel);
            } else {
                found.repeats.push(texel);
            }
            before = Some(key);
        }
        output_points(
            evaluator,
            &points,
            outputs,
            values,
            externals,
            rasters,
            &mut found.values,
            &mut found.kept,
            &mut counts,
        )?;
        // A relief over these values needs them one step along each chart leg too, where that
        // point is no adjacent texel's center (see `bake_block`): the values are evaluated at
        // exactly the samples a bake of this material alone takes there, reading the planes
        // beneath at their own leg points.
        let mut legs = Vec::new();
        for (x, _) in fresh.iter().enumerate().filter(|(_, new)| **new) {
            let (triangle, u, v) = atlas.texel(chart, x, y);
            let [step_u, step_v] = atlas.steps(triangle);
            for (leg, target) in [[u + step_u, v], [u, v + step_v]].into_iter().enumerate() {
                let adjacent = neighbor_texel(
                    |nx, ny| atlas.texel(chart, nx, ny),
                    chart,
                    [x, y],
                    triangle,
                    target,
                );
                if adjacent.is_none() {
                    let texel = (chart.y + y) * atlas.width + chart.x + x;
                    let sample = atlas.sample(triangle, target[0], target[1]);
                    legs.push((
                        program::Plane::leg(pixels, texel, leg),
                        sample.map_err(&failure)?,
                    ));
                    found.legs.push((texel, leg));
                }
            }
        }
        if !legs.is_empty() {
            evaluator.replenish_row(legs.len());
            output_points(
                evaluator,
                &legs,
                outputs,
                values,
                externals,
                rasters,
                &mut found.leg_values,
                &mut found.kept,
                &mut counts,
            )?;
        }
    }
    [found.held, found.evaluated] = counts;
    Ok(found)
}

/// Bakes outputs of a material over a receiver's atlas into planes bound to the external inputs
/// `wanted` names (input name, output), exactly as that material's own bake evaluates them at
/// each texel, with no filtering estimate: in the exact form, with the values one step along a
/// chart leg a relief over them needs. The material reads `externals` (the planes of a layered
/// stack beneath it) at each texel and leg point. Rows are split across `threads` and written
/// back in order, so threads never change a value; values on the lattice of an intrinsic output
/// are read from and added to its raster (`rasters`, by wanted slot), which never changes one
/// either. With `only`, just those rows are baked (a texel's values read nothing of other
/// rows), into planes that hold the texels from the first of them to the last.
#[allow(clippy::too_many_arguments)]
fn bake_output_planes(
    program: &Program,
    table: &program::DependencyTable,
    inputs: &Inputs,
    atlas: &Atlas,
    named: &program::NamedFeatures,
    wanted: &[(&str, &str)],
    externals: &[&program::Plane],
    threads: usize,
    rasters: &mut [Option<&mut IntrinsicRaster>],
    only: Option<&RowSet>,
    cancelled: &dyn Fn() -> bool,
) -> Result<Vec<program::Plane>, Diagnostic> {
    #[cfg(test)]
    PLANE_BAKES.with(|bakes| {
        let values = wanted.iter().map(|(_, value)| (*value).to_owned());
        bakes.borrow_mut().push(values.collect())
    });
    let entry = program.metadata().entry.as_str();
    let failure = |message: String| Diagnostic::message(entry, message);
    let pixels = atlas.width * atlas.height;
    let mut rows = atlas_rows(atlas);
    if let Some(only) = only {
        rows.retain(|row| only.contains(row));
    }
    let window = rows_window(atlas, only);
    let mut outputs = program::OutputSet::NONE;
    let mut planes = Vec::with_capacity(wanted.len());
    let mut values = Vec::with_capacity(wanted.len());
    for (name, value) in wanted {
        let (field, components, _) = source_value(program, table, value)
            .ok_or_else(|| failure(format!("No output '{value}'.")))?;
        if field.is_some() || pixels * components * 8 + 64 > PLANE_NODE_BYTES {
            return Err(failure(format!(
                "Output plane '{name}' is not an output or exceeds what a plane may hold."
            )));
        }
        outputs = outputs.union(program::OutputSet::named(value));
        planes.push(
            program::Plane::exact_window(name, components, pixels, window.clone())
                .map_err(&failure)?,
        );
        values.push(*value);
    }
    let evaluator = || -> Result<program::Evaluator<'_>, Diagnostic> {
        static NONE: std::sync::OnceLock<program::FeatureSet> = std::sync::OnceLock::new();
        let features = if queries_features(program) {
            atlas.features().0
        } else {
            NONE.get_or_init(program::FeatureSet::default)
        };
        program
            .evaluator_with_feature_sets(inputs, features, named)
            .map_err(Diagnostic::from)
    };
    let threads = threads.max(1);
    let mut serial = (threads == 1).then(&evaluator).transpose()?;
    // Points read from the rasters and points evaluated, for tests and reports.
    let mut points = [0usize; 2];
    for batch in rows.chunks(threads * BLOCK_ROWS) {
        if cancelled() {
            return Err(failure("Material authoring cancelled.".into()));
        }
        let reading: Vec<Option<&IntrinsicRaster>> =
            rasters.iter().map(|raster| raster.as_deref()).collect();
        let results: Vec<Result<OutputRows, Diagnostic>> = match &mut serial {
            Some(evaluator) => vec![output_rows(
                evaluator, atlas, batch, outputs, &values, externals, &reading, entry,
            )],
            None => std::thread::scope(|scope| {
                let handles: Vec<_> = batch
                    .chunks(batch.len().div_ceil(threads))
                    .map(|share| {
                        let (evaluator, values, reading) = (&evaluator, &values, &reading);
                        scope.spawn(move || -> Result<OutputRows, Diagnostic> {
                            let mut evaluator = evaluator()?;
                            output_rows(
                                &mut evaluator,
                                atlas,
                                share,
                                outputs,
                                values,
                                externals,
                                reading,
                                entry,
                            )
                        })
                    })
                    .collect();
                handles
                    .into_iter()
                    .map(|handle| handle.join().expect("plane rows do not panic"))
                    .collect()
            }),
        };
        drop(reading);
        let count = values.len();
        for result in results {
            let found = result?;
            points[0] += found.held;
            points[1] += found.evaluated;
            for (at, texel) in found.sets.iter().enumerate() {
                for (slot, plane) in planes.iter_mut().enumerate() {
                    plane.set(*texel, &found.values[at * count + slot]);
                }
            }
            for (at, (texel, leg)) in found.legs.iter().enumerate() {
                for (slot, plane) in planes.iter_mut().enumerate() {
                    plane.set_leg(*texel, *leg, None, &found.leg_values[at * count + slot]);
                }
            }
            for texel in &found.repeats {
                for plane in planes.iter_mut() {
                    plane.repeat(texel - 1, *texel);
                }
            }
            for (slot, point, value) in found.kept {
                if let Some(raster) = rasters.get_mut(slot).and_then(Option::as_deref_mut) {
                    raster.put(point, [value[0], value[1], value[2], 0.]);
                }
            }
        }
    }
    #[cfg(test)]
    LAYER_BAKES.with(|bakes| {
        let values = values.iter().map(|value| (*value).to_owned()).collect();
        bakes
            .borrow_mut()
            .push((entry.to_owned(), values, points[0], points[1]))
    });
    #[cfg(not(test))]
    let _ = points;
    Ok(planes)
}

/// Bakes values of another material of the request over a receiver's atlas into planes bound to
/// the external inputs `wanted` names (input name, value), exactly as that material's own bake
/// evaluates them at each texel: fields keep their filtering estimates, outputs have none. A
/// texel that samples the point of the texel before it in its row repeats that texel, as the
/// maps baked from these planes do. With `only`, just those rows are baked.
fn bake_source_planes(
    source: &Source,
    decoded: &Inputs,
    atlas: &Atlas,
    wanted: &[(&str, &str)],
    only: Option<&RowSet>,
    cancelled: &dyn Fn() -> bool,
) -> Result<Vec<program::Plane>, Diagnostic> {
    #[cfg(test)]
    PLANE_BAKES.with(|bakes| {
        let values = wanted.iter().map(|(_, value)| (*value).to_owned());
        bakes.borrow_mut().push(values.collect())
    });
    let program = &source.program;
    let entry = program.metadata().entry.as_str();
    let failure = |message: String| Diagnostic::message(entry, message);
    let pixels = atlas.width * atlas.height;
    if pixels * 32 + 64 > PLANE_NODE_BYTES {
        return Err(failure(
            "The receiver's atlas exceeds what a plane may hold.".into(),
        ));
    }
    let inputs = Inputs {
        parameters: source.request.parameters.clone(),
        seed: source.request.seed,
        images: decoded.images.clone(),
        edges: source.request.edges.clone(),
        features: Vec::new(),
        mips: decoded.mips.clone(),
        render: None,
    };
    // Planes of fields are filled by the evaluator; planes of outputs from each sample's result.
    let window = rows_window(atlas, only);
    let mut filled: Vec<program::Plane> = Vec::new();
    let mut field_names: Vec<&str> = Vec::new();
    let mut outputs = program::OutputSet::NONE;
    let mut output_planes: Vec<(usize, &str, program::Plane)> = Vec::new();
    let mut order: Vec<(bool, usize)> = Vec::new();
    for (slot, (name, value)) in wanted.iter().enumerate() {
        let (field, components, _) = source_value(program, &source.table, value)
            .ok_or_else(|| failure(format!("No field or output '{value}'.")))?;
        match field {
            Some(index) => {
                order.push((true, filled.len()));
                filled.push(program::Plane::new_window(
                    index,
                    components,
                    pixels,
                    window.clone(),
                ));
                field_names.push(value);
            }
            None => {
                outputs = outputs.union(program::OutputSet::named(value));
                let plane = program::Plane::exact_window(name, components, pixels, window.clone())
                    .map_err(&failure)?;
                order.push((false, output_planes.len()));
                output_planes.push((slot, value, plane));
            }
        }
    }
    let selection = program.select_fields(&field_names).map_err(&failure)?;
    let mut evaluator = receiver_evaluator(program, &inputs, atlas).map_err(Diagnostic::from)?;
    let mut values = Vec::new();
    for (chart_index, chart) in atlas.charts.iter().enumerate() {
        for y in 0..chart.height {
            if only.is_some_and(|only| !only.contains(&(chart_index, y))) {
                continue;
            }
            if cancelled() {
                return Err(failure("Material authoring cancelled.".into()));
            }
            evaluator.replenish_row(chart.width);
            let mut before: Option<(usize, u64, u64)> = None;
            let mut fresh = Vec::with_capacity(chart.width);
            let mut samples = Vec::new();
            let mut texels = Vec::new();
            for x in 0..chart.width {
                let (triangle, u, v) = atlas.texel(chart, x, y);
                let key = (triangle, u.to_bits(), v.to_bits());
                fresh.push(before != Some(key));
                if before != Some(key) {
                    samples.push(atlas.sample(triangle, u, v).map_err(&failure)?);
                    texels.push((chart.y + y) * atlas.width + chart.x + x);
                }
                before = Some(key);
            }
            let results = evaluator
                .fill_row(
                    &samples,
                    &texels,
                    outputs,
                    &selection,
                    &mut values,
                    &[],
                    &mut filled,
                )
                .map_err(Diagnostic::from)?;
            for (result, texel) in results.iter().zip(&texels) {
                for (_, value, plane) in output_planes.iter_mut() {
                    plane.set(*texel, &output_of(result, value));
                }
            }
            // A relief over these values needs them one step along each chart leg too, where
            // that point is no adjacent texel's center (see `bake_block`): the values are
            // evaluated at exactly the samples a bake of this material alone takes there.
            let mut leg_samples = Vec::new();
            let mut leg_of = Vec::new();
            for (x, _) in fresh.iter().enumerate().filter(|(_, new)| **new) {
                let (triangle, u, v) = atlas.texel(chart, x, y);
                let [step_u, step_v] = atlas.steps(triangle);
                for (leg, target) in [[u + step_u, v], [u, v + step_v]].into_iter().enumerate() {
                    let adjacent = neighbor_texel(
                        |nx, ny| atlas.texel(chart, nx, ny),
                        chart,
                        [x, y],
                        triangle,
                        target,
                    );
                    if adjacent.is_none() {
                        let sample = atlas.sample(triangle, target[0], target[1]);
                        leg_samples.push(sample.map_err(&failure)?);
                        leg_of.push(((chart.y + y) * atlas.width + chart.x + x, leg));
                    }
                }
            }
            if !leg_samples.is_empty() {
                let ordinals: Vec<usize> = (0..leg_samples.len()).collect();
                let mut beside: Vec<program::Plane> = filled
                    .iter()
                    .map(|plane| {
                        let field = plane.field().expect("a field's plane");
                        program::Plane::new(field, plane.components(), leg_samples.len())
                    })
                    .collect();
                evaluator.replenish_row(leg_samples.len());
                let results = evaluator
                    .fill_row(
                        &leg_samples,
                        &ordinals,
                        outputs,
                        &selection,
                        &mut values,
                        &[],
                        &mut beside,
                    )
                    .map_err(Diagnostic::from)?;
                for (at, ((texel, leg), result)) in leg_of.iter().zip(&results).enumerate() {
                    for (plane, beside) in filled.iter_mut().zip(&beside) {
                        plane.set_leg(*texel, *leg, Some((beside, at)), &[]);
                    }
                    for (_, value, plane) in output_planes.iter_mut() {
                        plane.set_leg(*texel, *leg, None, &output_of(result, value));
                    }
                }
            }
            for (x, new) in fresh.into_iter().enumerate() {
                if !new {
                    let texel = (chart.y + y) * atlas.width + chart.x + x;
                    let planes = filled
                        .iter_mut()
                        .chain(output_planes.iter_mut().map(|(_, _, plane)| plane));
                    for plane in planes {
                        plane.repeat(texel - 1, texel);
                    }
                }
            }
        }
    }
    let mut filled: Vec<Option<program::Plane>> = filled.into_iter().map(Some).collect();
    let mut output_planes: Vec<Option<program::Plane>> = output_planes
        .into_iter()
        .map(|(_, _, plane)| Some(plane))
        .collect();
    order
        .into_iter()
        .zip(wanted)
        .map(|((field, at), (name, _))| {
            if field {
                let plane = filled[at].take().expect("each plane once");
                plane.into_external(name).map_err(&failure)
            } else {
                Ok(output_planes[at].take().expect("each plane once"))
            }
        })
        .collect()
}

/// Rows of an atlas, as (chart, row in the chart).
type RowSet = std::collections::BTreeSet<(usize, usize)>;

/// The texels from the first of `only`'s rows to the last (all of the atlas without it): the
/// window of a plane baked over those rows.
fn rows_window(atlas: &Atlas, only: Option<&RowSet>) -> std::ops::Range<usize> {
    let pixels = atlas.width * atlas.height;
    let Some(only) = only else {
        return 0..pixels;
    };
    only.iter()
        .map(|&(chart, y)| {
            let chart = &atlas.charts[chart];
            let start = (chart.y + y) * atlas.width + chart.x;
            start..start + chart.width
        })
        .fold(pixels..0, |window, row| {
            window.start.min(row.start)..window.end.max(row.end)
        })
}

/// The rows whose planes a share of a layered bake reads: its own (see [`bake_rows`]) and the
/// rows of the same chart just above and below them, whose heights its relief reads.
fn share_rows(atlas: &Atlas, index: usize, count: usize) -> RowSet {
    let rows = atlas_rows(atlas);
    let (first, last) = (rows.len() * index / count, rows.len() * (index + 1) / count);
    let mut wanted = RowSet::new();
    for &(chart, y) in &rows[first..last] {
        wanted.insert((chart, y));
        if y > 0 {
            wanted.insert((chart, y - 1));
        }
        if y + 1 < atlas.charts[chart].height {
            wanted.insert((chart, y + 1));
        }
    }
    wanted
}

/// The decoded images of a layered stack's materials, by material name, among the artifacts
/// of a node's dependencies or a run's targets whose names start with `prefix`.
fn stack_sources(
    artifacts: &BTreeMap<String, Arc<Artifact>>,
    prefix: &str,
) -> BTreeMap<String, Arc<Inputs>> {
    artifacts
        .iter()
        .filter_map(|(name, artifact)| match artifact.as_ref() {
            Artifact::Inputs(inputs) => {
                Some((name.strip_prefix(prefix)?.to_owned(), inputs.clone()))
            }
            _ => None,
        })
        .collect()
}

/// A layered stack's planes in one build (see [`Request::layers`]). Planes are not nodes the
/// build graph runs: the maps that read them make them when those maps' rows are evaluated
/// here, so a map whose rows other sessions evaluated (each making the stack over its own
/// rows) costs none, and a map an earlier build kept costs none either.
#[derive(Default)]
struct StackPlanes {
    /// Planes by node id: kept by an earlier build, or made by this one.
    made: BTreeMap<String, Arc<BakedReceiver>>,
    /// Planes this build made, to keep when it finishes; empty when it keeps none.
    baked: Vec<(String, Arc<BakedReceiver>)>,
    /// Whether the build keeps what it makes. A build that does not lets each receiver's planes
    /// go once its maps are baked, and each plane once nothing more of its receiver reads it.
    keep: bool,
    /// The rasters of the stack's intrinsic outputs the build reads and adds to, and which
    /// plane each serves.
    rasters: Rasters,
    raster_of: BTreeMap<String, ContentKey>,
}

impl StackPlanes {
    /// Lets the planes of receiver `index` go (a build that keeps none, once its maps are baked).
    fn release(&mut self, context: &StackContext<'_>, index: usize) {
        self.made.retain(|id, _| {
            context
                .planes
                .nodes
                .get(id)
                .is_none_or(|node| node.index != index)
        });
    }
}

/// What a layered build makes its stack's planes from.
struct StackContext<'a> {
    planes: &'a BoundPlanes,
    /// The composite program, which with the stack's materials says how an asset plane is read.
    compiled: &'a Program,
    /// The composite request: its receivers' named features and its planes.
    request: &'a Request,
    receivers: &'a [super::Receiver],
    threads: usize,
}

impl StackContext<'_> {
    /// Makes the planes `wanted` of receiver `index` (node ids), and the planes they read,
    /// over `only` rows or all. Each material bakes the planes of it the receiver needs in one
    /// pass, after the planes beneath it; planes kept by an earlier build are read, not made.
    /// `sources` holds each material's decoded images.
    #[allow(clippy::too_many_arguments)]
    fn make(
        &self,
        stack: &mut StackPlanes,
        index: usize,
        atlas: &Atlas,
        sources: &BTreeMap<String, Arc<Inputs>>,
        wanted: &[String],
        only: Option<&RowSet>,
        stopped: &dyn Fn() -> bool,
    ) -> Result<(), Diagnostic> {
        let mut needed = std::collections::BTreeSet::new();
        let mut pending: Vec<&str> = wanted.iter().map(String::as_str).collect();
        while let Some(id) = pending.pop() {
            if stack.made.contains_key(id) || !needed.insert(id.to_owned()) {
                continue;
            }
            pending.extend(
                self.planes.nodes[id]
                    .reads
                    .iter()
                    .map(|(_, read)| read.as_str()),
            );
        }
        for id in wanted {
            self.ensure(
                stack, index, atlas, sources, id, &needed, wanted, only, stopped,
            )?;
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn ensure(
        &self,
        stack: &mut StackPlanes,
        index: usize,
        atlas: &Atlas,
        sources: &BTreeMap<String, Arc<Inputs>>,
        id: &str,
        needed: &std::collections::BTreeSet<String>,
        wanted: &[String],
        only: Option<&RowSet>,
        stopped: &dyn Fn() -> bool,
    ) -> Result<(), Diagnostic> {
        if stack.made.contains_key(id) {
            return Ok(());
        }
        let entry = self.request.entry.as_str();
        let node = &self.planes.nodes[id];
        let (material, value) = match &node.bound {
            Bound::Asset(bytes) => {
                let components = plane_components(self.compiled, self.planes, &node.name);
                let (decoded, _) = decoded_plane(components, &node.name, bytes, atlas)
                    .map_err(|message| Diagnostic::message(entry, message))?;
                let Artifact::Material(plane) = decoded else {
                    unreachable!("a decoded plane")
                };
                stack.made.insert(id.to_owned(), plane);
                return Ok(());
            }
            Bound::Material { material, value } => (material, value),
        };
        for (_, read) in &node.reads {
            self.ensure(
                stack, index, atlas, sources, read, needed, wanted, only, stopped,
            )?;
        }
        let source = &self.planes.sources[material];
        // A stack's field planes (a base's live inputs) and output planes are baked apart:
        // outputs have the faster pass.
        let field = |value: &str| {
            matches!(
                source_value(&source.program, &source.table, value),
                Some((Some(_), _, _))
            )
        };
        let own_field = field(value);
        let held: Vec<&String> = node.reads.iter().map(|(_, read)| read).collect();
        let mut group: Vec<(&str, &str, &str)> = vec![(id, &node.name, value)];
        for (other_id, other) in &self.planes.nodes {
            let Bound::Material {
                material: from,
                value,
            } = &other.bound
            else {
                continue;
            };
            if from == material
                && other_id != id
                && other.index == index
                && PlaneNode::layered(other_id)
                && needed.contains(other_id)
                && !stack.made.contains_key(other_id)
                && other.reads.iter().all(|(_, read)| held.contains(&read))
                && field(value) == own_field
            {
                group.push((other_id, &other.name, value));
            }
        }
        let decoded = sources.get(material).ok_or_else(|| {
            Diagnostic::message(
                entry,
                format!("The images of '{material}' are not decoded."),
            )
        })?;
        let pairs: Vec<(&str, &str)> = group
            .iter()
            .map(|(_, name, value)| (*name, *value))
            .collect();
        let StackPlanes {
            made,
            baked,
            keep,
            rasters,
            raster_of,
        } = stack;
        let baked_now = if own_field {
            bake_source_planes(source, decoded, atlas, &pairs, only, stopped)?
        } else {
            let inputs = Inputs {
                parameters: source.request.parameters.clone(),
                seed: source.request.seed,
                images: decoded.images.clone(),
                edges: source.request.edges.clone(),
                features: Vec::new(),
                mips: decoded.mips.clone(),
                render: None,
            };
            let renamed;
            let named = if source.follow.is_empty() {
                &atlas.named
            } else {
                let features = self
                    .request
                    .features
                    .get(&self.receivers[index].id)
                    .map_or(&[][..], Vec::as_slice);
                renamed = followed(features, &source.follow)
                    .map_err(|message| Diagnostic::message(entry, message))?;
                &renamed
            };
            let externals: Vec<&program::Plane> = node
                .reads
                .iter()
                .filter_map(|(_, read)| match made.get(read)?.as_ref() {
                    BakedReceiver::Plane(plane) => Some(plane),
                    _ => None,
                })
                .collect();
            // The rasters of the intrinsic outputs, out of the build's map while this pass reads
            // and fills them.
            let mut taken: Vec<Option<(ContentKey, WorkingRaster)>> = group
                .iter()
                .map(|(made, _, _)| {
                    let key = raster_of.get(*made)?;
                    Some((*key, rasters.remove(key)?))
                })
                .collect();
            let before: Vec<usize> = taken
                .iter()
                .map(|taken| taken.as_ref().map_or(0, |(_, w)| w.raster.points))
                .collect();
            let mut reading: Vec<Option<&mut IntrinsicRaster>> = taken
                .iter_mut()
                .map(|taken| taken.as_mut().map(|(_, w)| &mut w.raster))
                .collect();
            let result = bake_output_planes(
                &source.program,
                &source.table,
                &inputs,
                atlas,
                named,
                &pairs,
                &externals,
                self.threads,
                &mut reading,
                only,
                stopped,
            );
            drop(reading);
            for (taken, before) in taken.into_iter().zip(before) {
                if let Some((key, mut working)) = taken {
                    working.changed |= working.raster.points != before;
                    rasters.insert(key, working);
                }
            }
            result?
        };
        for ((made_id, _, _), plane) in group.iter().zip(baked_now) {
            let plane = Arc::new(BakedReceiver::Plane(plane));
            if *keep {
                baked.push(((*made_id).to_owned(), plane.clone()));
            }
            made.insert((*made_id).to_owned(), plane);
        }
        // A build that keeps no plane lets each go once nothing more of the receiver reads it.
        if !*keep {
            let planes = self.planes;
            let unread: Vec<String> = made
                .keys()
                .filter(|candidate| {
                    planes
                        .nodes
                        .get(*candidate)
                        .is_some_and(|node| node.index == index)
                        && !wanted.contains(candidate)
                        && needed.iter().all(|consumer| {
                            made.contains_key(consumer)
                                || !planes.nodes[consumer]
                                    .reads
                                    .iter()
                                    .any(|(_, read)| read == *candidate)
                        })
                })
                .cloned()
                .collect();
            for id in unread {
                made.remove(&id);
            }
        }
        Ok(())
    }
}

/// Whether rows evaluated elsewhere (`have` says what they carry) fit `atlas` and hold the data
/// of the maps `wanted` (see [`bake_groups`]), which then merges them instead of evaluating.
fn provided_fits(
    atlas: &Atlas,
    wanted: Groups,
    live_count: usize,
    have: Groups,
    provided: &[Row],
) -> bool {
    let data = if live_count == 0 {
        wanted.rows().without(Group::Live)
    } else {
        wanted.rows()
    };
    let has = |group: Group| data.contains(group);
    // The width of every chart row, in the order rows are merged (see `atlas_rows`).
    let cells = || {
        atlas
            .charts
            .iter()
            .flat_map(|chart| std::iter::repeat_n(chart.width, chart.height))
    };
    have.contains_all(data)
        && provided.len() == cells().count()
        && provided.iter().zip(cells()).all(|(row, cell)| {
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
}

impl Session {
    /// Keeps what a finished build made of its layered stacks: their planes (under `key`) and the
    /// rasters of intrinsic outputs that gained values; nothing for a build that keeps none.
    fn keep_stack(&self, stack: StackPlanes, key: &dyn Fn(&str) -> ContentKey, keep: bool) {
        if !keep {
            return;
        }
        for (raster, working) in stack.rasters {
            if working.changed {
                let bytes = working.raster.bytes();
                self.runtime.retain(
                    INTRINSIC_OUTPUTS,
                    raster,
                    Artifact::Material(Arc::new(BakedReceiver::Intrinsic(working.raster))),
                    bytes,
                    true,
                );
            }
        }
        self.keep_planes(stack.baked, key);
    }

    /// Keeps planes of a layered stack under `key`, apart from the build graph's store.
    fn keep_planes(
        &self,
        planes: Vec<(String, Arc<BakedReceiver>)>,
        key: &dyn Fn(&str) -> ContentKey,
    ) {
        for (id, plane) in planes {
            let bytes = plane.bytes();
            self.runtime.retain(
                LAYER_PLANES,
                key(&id),
                Artifact::Material(plane),
                bytes,
                false,
            );
        }
    }
}

/// A plane baked for the external input `name`, as a `plane/` node's artifact, checked against
/// the type the program reads it as.
fn baked_plane(
    program: &Program,
    name: &str,
    plane: program::Plane,
) -> Result<Arc<BakedReceiver>, String> {
    let wanted = program
        .metadata()
        .externals
        .iter()
        .find(|external| external.name == name)
        .map(|external| match external.kind {
            program::Type::Scalar => 1,
            program::Type::Vec2 => 2,
            _ => 3,
        });
    if wanted != Some(plane.components()) {
        return Err(format!(
            "External plane '{name}' holds {} components where the program reads {}.",
            plane.components(),
            wanted.unwrap_or(0)
        ));
    }
    Ok(Arc::new(BakedReceiver::Plane(plane)))
}

/// The components a program of the build reads the external input `name` as: the entry, or
/// else a material planes are baked from.
fn plane_components(program: &Program, planes: &BoundPlanes, name: &str) -> Option<usize> {
    std::iter::once(program)
        .chain(
            planes
                .sources
                .values()
                .map(|source| source.program.as_ref()),
        )
        .flat_map(|program| &program.metadata().externals)
        .find(|external| external.name == name)
        .map(|external| match external.kind {
            program::Type::Scalar => 1,
            program::Type::Vec2 => 2,
            _ => 3,
        })
}

/// A `plane/` node's artifact: the decoded plane, checked against the atlas it covers and the
/// components it is read as (`wanted`).
fn decoded_plane(
    wanted: Option<usize>,
    name: &str,
    bytes: &[u8],
    atlas: &Atlas,
) -> Result<(Artifact, usize), String> {
    let plane = program::Plane::decode_external(name, bytes, atlas.width, atlas.height)?;
    if wanted != Some(plane.components()) {
        return Err(format!(
            "External plane '{name}' holds {} components where the program reads {}.",
            plane.components(),
            wanted.unwrap_or(0)
        ));
    }
    let bytes = plane.bytes();
    Ok((
        Artifact::Material(Arc::new(BakedReceiver::Plane(plane))),
        bytes,
    ))
}

/// The external planes among a node's dependencies (roles `plane/<name>`).
fn external_planes(dependencies: &BTreeMap<String, Arc<Artifact>>) -> Vec<&program::Plane> {
    dependencies
        .iter()
        .filter(|(role, _)| role.starts_with("plane/"))
        .filter_map(|(_, artifact)| match artifact.as_ref() {
            Artifact::Material(baked) => match baked.as_ref() {
                BakedReceiver::Plane(plane) => Some(plane),
                _ => None,
            },
            _ => None,
        })
        .collect()
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
    /// The values of one intrinsic field by position of the material's domain.
    Intrinsic(IntrinsicRaster),
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
            Self::Plane(_) | Self::Texels(_) | Self::Intrinsic(_) => Vec::new(),
        }
    }

    /// Bytes this result retains, for artifact admission.
    fn bytes(&self) -> usize {
        match self {
            Self::Plane(plane) => return plane.bytes(),
            Self::Intrinsic(raster) => return raster.bytes(),
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

/// The per-pixel program a material that declares live inputs (`shading: painted`) is
/// delivered with, its uniforms set from the request's parameters and seed. `None` for a lit
/// material, and for a painted one whose program reads what a delivered program cannot carry
/// (supplied images, receiver features, source edges or planes): it is delivered with its baked
/// maps alone. Render-only values never enter a bake key: this payload is beside the maps.
pub(crate) fn live_program(
    compiled: &program::Program,
    request: &Request,
) -> Result<Option<superspec_types::material::LiveProgram>, String> {
    use superspec_types::material::{LiveProgram, ProgramInput, ProgramUniform};
    if compiled.metadata().shading != program::Shading::Painted {
        return Ok(None);
    }
    let shader = compiled.shader()?;
    let Some(painted) = shader.painted.clone() else {
        return Ok(None);
    };
    let reads = shader
        .reads
        .outputs
        .values()
        .chain(&shader.reads.painted)
        .collect::<Vec<_>>();
    if !shader.images.is_empty()
        || shader.features
        || reads
            .iter()
            .any(|read| read.edges || !read.planes.is_empty() || !read.cpu_fields.is_empty())
    {
        return Ok(None);
    }
    let mut uniforms = Vec::new();
    for parameter in &shader.parameters {
        let declared = compiled
            .metadata()
            .parameters
            .iter()
            .find(|known| known.name == parameter.name)
            .ok_or("A shader parameter is not a parameter of the program.")?;
        let value = request
            .parameters
            .get(&parameter.name)
            .unwrap_or(&declared.default);
        let (kind, value) = match value {
            serde_json::Value::Bool(flag) => ("bool", vec![f64::from(u8::from(*flag))]),
            serde_json::Value::Number(number) => ("float", vec![number.as_f64().unwrap_or(0.)]),
            serde_json::Value::Array(items) => (
                if items.len() == 2 { "vec2" } else { "vec3" },
                items
                    .iter()
                    .map(|item| item.as_f64().unwrap_or(0.))
                    .collect(),
            ),
            _ => {
                return Err(format!(
                    "Parameter {} has no numeric value.",
                    parameter.name
                ));
            }
        };
        uniforms.push(ProgramUniform {
            uniform: parameter.uniform.clone(),
            kind: kind.into(),
            value,
        });
    }
    for seed in &shader.seeds {
        uniforms.push(ProgramUniform {
            uniform: seed.uniform.clone(),
            kind: "uint".into(),
            value: vec![f64::from(program::seed_value(request.seed, &seed.name))],
        });
    }
    let mut requires: std::collections::BTreeSet<String> = reads
        .iter()
        .flat_map(|read| read.sample.iter().map(|name| (*name).to_owned()))
        .collect();
    let inputs: Vec<ProgramInput> = shader
        .render_inputs
        .iter()
        .map(|input| {
            if let Some(name) = &input.name {
                requires.insert(format!("field:{name}"));
            }
            ProgramInput {
                symbol: input.symbol.clone(),
                kind: input.kind.into(),
                glsl: input.glsl.into(),
                name: input.name.clone(),
                fallback: input.fallback.clone(),
            }
        })
        .collect();
    if shader.source.contains("ms_cast_shadow") {
        requires.insert("shadow".into());
    }
    let mapping = serde_json::to_value(request.mapping)
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_else(|| "object".into());
    Ok(Some(LiveProgram {
        version: LiveProgram::VERSION,
        source: shader.source,
        painted,
        mapping,
        domain: request.domain,
        uniforms,
        requires: requires.into_iter().collect(),
        inputs,
    }))
}

/// The material of a receiver, assembled from its groups' maps exactly as a bake of the whole
/// receiver builds it. A painted material keeps its program beside the maps (`live`).
fn assemble(
    groups: &BTreeMap<Group, Arc<BakedReceiver>>,
    inspecting: bool,
    declared: &program::Metadata,
    live: Option<&superspec_types::material::LiveProgram>,
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
    // What the recipe declares about how it is drawn. A cutout is drawn where its opacity
    // reaches the cutoff, whatever the opacities are.
    if let (Some(cutoff), false) = (declared.alpha_cutoff, inspecting) {
        material.alpha_mode = Some(AlphaMode::Mask);
        material.alpha_cutoff = Some(cutoff);
    }
    if declared.double_sided {
        material.double_sided = Some(true);
    }
    if declared.shading == program::Shading::Painted && !inspecting {
        material.shading = Some(superspec_types::material::Shading::Painted);
        material.program = live.cloned();
    }
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
                | BakedReceiver::Texels(_)
                | BakedReceiver::Intrinsic(_) => return Err(missing()),
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
    let mut inputs = json(&(
        identity,
        request.resolution,
        request.mapping,
        request.domain,
        request.gravity,
        request.density_of(&receiver.id),
    ));
    // Named features are part of what a bake of the layout reads; a receiver without any keeps
    // the key it always had.
    if let Some(named) = request.features.get(&receiver.id).filter(|n| !n.is_empty()) {
        inputs.extend(json(named));
    }
    // So is the pattern anchor, where the mapping reads one.
    if let (super::Mapping::Anchored, Some(anchor)) =
        (request.mapping, request.anchors.get(&receiver.id))
    {
        inputs.extend(json(anchor));
    }
    node(
        format!("layout/{index}"),
        if request.mapping == super::Mapping::Anchored && request.density_of(&receiver.id).is_some()
        {
            "receiver-atlas-v4"
        } else {
            "receiver-atlas-v3"
        },
        inputs,
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

/// An `assets:<material>` node's artifact: inputs holding the decoded images among its
/// dependencies.
fn source_assets(
    dependencies: &BTreeMap<String, Arc<Artifact>>,
    mips: &program::MipCache,
) -> Result<(Artifact, usize), String> {
    let mut images = BTreeMap::new();
    let mut count = 0usize;
    for (role, artifact) in dependencies {
        if let (Some(name), Artifact::Image(image)) =
            (role.strip_prefix("image/"), artifact.as_ref())
        {
            count += image.rgba.len();
            if count > 64 * 1024 * 1024 {
                return Err("Decoded material images exceed the authoring allowance.".into());
            }
            images.insert(name.to_owned(), image.clone());
        }
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
    let atlas = Atlas::for_request(receiver.clone(), request)?;
    let bytes = atlas.mesh.positions.len() * 88
        + atlas.mesh.indices.len() * 4
        + receiver.positions.len() * 8 * 3
        + atlas.named.bytes();
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
    generation: u64,
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
        let Built::Output(mut output) =
            self.material_with(request, revision, assets, cancelled, None)?
        else {
            unreachable!("no share was requested")
        };
        self.attach_material_lods(&mut output, request, revision, assets, cancelled)?;
        Ok(output)
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
        let distribution = self.distribution.get_mut();
        if !enabled && distribution.enabled {
            distribution.reset();
        }
        distribution.enabled = enabled;
    }

    /// The next bake the last build stopped for, if any, in the order they were reported.
    pub fn take_pending_bake(&mut self) -> Option<PendingBake> {
        self.distribution.get_mut().pending.pop_front()
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
            generation: pending.generation,
            groups: &groups,
        };
        match self.material_with(&pending.request, revision, assets, cancelled, Some(share))? {
            Built::Share(bytes) => Ok(bytes),
            Built::Output(_) => unreachable!("a share was requested"),
        }
    }

    /// Accepts rows computed by [`Self::bake_share`] for a bake this session reported.
    pub fn provide_bake_share(&mut self, bytes: &[u8]) -> Result<(), String> {
        let distribution = self.distribution.get_mut();
        for (key, groups, total, rows) in read_shares(bytes)? {
            // Rows of a bake this session is not waiting for (a superseded edit, or one already
            // built) are dropped: no later build would ask for them.
            if !distribution.reported.contains_key(&key) {
                continue;
            }
            let provided = distribution.provided.entry(key).or_default();
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
            while distribution.provided_bytes() > PROVIDED_BYTES
                && distribution.forget_oldest(Some(&key))
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
        // A decal is baked on its own rectangle, in meters from its lower left corner, whatever
        // the request's domain and mapping: it is the same picture wherever it is placed.
        let decal;
        let request = match (compiled.metadata().kind, compiled.metadata().size) {
            (program::RecipeKind::Decal, Some(size)) => {
                if !request.receivers.is_empty() {
                    return Err(vec![Diagnostic::message(
                        &request.entry,
                        "A decal is baked on its own rectangle, without receivers; its owner places it.",
                    )]);
                }
                decal = Request {
                    domain: size,
                    mapping: super::Mapping::Object,
                    ..request.clone()
                };
                &decal
            }
            _ => request,
        };
        // A layered slot bakes a composite that reads its stack's planes; what the slot declares
        // about how it is drawn is its base's. Other sessions bake shares of the slot as written.
        let original = request;
        let layered = self.layered(request)?;
        let declared = compiled.metadata().clone();
        // A painted material's program travels beside its maps; it is never baked.
        let live_payload = match (layered.is_some(), request.inspection.is_some()) {
            (false, false) => live_program(&compiled, request)
                .map_err(|message| vec![Diagnostic::message(&request.entry, message)])?,
            _ => None,
        };
        let (request, chain, compiled) = match &layered {
            Some((composite, chain, _)) => {
                let program = self.composite(composite, chain, &original.entry)?;
                (composite, Some(chain), program)
            }
            None => (request, None, compiled),
        };
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
        // Planes bound to the program's external inputs enter the graph by content: a map is
        // keyed by the planes of only the inputs its outputs read.
        let planes = self.bound_planes(&compiled, request, &receivers, assets, chain)?;
        // Images only the materials planes are baked from read.
        let mut source_images: BTreeMap<String, Arc<[u8]>> = BTreeMap::new();
        let plane_dependencies = |i: usize, read: Option<&program::Dependencies>| {
            planes[i]
                .iter()
                .filter(|(name, _)| read.is_none_or(|read| read.externals.contains(name)))
                .map(|(name, id)| dependency(&format!("plane/{name}"), id.clone()))
                .collect::<Vec<_>>()
        };
        let mut targets = vec![];
        for (i, receiver) in receivers.iter().enumerate() {
            nodes.push(layout_node(i, receiver, request));
            plane_nodes(
                &planes,
                i,
                assets,
                &mut nodes,
                &mut source_images,
                chain.is_some(),
            )
            .map_err(&fail)?;
        }
        // A layered stack's planes are made by the maps that read them, when those maps are
        // evaluated here (see `StackPlanes`): never for maps whose rows other sessions evaluated.
        // A map is keyed by the keys of the planes it reads, and depends on the decoded images of
        // the stack's materials, which it bakes them from.
        let stack_keys: BTreeMap<String, ContentKey> = if chain.is_some() {
            let ids: Vec<String> = planes
                .nodes
                .keys()
                .filter(|id| PlaneNode::layered(id))
                .cloned()
                .collect();
            let keyed = Plan::new(nodes.clone(), ids.clone()).map_err(|e| fail(e.to_string()))?;
            ids.into_iter()
                .map(|id| {
                    let key = keyed.key(&id).expect("planned plane");
                    (id, key)
                })
                .collect()
        } else {
            BTreeMap::new()
        };
        let stack_materials: Vec<String> = planes
            .nodes
            .iter()
            .filter(|(id, _)| PlaneNode::layered(id))
            .filter_map(|(_, plane)| match &plane.bound {
                Bound::Material { material, .. } => Some(material.clone()),
                Bound::Asset(_) => None,
            })
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect();
        let stack_dependencies = |i: usize, read: Option<&program::Dependencies>| {
            let mut inputs = Vec::new();
            for (name, id) in &planes[i] {
                if read.is_none_or(|read| read.externals.contains(name)) {
                    inputs.extend(name.as_bytes());
                    inputs.push(0);
                    inputs.extend(stack_keys[id]);
                }
            }
            let dependencies: Vec<Dependency> = stack_materials
                .iter()
                .map(|material| {
                    dependency(
                        &format!("stack-assets/{material}"),
                        format!("assets:{material}"),
                    )
                })
                .collect();
            (inputs, dependencies)
        };
        for i in 0..receivers.len() {
            let layout = format!("layout/{i}");
            let bake = format!("bake/{i}");
            let mut bake_dependencies = vec![
                dependency("mapping", layout.clone()),
                dependency("program", "program"),
                dependency("inputs", "inputs"),
            ];
            bake_dependencies.extend(plane_dependencies(i, None));
            nodes.push(node(
                bake.clone(),
                "material-receiver-v5",
                // Production bakes compress their PNG maps harder: same pixels, different bytes.
                json(&(
                    &request.inspection,
                    request.profile == super::Profile::Production,
                )),
                bake_dependencies,
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
                let mut group_inputs = vec![
                    dependency("mapping", layout.clone()),
                    dependency("program", "program"),
                    dependency("assets", "assets"),
                ];
                if chain.is_some() {
                    let (planes_read, stack) = stack_dependencies(i, Some(&dependencies));
                    inputs_identity.push(0);
                    inputs_identity.extend(planes_read);
                    group_inputs.extend(stack);
                } else {
                    group_inputs.extend(plane_dependencies(i, Some(&dependencies)));
                }
                nodes.push(node(
                    id.clone(),
                    "material-group-v2",
                    inputs_identity,
                    group_inputs,
                ));
                targets.push(id);
            }
        }
        let plane_ids = |i: usize| planes[i].iter().map(|(_, id)| id.clone());
        let dependencies_only = share.is_some().then(|| {
            // A share needs the program, inputs, layouts and bound planes, but never bakes or
            // caches a receiver.
            let nodes: Vec<Node> = nodes
                .iter()
                .filter(|node| !node.id.starts_with("bake/"))
                .cloned()
                .collect();
            // A layered share bakes the planes its rows read itself, from the decoded images of
            // the stack's materials.
            let layered = chain.is_some();
            let targets = (0..receivers.len())
                .flat_map(|i| {
                    std::iter::once(format!("layout/{i}"))
                        .chain(plane_ids(i).filter(move |_| !layered))
                })
                .chain(["program".to_owned(), "inputs".to_owned()])
                .chain(
                    stack_materials
                        .iter()
                        .map(|material| format!("assets:{material}")),
                )
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
        // A bake that reads planes baked from another material of the request stays here: a
        // session that bakes a share would have to bake those planes over again, or be sent them.
        // A layered slot's share is the whole stack over its rows (the planes of a texel read
        // only the planes beneath at that texel), which the session baking it makes itself.
        let distribution = self.distribution.get_mut();
        if share.is_none() && distribution.enabled && (planes.sources.is_empty() || chain.is_some())
        {
            let complete = |key: &ContentKey, wanted: Groups| {
                distribution
                    .provided
                    .get(&distribution.share_key(key))
                    .is_some_and(|provided| {
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
                // Every asset plane the build reads: the entry's, and those a stack's layers read.
                let plane_assets: std::collections::BTreeSet<String> = planes
                    .nodes
                    .values()
                    .filter_map(|plane| {
                        let Bound::Asset(_) = plane.bound else {
                            return None;
                        };
                        let named = request.planes.get(&receivers[plane.index].id)?;
                        match named.get(&plane.name)? {
                            super::PlaneSource::Asset(asset) => Some(asset.clone()),
                            super::PlaneSource::Material { .. } => None,
                        }
                    })
                    .collect();
                let images: std::collections::BTreeSet<String> = image_bytes
                    .keys()
                    .chain(source_images.keys().filter(|_| chain.is_some()))
                    .cloned()
                    .collect();
                distribution.enqueue(PendingBake {
                    request: if chain.is_some() {
                        original.clone()
                    } else {
                        request.clone()
                    },
                    keys,
                    images: images.into_iter().collect(),
                    groups,
                    planes: plane_assets.into_iter().collect(),
                    generation: distribution.generation,
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
                // This build bakes it, from these rows or here: it is no longer waited for.
                let share_key = distribution.share_key(key);
                distribution.reported.remove(&share_key);
                if let Some(rows) = distribution.provided.remove(&share_key) {
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
        let mut rasters = Rasters::new();
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
                let plane_keys: Vec<(String, ContentKey)> = planes[i]
                    .iter()
                    .map(|(name, id)| (name.clone(), plan.key(id).expect("planned plane")))
                    .collect();
                let mut chosen = FieldPlan::default();
                let mut missing = Vec::new();
                for field in candidates {
                    let key = field_plane_key(
                        &compiled,
                        &dependency_table.fields[field_names[field]],
                        request,
                        [&layout_key, &program_key, &assets_key],
                        &plane_keys,
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
                    // A field that reads only the surface position and footprint has the same
                    // value wherever a sample has the same bits of both: its raster is keyed
                    // like its plane, but by no layout.
                    let reads = &dependency_table.fields[field_names[field]];
                    if reads.intrinsic() && components > 0 {
                        let mut digest = Sha256::new();
                        digest.update(b"material-intrinsic-v1");
                        digest.update(field_plane_key(
                            &compiled,
                            reads,
                            request,
                            [&[0; 32], &program_key, &assets_key],
                            &[],
                            field,
                        ));
                        let raster: ContentKey = digest.finalize().into();
                        chosen.intrinsic.insert(field, raster);
                        if !rasters.contains_key(&raster) {
                            let kept = self.runtime.lookup(INTRINSIC_RASTERS, &raster);
                            let kept = kept.and_then(|found| match found.as_ref() {
                                Artifact::Material(baked) => match baked.as_ref() {
                                    BakedReceiver::Intrinsic(raster) => Some(raster.clone()),
                                    _ => None,
                                },
                                _ => None,
                            });
                            // Several receivers of one build are several layouts already.
                            let working = WorkingRaster {
                                shared: kept.is_some() || receivers.len() > 1,
                                raster: kept.unwrap_or_default(),
                                changed: false,
                            };
                            rasters.insert(raster, working);
                        }
                    }
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
        // The maps of each receiver still to bake: a producer takes the ones it bakes off the
        // list, so no later producer bakes them again.
        let mut dirty = dirty;
        // Planes baked alongside the one a producer was asked for, and the planes no retained
        // artifact holds.
        let mut baked_planes: BTreeMap<String, Arc<BakedReceiver>> = BTreeMap::new();
        // A layered stack's planes: those an earlier build kept apart from the graph's store,
        // by id, then those this build bakes. A share keeps the planes of its rows under keys
        // of its own. A production build keeps none beyond the receiver that reads them.
        let keep_stack = request.profile != super::Profile::Production;
        let stack_key = |id: &str| -> ContentKey {
            let key = stack_keys[id];
            match &share {
                None => key,
                Some(share) => {
                    let mut digest = Sha256::new();
                    digest.update(b"material-layer-share-v1");
                    digest.update(key);
                    digest.update((share.index as u64).to_le_bytes());
                    digest.update((share.count as u64).to_le_bytes());
                    digest.finalize().into()
                }
            }
        };
        let mut stack = StackPlanes {
            keep: keep_stack,
            ..StackPlanes::default()
        };
        for (id, plane) in &planes.nodes {
            let (true, Bound::Material { material, value }) =
                (PlaneNode::layered(id), &plane.bound)
            else {
                continue;
            };
            if let Some(found) = self.runtime.lookup(LAYER_PLANES, &stack_key(id)) {
                if let Artifact::Material(baked) = found.as_ref() {
                    stack.made.insert(id.clone(), baked.clone());
                    continue;
                }
            }
            let source = &planes.sources[material];
            let Some((None, _, reads)) = source_value(&source.program, &source.table, value) else {
                continue;
            };
            if !reads.intrinsic() || !reads.externals.is_empty() {
                continue;
            }
            // Keyed by the material, its images and what the output reads of its parameters
            // and seed: by no layout, no feature and nothing beneath it.
            let mut digest = Sha256::new();
            digest.update(b"material-intrinsic-output-v1");
            for part in [
                plan.key(&format!("program:{material}")),
                plan.key(&format!("assets:{material}")),
            ] {
                digest.update(part.expect("planned source"));
            }
            digest.update((value.len() as u64).to_le_bytes());
            digest.update(value.as_bytes());
            match source.program.input_fingerprint(
                &Inputs {
                    parameters: source.request.parameters.clone(),
                    seed: source.request.seed,
                    edges: source.request.edges.clone(),
                    ..Inputs::default()
                },
                reads,
            ) {
                Ok(fingerprint) => digest.update(fingerprint),
                Err(_) => continue,
            }
            let raster: ContentKey = digest.finalize().into();
            stack.raster_of.insert(id.clone(), raster);
            if !stack.rasters.contains_key(&raster) {
                let kept = self.runtime.lookup(INTRINSIC_OUTPUTS, &raster);
                let kept = kept.and_then(|found| match found.as_ref() {
                    Artifact::Material(baked) => match baked.as_ref() {
                        BakedReceiver::Intrinsic(raster) => Some(raster.clone()),
                        _ => None,
                    },
                    _ => None,
                });
                let raster_value = kept.unwrap_or(IntrinsicRaster {
                    cap: INTRINSIC_OUTPUT_BYTES,
                    ..Default::default()
                });
                stack.rasters.insert(
                    raster,
                    WorkingRaster {
                        raster: raster_value,
                        shared: true,
                        changed: false,
                    },
                );
            }
        }
        // The planes of other materials of the request (not of a stack) no retained artifact holds.
        let mut plane_dirty: std::collections::BTreeSet<String> = planes
            .nodes
            .keys()
            .filter(|id| {
                let key = plan.key(id).expect("planned plane");
                !PlaneNode::layered(id) && !self.runtime.holds(&key)
            })
            .cloned()
            .collect();
        let stack_context = StackContext {
            planes: &planes,
            compiled: &compiled,
            request,
            receivers: &receivers,
            threads: request.profile.threads(),
        };
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
                        let name = &id["image/".len()..];
                        decoded_image(
                            image_bytes
                                .get(name)
                                .unwrap_or_else(|| &source_images[name]),
                        )
                    }
                    id if id.starts_with("program:") => {
                        let program = &planes.sources[&id["program:".len()..]].program;
                        Ok((Artifact::Program(program.clone()), program.retained_bytes()))
                    }
                    id if id.starts_with("assets:") => source_assets(dependencies, &mips),
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
                                render: None,
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
                    id if id.starts_with("plane/") => {
                        let plane = &planes.nodes[id];
                        let index = plane.index;
                        let Artifact::Receiver(atlas) = dependencies["mapping"].as_ref() else {
                            unreachable!()
                        };
                        let (material, value) = match &plane.bound {
                            Bound::Asset(bytes) => {
                                let components = plane_components(&compiled, &planes, &plane.name);
                                return decoded_plane(components, &plane.name, bytes, atlas);
                            }
                            Bound::Material { material, value } => (material, value),
                        };
                        if let Some(done) = baked_planes.remove(id) {
                            let bytes = done.bytes();
                            return Ok((Artifact::Material(done), bytes));
                        }
                        // A plane of another material of the request (a stack's planes are made
                        // by the maps that read them, see `StackPlanes`). One pass bakes every
                        // plane of this material the build still needs for the receiver whose
                        // inputs this node holds, so what the values share is evaluated once.
                        let held: Vec<&String> = plane.reads.iter().map(|(_, id)| id).collect();
                        let mut wanted: Vec<(&str, &str, &str)> = vec![(id, &plane.name, value)];
                        for (other_id, other) in &planes.nodes {
                            let Bound::Material {
                                material: from,
                                value,
                            } = &other.bound
                            else {
                                continue;
                            };
                            if from == material
                                && other_id != id
                                && other.index == index
                                && plane_dirty.contains(other_id)
                                && other.reads.iter().all(|(_, read)| held.contains(&read))
                            {
                                wanted.push((other_id, &other.name, value));
                            }
                        }
                        let Artifact::Inputs(decoded) = dependencies["source-assets"].as_ref()
                        else {
                            unreachable!()
                        };
                        let source = &planes.sources[material];
                        let pairs: Vec<(&str, &str)> = wanted
                            .iter()
                            .map(|(_, name, value)| (*name, *value))
                            .collect();
                        let outputs_only = pairs.iter().all(|(_, value)| {
                            matches!(
                                source_value(&source.program, &source.table, value),
                                Some((None, _, _))
                            )
                        });
                        let baked = if outputs_only {
                            let inputs = Inputs {
                                parameters: source.request.parameters.clone(),
                                seed: source.request.seed,
                                images: decoded.images.clone(),
                                edges: source.request.edges.clone(),
                                features: Vec::new(),
                                mips: decoded.mips.clone(),
                                render: None,
                            };
                            bake_output_planes(
                                &source.program,
                                &source.table,
                                &inputs,
                                atlas,
                                &atlas.named,
                                &pairs,
                                &external_planes(dependencies),
                                request.profile.threads(),
                                &mut [],
                                None,
                                stopped,
                            )
                        } else {
                            bake_source_planes(source, decoded, atlas, &pairs, None, stopped)
                        }
                        .map_err(|error| {
                            let message = error.message.clone();
                            evaluation_error = Some(error);
                            message
                        })?;
                        let mut requested = None;
                        for ((made, name, _), plane) in wanted.iter().zip(baked) {
                            plane_dirty.remove(*made);
                            let plane = baked_plane(&compiled, name, plane)?;
                            if *made == id {
                                requested = Some(plane);
                            } else {
                                baked_planes.insert((*made).to_owned(), plane);
                            }
                        }
                        let plane = requested.expect("the requested plane was baked");
                        let bytes = plane.bytes();
                        Ok((Artifact::Material(plane), bytes))
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
                        // outputs the maps share are computed once. A producer holds the planes
                        // of the external inputs its own map reads, so maps that read others
                        // are left to their own producers.
                        let wanted = dirty[index].with(group);
                        let rows = provided.remove(&index);
                        let layered = chain.is_some();
                        // A stack's planes are made here for the maps evaluated here: all the
                        // maps this build needs of the receiver, unless other sessions evaluated
                        // their rows.
                        let mut stacked: Vec<Arc<BakedReceiver>> = Vec::new();
                        if layered
                            && !rows.as_ref().is_some_and(|(have, rows)| {
                                provided_fits(atlas, wanted, live_fields.len(), *have, rows)
                            })
                        {
                            let read = wanted.iter().fold(
                                program::Dependencies::default(),
                                |read, group| {
                                    read.union(&group_dependencies(
                                        &dependency_table,
                                        group,
                                        &live_fields,
                                    ))
                                },
                            );
                            let ids: Vec<String> = planes[index]
                                .iter()
                                .filter(|(name, _)| read.externals.contains(name))
                                .map(|(_, id)| id.clone())
                                .collect();
                            let sources = stack_sources(dependencies, "stack-assets/");
                            stack_context
                                .make(&mut stack, index, atlas, &sources, &ids, None, stopped)
                                .map_err(|error| {
                                    let message = error.message.clone();
                                    evaluation_error = Some(error);
                                    message
                                })?;
                            stacked = ids.iter().map(|id| stack.made[id].clone()).collect();
                        }
                        let externals: Vec<&program::Plane> = if layered {
                            stacked
                                .iter()
                                .filter_map(|baked| match baked.as_ref() {
                                    BakedReceiver::Plane(plane) => Some(plane),
                                    _ => None,
                                })
                                .collect()
                        } else {
                            external_planes(dependencies)
                        };
                        // Rows evaluated elsewhere were evaluated with every plane.
                        let whole = planes[index].is_empty() || layered;
                        let evaluable = dirty[index]
                            .iter()
                            .filter(|other| {
                                whole || {
                                    let read = |group: Group| {
                                        let read = group_dependencies(
                                            &dependency_table,
                                            group,
                                            &live_fields,
                                        );
                                        planes[index]
                                            .iter()
                                            .map(|(name, _)| name.as_str())
                                            .filter(|name| read.externals.contains(*name))
                                            .collect::<Vec<_>>()
                                    };
                                    let held = read(group);
                                    read(*other).iter().all(|name| held.contains(name))
                                }
                            })
                            .fold(Groups::NONE, Groups::with)
                            .with(group);
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
                                render: None,
                            })
                        };
                        match bake_groups(
                            program,
                            &inputs,
                            atlas,
                            request.inspection.as_deref(),
                            request.profile.threads(),
                            compact,
                            [wanted, evaluable],
                            rows,
                            field_plans.get(index),
                            &externals,
                            &mut capture_budget,
                            &mut rasters,
                            stopped,
                        ) {
                            Ok((results, planes)) => {
                                captured_planes.extend(planes);
                                // Every map of the receiver this build needs is baked: a build
                                // that keeps no plane lets the receiver's go.
                                drop(externals);
                                drop(stacked);
                                if layered && !keep_stack {
                                    stack.release(&stack_context, index);
                                }
                                let mut requested = None;
                                for (made, value) in results {
                                    dirty[index] = dirty[index].without(made);
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
        for (
            key,
            WorkingRaster {
                raster, changed, ..
            },
        ) in rasters
        {
            if changed {
                let bytes = raster.bytes();
                self.runtime.retain(
                    INTRINSIC_RASTERS,
                    key,
                    Artifact::Material(Arc::new(BakedReceiver::Intrinsic(raster))),
                    bytes,
                    true,
                );
            }
        }
        if let Some(share) = &share {
            let Artifact::Program(program) = complete["program"].as_ref() else {
                unreachable!()
            };
            let Artifact::Inputs(inputs) = complete["inputs"].as_ref() else {
                unreachable!()
            };
            let sources = stack_sources(&complete, "assets:");
            let mut bytes = Vec::new();
            for (i, key) in bake_keys.iter().enumerate() {
                let Some(position) = share.keys.iter().position(|wanted| wanted == key) else {
                    continue;
                };
                let Artifact::Receiver(atlas) = complete[&format!("layout/{i}")].as_ref() else {
                    unreachable!()
                };
                let groups = share.groups[position];
                // A layered share makes the stack's planes over its rows and the rows beside
                // them (a relief reads the heights of the rows next to its own), from the
                // planes beneath at the same texels.
                let mut stacked: Vec<Arc<BakedReceiver>> = Vec::new();
                if chain.is_some() {
                    let read =
                        groups
                            .iter()
                            .fold(program::Dependencies::default(), |read, group| {
                                read.union(&group_dependencies(
                                    &dependency_table,
                                    group,
                                    &live_fields,
                                ))
                            });
                    let ids: Vec<String> = planes[i]
                        .iter()
                        .filter(|(name, _)| read.externals.contains(name))
                        .map(|(_, id)| id.clone())
                        .collect();
                    let only = share_rows(atlas, share.index, share.count);
                    stack_context
                        .make(&mut stack, i, atlas, &sources, &ids, Some(&only), cancelled)
                        .map_err(|error| vec![error])?;
                    stacked = ids.iter().map(|id| stack.made[id].clone()).collect();
                }
                let externals: Vec<&program::Plane> = if chain.is_some() {
                    stacked
                        .iter()
                        .filter_map(|baked| match baked.as_ref() {
                            BakedReceiver::Plane(plane) => Some(plane),
                            _ => None,
                        })
                        .collect()
                } else {
                    plane_ids(i)
                        .filter_map(|id| match complete[&id].as_ref() {
                            Artifact::Material(baked) => match baked.as_ref() {
                                BakedReceiver::Plane(plane) => Some(plane),
                                _ => None,
                            },
                            _ => None,
                        })
                        .collect()
                };
                let (total, rows) = bake_rows(
                    program,
                    inputs,
                    atlas,
                    request.inspection.as_deref(),
                    (share.index, share.count),
                    groups,
                    &externals,
                    cancelled,
                )
                .map_err(|error| vec![error])?;
                write_share(
                    &mut bytes,
                    &bake_share_key(share.generation, key),
                    groups,
                    total,
                    &rows,
                );
                // The receiver's planes are kept (within the partition's bounds) or let go now,
                // so a share of many receivers holds one receiver's planes at a time.
                if chain.is_some() {
                    drop(externals);
                    drop(stacked);
                    if keep_stack {
                        self.keep_planes(std::mem::take(&mut stack.baked), &stack_key);
                    }
                    stack.release(&stack_context, i);
                }
            }
            self.keep_stack(stack, &stack_key, keep_stack);
            return Ok(Built::Share(bytes));
        }
        self.keep_stack(stack, &stack_key, keep_stack);
        let mut model = Model {
            id: "studio/material".into(),
            name: "Material preview".into(),
            ..Default::default()
        };
        let mut layouts = Vec::with_capacity(receivers.len());
        let mut dropped = Vec::with_capacity(receivers.len());
        let mut decal_rectangle = None;
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
            let (material, images) =
                assemble(&maps, inspecting, &declared, live_payload.as_ref()).map_err(&fail)?;
            layouts.push([atlas.width as f64, atlas.height as f64, atlas.density]);
            if compiled.metadata().kind == program::RecipeKind::Decal {
                let bound = |pick: fn(f64, f64) -> f64, start: f64| {
                    [0, 1].map(|axis| atlas.mesh.uvs.iter().map(|uv| uv[axis]).fold(start, pick))
                };
                decal_rectangle = Some([
                    bound(f64::min, f64::INFINITY),
                    bound(f64::max, f64::NEG_INFINITY),
                ]);
            }
            // A receiver with more features than a bake reads lost some: say so, never silently.
            dropped.push(queries_features(&compiled) && atlas.features().1);
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
        // A layered slot reports its base, with each layer's own metadata and the material each
        // channel of the result comes from.
        let mut metadata = serde_json::to_value(&declared).unwrap();
        if let Some((_, chain, programs)) = &layered {
            metadata["layers"] = programs[1..]
                .iter()
                .map(|program| serde_json::to_value(program.metadata()).unwrap())
                .collect();
            metadata["channels"] = chain
                .entry
                .iter()
                .filter_map(|(input, (producer, _))| {
                    let channel = input.strip_prefix(program::BASE_PLANE)?;
                    Some((channel.to_owned(), serde_json::json!(producer)))
                })
                .collect::<serde_json::Map<_, _>>()
                .into();
        }
        metadata["mapping"] = serde_json::to_value(request.mapping).unwrap();
        // Where a decal's rectangle lies in its texture, for whoever places it.
        if let Some([low, high]) = decal_rectangle {
            metadata["decal"] = serde_json::json!({ "uvMin": low, "uvMax": high });
        }
        // Each receiver's texture size and detail, for hosts that report or budget them.
        metadata["atlases"] = receivers
            .iter()
            .zip(&layouts)
            .zip(&dropped)
            .map(|((receiver, [width, height, density]), dropped)| {
                let mut atlas = serde_json::json!({
                    "id": receiver.id, "width": width, "height": height, "texelsPerMeter": density,
                });
                if *dropped {
                    atlas["featuresDropped"] = true.into();
                }
                atlas
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
    /// For each intrinsic field a bake filled a plane of on this thread: the receiver's texels
    /// on the lattice, and how many of them the field's raster already held.
    static INTRINSIC: std::cell::RefCell<Vec<(usize, usize)>> = const { std::cell::RefCell::new(Vec::new()) };
    /// The values each bake of another material's planes evaluated on this thread.
    static PLANE_BAKES: std::cell::RefCell<Vec<Vec<String>>> = const { std::cell::RefCell::new(Vec::new()) };
    /// Each output plane bake on this thread: the material's entry, its outputs, and the points
    /// read from intrinsic rasters and evaluated.
    static LAYER_BAKES: std::cell::RefCell<Vec<(String, Vec<String>, usize, usize)>> = const { std::cell::RefCell::new(Vec::new()) };
    /// Field planes (read, kept) of each bake that evaluated rows on this thread.
    static PLANES: std::cell::RefCell<Vec<(usize, usize)>> = const { std::cell::RefCell::new(Vec::new()) };
}
/// Bakes below this many requested texels are evaluated where they are requested.
const DISTRIBUTE_TEXELS: usize = 8192;

/// A session's state for baking rows in other sessions (see [`Session::distribute_bakes`]).
#[derive(Default)]
pub(crate) struct Distribution {
    enabled: bool,
    generation: u64,
    /// Bakes reported by builds and not yet taken by the host.
    pending: std::collections::VecDeque<PendingBake>,
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
    pub(crate) fn reset(&mut self) {
        *self = Self {
            enabled: self.enabled,
            generation: self.generation + 1,
            ..Self::default()
        };
    }

    fn share_key(&self, content: &ContentKey) -> ContentKey {
        bake_share_key(self.generation, content)
    }

    fn enqueue(&mut self, pending: PendingBake) {
        self.pending
            .retain(|queued| queued.keys != pending.keys || queued.groups != pending.groups);
        for key in pending.keys.iter().filter_map(|key| parse_key(key)) {
            self.report(self.share_key(&key));
        }
        self.pending.push_back(pending);
        if self.pending.len() > MAX_PENDING {
            self.pending.pop_front();
        }
    }

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
    /// Assets holding the planes the request binds to external inputs the program reads; a
    /// session that bakes a share needs them like the images.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub planes: Vec<String>,
    /// Separates responses issued before and after a host resets the reporting session.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub generation: u64,
}

fn is_zero(value: &u64) -> bool {
    *value == 0
}

fn bake_share_key(generation: u64, content: &ContentKey) -> ContentKey {
    if generation == 0 {
        return *content;
    }
    let mut input = [0; 40];
    input[..8].copy_from_slice(&generation.to_le_bytes());
    input[8..].copy_from_slice(content);
    crate::runtime::dependencies::producer_key("distributed-bake-share-v1", &input)
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

/// The height output at each sample, evaluated together where the program allows. `externals`
/// are read at `texels`, one per sample (both empty for a bake without external planes).
fn heights_of(
    evaluator: &mut program::Evaluator<'_>,
    samples: &[program::Sample],
    texels: &[usize],
    externals: &[&program::Plane],
) -> Result<Vec<f64>, Diagnostic> {
    if samples.is_empty() {
        return Ok(Vec::new());
    }
    let mut unused = Vec::new();
    Ok(evaluator
        .evaluate_row_with_planes(
            samples,
            texels,
            program::OutputSet::HEIGHT,
            &program::FieldSelection::default(),
            &mut unused,
            externals,
            &mut [],
        )
        .map_err(Diagnostic::from)?
        .into_iter()
        .map(|outputs| outputs.height)
        .collect())
}

/// Planes a block of rows reads and fills: the planes of external inputs, and the field planes
/// bakes keep (see [FieldPlan]); empty for bakes that read and keep none.
#[derive(Default)]
struct FieldAccess<'a> {
    /// Every plane the rows' samples read: kept fields and external inputs.
    injected: &'a [&'a program::Plane],
    /// The external inputs among `injected`. Height samples for the relief read only these, at
    /// the texel they are taken for.
    externals: &'a [&'a program::Plane],
    capture: &'a mut [program::FieldPlane],
}

impl<'a> FieldAccess<'a> {
    /// Access to the planes of external inputs alone.
    fn reading(externals: &'a [&'a program::Plane]) -> Self {
        Self {
            injected: externals,
            externals,
            capture: &mut [],
        }
    }
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
    // A sample that reads an external plane reads it at a texel, so equal sample points no longer
    // mean equal results. With such planes a texel repeats only the texel before it in its row
    // (the plane's own value there is not read) and never the row above, which would depend on
    // where blocks of rows begin.
    let reads_planes = !fields.externals.is_empty();
    let above = |row: usize| {
        !reads_planes && row > 0 && rows[row - 1] == (rows[row].0, rows[row].1.wrapping_sub(1))
    };
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
        // Each row of texels has its own operation allowance, sized by its length; every sample
        // keeps its step budget.
        evaluator.replenish_row(cell);
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
        // The relief samples up to two more points for each texel, and two neighbor rows.
        evaluator.replenish_row(chart.width * 4);
        // The heights one step along each leg that no adjacent texel already has, for the texels
        // that need new slopes, are evaluated together; the texels below take them in order.
        let mut direct = Vec::new();
        // A point one step along a leg that is no texel's center reads external planes at that
        // texel's leg point: the value a plane baked from another material holds for it, or
        // the texel's own in a plane that has none between texels.
        let mut direct_texels = Vec::new();
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
                        if reads_planes {
                            let own = (chart.y + y) * atlas.width + chart.x + x;
                            direct_texels.push(program::Plane::leg(
                                atlas.width * atlas.height,
                                own,
                                leg,
                            ));
                        }
                    }
                }
                neighbors.push(found);
            }
        }
        let mut neighbors = neighbors.into_iter();
        let externals = fields.externals;
        let mut direct_heights =
            heights_of(evaluator, &direct, &direct_texels, externals)?.into_iter();
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
                        let mut point_texels = Vec::new();
                        let mut fresh = Vec::new();
                        for other in 0..chart.width {
                            let (triangle, u, v) = atlas.texel(chart, other, ny);
                            let key = (triangle, u.to_bits(), v.to_bits());
                            let new = before != Some(key);
                            fresh.push(new);
                            if new {
                                points.push(atlas.sample(triangle, u, v).map_err(&failure)?);
                                if reads_planes {
                                    point_texels
                                        .push((chart.y + ny) * atlas.width + chart.x + other);
                                }
                            }
                            before = Some(key);
                        }
                        let mut computed =
                            heights_of(evaluator, &points, &point_texels, externals)?.into_iter();
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

/// Whether a program queries the receiver's own creases, edges, seams and contacts.
fn queries_features(program: &Program) -> bool {
    let inputs = &program.metadata().required_inputs;
    inputs.iter().any(|input| input == "features")
}

/// An evaluator of `program` over a receiver: feature queries see the receiver's own creases
/// and open edges, from the same world-space positions the atlas samples, and its named
/// features. Both are derived and indexed once with the layout (see [`Atlas::features`]), not
/// for each bake or thread.
fn receiver_evaluator<'a>(
    program: &'a Program,
    inputs: &'a Inputs,
    atlas: &'a Atlas,
) -> Result<program::Evaluator<'a>, program::ProgramDiagnostic> {
    static NONE: std::sync::OnceLock<program::FeatureSet> = std::sync::OnceLock::new();
    let features = if queries_features(program) {
        atlas.features().0
    } else {
        NONE.get_or_init(program::FeatureSet::default)
    };
    program.evaluator_with_feature_sets(inputs, features, &atlas.named)
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
    externals: &[&program::Plane],
    cancelled: &dyn Fn() -> bool,
) -> Result<(usize, Vec<(usize, Row)>), Diagnostic> {
    let entry = program.metadata().entry.as_str();
    let failure = |message: String| Diagnostic::message(entry, message);
    let live_fields = live_field_names(program, inspect);
    let live_selection = if groups.contains(Group::Live) {
        program.select_fields(&live_fields).map_err(&failure)?
    } else {
        program::FieldSelection::default()
    };
    let rows = atlas_rows(atlas);
    let inspected = inspected_fields(program, inspect);
    let mut evaluator = receiver_evaluator(program, inputs, atlas).map_err(Diagnostic::from)?;
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
            &mut FieldAccess::reading(externals),
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
    [wanted, evaluable]: [Groups; 2],
    provided: Option<(Groups, Vec<Row>)>,
    field_plan: Option<&FieldPlan>,
    externals: &[&program::Plane],
    capture_budget: &mut usize,
    rasters: &mut Rasters,
    cancelled: &dyn Fn() -> bool,
) -> Result<
    (
        Vec<(Group, BakedReceiver)>,
        Vec<(ContentKey, program::FieldPlane, bool)>,
    ),
    Diagnostic,
> {
    let failure = |message: String| Diagnostic::message(&program.metadata().entry, message);
    let pixels = atlas.width * atlas.height;
    if pixels > 4 * 1024 * 1024 {
        return Err(failure(
            "Material atlas exceeds its pixel allowance.".into(),
        ));
    }
    let live_fields = live_field_names(program, inspect);
    // The per-texel data maps are baked from. Without live operations there is nothing to
    // evaluate for the live group.
    let data_of = |groups: Groups| {
        if live_fields.is_empty() {
            groups.rows().without(Group::Live)
        } else {
            groups.rows()
        }
    };
    // Rows evaluated elsewhere are merged exactly like rows evaluated here; a set that does not
    // fit this atlas or lacks data is ignored and the rows are evaluated here instead.
    let provided = provided.filter(|(have, provided)| {
        provided_fits(atlas, wanted, live_fields.len(), *have, provided)
    });
    // Rows evaluated here read only the planes this call was given (`externals`), so they are
    // evaluated for the maps those planes serve; `evaluable` is every wanted map unless the
    // request binds planes that only some maps read.
    let wanted = if provided.is_some() {
        wanted
    } else {
        evaluable
    };
    #[cfg(test)]
    BAKED.with(|baked| baked.borrow_mut().push(wanted));
    let data = data_of(wanted);
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
    let mut provided = provided.map(|(_, rows)| rows.into_iter());
    let evaluator = || receiver_evaluator(program, inputs, atlas).map_err(Diagnostic::from);
    let entry = program.metadata().entry.as_str();
    let threads = threads.max(1);
    let mut serial = if threads == 1 && provided.is_none() && !data.is_empty() {
        Some(evaluator()?)
    } else {
        None
    };
    // Planes of fields kept from earlier bakes are read, and planes of fields this bake computes are
    // filled, by the serial evaluator only. Planes of external inputs are read by every evaluator.
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
    // The intrinsic fields among the planes being filled: the values their rasters hold at
    // this receiver's lattice points are read instead of computed (as a kept plane's are), and
    // what this bake computes at lattice points goes to the rasters when it finishes.
    let mut lattice: Vec<(usize, LatticePoint)> = Vec::new();
    let mut mapped: Vec<program::Plane> = Vec::new();
    let mut intrinsic: Vec<(usize, ContentKey)> = Vec::new();
    if let Some(plan) = field_plan.filter(|plan| !plan.intrinsic.is_empty()) {
        for (slot, plane) in captured.iter_mut().enumerate() {
            let Some(key) = plane.field().and_then(|field| plan.intrinsic.get(&field)) else {
                continue;
            };
            let Some(working) = rasters.get_mut(key) else {
                continue;
            };
            if !working.shared {
                // The first layout to bake the field: leave the mark for the next.
                working.shared = true;
                working.changed = true;
                continue;
            }
            if intrinsic.is_empty() {
                lattice = lattice_points(atlas);
            }
            let raster = &working.raster;
            let mut found = 0;
            for (texel, point) in &lattice {
                if let Some(value) = raster.get(*point) {
                    plane.put_raw(*texel, value);
                    found += 1;
                }
            }
            if found > 0 {
                mapped.push(plane.completing());
            }
            #[cfg(test)]
            INTRINSIC.with(|log| log.borrow_mut().push((lattice.len(), found)));
            intrinsic.push((slot, *key));
        }
    }
    let mut injected_planes: Vec<&program::Plane> = field_plan
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
    let kept_planes = injected_planes.len();
    injected_planes.extend(&mapped);
    injected_planes.extend(externals);
    #[cfg(test)]
    PLANES.with(|planes| planes.borrow_mut().push((kept_planes, captured.len())));
    #[cfg(not(test))]
    let _ = kept_planes;
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
                    externals,
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
                                &mut FieldAccess::reading(externals),
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
    // What this bake computed of intrinsic fields at lattice points goes to the build's rasters.
    for (slot, key) in intrinsic {
        let Some(working) = rasters.get_mut(&key) else {
            continue;
        };
        for (texel, point) in &lattice {
            if let Some(value) = captured[slot].raw(*texel) {
                if working.raster.get(*point).is_none() {
                    working.raster.put(*point, value);
                    working.changed = true;
                }
            }
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
        // Planes bound to the program's external inputs, as a build of the request binds them.
        let bound = self.bound_planes(
            &program,
            request,
            std::slice::from_ref(&receiver),
            assets,
            None,
        )?;
        let planes = &bound[0];
        let mut targets: Vec<String> = vec!["program".into(), "assets".into(), "layout/0".into()];
        let mut source_images: BTreeMap<String, Arc<[u8]>> = BTreeMap::new();
        plane_nodes(&bound, 0, assets, &mut nodes, &mut source_images, false).map_err(&fail)?;
        targets.extend(planes.iter().map(|(_, id)| id.clone()));
        let plan = Plan::new(nodes, targets).map_err(|e| fail(e.to_string()))?;
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
                        let name = &id["image/".len()..];
                        decoded_image(
                            image_bytes
                                .get(name)
                                .unwrap_or_else(|| &source_images[name]),
                        )
                    }
                    id if id.starts_with("program:") => {
                        let program = &bound.sources[&id["program:".len()..]].program;
                        Ok((Artifact::Program(program.clone()), program.retained_bytes()))
                    }
                    id if id.starts_with("assets:") => source_assets(dependencies, &mips),
                    "assets" => decoded_assets(&image_bytes, dependencies, &mips),
                    "layout/0" => receiver_layout(&receiver, request),
                    id if id.starts_with("plane/0/") => {
                        let name = bound.nodes[id].name.as_str();
                        let Artifact::Receiver(atlas) = dependencies["mapping"].as_ref() else {
                            unreachable!()
                        };
                        match &bound.nodes[id].bound {
                            Bound::Asset(bytes) => {
                                let components = plane_components(&program, &bound, name);
                                decoded_plane(components, name, bytes, atlas)
                            }
                            Bound::Material { material, value } => {
                                let Artifact::Inputs(decoded) =
                                    dependencies["source-assets"].as_ref()
                                else {
                                    unreachable!()
                                };
                                let mut baked = bake_source_planes(
                                    &bound.sources[material],
                                    decoded,
                                    atlas,
                                    &[(name, value.as_str())],
                                    None,
                                    cancelled,
                                )
                                .map_err(|error| error.message)?;
                                let plane = baked_plane(&program, name, baked.remove(0))?;
                                let bytes = plane.bytes();
                                Ok((Artifact::Material(plane), bytes))
                            }
                        }
                    }
                    _ => Ok((Artifact::Source, node.inputs.len())),
                },
            )
            .map_err(&fail)?;
        let plane_keys: Vec<(String, ContentKey)> = planes
            .iter()
            .map(|(name, id)| (name.clone(), plan.key(id).expect("planned plane")))
            .collect();
        let externals: Vec<&program::Plane> = planes
            .iter()
            .filter_map(|(_, id)| match complete[id].as_ref() {
                Artifact::Material(baked) => match baked.as_ref() {
                    BakedReceiver::Plane(plane) => Some(plane),
                    _ => None,
                },
                _ => None,
            })
            .collect();
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
            render: None,
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
                    &plane_keys,
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
            let selection = program.select_fields(&missing).map_err(&fail)?;
            let mut evaluator = receiver_evaluator(&program, &inputs, atlas)
                .map_err(|e| vec![Diagnostic::from(e)])?;
            let slots = selection.len();
            let mut computed = vec![vec![0f32; pixels * 4]; slots];
            let mut values = Vec::new();
            for chart in &atlas.charts {
                for y in 0..chart.height {
                    if cancelled() {
                        return Err(fail("Field bake cancelled.".into()));
                    }
                    evaluator.replenish_row(chart.width);
                    // A texel that samples its left neighbor's point repeats its values; the
                    // others are evaluated together, as a bake's rows are.
                    let mut before: Option<(usize, u64, u64)> = None;
                    let mut fresh = Vec::with_capacity(chart.width);
                    let mut samples = Vec::new();
                    // Each sample's texel, for the planes of external inputs (none without them).
                    let mut texels = Vec::new();
                    let mut sample_error = None;
                    for x in 0..chart.width {
                        let (triangle, u, v) = atlas.texel(chart, x, y);
                        let key = (triangle, u.to_bits(), v.to_bits());
                        let new = before != Some(key);
                        before = Some(key);
                        fresh.push(new);
                        if new {
                            match atlas.sample(triangle, u, v) {
                                Ok(sample) => {
                                    samples.push(sample);
                                    if !externals.is_empty() {
                                        texels.push((chart.y + y) * atlas.width + chart.x + x);
                                    }
                                }
                                Err(message) => {
                                    sample_error = Some(message);
                                    break;
                                }
                            }
                        }
                    }
                    // Samples before one the atlas cannot place fail first, as in texel order.
                    evaluator
                        .evaluate_row_with_planes(
                            &samples,
                            &texels,
                            program::OutputSet::NONE,
                            &selection,
                            &mut values,
                            &externals,
                            &mut [],
                        )
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
        let pending: PendingBake = serde_json::from_str(
            &serde_json::to_string(&session.distribution.get_mut().pending[0]).unwrap(),
        )
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
            session.distribution.get_mut().pending[0].groups,
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
        assert!(stranger.distribution.get_mut().provided.is_empty());
        // The reporting session keeps it until a build takes it.
        session.provide_bake_share(&share).unwrap();
        assert_eq!(session.distribution.get_mut().provided.len(), 1);
        // Many superseded edits: reports, descriptions and rows stay bounded.
        for edit in 0..(MAX_REPORTED + 40) {
            let edited = request(json!({ "tint": edit as f64 / 2000. }), 1, 96);
            assert!(
                session
                    .build(&edited, 2 + edit as u64, &no_assets, &|| false)
                    .is_err()
            );
        }
        let distribution = session.distribution.get_mut();
        assert!(distribution.reported.len() <= MAX_REPORTED);
        assert!(distribution.pending.len() <= MAX_PENDING);
        assert!(
            distribution.provided.is_empty(),
            "the rows of the forgotten first report went with it"
        );
        session.provide_bake_share(&share).unwrap();
        assert!(
            session.distribution.get_mut().provided.is_empty(),
            "a forgotten bake's rows are dropped"
        );
        // Disabling drops everything.
        session.distribute_bakes(false);
        let distribution = session.distribution.get_mut();
        assert!(distribution.reported.is_empty() && distribution.pending.is_empty());
    }

    #[test]
    fn late_rows_do_not_survive_reset_even_when_a_new_build_requests_the_same_content() {
        let mut session = Session::default();
        session.distribute_bakes(true);
        let recipe = request(json!({}), 0, 96);
        let mut helpers = Session::default();
        assert!(session.build(&recipe, 1, &no_assets, &|| false).is_err());
        let before = session.take_pending_bake().unwrap();
        let old_share = helpers
            .bake_share(&before, 0, 1, &no_assets, &|| false)
            .unwrap();
        session.provide_bake_share(&old_share).unwrap();
        session.clear();
        assert!(session.take_pending_bake().is_none());
        assert!(session.build(&recipe, 1, &no_assets, &|| false).is_err());
        let after = session.take_pending_bake().unwrap();
        assert_eq!(before.keys, after.keys);
        session.provide_bake_share(&old_share).unwrap();
        let errors = session
            .build(&recipe, 1, &no_assets, &|| false)
            .err()
            .expect("stale rows must leave the new bake pending");
        assert_eq!(errors[0].code, "BAKE_PENDING");
        let after = session.take_pending_bake().unwrap();
        let new_share = helpers
            .bake_share(&after, 0, 1, &no_assets, &|| false)
            .unwrap();
        session.provide_bake_share(&new_share).unwrap();
        assert!(session.build(&recipe, 1, &no_assets, &|| false).is_ok());
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

#[cfg(test)]
#[path = "material_planes_tests.rs"]
mod plane_tests;

#[cfg(test)]
#[path = "material_layers_tests.rs"]
mod layer_tests;

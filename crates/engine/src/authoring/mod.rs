//! Native/browser authoring sessions. Source, supplied bytes and preview assignments are explicit.
mod item;
mod layout;
pub(crate) mod material;
pub mod occlusion;
mod project;
pub(crate) mod receiver;
pub mod recipe;
pub use layout::{LAYOUT_RECORD, ProjectLayouts, ReceiverLayout};
pub use material::{FieldBake, PendingBake};
pub use project::{
    Base, MAX_ASSIGNMENTS, MAX_MODELS, MaterialAssignment, ModelSource, Project, ProjectOutput,
    Target,
};

use crate::materialspec::program::{self, Program, Sources};
use crate::session::Artifact;
pub use crate::session::Session;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, sync::Arc};
use superspec_model::{Scene, instances::Model};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Receiver {
    pub id: String,
    pub object_id: String,
    pub positions: Vec<f64>,
    pub normals: Vec<f64>,
    pub uvs: Vec<f64>,
    pub indices: Vec<u32>,
    #[serde(default)]
    pub object_to_world: Option<[f64; 16]>,
    /// Segments where this receiver's edges meet another surface, in world meters. Hosts that
    /// know the whole scene supply them; they become `contact` features for receiver queries.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub contacts: Vec<Contact>,
}

/// A finite segment, in world meters, where a receiver meets another surface.
#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
pub struct Contact {
    pub start: [f64; 3],
    pub end: [f64; 3],
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Mapping {
    Uv,
    #[default]
    Object,
    World,
}

/// How a bake is scheduled. Both profiles evaluate the same program with the same seeds and
/// write byte-identical texels for a given resolution; production only admits larger atlases and
/// splits a receiver's rows across native threads.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Profile {
    #[default]
    Preview,
    Production,
}

impl Profile {
    /// Largest texture edge, in pixels, admitted for one receiver.
    pub fn max_resolution(self) -> usize {
        match self {
            Self::Preview => 512,
            Self::Production => 2048,
        }
    }

    /// Worker threads for one receiver's rows. Wasm and previews bake on the calling thread.
    pub fn threads(self) -> usize {
        if cfg!(target_arch = "wasm32") || self == Self::Preview {
            return 1;
        }
        std::thread::available_parallelism().map_or(1, |n| n.get().min(8))
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Request {
    pub kind: String,
    pub entry: String,
    pub source: String,
    pub modules: BTreeMap<String, String>,
    pub parameters: BTreeMap<String, serde_json::Value>,
    pub seed: u32,
    pub resolution: usize,
    pub inspection: Option<String>,
    pub receivers: Vec<Receiver>,
    pub mapping: Mapping,
    pub domain: [f64; 2],
    pub edges: Vec<program::Edge>,
    pub gravity: [f64; 3],
    pub placement_transform: Option<crate::itemspec::definition::Transform>,
    pub slot_overrides: BTreeMap<String, program::MaterialRequest>,
    pub profile: Profile,
    /// Texels per meter every receiver is baked at when its atlas can reach it within
    /// `resolution`; receivers that reach it in a smaller texture get the smaller texture.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub texel_density: Option<f64>,
    /// Planes supplied for the program's external inputs (`external(name, fallback)`), by
    /// receiver id and then input name (see [`PlaneSource`]). The swatch a request without
    /// receivers bakes has the id `studio/swatch`. Names the program does not read are neither
    /// fetched nor baked.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub planes: BTreeMap<String, BTreeMap<String, PlaneSource>>,
    /// Other materials of this request's modules whose values planes are baked from
    /// ([`PlaneSource::Material`]), by a name of the host's choosing: a base under this
    /// request's layer, for example. Each is evaluated over the same receivers, mapping, edges
    /// and gravity, with its own parameters and seed.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub materials: BTreeMap<String, program::MaterialRequest>,
    /// The features each receiver's owner named, by receiver id: polylines in world meters that
    /// programs measure to with `feature_distance(name, limit)` and `feature_tangent`. They are
    /// part of a receiver's layout, like its geometry.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub features: BTreeMap<String, Vec<program::NamedFeature>>,
}

/// An item's features (`itemspec::definition::output::Feature`: joint, end, bracket-contact,
/// opening-edge, sill, base, top) as named receiver features, in world meters: `transform` is
/// the column-major placement of the space the item's features and bounds are in. Every
/// receiver of the item may be given all of them; a query finds only those near its samples.
pub fn item_features(
    features: &[crate::itemspec::definition::Feature],
    transform: &[f64; 16],
) -> Vec<program::NamedFeature> {
    let place = |p: &[f64; 3]| -> [f64; 3] {
        std::array::from_fn(|row| {
            transform[row] * p[0] + transform[4 + row] * p[1] + transform[8 + row] * p[2]
                + transform[12 + row]
        })
    };
    features
        .iter()
        .map(|feature| program::NamedFeature {
            name: feature.name.clone(),
            points: feature.points.iter().map(place).collect(),
            closed: feature.closed,
        })
        .collect()
}

/// Where the plane bound to an external input comes from.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(untagged)]
pub enum PlaneSource {
    /// An asset holding an encoded plane (`program::Plane::encode_external`) over the
    /// receiver's atlas.
    Asset(String),
    /// A value of one of the request's [`Request::materials`], baked over the receiver in the
    /// same build: a field (exact, with its filtering estimate) or one of the outputs
    /// `base_color`, `roughness`, `metalness`, `height`, `emissive` and `opacity`.
    Material { material: String, value: String },
}
impl From<&str> for PlaneSource {
    fn from(asset: &str) -> Self {
        Self::Asset(asset.to_owned())
    }
}
impl From<String> for PlaneSource {
    fn from(asset: String) -> Self {
        Self::Asset(asset)
    }
}
impl Default for Request {
    fn default() -> Self {
        Self {
            kind: "material".into(),
            entry: "main.ms".into(),
            source: String::new(),
            modules: BTreeMap::new(),
            parameters: BTreeMap::new(),
            seed: 0,
            resolution: 128,
            inspection: None,
            receivers: vec![],
            mapping: Mapping::Object,
            domain: [1., 1.],
            edges: vec![],
            gravity: [0., 0., -1.],
            placement_transform: None,
            slot_overrides: BTreeMap::new(),
            profile: Profile::Preview,
            texel_density: None,
            planes: BTreeMap::new(),
            materials: BTreeMap::new(),
            features: BTreeMap::new(),
        }
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Diagnostic {
    pub code: String,
    pub source_name: String,
    pub line: usize,
    pub column: usize,
    pub offset: usize,
    pub end: Option<usize>,
    pub message: String,
}
impl From<program::ProgramDiagnostic> for Diagnostic {
    fn from(value: program::ProgramDiagnostic) -> Self {
        Self {
            code: value.code.into(),
            source_name: value.source.module,
            line: value.source.location.line,
            column: value.source.location.column,
            offset: value.source.location.offset,
            end: value.source.location.end,
            message: value.message,
        }
    }
}
impl Diagnostic {
    pub fn message(source: &str, message: impl Into<String>) -> Self {
        Self {
            code: "AUTHORING_INPUT".into(),
            source_name: source.into(),
            line: 1,
            column: 1,
            offset: 0,
            end: None,
            message: message.into(),
        }
    }
}

pub struct Output {
    pub model: Model,
    pub metadata: serde_json::Value,
}
impl Output {
    pub fn scene(&self, revision: u32) -> Result<Scene, String> {
        self.model.scene(revision)
    }
}

pub(crate) struct CachedProgram {
    sources: BTreeMap<String, String>,
    entry: String,
    program: Arc<Program>,
    bytes: usize,
}
impl Session {
    /// Discovers explicit built-in image identities from checked material modules, never source text guesses.
    pub fn library_assets(&mut self, request: &Request) -> Vec<String> {
        let entries = if request.kind == "material" {
            vec![request.entry.clone()]
        } else if request.kind == "item" {
            crate::itemspec::definition::parse(&request.entry, &request.source)
                .map(|mut definition| {
                    definition.slots.extend(request.slot_overrides.clone());
                    definition
                        .slots
                        .values()
                        .map(|slot| slot.entry.clone())
                        .collect()
                })
                .unwrap_or_default()
        } else {
            vec![]
        };
        let mut names = std::collections::BTreeSet::new();
        for entry in entries {
            let source = if entry == request.entry {
                Some(request.source.clone())
            } else {
                request.modules.get(&entry).cloned()
            };
            if let Some(source) = source {
                if let Ok(program) = self.program(&Request {
                    kind: "material".into(),
                    entry,
                    source,
                    ..request.clone()
                }) {
                    names.extend(
                        program
                            .metadata()
                            .required_inputs
                            .iter()
                            .filter_map(|input| input.strip_prefix("image:"))
                            .filter(|name| name.starts_with("superspec-material:"))
                            .map(str::to_owned),
                    );
                }
            }
        }
        names.into_iter().collect()
    }

    pub fn build(
        &mut self,
        request: &Request,
        revision: u64,
        assets: &dyn Fn(&str) -> Result<Arc<[u8]>, String>,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Output, Vec<Diagnostic>> {
        if request.source.len() > 1024 * 1024
            || request.modules.len() > 128
            || request.modules.values().map(String::len).sum::<usize>() > 8 * 1024 * 1024
        {
            return Err(vec![Diagnostic::message(
                &request.entry,
                "Authoring source allowance exceeded.",
            )]);
        }
        self.begin_authoring(revision, &request.entry)?;
        match request.kind.as_str() {
            "material" => self.material(request, revision, assets, cancelled),
            "item" | "import-glb" => self.item(request, revision, assets, cancelled),
            _ => Err(vec![Diagnostic::message(
                &request.entry,
                "Unknown authoring request kind.",
            )]),
        }
    }

    pub(crate) fn begin_authoring(
        &self,
        revision: u64,
        entry: &str,
    ) -> Result<(), Vec<Diagnostic>> {
        if self
            .authoring_revision
            .get()
            .is_some_and(|previous| revision < previous)
        {
            return Err(vec![Diagnostic::message(
                entry,
                "Build revision is obsolete.",
            )]);
        }
        self.authoring_revision.set(Some(revision));
        self.begin_build();
        Ok(())
    }

    fn program(&mut self, request: &Request) -> Result<Arc<Program>, Vec<Diagnostic>> {
        let mut modules = request.modules.clone();
        modules.insert(request.entry.clone(), request.source.clone());
        if let Some(cached) = self.programs.get_mut().iter().find(|cached| {
            cached.entry == request.entry
                && cached
                    .sources
                    .iter()
                    .all(|(name, text)| modules.get(name) == Some(text))
        }) {
            return Ok(cached.program.clone());
        }
        let program = Arc::new(
            program::compile(&Sources {
                entry: request.entry.clone(),
                modules: modules.clone(),
            })
            .map_err(|errors| errors.into_iter().map(Diagnostic::from).collect::<Vec<_>>())?,
        );
        self.retain_program(program.clone(), 32 * 1024 * 1024);
        Ok(program)
    }

    fn retain_program(&mut self, program: Arc<Program>, allowance: usize) {
        let sources: BTreeMap<String, String> = program
            .source_modules()
            .map(|(name, text)| (name.to_owned(), text.to_owned()))
            .collect();
        let entry = program.metadata().entry.clone();
        // This cache owns another source lookup copy in addition to the Program.
        let map_node = 11 * std::mem::size_of::<(String, String)>() + 128;
        let bytes = program
            .retained_bytes()
            .saturating_add(std::mem::size_of::<CachedProgram>())
            .saturating_add(entry.capacity() + 32)
            .saturating_add(sources.len().saturating_mul(map_node))
            .saturating_add(
                sources
                    .iter()
                    .map(|(name, text)| name.capacity() + text.capacity() + 64)
                    .sum::<usize>(),
            );
        if bytes > allowance {
            return;
        }
        while !self.programs.get_mut().is_empty()
            && (self.programs.get_mut().len() >= 16
                || self.program_bytes.get().saturating_add(bytes) > allowance)
        {
            self.program_bytes
                .set(self.program_bytes.get() - self.programs.get_mut().remove(0).bytes);
        }
        self.program_bytes.set(self.program_bytes.get() + bytes);
        self.programs.get_mut().push(CachedProgram {
            sources,
            entry,
            program,
            bytes,
        });
    }
}

#[cfg(test)]
mod program_retention_tests {
    use super::*;

    fn request(value: f64) -> Request {
        Request {
            source: format!("material 1; output base_color=vec3({value});"),
            ..Default::default()
        }
    }

    #[test]
    fn program_retention_is_byte_bounded_and_oversize_artifacts_still_work() {
        let mut session = Session::default();
        let a = session.program(&request(0.2)).unwrap();
        assert!(Arc::ptr_eq(&a, &session.program(&request(0.2)).unwrap()));
        let a_cost = session.programs.borrow()[0].bytes;
        let b = session.program(&request(0.8)).unwrap();
        let b_cost = session.programs.borrow()[1].bytes;
        assert!(
            a_cost > a.retained_bytes(),
            "the source lookup copy must also be charged"
        );
        let budget = a_cost.max(b_cost);
        let mut bounded = Session::default();
        bounded.retain_program(a.clone(), budget);
        bounded.retain_program(b.clone(), budget);
        assert_eq!(bounded.programs.borrow().len(), 1);
        assert!(Arc::ptr_eq(&bounded.programs.borrow()[0].program, &b));
        assert!(bounded.program_bytes.get() <= budget);
        let mut rejected = Session::default();
        rejected.retain_program(a.clone(), a_cost - 1);
        assert!(rejected.programs.borrow().is_empty());
        assert_eq!(
            a.evaluate(&Default::default(), &Default::default())
                .unwrap()
                .base_color,
            [0.2; 3]
        );
        rejected.retain_program(b, b_cost);
        assert_eq!(rejected.programs.borrow().len(), 1);
        assert_eq!(rejected.program_bytes.get(), b_cost);
    }
}

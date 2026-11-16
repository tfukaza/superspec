//! A bounded, portable material language. Hosts supply source modules and decoded images.
mod bake;
mod check;
mod compiled;
mod eval;
mod features;
mod images;
mod memory;
mod numeric;
mod syntax;
mod ts;
pub use eval::FieldSelection;
pub use features::{
    COPLANAR_DEGREES, MAX_FEATURES, bend_shaded, orient_relief, receiver_features, receiver_seams,
};
pub use ts::{
    EditFailure, EditOp, Edited, Inspection, Patch, Span, apply as apply_edit,
    inspect as inspect_recipe,
};

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use superspec_types::diagnostic::SourceLocation;

pub const COMMON_HELPERS: &str = include_str!("examples/common.ms");
pub const CHIPPED_CONCRETE: &str = include_str!("examples/chipped-concrete.ms");
pub const PAINTED_METAL: &str = include_str!("examples/painted-metal.ms");
/// TypeScript recipes: the same studies as the `.ms` examples, plus a layered group showcase.
pub const COMMON_HELPERS_TS: &str = include_str!("examples/common.material.ts");
pub const CHIPPED_CONCRETE_TS: &str = include_str!("examples/chipped-concrete.material.ts");
pub const PAINTED_METAL_TS: &str = include_str!("examples/painted-metal.material.ts");
pub const TILED_COURTYARD_TS: &str = include_str!("examples/tiled-courtyard.material.ts");

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct Sources {
    pub entry: String,
    pub modules: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct MaterialRequest {
    pub entry: String,
    #[serde(default)]
    pub parameters: BTreeMap<String, serde_json::Value>,
    #[serde(default)]
    pub seed: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Type {
    Scalar,
    Vec2,
    Vec3,
    Bool,
    String,
}

#[derive(Clone, Debug, Serialize)]
pub struct Source {
    pub module: String,
    pub location: SourceLocation,
}

#[derive(Clone, Debug, Serialize)]
pub struct ProgramDiagnostic {
    pub code: &'static str,
    pub message: String,
    pub source: Source,
}
impl std::fmt::Display for ProgramDiagnostic {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}:{}:{} {}",
            self.source.module,
            self.source.location.line,
            self.source.location.column,
            self.message
        )
    }
}
impl std::error::Error for ProgramDiagnostic {}

#[derive(Clone, Debug, Serialize)]
pub struct Parameter {
    pub name: String,
    #[serde(rename = "type")]
    pub kind: Type,
    pub default: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    pub source: Source,
}
#[derive(Clone, Debug, Serialize)]
pub struct Field {
    pub name: String,
    #[serde(rename = "type")]
    pub kind: Type,
    pub source: Source,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Metadata {
    pub version: u32,
    pub entry: String,
    pub dependencies: Vec<String>,
    pub parameters: Vec<Parameter>,
    pub fields: Vec<Field>,
    pub outputs: Vec<String>,
    pub required_inputs: Vec<String>,
    /// Live renderer operations declared by a TypeScript recipe. The evaluator never runs them.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub live: Vec<LiveOperation>,
}

/// A renderer operation recorded in the material with the fields that feed it.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveOperation {
    pub id: String,
    pub version: u32,
    pub stage: String,
    /// Operation argument name to the program field holding its value.
    pub inputs: BTreeMap<String, String>,
    /// Inputs and capabilities the renderer must supply.
    pub requires: Vec<String>,
    pub source: Source,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Image {
    pub width: usize,
    pub height: usize,
    pub rgba: Vec<u8>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Edge {
    pub start: [f64; 3],
    pub end: [f64; 3],
}
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct Inputs {
    #[serde(default)]
    pub parameters: BTreeMap<String, serde_json::Value>,
    #[serde(default)]
    pub seed: u32,
    #[serde(default)]
    pub images: BTreeMap<String, Image>,
    #[serde(default)]
    pub edges: Vec<Edge>,
    /// Receiver creases and open edges in world meters. Hosts that bake receivers derive these
    /// per receiver with [`receiver_features`]; direct callers may supply them here.
    #[serde(default)]
    pub features: Vec<Feature>,
    /// Mip levels of `images`, built on first use by filtered sampling and shared by clones.
    #[serde(skip)]
    pub mips: images::MipCache,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum FeatureKind {
    /// Two faces meeting outward, like the corner of a box.
    Convex,
    /// Two faces meeting inward, like a floor meeting a wall.
    Concave,
    /// An edge with only one face, like the rim of an open panel.
    Boundary,
    /// An edge between faces whose UVs differ across it, whether or not the faces bend.
    Seam,
    /// An edge where the receiver meets another surface, supplied by a host that knows the scene.
    Contact,
}

/// A finite receiver edge in world meters.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Feature {
    pub start: [f64; 3],
    pub end: [f64; 3],
    pub kind: FeatureKind,
    /// Angle between the adjoining face normals in degrees; 180 for boundary edges.
    pub angle: f64,
    /// World-space unit normal halfway between the adjoining faces: the normal at the middle of
    /// the edge once it is rounded over. Absent for boundaries, which have one face.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub normal: Option<[f64; 3]>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Sample {
    pub uv: [f64; 2],
    pub surface: [f64; 2],
    pub object: [f64; 3],
    pub world: [f64; 3],
    pub normal: [f64; 3],
    /// Sample diameter in meters; coordinate arithmetic propagates a filtering footprint estimate.
    pub footprint: f64,
    /// Dimensionless UV sample diameter. None leaves UV fields unfiltered.
    pub uv_footprint: Option<f64>,
    pub gravity: [f64; 3],
    /// World-space bounding box of the receiver being sampled, in meters. Hosts that bake a
    /// receiver supply its bounds; a swatch bake supplies its rectangle's.
    pub bounds_min: [f64; 3],
    pub bounds_max: [f64; 3],
}
impl Default for Sample {
    fn default() -> Self {
        Self {
            uv: [0.; 2],
            surface: [0.; 2],
            object: [0.; 3],
            world: [0.; 3],
            normal: [0., 0., 1.],
            footprint: 0.,
            uv_footprint: None,
            gravity: [0., 0., -1.],
            bounds_min: [0.; 3],
            bounds_max: [0.; 3],
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(untagged)]
pub enum FieldValue {
    Scalar(f64),
    Vec2([f64; 2]),
    Vec3([f64; 3]),
    Bool(bool),
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Outputs {
    /// Linear-light RGB; texture baking encodes this as sRGB.
    pub base_color: [f64; 3],
    pub roughness: f64,
    pub metalness: f64,
    /// Signed displacement in meters; baking derives a tangent-space +Y normal map.
    pub height: f64,
    pub emissive: [f64; 3],
    pub opacity: f64,
    /// World-space unit shading normal the relief is applied around; None keeps the receiver's own.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub receiver_normal: Option<[f64; 3]>,
    /// World-space vector added to the shaded normal after the height relief; None adds nothing.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub normal_offset: Option<[f64; 3]>,
    pub fields: BTreeMap<String, FieldValue>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Domain {
    pub origin: [f64; 3],
    /// Object-space edges of the bake rectangle, in meters.
    pub u: [f64; 3],
    pub v: [f64; 3],
    /// Column-major affine object-to-world transform.
    pub world_from_object: [f64; 16],
}
impl Default for Domain {
    fn default() -> Self {
        Self {
            origin: [0.; 3],
            u: [1., 0., 0.],
            v: [0., 1., 0.],
            world_from_object: [
                1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.,
            ],
        }
    }
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BakeRequest {
    pub width: usize,
    pub height: usize,
    #[serde(default)]
    pub domain: Domain,
    #[serde(default)]
    pub inspection: Option<String>,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Baked {
    pub width: usize,
    pub height: usize,
    pub base_color: Vec<u8>,
    pub normal: Vec<u8>,
    /// Linear RGBA8: R=occlusion(1), G=roughness, B=metalness, A=1.
    pub properties: Vec<u8>,
    pub emissive: Vec<u8>,
    /// Multiply decoded emissive RGB by this factor to restore HDR emission.
    pub emissive_strength: f64,
    pub height_meters: Vec<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inspection: Option<Vec<u8>>,
}

#[derive(Clone)]
pub struct Program {
    sources: Sources,
    definitions: BTreeMap<String, syntax::Definition>,
    outputs: BTreeMap<String, syntax::Expr>,
    metadata: Metadata,
    /// The same definitions with names and calls resolved, built once after checking.
    compiled: std::sync::Arc<compiled::Compiled>,
}
/// A finite batch of samples over one immutable, validated set of material inputs.
pub struct Evaluator<'a> {
    program: &'a Program,
    inputs: &'a Inputs,
    features: &'a [Feature],
    parameters: Vec<eval::Value<'static>>,
    remaining: usize,
    scratch: eval::Scratch<'a>,
}
impl<'a> Evaluator<'a> {
    pub fn evaluate(&mut self, sample: &Sample) -> Result<Outputs, ProgramDiagnostic> {
        self.wanted(sample, eval::Want::All)
    }
    /// Every output channel, but only the one inspected field (or none), for bakes. Channel values
    /// are identical to [`Self::evaluate`]'s; unrequested fields are simply not computed.
    pub fn evaluate_channels(
        &mut self,
        sample: &Sample,
        inspect: Option<&str>,
    ) -> Result<Outputs, ProgramDiagnostic> {
        match inspect {
            Some(name) => self.wanted(sample, eval::Want::Channels(&[name])),
            None => self.wanted(sample, eval::Want::Channels(&[])),
        }
    }
    /// Every output channel plus the named fields, such as live renderer arguments.
    pub fn evaluate_channels_with(
        &mut self,
        sample: &Sample,
        fields: &[&str],
    ) -> Result<Outputs, ProgramDiagnostic> {
        self.wanted(sample, eval::Want::Channels(fields))
    }
    /// Every output channel plus fields chosen once with [Program::select_fields], written to
    /// `values` in the order they were named. The returned outputs' own field map stays empty, so
    /// a bake can read per-texel fields without allocating or comparing names.
    pub fn evaluate_selected(
        &mut self,
        sample: &Sample,
        selection: &FieldSelection,
        values: &mut [FieldValue],
    ) -> Result<Outputs, ProgramDiagnostic> {
        self.wanted(sample, eval::Want::Selected(selection, values))
    }
    /// Only the height output, for derivative samples; identical to [`Self::evaluate`]'s height.
    pub fn evaluate_height(&mut self, sample: &Sample) -> Result<f64, ProgramDiagnostic> {
        Ok(self.wanted(sample, eval::Want::Height)?.height)
    }
    /// Grants a fresh operation allowance (at most 64,000,000), for hosts that bound work per
    /// row of a bake; each sample keeps its own step and depth budget.
    pub fn replenish(&mut self, operations: usize) {
        self.remaining = operations.min(64_000_000);
    }
    fn wanted(
        &mut self,
        sample: &Sample,
        want: eval::Want<'_>,
    ) -> Result<Outputs, ProgramDiagnostic> {
        eval::evaluate_wanted(
            self.program,
            &self.program.compiled,
            self.inputs,
            self.features,
            sample,
            &self.parameters,
            &mut self.remaining,
            &mut self.scratch,
            want,
        )
    }
}
pub fn compile(sources: &Sources) -> Result<Program, Vec<ProgramDiagnostic>> {
    check::compile(sources)
}
impl Program {
    pub fn metadata(&self) -> &Metadata {
        &self.metadata
    }
    pub fn source_modules(&self) -> impl Iterator<Item = (&str, &str)> {
        std::iter::once(self.metadata.entry.as_str())
            .chain(self.metadata.dependencies.iter().map(String::as_str))
            .map(|name| (name, self.sources.modules[name].as_str()))
    }
    pub fn evaluate(&self, inputs: &Inputs, sample: &Sample) -> Result<Outputs, ProgramDiagnostic> {
        eval::evaluate(self, inputs, sample)
    }
    /// Resolves field names once for [Evaluator::evaluate_selected].
    pub fn select_fields(&self, names: &[&str]) -> Result<FieldSelection, String> {
        FieldSelection::new(self, names)
    }
    pub fn evaluator<'a>(&'a self, inputs: &'a Inputs) -> Result<Evaluator<'a>, ProgramDiagnostic> {
        self.evaluator_with_budget(inputs, 64_000_000)
    }
    pub fn evaluator_with_budget<'a>(
        &'a self,
        inputs: &'a Inputs,
        operations: usize,
    ) -> Result<Evaluator<'a>, ProgramDiagnostic> {
        if operations == 0 || operations > 64_000_000 {
            return Err(self.diagnostic(
                "MS_LIMIT",
                "A sample batch requires an operation allowance from 1 to 64000000.",
                &syntax::Mark::root(&self.metadata.entry),
            ));
        }
        let parameters = eval::validate_inputs(self, inputs)?;
        Ok(Evaluator {
            program: self,
            inputs,
            features: &inputs.features,
            parameters,
            remaining: operations,
            scratch: eval::Scratch::default(),
        })
    }
    /// An evaluator whose receiver features replace `inputs.features`, so a host can bake each
    /// receiver against its own geometry while sharing parameters and decoded images.
    pub fn evaluator_with_features<'a>(
        &'a self,
        inputs: &'a Inputs,
        features: &'a [Feature],
    ) -> Result<Evaluator<'a>, ProgramDiagnostic> {
        let mut evaluator = self.evaluator(inputs)?;
        eval::validate_features(self, features)?;
        evaluator.features = features;
        Ok(evaluator)
    }
    pub fn bake(&self, request: &BakeRequest, inputs: &Inputs) -> Result<Baked, ProgramDiagnostic> {
        bake::bake(self, request, inputs)
    }
    pub fn bake_cancellable(
        &self,
        request: &BakeRequest,
        inputs: &Inputs,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Baked, ProgramDiagnostic> {
        bake::bake_cancellable(self, request, inputs, cancelled)
    }
    fn diagnostic(
        &self,
        code: &'static str,
        message: impl Into<String>,
        mark: &syntax::Mark,
    ) -> ProgramDiagnostic {
        diagnostic(&self.sources, code, message, mark)
    }
}

fn diagnostic(
    sources: &Sources,
    code: &'static str,
    message: impl Into<String>,
    mark: &syntax::Mark,
) -> ProgramDiagnostic {
    let text = sources
        .modules
        .get(&mark.module)
        .map(String::as_str)
        .unwrap_or("");
    let start = mark.start.min(text.len());
    let end = mark.end.min(text.len());
    let before = &text[..start];
    ProgramDiagnostic {
        code,
        message: message.into(),
        source: Source {
            module: mark.module.clone(),
            location: SourceLocation {
                line: before.bytes().filter(|c| *c == b'\n').count() + 1,
                column: before
                    .rsplit('\n')
                    .next()
                    .unwrap_or("")
                    .encode_utf16()
                    .count()
                    + 1,
                offset: before.encode_utf16().count(),
                end: Some(text[..end].encode_utf16().count()),
            },
        },
    }
}

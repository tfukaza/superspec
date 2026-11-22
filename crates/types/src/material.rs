use crate::{architecture::Node, diagnostic::Diagnostic};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum AlphaMode {
    Opaque,
    Mask,
    Blend,
}

/// An authored lower-detail surface on the same geometry and UV layout.
/// Coverage is the fraction of viewport height below which it replaces the
/// preceding material. Variants are ordered from near to far.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MaterialLod {
    pub screen_coverage: f64,
    pub material: Box<Material>,
}

/// Renderer-neutral PBR material. Colors are sRGB; scalar data textures are linear.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Material {
    pub id: String,
    pub color: String,
    pub repeat: f64,
    pub roughness: f64,
    pub metalness: f64,
    pub emissive: Option<String>,
    pub emissive_strength: Option<f64>,
    pub texture: Option<String>,
    pub normal_texture: Option<String>,
    pub normal_scale: Option<f64>,
    pub metallic_roughness_texture: Option<String>,
    pub height_texture: Option<String>,
    pub height_scale: Option<f64>,
    pub height_offset: Option<f64>,
    pub opacity: Option<f64>,
    pub alpha_cutoff: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub emissive_texture: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub occlusion_texture: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub occlusion_strength: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alpha_mode: Option<AlphaMode>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub double_sided: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_color_factor: Option<[f64; 3]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub emissive_factor: Option<[f64; 3]>,
    /// View- and lighting-dependent operations the renderer applies on top of the baked maps,
    /// in order. Exported as `extras.materialSpec`; renderers without support ignore them.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub live: Vec<LiveOperation>,
    /// How the material is drawn; absent is `lit`. Exported in `extras.materialSpec.shading`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shading: Option<Shading>,
    /// The material's per-pixel program, delivered only for a material that declares live
    /// inputs (`shading: painted`). The baked maps above stay the fallback a renderer without
    /// it shows. Exported in `extras.materialSpec.program`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub program: Option<LiveProgram>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub lods: Vec<MaterialLod>,
}

/// How a material is drawn: `lit` by the scene's lights, or `painted`, whose program's color
/// reaches the screen without the light loop.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Shading {
    #[default]
    Lit,
    Painted,
}

/// A material's generated fragment program, as a renderer binds and runs it per pixel: GLSL ES
/// 3.00 that declares its uniforms and `void ms_evaluate()`. Before calling it the host sets the
/// sample globals (`ms_world`, `ms_object`, `ms_normal`, `ms_uv`, `ms_surface`, `ms_footprint`,
/// `ms_uv_footprint`), declares and sets the render inputs `inputs` lists, and sets
/// `ms_cast_shadow` where the source declares it; afterwards `painted` holds the color.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveProgram {
    /// Version of this contract.
    pub version: u32,
    pub source: String,
    /// The GLSL expression holding the drawn color (linear RGB) after `ms_evaluate()`.
    pub painted: String,
    /// How `ms_surface` is measured: `uv`, `object`, `world` or `anchored`.
    pub mapping: String,
    /// Physical size of the UV square for `uv` mapping, meters.
    pub domain: [f64; 2],
    /// The values of the uniforms the source declares for parameters and named seeds.
    pub uniforms: Vec<ProgramUniform>,
    /// What the host must supply, by name: render inputs (`sun`, `instance`, `vertex`, `time`,
    /// `field:<name>`) and the sample inputs the program reads (`world`, `normal`, ...).
    pub requires: Vec<String>,
    /// The render-only symbols the host declares or binds (see `ProgramInput`).
    pub inputs: Vec<ProgramInput>,
}

/// One uniform value of a delivered program: `float`, `vec2`, `vec3`, `bool` (0 or 1) or `uint`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProgramUniform {
    pub uniform: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub value: Vec<f64>,
}

/// A render-only input of a delivered program: a uniform the host binds, a varying it declares
/// and sets, or a scene field's texture and rectangle.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProgramInput {
    pub symbol: String,
    /// `uniform`, `instance`, `vertex`, `derived` or `field`.
    pub kind: String,
    pub glsl: String,
    /// For a field, the scene field it is.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// For a named attribute, the GLSL value to read where the drawn mesh has none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fallback: Option<String>,
}

impl LiveProgram {
    pub const VERSION: u32 = 1;
    /// Whether the program states what a host needs: its version, a source, finite uniforms.
    pub fn valid(&self) -> bool {
        self.version == Self::VERSION
            && self.source.contains("void ms_evaluate()")
            && !self.painted.is_empty()
            && self.domain.iter().all(|v| v.is_finite() && *v > 0.)
            && self.uniforms.iter().all(|uniform| {
                !uniform.uniform.is_empty() && uniform.value.iter().all(|v| v.is_finite())
            })
    }
}

/// One live renderer operation and its arguments.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveOperation {
    /// Catalog identity, such as `modulateDiffuse`.
    pub id: String,
    /// The rendering stage the operation acts in, such as `lighting-composition`.
    pub stage: String,
    /// Renderer-supplied inputs the operation needs.
    pub requires: Vec<String>,
    pub arguments: std::collections::BTreeMap<String, LiveArgument>,
}

/// A live argument: one value for the whole material, or a channel of a linear texture on the
/// material's UV0, decoded as `byte / 255 * scale + offset`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged, rename_all = "camelCase")]
pub enum LiveArgument {
    Constant {
        value: f64,
    },
    Texture {
        texture: String,
        /// 0 to 3 for R, G, B, A.
        channel: u8,
        scale: f64,
        offset: f64,
    },
}

impl LiveArgument {
    /// Whether the argument is finite and names a channel that exists.
    pub fn valid(&self) -> bool {
        match self {
            Self::Constant { value } => value.is_finite(),
            Self::Texture {
                texture,
                channel,
                scale,
                offset,
            } => !texture.is_empty() && *channel < 4 && scale.is_finite() && offset.is_finite(),
        }
    }
}

impl Default for Material {
    fn default() -> Self {
        Self {
            id: "default".into(),
            color: "#b9c7d1".into(),
            repeat: 1.,
            roughness: 0.8,
            metalness: 0.,
            emissive: None,
            emissive_strength: None,
            texture: None,
            normal_texture: None,
            normal_scale: None,
            metallic_roughness_texture: None,
            height_texture: None,
            height_scale: None,
            height_offset: None,
            opacity: None,
            alpha_cutoff: None,
            emissive_texture: None,
            occlusion_texture: None,
            occlusion_strength: None,
            alpha_mode: None,
            double_sided: None,
            base_color_factor: None,
            emissive_factor: None,
            live: Vec::new(),
            shading: None,
            program: None,
            lods: Vec::new(),
        }
    }
}

fn color(value: &str) -> bool {
    value.len() == 7 && value.starts_with('#') && value[1..].bytes().all(|c| c.is_ascii_hexdigit())
}

impl Material {
    /// Validates channel combinations before any texture or renderer allocation.
    pub fn read(node: &Node) -> Result<Self, Diagnostic> {
        let material = Self {
            id: node.id.clone(),
            color: node.string("color", "#b9c7d1"),
            repeat: node.number("repeat", 1.),
            roughness: node.number("roughness", 0.8),
            metalness: node.number("metalness", 0.),
            emissive: node.text("emissive").map(str::to_owned),
            emissive_strength: node.optional_number("emissive-strength"),
            texture: node.text("texture").map(str::to_owned),
            normal_texture: node.text("normal-texture").map(str::to_owned),
            normal_scale: node.optional_number("normal-scale"),
            metallic_roughness_texture: node.text("metallic-roughness-texture").map(str::to_owned),
            height_texture: node.text("height-texture").map(str::to_owned),
            height_scale: node.optional_number("height-scale"),
            height_offset: node.optional_number("height-offset"),
            opacity: node.optional_number("opacity"),
            alpha_cutoff: node.optional_number("alpha-cutoff"),
            ..Self::default()
        };
        material
            .validate()
            .map_err(|message| node.fail("AML_MATERIAL", message))?;
        Ok(material)
    }

    /// Validates renderer-neutral data supplied by either AML or procedural material producers.
    pub fn validate(&self) -> Result<(), String> {
        let material = self;
        let mut last = 1.0;
        let mut ids = std::collections::BTreeSet::from([self.id.as_str()]);
        let allowed = crate::limits::MAX_MATERIAL_LOD_ALTERNATIVES;
        if self.lods.len() > allowed {
            return Err(format!(
                "Material {} exceeds {allowed} LOD alternatives.",
                self.id
            ));
        }
        for lod in &self.lods {
            if !(lod.screen_coverage > 0. && lod.screen_coverage < last)
                || lod.material.id.is_empty()
                || !ids.insert(&lod.material.id)
                || !lod.material.lods.is_empty()
            {
                return Err(format!(
                    "Invalid material LOD for {}: expected distinct, flat alternatives with descending screen coverage.",
                    self.id
                ));
            }
            last = lod.screen_coverage;
            lod.material.validate()?;
        }
        let error = |message: &str| message.to_owned();
        if [
            Some(material.repeat),
            Some(material.roughness),
            Some(material.metalness),
            material.emissive_strength,
            material.normal_scale,
            material.height_scale,
            material.height_offset,
            material.opacity,
            material.alpha_cutoff,
            material.occlusion_strength,
        ]
        .into_iter()
        .flatten()
        .any(|value| !value.is_finite())
        {
            return Err(error("Material scalars must be finite."));
        }
        if !color(&material.color) || material.emissive.as_deref().is_some_and(|v| !color(v)) {
            return Err(error("Colors must be six-digit sRGB hex values."));
        }
        if material
            .base_color_factor
            .into_iter()
            .flatten()
            .any(|v| !v.is_finite() || !(0.0..=1.0).contains(&v))
            || material
                .emissive_factor
                .into_iter()
                .flatten()
                .any(|v| !v.is_finite() || v < 0.)
        {
            return Err(error(
                "Linear material color factors must be finite and in their channel ranges.",
            ));
        }
        if material.repeat <= 0.
            || !(0.0..=1.0).contains(&material.roughness)
            || !(0.0..=1.0).contains(&material.metalness)
        {
            return Err(error(
                "Repeat must be positive and PBR factors must lie in [0,1].",
            ));
        }
        for (value, companion) in [
            (material.emissive_strength, &material.emissive),
            (material.normal_scale, &material.normal_texture),
            (material.height_scale, &material.height_texture),
        ] {
            if value.is_some_and(|v| v < 0.) || (value.is_some() && companion.is_none()) {
                return Err(error(
                    "Channel scale must be nonnegative and requires its corresponding channel.",
                ));
            }
        }
        if material.height_offset.is_some() && material.height_texture.is_none() {
            return Err(error("height-offset requires height-texture."));
        }
        if material
            .occlusion_strength
            .is_some_and(|v| !(0.0..=1.0).contains(&v))
            || (material.occlusion_strength.is_some() && material.occlusion_texture.is_none())
            || (material.emissive_texture.is_some() && material.emissive.is_none())
        {
            return Err(error(
                "Image channel factors require valid values and their corresponding channels.",
            ));
        }
        if material.opacity.is_some_and(|v| !(0.0..=1.0).contains(&v))
            || material.alpha_cutoff.is_some_and(|v| {
                if material.alpha_mode.is_some() {
                    v < 0.
                } else {
                    v <= 0. || v >= 1.
                }
            })
            || (material.alpha_cutoff.is_some()
                && material.alpha_mode.is_none()
                && (material.texture.is_none() || material.opacity.is_some_and(|v| v < 1.)))
        {
            return Err(error(
                "Invalid opacity or alpha-cutoff channel combination.",
            ));
        }
        if material
            .companions()
            .iter()
            .any(|path| path.trim().is_empty())
        {
            return Err(error("Texture channels must name a companion asset."));
        }
        if material
            .program
            .as_ref()
            .is_some_and(|program| !program.valid())
        {
            return Err(error(
                "A delivered program needs its version, a source with ms_evaluate, a painted color and finite uniforms.",
            ));
        }
        Ok(())
    }

    pub fn companions(&self) -> Vec<&str> {
        [
            &self.texture,
            &self.normal_texture,
            &self.metallic_roughness_texture,
            &self.height_texture,
            &self.emissive_texture,
            &self.occlusion_texture,
        ]
        .into_iter()
        .filter_map(|v| v.as_deref())
        // Live operations sample their own argument textures.
        .chain(self.live.iter().flat_map(|operation| {
            operation
                .arguments
                .values()
                .filter_map(|argument| match argument {
                    LiveArgument::Texture { texture, .. } => Some(texture.as_str()),
                    LiveArgument::Constant { .. } => None,
                })
        }))
        .chain(self.lods.iter().flat_map(|lod| lod.material.companions()))
        .collect()
    }
}

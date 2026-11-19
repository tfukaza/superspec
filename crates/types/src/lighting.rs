//! The portable lighting record (version 1): how a scene is lit and rendered, apart from its
//! models and materials. Global settings (exposure, tone mapping, environment, ambient light,
//! ambient occlusion) and per-light settings (direction, color, shadow style). Never part of a
//! material. See `docs/plans/lighting-settings.md`.
//!
//! Meters, right-handed Z-up, radians, linear RGB in [0, 1]. Every field but `version` has a
//! default, so `{"version": 1}` is the shipped look. Unknown fields are refused when the record is
//! read; `Lighting::validate` reports out-of-range values by field path. The Studio viewer mirrors
//! this record in `browser/renderer/lighting.ts`.

use crate::color::srgb;
use serde::{Deserialize, Serialize};

/// The only version of the record so far. A change in the meaning of a field bumps it.
pub const VERSION: u32 = 1;
/// Version 1 styles one light, the sun.
pub const MAX_LIGHTS: usize = 1;

pub type Rgb = [f64; 3];

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Lighting {
    /// Required, so a record always states what its fields mean.
    pub version: u32,
    /// Linear multiplier before tone mapping.
    #[serde(default = "one")]
    pub exposure: f64,
    #[serde(default)]
    pub tone_mapping: ToneMapping,
    #[serde(default)]
    pub environment: Environment,
    #[serde(default)]
    pub ambient: Ambient,
    #[serde(default)]
    pub ambient_occlusion: AmbientOcclusion,
    /// Absent means the default sun; an empty list means no direct light.
    #[serde(default = "default_lights")]
    pub lights: Vec<Light>,
    /// How the image's finishing steps treat materials with `shading: painted`, for the whole
    /// scene (never per material). Written only when it differs from the default, so records and
    /// exports that never set it keep their bytes.
    #[serde(default, skip_serializing_if = "Painted::is_default")]
    pub painted: Painted,
    /// Contour lines over the finished image, for the whole scene (never per material;
    /// experimental). Off by default, and written only when it differs from the default.
    #[serde(default, skip_serializing_if = "Contours::is_default")]
    pub contours: Contours,
}

/// Drawn contour lines: a line in the darkened color of each surface along silhouettes (where
/// depth steps) and creases (where the surface turns). Features too small to draw get none.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub struct Contours {
    pub enabled: bool,
    /// How much of the line's color replaces the image on a line, 0 to 1.
    pub strength: f64,
    /// Line weight on a silhouette, CSS pixels; a crease is drawn at 0.6 of it.
    pub width: f64,
    /// The line is the color of the surface it outlines, this bright to the eye (0 black, 1 the
    /// surface's own color; applied through the display's gamma).
    pub darken: f64,
    /// An optional tint the line is multiplied by as well, linear RGB; absent is none.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<Rgb>,
    /// The step in depth between neighboring surfaces that draws a silhouette, meters.
    pub depth: f64,
    /// The angle between neighboring surfaces that draws a full crease, radians.
    pub normal: f64,
    /// Meters one CSS pixel covers where lines have faded out.
    pub fade: f64,
}

impl Default for Contours {
    fn default() -> Self {
        Self {
            enabled: false,
            strength: 1.0,
            width: 1.25,
            darken: 0.6,
            color: None,
            depth: 0.04,
            normal: 0.7,
            fade: 0.08,
        }
    }
}

impl Contours {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

/// What the finishing steps of the image do to painted materials. Default: tone mapping and
/// exposure are skipped, so authored colors reach the screen unchanged, and ambient occlusion
/// applies, because it acts on the finished image (a renderer that cannot exempt painted pixels
/// from it says so when asked to).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub struct Painted {
    /// Whether exposure and the tone curve apply to painted materials.
    pub tone_mapping: bool,
    /// Whether ambient occlusion darkens painted materials.
    pub ambient_occlusion: bool,
}

impl Default for Painted {
    fn default() -> Self {
        Self {
            tone_mapping: false,
            ambient_occlusion: true,
        }
    }
}

impl Painted {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ToneMapping {
    #[default]
    Aces,
    Neutral,
    None,
}

/// Image-based light.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub struct Environment {
    /// `studio` is the built-in neutral room; `none` disables the environment. Other names are
    /// catalog identities a consumer may not have; it then reports its fallback.
    pub source: String,
    pub intensity: f64,
    /// Radians about +Z.
    pub rotation: f64,
}

/// Hemisphere light: `sky` from +Z, `ground` from -Z.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub struct Ambient {
    pub sky: Rgb,
    pub ground: Rgb,
    pub intensity: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub struct AmbientOcclusion {
    pub enabled: bool,
    /// How far around a point occluders are looked for, meters.
    pub radius: f64,
    /// Up to 1, the share of the occlusion that darkens the image; above 1, creases deepen.
    pub intensity: f64,
    /// How thick surfaces are taken to be behind what the camera sees, meters.
    pub thickness: f64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LightRole {
    #[default]
    Sun,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LightKind {
    #[default]
    Directional,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub struct Light {
    pub id: String,
    pub role: LightRole,
    pub kind: LightKind,
    /// Unit vector from the scene toward the light.
    pub direction: [f64; 3],
    pub color: Rgb,
    pub intensity: f64,
    pub casts_shadow: bool,
    pub shadow: Shadow,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ShadowStyle {
    /// The renderer's own shadow; the style values below are kept but unused.
    Plain,
    #[default]
    Illustration,
}

/// The look of one light's shadows.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub struct Shadow {
    pub style: ShadowStyle,
    /// Penumbra radius where the occluder touches the receiver, meters.
    pub softness: f64,
    /// Penumbra radius gained per meter between occluder and receiver.
    pub spread: f64,
    /// Largest penumbra radius, meters.
    pub max_softness: f64,
    /// How much of the shaded color the shadow removes beyond the tint, 0 to 1.
    pub darkness: f64,
    pub tint: Rgb,
    /// Second hue the interior noise drifts to.
    pub tint_alt: Rgb,
    /// 0 is a neutral gray shadow, 1 the tint's own saturation.
    pub tint_strength: f64,
    /// Cosine of the angle to the light below which a face starts to count as shadowed.
    pub wrap: f64,
    pub noise: ShadowNoise,
    /// Form light in shadow, 0 to 1 (experimental): the share of the light's diffuse shading a
    /// surface keeps where something else shades it, from the light's compass direction level
    /// with the ground. Written only when it is not 0.
    #[serde(skip_serializing_if = "is_zero")]
    pub form: f64,
    /// A weaker form light from the opposite compass direction, 0 to 1.
    #[serde(skip_serializing_if = "is_zero")]
    pub form_fill: f64,
}

/// Noise in the light's plane; a pure function of position there and `seed`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub struct ShadowNoise {
    /// Feature size, meters.
    pub scale: f64,
    /// Displacement of the shadow's edge, meters.
    pub edge: f64,
    /// Variation of darkness and hue inside the shadow, 0 to 1.
    pub amount: f64,
    pub seed: u32,
}

fn one() -> f64 {
    1.0
}

fn is_zero(value: &f64) -> bool {
    *value == 0.0
}

fn default_lights() -> Vec<Light> {
    vec![Light::default()]
}

impl Default for Lighting {
    /// The shipped look: the Studio viewer's lights, the illustration shadow and ambient occlusion.
    fn default() -> Self {
        Self {
            version: VERSION,
            exposure: 1.0,
            tone_mapping: ToneMapping::default(),
            environment: Environment::default(),
            ambient: Ambient::default(),
            ambient_occlusion: AmbientOcclusion::default(),
            lights: default_lights(),
            painted: Painted::default(),
            contours: Contours::default(),
        }
    }
}

impl Default for Environment {
    fn default() -> Self {
        Self {
            source: "studio".into(),
            intensity: 0.35,
            rotation: 0.0,
        }
    }
}

impl Default for Ambient {
    fn default() -> Self {
        Self {
            sky: [1.0; 3],
            ground: srgb("#777768"),
            intensity: 0.65,
        }
    }
}

impl Default for AmbientOcclusion {
    fn default() -> Self {
        Self {
            enabled: true,
            radius: 0.35,
            intensity: 2.0,
            thickness: 0.5,
        }
    }
}

impl Default for Light {
    fn default() -> Self {
        // (-3, -5, 8) normalized. A square root is exact in IEEE arithmetic, so native and Wasm agree.
        let length = 98f64.sqrt();
        Self {
            id: "sun".into(),
            role: LightRole::Sun,
            kind: LightKind::Directional,
            direction: [-3.0 / length, -5.0 / length, 8.0 / length],
            color: srgb("#ffefdb"),
            intensity: 3.0,
            casts_shadow: true,
            shadow: Shadow::default(),
        }
    }
}

impl Default for Shadow {
    fn default() -> Self {
        Self {
            style: ShadowStyle::Illustration,
            softness: 0.012,
            spread: 0.025,
            max_softness: 0.06,
            darkness: 0.25,
            tint: srgb("#2f6fe0"),
            tint_alt: srgb("#3f4fd8"),
            tint_strength: 0.22,
            wrap: 0.3,
            noise: ShadowNoise::default(),
            form: 0.0,
            form_fill: 0.0,
        }
    }
}

impl Default for ShadowNoise {
    fn default() -> Self {
        Self {
            scale: 1.1,
            edge: 0.07,
            amount: 0.6,
            seed: 7,
        }
    }
}

/// One refused value: the field's path in the record (`lights[0].shadow.softness`) and why.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Invalid {
    pub field: String,
    pub message: String,
}

impl std::fmt::Display for Invalid {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // A record refused while it is read has no field path; its message carries line and column.
        let separator = if self.field.is_empty() { "" } else { "." };
        write!(
            formatter,
            "lighting{separator}{}: {}",
            self.field, self.message
        )
    }
}

struct Check(Vec<Invalid>);

impl Check {
    fn fail(&mut self, field: &str, message: String) {
        self.0.push(Invalid {
            field: field.into(),
            message,
        });
    }

    fn range(&mut self, field: &str, value: f64, min: f64, max: f64) {
        if !(value.is_finite() && value >= min && value <= max) {
            self.fail(field, format!("must be between {min} and {max}"));
        }
    }

    fn color(&mut self, field: &str, value: &Rgb) {
        if !value
            .iter()
            .all(|channel| channel.is_finite() && (0.0..=1.0).contains(channel))
        {
            self.fail(field, "must be linear RGB between 0 and 1".into());
        }
    }
}

/// Whether `id` is a plain identity: 1 to 64 of `A-Z a-z 0-9 . _ -`.
fn identity(id: &str) -> bool {
    (1..=64).contains(&id.len())
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

impl Lighting {
    /// Every out-of-range value, in record order; empty when the record is valid. The limits bound
    /// what an author may ask for; they are not what a renderer must reach.
    pub fn validate(&self) -> Vec<Invalid> {
        let mut check = Check(vec![]);
        if self.version != VERSION {
            check.fail("version", format!("must be {VERSION}"));
        }
        check.range("exposure", self.exposure, 0.0, 100.0);
        if !identity(&self.environment.source) {
            check.fail(
                "environment.source",
                "must be an identity of 1 to 64 letters, digits, '.', '_' or '-'".into(),
            );
        }
        check.range(
            "environment.intensity",
            self.environment.intensity,
            0.0,
            100.0,
        );
        check.range(
            "environment.rotation",
            self.environment.rotation,
            -std::f64::consts::TAU,
            std::f64::consts::TAU,
        );
        check.color("ambient.sky", &self.ambient.sky);
        check.color("ambient.ground", &self.ambient.ground);
        check.range("ambient.intensity", self.ambient.intensity, 0.0, 100.0);
        let occlusion = &self.ambient_occlusion;
        check.range("ambientOcclusion.radius", occlusion.radius, 0.01, 10.0);
        check.range("ambientOcclusion.intensity", occlusion.intensity, 0.0, 4.0);
        check.range(
            "ambientOcclusion.thickness",
            occlusion.thickness,
            0.01,
            10.0,
        );
        if self.lights.len() > MAX_LIGHTS {
            check.fail(
                "lights",
                format!("version {VERSION} admits at most {MAX_LIGHTS} light, the sun"),
            );
        }
        for (index, light) in self.lights.iter().enumerate() {
            let at = |name: &str| format!("lights[{index}].{name}");
            if !identity(&light.id) {
                check.fail(
                    &at("id"),
                    "must be an identity of 1 to 64 letters, digits, '.', '_' or '-'".into(),
                );
            }
            if self.lights[..index]
                .iter()
                .any(|other| other.id == light.id)
            {
                check.fail(&at("id"), "must be unique".into());
            }
            let length = light
                .direction
                .iter()
                .map(|axis| axis * axis)
                .sum::<f64>()
                .sqrt();
            if !(length.is_finite() && (length - 1.0).abs() <= 1e-3) {
                check.fail(&at("direction"), "must be a unit vector".into());
            }
            check.color(&at("color"), &light.color);
            check.range(&at("intensity"), light.intensity, 0.0, 100.0);
            let shadow = &light.shadow;
            check.range(&at("shadow.softness"), shadow.softness, 0.0, 2.0);
            check.range(&at("shadow.spread"), shadow.spread, 0.0, 2.0);
            check.range(&at("shadow.maxSoftness"), shadow.max_softness, 0.0, 2.0);
            check.range(&at("shadow.darkness"), shadow.darkness, 0.0, 1.0);
            check.color(&at("shadow.tint"), &shadow.tint);
            check.color(&at("shadow.tintAlt"), &shadow.tint_alt);
            check.range(&at("shadow.tintStrength"), shadow.tint_strength, 0.0, 1.0);
            check.range(&at("shadow.wrap"), shadow.wrap, 0.0, 1.0);
            check.range(&at("shadow.noise.scale"), shadow.noise.scale, 0.01, 100.0);
            check.range(&at("shadow.noise.edge"), shadow.noise.edge, 0.0, 1.0);
            check.range(&at("shadow.noise.amount"), shadow.noise.amount, 0.0, 1.0);
            check.range(&at("shadow.form"), shadow.form, 0.0, 1.0);
            check.range(&at("shadow.formFill"), shadow.form_fill, 0.0, 1.0);
        }
        let contours = &self.contours;
        check.range("contours.strength", contours.strength, 0.0, 1.0);
        check.range("contours.width", contours.width, 0.5, 8.0);
        check.range("contours.darken", contours.darken, 0.0, 1.0);
        if let Some(color) = &contours.color {
            check.color("contours.color", color);
        }
        check.range("contours.depth", contours.depth, 0.001, 10.0);
        check.range(
            "contours.normal",
            contours.normal,
            0.01,
            std::f64::consts::PI,
        );
        check.range("contours.fade", contours.fade, 0.001, 10.0);
        check.0
    }

    /// The light materials and surveys mean by "the sun", if the scene has one.
    pub fn sun(&self) -> Option<&Light> {
        self.lights
            .iter()
            .find(|light| light.role == LightRole::Sun)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_the_shipped_look_and_valid() {
        let lighting = Lighting::default();
        assert_eq!(lighting.validate(), vec![]);
        assert_eq!(lighting.version, 1);
        assert_eq!(lighting.exposure, 1.0);
        assert_eq!(lighting.tone_mapping, ToneMapping::Aces);
        assert_eq!(
            (
                lighting.environment.source.as_str(),
                lighting.environment.intensity
            ),
            ("studio", 0.35)
        );
        assert_eq!(lighting.ambient.intensity, 0.65);
        let occlusion = &lighting.ambient_occlusion;
        assert!(occlusion.enabled);
        assert_eq!(
            (occlusion.radius, occlusion.intensity, occlusion.thickness),
            (0.35, 2.0, 0.5)
        );
        let sun = lighting.sun().expect("default sun");
        assert_eq!(lighting.lights.len(), 1);
        assert!(sun.casts_shadow);
        assert_eq!(sun.intensity, 3.0);
        // The viewer's sun stood at (-3, -5, 8) from the scene.
        for (axis, expected) in sun.direction.iter().zip([-3.0, -5.0, 8.0]) {
            assert!((axis * 98f64.sqrt() - expected).abs() < 1e-12);
        }
        assert_eq!(sun.shadow.style, ShadowStyle::Illustration);
        assert_eq!(
            (
                sun.shadow.softness,
                sun.shadow.spread,
                sun.shadow.max_softness
            ),
            (0.012, 0.025, 0.06)
        );
        assert_eq!(
            (
                sun.shadow.darkness,
                sun.shadow.tint_strength,
                sun.shadow.wrap
            ),
            (0.25, 0.22, 0.3)
        );
        assert_eq!(sun.shadow.noise, ShadowNoise::default());
        assert_eq!(sun.shadow.noise.seed, 7);
    }

    #[test]
    fn limits_are_inclusive() {
        let mut lighting = Lighting::default();
        lighting.exposure = 100.0;
        lighting.ambient_occlusion.intensity = 0.0;
        lighting.lights[0].shadow.softness = 2.0;
        lighting.lights[0].shadow.noise.scale = 0.01;
        assert_eq!(lighting.validate(), vec![]);
        lighting.lights.clear();
        assert_eq!(lighting.validate(), vec![]);
        assert!(lighting.sun().is_none());
    }

    #[test]
    fn every_refused_value_names_its_field() {
        let mut lighting = Lighting::default();
        lighting.version = 2;
        lighting.exposure = f64::NAN;
        lighting.environment.source = "two words".into();
        lighting.ambient.sky = [1.5, 0.0, 0.0];
        lighting.ambient_occlusion.radius = 0.0;
        lighting.lights[0].direction = [0.0, 0.0, 2.0];
        lighting.lights[0].shadow.noise.amount = -0.1;
        let fields: Vec<String> = lighting
            .validate()
            .into_iter()
            .map(|invalid| invalid.field)
            .collect();
        assert_eq!(
            fields,
            [
                "version",
                "exposure",
                "environment.source",
                "ambient.sky",
                "ambientOcclusion.radius",
                "lights[0].direction",
                "lights[0].shadow.noise.amount"
            ]
        );
    }

    #[test]
    fn painted_materials_skip_tone_mapping_and_keep_occlusion_by_default() {
        let lighting = Lighting::default();
        assert!(!lighting.painted.tone_mapping && lighting.painted.ambient_occlusion);
        assert!(lighting.painted.is_default());
    }

    #[test]
    fn form_light_and_contours_are_off_by_default_and_checked() {
        let mut lighting = Lighting::default();
        assert_eq!(lighting.lights[0].shadow.form, 0.0);
        assert_eq!(lighting.lights[0].shadow.form_fill, 0.0);
        assert!(!lighting.contours.enabled && lighting.contours.is_default());
        lighting.lights[0].shadow.form = 1.5;
        lighting.contours.width = 0.0;
        lighting.contours.normal = 4.0;
        let fields: Vec<String> = lighting
            .validate()
            .into_iter()
            .map(|invalid| invalid.field)
            .collect();
        assert_eq!(
            fields,
            ["lights[0].shadow.form", "contours.width", "contours.normal"]
        );
    }

    #[test]
    fn a_second_light_is_refused_in_version_one() {
        let mut lighting = Lighting::default();
        lighting.lights.push(Light::default());
        let findings = lighting.validate();
        assert_eq!(findings[0].field, "lights");
        assert!(
            findings
                .iter()
                .any(|invalid| invalid.field == "lights[1].id")
        );
        assert_eq!(
            findings[0].to_string(),
            "lighting.lights: version 1 admits at most 1 light, the sun"
        );
    }
}

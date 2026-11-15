//! Filtered ceramic pigment, chipped joints and localized loss; all relief is normal-only.
use super::{
    noise::{fractal_noise, lattice_hash, smooth_step, value_noise},
    surface::{Coat, Layer, Sample},
};
use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::f64::consts::PI;
use superspec_geometry::{
    V3,
    math::{cos, hypot, sin},
};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Settings {
    pub seed: u32,
    pub share: f64,
    pub wear: f64,
    pub grime: f64,
    pub size: Option<f64>,
    pub aspect: Option<f64>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Materials {
    pub ceramic: String,
    pub bed: String,
    pub grime: String,
    pub repeat: f64,
    pub bed_repeat: f64,
    pub bed_roughness: f64,
    pub roughness: f64,
}
#[derive(Debug, Deserialize, Serialize)]
pub struct Appearance {
    pub tint: V3,
    pub deposit: f64,
    pub fade: f64,
}
#[derive(Debug, Deserialize, Serialize)]
pub struct Bed {
    pub bump: f64,
    pub tone: f64,
}
#[derive(Debug, Deserialize, Serialize)]
pub struct Channels {
    pub installed: f64,
    pub coverage: f64,
    pub bed: Bed,
    pub seam: f64,
    pub relief: f64,
    pub appearance: Appearance,
}
#[derive(Debug, Deserialize, Serialize)]
pub struct Surface {
    pub installed: f64,
    pub bump: f64,
    pub layers: Vec<Layer>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Recipe {
    pub bounds: [f64; 4],
    pub tiles: Settings,
    pub materials: Materials,
    pub bed_tint: V3,
    pub density: f64,
}
pub struct Field<'a> {
    recipe: &'a Recipe,
    tiles: Tile,
}
impl<'a> Field<'a> {
    pub fn new(recipe: &'a Recipe) -> Result<Self, String> {
        recipe.materials.validate()?;
        if recipe
            .bounds
            .iter()
            .any(|v| !v.is_finite() || v.abs() > 1000.)
            || recipe.bounds[0] >= recipe.bounds[2]
            || recipe.bounds[1] >= recipe.bounds[3]
            || !recipe.density.is_finite()
            || !(0.1..=4096.).contains(&recipe.density)
            || recipe
                .bed_tint
                .iter()
                .any(|v| !v.is_finite() || !(0. ..=4.).contains(v))
        {
            return Err(
                "Ceramic raster requires bounded positive chart, density and bed tint.".into(),
            );
        }
        Ok(Self {
            recipe,
            tiles: Tile::new(&recipe.tiles)?,
        })
    }
    pub fn structure(&self) -> Sample {
        let m = &self.recipe.materials;
        Sample {
            height: 0.,
            bump: Some(0.),
            roughness: None,
            height_weight: None,
            layer_elevation_weight: None,
            layers: vec![
                Layer {
                    material: m.bed.clone(),
                    weight: 1.,
                    uv: [0.; 2],
                    tint: Some([1.; 3]),
                    roughness: Some(m.bed_roughness),
                    coatings: vec![],
                },
                Layer {
                    material: m.ceramic.clone(),
                    weight: 1.,
                    uv: [0.; 2],
                    tint: Some([1.; 3]),
                    roughness: Some(m.roughness),
                    coatings: vec![Coat {
                        material: m.grime.clone(),
                        uv: [0.; 2],
                        opacity: 0.,
                        tint: None,
                        roughness: None,
                    }],
                },
            ],
        }
    }
    pub fn sample(&self, u: f64, v: f64) -> Result<Sample, String> {
        let surface = self.tiles.compose(
            u,
            v,
            1. / self.recipe.density,
            &self.recipe.materials,
            self.recipe.bed_tint,
        )?;
        Ok(Sample {
            height: 0.,
            bump: Some(surface.bump),
            roughness: None,
            height_weight: None,
            layer_elevation_weight: None,
            layers: surface.layers,
        })
    }
}
pub fn panel(
    drawing: &mut super::fixture::Fixture<'_, '_>,
    slot: &str,
    recipe: &Recipe,
    group: &str,
) -> Result<(), String> {
    let b = recipe.bounds;
    if drawing.mesh.surveying() {
        return drawing.panel(slot, [b[0], b[2], b[1], b[3]], 0., [1.; 3]);
    }
    Field::new(recipe)?;
    super::bake_jobs::face(
        drawing,
        slot,
        &[
            [b[0], 0., b[1]],
            [b[2], 0., b[1]],
            [b[2], 0., b[3]],
            [b[0], 0., b[3]],
        ],
        super::bake_jobs::Job {
            density: recipe.density,
            recipe: super::bake_jobs::Recipe::Tile(recipe.clone()),
        },
        group.into(),
    )
}
#[derive(Clone, Copy)]
struct Contour {
    u: f64,
    v: f64,
    cosine: f64,
    sine: f64,
    seed: f64,
}
#[derive(Clone, Copy)]
struct Blob {
    u: f64,
    v: f64,
    seed: f64,
    contours: [Contour; 3],
}
struct Neighborhood {
    column: f64,
    row: f64,
    blobs: Vec<Blob>,
}
struct TilePoint {
    column: f64,
    row: f64,
    shifted: f64,
    x: f64,
    y: f64,
}
pub struct Tile {
    settings: Settings,
    width: f64,
    height: f64,
    half_gap: f64,
    chip_width: f64,
    stagger: bool,
    neighborhood: RefCell<Option<Neighborhood>>,
}
/// Exact integral of periodic solid strips over one physical texel footprint.
pub(crate) fn coverage(position: f64, period: f64, half_gap: f64, footprint: f64) -> f64 {
    let solid = |at: f64| {
        let cycle = (at / period).floor();
        let local = at - cycle * period;
        cycle * (period - 2. * half_gap) + (local - half_gap).clamp(0., period - 2. * half_gap)
    };
    ((solid(position + footprint / 2.) - solid(position - footprint / 2.)) / footprint)
        .clamp(0., 1.)
}
impl Tile {
    pub fn new(settings: &Settings) -> Result<Self, String> {
        let pattern = (settings.seed % 3) as usize;
        let width = settings.size.unwrap_or([0.16, 0.24, 0.09][pattern]);
        let aspect = settings.aspect.unwrap_or([1., 2., 1.][pattern]);
        if [settings.share, settings.wear, settings.grime]
            .iter()
            .any(|v| !v.is_finite() || !(0. ..=1.).contains(v))
            || !width.is_finite()
            || !(0.01..=2.).contains(&width)
            || !aspect.is_finite()
            || !(0.1..=10.).contains(&aspect)
        {
            return Err(
                "Ceramic requires normalized aging and bounded positive tile dimensions.".into(),
            );
        }
        let height = width / aspect;
        Ok(Self {
            settings: settings.clone(),
            width,
            height,
            half_gap: 0.00225_f64.min(width * 0.02).min(height * 0.02),
            chip_width: 0.004_f64.min(width * 0.035).min(height * 0.035),
            stagger: width / height >= 1.5,
            neighborhood: RefCell::new(None),
        })
    }
    fn seed(&self) -> f64 {
        f64::from(self.settings.seed)
    }
    fn tile(&self, u: f64, v: f64) -> TilePoint {
        let row = (v / self.height).floor();
        let shifted = u - if self.stagger && row % 2. != 0. {
            self.width / 2.
        } else {
            0.
        };
        let column = (shifted / self.width).floor();
        TilePoint {
            row,
            column,
            shifted,
            x: shifted - column * self.width,
            y: v - row * self.height,
        }
    }
    fn joint(&self, t: &TilePoint, side: usize, along: f64, length: f64, footprint: f64) -> f64 {
        let resolved = 1. - smooth_step(2. * self.half_gap * 0.8, 2. * self.half_gap, footprint);
        if resolved == 0. {
            return self.half_gap;
        }
        let endpoint = usize::from(along >= length / 2.);
        let corner = if side < 2 {
            side + endpoint * 2
        } else {
            endpoint + (side - 2) * 2
        };
        let corner_seed = self.seed() + 307. + corner as f64 * 91.;
        let hash = |s| lattice_hash(t.column, t.row, s);
        let mut corner_chip = 0.;
        if hash(corner_seed) < 0.12 + self.settings.wear * 0.2 {
            let span = (length * 0.14).min(0.006 + hash(corner_seed + 17.) * 0.012);
            corner_chip = (1. - along.min(length - along) / span).max(0.)
                * (0.55 + hash(corner_seed + 29.) * 0.45);
        }
        let edge_seed = self.seed() + 809. + side as f64 * 131.;
        let mut edge_chip = 0.;
        if hash(edge_seed) < 0.02 + self.settings.wear * 0.04 {
            let center = length * (0.2 + hash(edge_seed + 17.) * 0.6);
            let span = (length * 0.045).min(0.004 + hash(edge_seed + 29.) * 0.004);
            edge_chip = (1. - (along - center).abs() / span).max(0.) * 0.65;
        }
        let chip = corner_chip.max(edge_chip);
        if chip == 0. {
            return self.half_gap;
        }
        let grain = 1.
            - value_noise(along * 211., t.column + t.row * 19., edge_seed + 43.)
                * 0.25
                * (1. - smooth_step(0.002, 0.006, footprint));
        self.half_gap + self.chip_width * resolved * chip * grain
    }
    fn widths(&self, t: &TilePoint, footprint: f64) -> [f64; 2] {
        [
            self.joint(
                t,
                usize::from(t.x >= self.width / 2.),
                t.y,
                self.height,
                footprint,
            ),
            self.joint(
                t,
                2 + usize::from(t.y >= self.height / 2.),
                t.x,
                self.width,
                footprint,
            ),
        ]
    }
    fn installation(&self, u: f64, v: f64) -> f64 {
        if self.settings.share <= 0. {
            0.
        } else if self.settings.share >= 1. {
            1.
        } else {
            smooth_step(
                -0.025,
                0.025,
                self.settings.share - value_noise(u * 0.23, v * 0.31, self.seed()),
            )
        }
    }
    fn blob(&self, column: f64, row: f64) -> Option<Blob> {
        let random = |s| lattice_hash(column, row, self.seed() + s);
        if random(107.) >= self.settings.wear * 0.95 {
            return None;
        }
        let seed = (random(109.) * 2147483647.).floor();
        let angle = random(113.) * PI * 2.;
        let growth = 0.55 + self.settings.wear * 0.45;
        let radius_u = (0.32 + random(127.) * 0.34) * growth;
        let radius_v = (0.25 + random(131.) * 0.34) * growth;
        Some(Blob {
            u: (column + 0.18 + random(137.) * 0.64) * 1.6,
            v: (row + 0.18 + random(139.) * 0.64) * 1.6,
            seed,
            contours: std::array::from_fn(|variant| {
                let shape = |s| lattice_hash(variant as f64, s, seed);
                let rotation = angle + (shape(3.) - 0.5) * 0.5;
                Contour {
                    u: radius_u * (0.8 + shape(5.) * 0.38),
                    v: radius_v * (0.8 + shape(7.) * 0.38),
                    cosine: cos(rotation),
                    sine: sin(rotation),
                    seed: seed + variant as f64 * 3571.,
                }
            }),
        })
    }
    fn damage_retention(&self, u: f64, v: f64, t: &TilePoint, footprint: f64) -> f64 {
        if self.settings.wear == 0. {
            return 1.;
        }
        let column = (u / 1.6).floor();
        let row = (v / 1.6).floor();
        let mut cached = self.neighborhood.borrow_mut();
        if cached
            .as_ref()
            .is_none_or(|n| n.column != column || n.row != row)
        {
            let mut blobs = Vec::new();
            for x in -1..=1 {
                for y in -1..=1 {
                    if let Some(b) = self.blob(column + x as f64, row + y as f64) {
                        blobs.push(b);
                    }
                }
            }
            *cached = Some(Neighborhood { column, row, blobs });
        }
        let feather = 0.002_f64.max(footprint.min(0.05) * 0.5);
        let mut damage = 0_f64;
        for blob in &cached.as_ref().unwrap().blobs {
            let variant = (lattice_hash(t.column, t.row, blob.seed + 151.) * 3.).floor() as usize;
            let contour = blob.contours[variant];
            let du = u - blob.u;
            let dv = v - blob.v;
            let radius = hypot(
                (du * contour.cosine + dv * contour.sine) / contour.u,
                (-du * contour.sine + dv * contour.cosine) / contour.v,
            );
            let minimum = contour.u.min(contour.v);
            if (radius - 1.31) * minimum > feather {
                continue;
            }
            let shared = (value_noise(u * 4.5, v * 4.5, blob.seed) - 0.5) * 0.24;
            let fractured = (value_noise(u * 9., v * 9., contour.seed + 163.) - 0.5) * 0.3
                + (value_noise(u * 31., v * 31., contour.seed + 167.) - 0.5) * 0.08;
            damage = damage.max(smooth_step(
                -feather,
                feather,
                (1. + shared + fractured - radius) * minimum,
            ));
        }
        1. - damage
    }
    fn retention(&self, u: f64, v: f64, footprint: f64) -> f64 {
        let t = self.tile(u, v);
        if lattice_hash(t.column, t.row, self.seed() + 31.) < self.settings.wear * 0.06 {
            return 0.;
        }
        self.installation(u, v) * self.damage_retention(u, v, &t, footprint)
    }
    fn concrete_noise(&self, u: f64, v: f64, mut frequency: f64, footprint: f64, seed: f64) -> f64 {
        let mut x = u * frequency;
        let mut y = v * frequency;
        let mut field = 0.5;
        let mut amplitude = 1. / (1. + 0.28 + 0.28_f64.powi(2));
        for _ in 0..3 {
            field += (value_noise(x, y, seed) - 0.5)
                * amplitude
                * (1. - smooth_step(0.35, 0.8, frequency * footprint));
            let next = (x * 0.8 + y * 0.6) * 2.03 + 13.7;
            y = (-x * 0.6 + y * 0.8) * 2.03 + 9.2;
            x = next;
            frequency *= 2.03;
            amplitude *= 0.28;
        }
        field
    }
    fn bed(&self, u: f64, v: f64, footprint: f64) -> Bed {
        let seed = self.seed();
        let sweep = value_noise(u * 1.7, v * 0.8, seed + 101.);
        let waviness = value_noise(u * 4., v * 1.5, seed + 103.);
        let grit = value_noise(u * 48., v * 51., seed + 109.);
        let fine = value_noise(u * 103., v * 97., seed + 113.);
        let grain = 1. - smooth_step(0.003, 0.007, footprint);
        let spacing = 0.018;
        let phase = (v + 0.065 * sweep + 0.012 * waviness + (grit - 0.5) * 0.004 * grain) / spacing;
        let resolved = 1. - smooth_step(spacing / 4., spacing / 2., footprint);
        let ridge = (1. + cos(phase * PI * 2.)) / 2.;
        let comb = ridge * ridge * (1. + (grit - 0.5) * 1.2 * grain);
        let pits = smooth_step(0.67, 0.85, fine);
        let mottling = self.concrete_noise(u * 0.85, v * 0.68, 4.5, footprint, seed + 127.);
        let mineral = self.concrete_noise(u, v, 28., footprint, seed + 131.) - 0.5;
        Bed {
            bump: 0.0016 * resolved * comb
                + mineral * 0.0012
                + grain * ((grit - 0.5) * 0.0013 + (fine - 0.5) * 0.0015 - pits * 0.0008),
            tone: 0.90
                + smooth_step(0.25, 0.75, mottling) * 0.20
                + mineral * 0.14
                + ((fine - 0.5) * 0.07 - pits * 0.04) * grain,
        }
    }
    fn seam(&self, u: f64, v: f64, t: &TilePoint, widths: [f64; 2], footprint: f64) -> f64 {
        let size = self.width.min(self.height);
        let resolved = 1. - smooth_step(size / 4., size / 2., footprint);
        if resolved == 0. || self.settings.share <= 0. {
            return 0.;
        }
        let max_width = self.half_gap + self.chip_width;
        let reach = max_width + 0.006_f64.min(size * 0.06);
        let margin = max_width + footprint / 2.;
        let mut vertical = 0_f64;
        let mut horizontal = 0_f64;
        if t.x.min(self.width - t.x) <= margin {
            let joint = if t.x < self.width / 2. {
                u - t.x
            } else {
                u + self.width - t.x
            };
            vertical = (1. - coverage(t.shifted, self.width, widths[0], footprint))
                * self
                    .retention(joint - reach, v, footprint)
                    .min(self.retention(joint + reach, v, footprint));
        }
        if t.y.min(self.height - t.y) <= margin {
            let joint = if t.y < self.height / 2. {
                v - t.y
            } else {
                v + self.height - t.y
            };
            horizontal = (1. - coverage(v, self.height, widths[1], footprint))
                * self
                    .retention(u, joint - reach, footprint)
                    .min(self.retention(u, joint + reach, footprint));
        }
        vertical.max(horizontal) * resolved
    }
    fn appearance(&self, u: f64, v: f64, t: &TilePoint, footprint: f64) -> Appearance {
        let seed = self.seed();
        let wear = self.settings.wear;
        let grime = self.settings.grime;
        let fade = if wear == 0. {
            0.
        } else {
            let tile_seed = seed + (lattice_hash(t.column, t.row, seed + 491.) * 65536.).floor();
            smooth_step(
                0.2,
                0.8,
                self.concrete_noise(u, v, 9., footprint, tile_seed),
            ) * wear
        };
        let base = (0.83 + lattice_hash(t.column, t.row, seed + 53.) * 0.17) * (1. + fade * 0.28);
        if grime == 0. {
            return Appearance {
                tint: [base; 3],
                deposit: 0.,
                fade,
            };
        }
        let patch = fractal_noise(u * 2.1, v * 2.5, seed + 211., 3);
        let streak = fractal_noise(u * 8., v * 0.7, seed + 223., 2);
        let grain = value_noise(u * 38., v * 38., seed + 227.);
        let edge = t.x.min(self.width - t.x).min(t.y).min(self.height - t.y);
        let edge_width = 0.025_f64.min(self.width * 0.18).min(self.height * 0.18);
        let joint = 1. - smooth_step(0., edge_width, edge);
        let patches = smooth_step(
            0.62 - grime * 0.16,
            0.79 - grime * 0.16,
            patch * 0.82 + streak * 0.18 + (grain - 0.5) * 0.14,
        );
        let deposit = 0.9_f64.min(grime * (0.07 + patches * 0.78 + joint * streak * 0.3)) * 0.25;
        let variation = lattice_hash(t.column, t.row, seed + 233.) - 0.5;
        let tone = 0.55_f64.max(base + grime * (variation * 0.07 + (patch - 0.5) * 0.03));
        let warm = grime * lattice_hash(t.column, t.row, seed + 239.);
        Appearance {
            tint: [tone, tone * (1. - warm * 0.015), tone * (1. - warm * 0.035)],
            deposit,
            fade,
        }
    }
    pub fn channels(&self, u: f64, v: f64, footprint: f64) -> Result<Channels, String> {
        if [u, v].iter().any(|v| !v.is_finite() || v.abs() > 10000.)
            || !footprint.is_finite()
            || !(1e-9..=10.).contains(&footprint)
        {
            return Err(
                "Ceramic sampling requires bounded coordinates and a positive physical footprint."
                    .into(),
            );
        }
        let t = self.tile(u, v);
        let widths = self.widths(&t, footprint);
        let installed = self.installation(u, v);
        let kept = if self.settings.share <= 0. {
            0.
        } else {
            coverage(t.shifted, self.width, widths[0], footprint)
                * coverage(v, self.height, widths[1], footprint)
                * self.retention(u, v, footprint)
        };
        let size = self.width.min(self.height);
        let thickness = 0.009_f64.min(size * 0.08);
        let bevel_width = 0.002_f64.min(size * 0.025);
        let resolved = 1. - smooth_step(bevel_width / 2., bevel_width * 2., footprint);
        let edge = (t.x - widths[0])
            .min(self.width - t.x - widths[0])
            .min(t.y - widths[1])
            .min(self.height - t.y - widths[1]);
        let relief = thickness
            - 0.0008_f64.min(thickness * 0.15)
                * (1. - smooth_step(0., bevel_width, edge))
                * resolved;
        Ok(Channels {
            installed,
            coverage: kept,
            bed: self.bed(u, v, footprint),
            seam: self.seam(u, v, &t, widths, footprint),
            relief,
            appearance: self.appearance(u, v, &t, footprint),
        })
    }
    pub fn surface(
        &self,
        u: f64,
        v: f64,
        footprint: f64,
        materials: &Materials,
        bed_tint: V3,
    ) -> Result<Surface, String> {
        materials.validate()?;
        if bed_tint
            .iter()
            .any(|v| !v.is_finite() || !(0. ..=4.).contains(v))
        {
            return Err("Invalid ceramic bed tint.".into());
        }
        self.compose(u, v, footprint, materials, bed_tint)
    }
    fn compose(
        &self,
        u: f64,
        v: f64,
        footprint: f64,
        materials: &Materials,
        bed_tint: V3,
    ) -> Result<Surface, String> {
        let c = self.channels(u, v, footprint)?;
        let bed_coverage = (c.installed - c.coverage).max(0.);
        let seam = if bed_coverage > 0. { c.seam } else { 0. };
        Ok(Surface {
            installed: c.installed,
            bump: c.bed.bump * bed_coverage + c.relief * c.coverage,
            layers: vec![
                Layer {
                    material: materials.bed.clone(),
                    weight: bed_coverage,
                    uv: [u * materials.bed_repeat, v * materials.bed_repeat],
                    roughness: Some(materials.bed_roughness),
                    tint: Some(bed_tint.map(|v| v * c.bed.tone * (1. - seam * 0.35))),
                    coatings: vec![],
                },
                Layer {
                    material: materials.ceramic.clone(),
                    weight: c.coverage,
                    uv: [u * materials.repeat, v * materials.repeat],
                    roughness: Some(
                        materials.roughness + (1. - materials.roughness) * c.appearance.fade * 0.2,
                    ),
                    tint: Some(c.appearance.tint),
                    coatings: vec![Coat {
                        material: materials.grime.clone(),
                        uv: [u, v],
                        opacity: c.appearance.deposit,
                        tint: None,
                        roughness: None,
                    }],
                },
            ],
        })
    }
}
impl Materials {
    pub fn validate(&self) -> Result<(), String> {
        if [&self.ceramic, &self.bed, &self.grime]
            .iter()
            .any(|v| v.is_empty() || v.len() > 4096)
            || [self.repeat, self.bed_repeat]
                .iter()
                .any(|v| !v.is_finite() || *v <= 0. || *v > 10000.)
            || [self.roughness, self.bed_roughness]
                .iter()
                .any(|v| !v.is_finite() || !(0. ..=1.).contains(v))
        {
            return Err(
                "Ceramic sources require bounded identities, repeats and roughness.".into(),
            );
        }
        Ok(())
    }
}

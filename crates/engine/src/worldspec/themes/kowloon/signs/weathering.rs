//! Complete painted faces: ordered artwork, edge chips, fixing runoff and opaque PBR output.
use super::{
    Construction, Outline,
    lettering::{Paint, Surface},
};
use crate::worldspec::themes::kowloon::{
    bake_jobs::{Job, Recipe as RasterRecipe},
    fixture::Fixture,
    mesh::rounded,
    noise::{fractal_noise, smooth_step, value_noise},
    palette::{mix, srgb},
    pen::Tint,
    surface::{Coat, Layer, Sample},
    wall::finish_seed,
    weathered,
};
use serde::{Deserialize, Serialize};
use superspec_geometry::{V3, math::hypot};
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Recipe {
    pub width: f64,
    pub height: f64,
    pub construction: Construction,
    pub outline: Outline,
    pub age: f64,
    pub phase: f64,
    pub grime: f64,
    pub wear: f64,
    pub rust: f64,
    pub underpaint: String,
    pub oxide: String,
    /// First entry is the complete substrate; subsequent entries are in painting order.
    pub paints: Vec<Paint>,
}
impl Recipe {
    pub fn owned_bytes(&self) -> usize {
        self.underpaint.capacity()
            + self.oxide.capacity()
            + self.paints.capacity() * std::mem::size_of::<Paint>()
            + self
                .paints
                .iter()
                .map(|p| p.slot.capacity() + p.points.capacity() * std::mem::size_of::<[f64; 2]>())
                .sum::<usize>()
    }
}
pub struct Field<'a> {
    recipe: &'a Recipe,
    order: Vec<usize>,
    seed: f64,
}
struct Patina {
    loss: f64,
    corrosion: f64,
    fade: f64,
    tone: V3,
    pigment: V3,
    roughness: f64,
    bump: f64,
}
fn valid_polygon(points: &[[f64; 2]]) -> bool {
    if !(3..=64).contains(&points.len())
        || points
            .iter()
            .flatten()
            .any(|v| !v.is_finite() || v.abs() > 1000.)
    {
        return false;
    }
    for i in 0..points.len() {
        let a = points[i];
        let b = points[(i + 1) % points.len()];
        let dx = b[0] - a[0];
        let dy = b[1] - a[1];
        if dx * dx + dy * dy < 1e-16
            || points
                .iter()
                .any(|p| dx * (p[1] - a[1]) - dy * (p[0] - a[0]) < -1e-9)
        {
            return false;
        }
    }
    points
        .iter()
        .enumerate()
        .map(|(i, a)| {
            let b = points[(i + 1) % points.len()];
            a[0] * b[1] - a[1] * b[0]
        })
        .sum::<f64>()
        > 1e-12
}
impl<'a> Field<'a> {
    pub fn new(r: &'a Recipe) -> Result<Self, String> {
        if [r.width, r.height]
            .iter()
            .any(|v| !v.is_finite() || *v < 0.001 || *v > 100.)
            || [r.age, r.grime, r.wear, r.rust]
                .iter()
                .any(|v| !v.is_finite() || !(0. ..=1.).contains(v))
            || !r.phase.is_finite()
            || r.phase.abs() > 4294967296.
            || r.paints.is_empty()
            || r.paints.len() > 512
            || [&r.underpaint, &r.oxide]
                .iter()
                .any(|s| s.is_empty() || s.len() > 4096)
            || r.paints.iter().any(|p| {
                p.slot.is_empty()
                    || p.slot.len() > 4096
                    || !valid_polygon(&p.points)
                    || !p.lift.is_finite()
                    || p.lift.abs() > 1.
                    || p.tint
                        .iter()
                        .any(|v| !v.is_finite() || !(0. ..=4.).contains(v))
                    || p.mapping
                        .origin
                        .iter()
                        .chain(&p.mapping.u)
                        .chain(&p.mapping.v)
                        .any(|v| !v.is_finite() || v.abs() > 1e12)
            })
            || r.owned_bytes() > 1024 * 1024
        {
            return Err("Sign paint requires bounded convex artwork, mappings, age and material identities.".into());
        }
        let mut order: Vec<_> = (1..r.paints.len()).collect();
        order.sort_by(|a, b| r.paints[*a].lift.total_cmp(&r.paints[*b].lift));
        let mut js = ryu_js::Buffer::new();
        Ok(Self {
            recipe: r,
            order,
            seed: finish_seed(&format!("sign/{}", js.format(r.phase))) as f64,
        })
    }
    fn patina(&self, x: f64, y: f64) -> Patina {
        let r = self.recipe;
        let seed = self.seed;
        let wear = r.wear * r.age;
        let grime = r.grime * (0.3 + 0.7 * r.age);
        let smooth = matches!(r.construction, Construction::Lightbox | Construction::Cloth);
        let mut distance = f64::INFINITY;
        let points = &r.paints[0].points;
        for i in 0..points.len() {
            let [ax, ay] = points[i];
            let [bx, by] = points[(i + 1) % points.len()];
            let dx = bx - ax;
            let dy = by - ay;
            let along = (((x - ax) * dx + (y - ay) * dy) / (dx * dx + dy * dy)).clamp(0., 1.);
            distance = distance.min(hypot(x - ax - along * dx, y - ay - along * dy));
        }
        let edge = (-distance / 0.035).exp();
        let cloud = fractal_noise(x * 2.7, y * 2.1, seed, 4);
        let warm = (fractal_noise(x * 5.1, y * 4.7, seed + 53., 3) - 0.5) * 0.08;
        let dye = (fractal_noise(x * 7.3, y * 5.9, seed + 59., 3) - 0.5) * 0.06;
        let value = 1. + (cloud - 0.5) * 0.14;
        let pigment = [
            value * (1. + warm),
            value * (1. + dye * 0.45),
            value * (1. - warm * 0.65 - dye * 0.25),
        ];
        let flecks = fractal_noise(x * 22., y * 27., seed + 31., 4) * 0.85
            + fractal_noise(x * 72., y * 89., seed + 33., 2) * 0.15;
        let cluster = smooth_step(0.48, 0.72, fractal_noise(x * 6., y * 8., seed + 37., 3));
        let threshold = 0.73 - wear * (0.1 + edge * 0.42 + cluster * 0.16);
        let loss = if smooth || wear == 0. {
            0.
        } else {
            smooth_step(threshold, threshold + 0.065, flecks)
        };
        let head = (r.height - y).max(0.);
        let foot = y.max(0.);
        let rain = smooth_step(0.35, 0.72, value_noise(x * 39., y * 0.7, seed + 41.));
        let mut runs = 0.;
        let inset = 0.03f64.min(r.width.min(r.height) * 0.12);
        if r.outline == Outline::Rectangle
            && matches!(
                r.construction,
                Construction::Sheet | Construction::Tile | Construction::Enamel
            )
        {
            for fixing in [inset, r.width - inset] {
                let drop = r.height - inset - y;
                if drop > 0. {
                    runs += (-(x - fixing).abs() / (0.005 + drop * 0.018)).exp()
                        * (-drop / 0.32).exp()
                        * (0.5 + 0.5 * rain)
                }
            }
        }
        let dirt = grime
            * 0.65f64.min(
                ((-head / 0.2).exp() * (0.2 + rain * 0.42)
                    + (-foot / 0.08).exp() * 0.22
                    + runs * 0.28)
                    * (0.65 + cloud * 0.7),
            );
        let fade = wear * (0.1 + cloud * 0.22);
        let corrosion = if matches!(
            r.construction,
            Construction::Sheet | Construction::Enamel | Construction::Tile
        ) {
            r.rust * 1f64.min(loss * (0.12 + edge * 0.88) + runs * grime * wear * 0.35)
        } else {
            0.
        };
        Patina {
            loss,
            corrosion,
            fade,
            tone: [1. - dirt * 0.86, 1. - dirt * 0.92, 1. - dirt],
            pigment,
            roughness: 1f64.min(0.5 + (cloud - 0.5) * 0.16 + fade + dirt * 0.28 + loss * 0.28),
            bump: (value_noise(x * 120., y * 67., seed + 47.) - 0.5) * 0.00005 - loss * 0.00045,
        }
    }
    pub fn structure(&self) -> Sample {
        self.sample(0., 0.).expect("admitted sign sample")
    }
    pub fn sample(&self, x: f64, y: f64) -> Result<Sample, String> {
        if [x, y].iter().any(|v| !v.is_finite() || v.abs() > 100000.) {
            return Err("Sign sample exceeds bounded local meters.".into());
        }
        let r = self.recipe;
        let w = self.patina(x, y);
        let ground = &r.paints[0];
        let tint = |c: V3, fade| {
            mix(
                std::array::from_fn(|i| c[i] * w.pigment[i]),
                srgb("#ded8ca"),
                fade,
            )
        };
        let coat = |material: &str, uv, opacity, color, roughness| Coat {
            material: material.into(),
            uv,
            opacity,
            tint: Some(color),
            roughness: Some(roughness),
        };
        let mut coatings = Vec::with_capacity(self.order.len() + 4);
        coatings.push(coat(
            &ground.slot,
            ground.mapping.at(x, y),
            1.,
            tint(ground.tint, w.fade),
            w.roughness,
        ));
        coatings.push(coat(
            &r.underpaint,
            [x, y],
            0.72,
            tint(ground.tint, w.fade),
            w.roughness,
        ));
        for &i in &self.order {
            let p = &r.paints[i];
            let inside = p.points.iter().enumerate().all(|(i, a)| {
                let b = p.points[(i + 1) % p.points.len()];
                (b[0] - a[0]) * (y - a[1]) - (b[1] - a[1]) * (x - a[0]) >= -1e-9
            });
            coatings.push(coat(
                &p.slot,
                p.mapping.at(x, y),
                if inside { 1. } else { 0. },
                tint(p.tint, w.fade * 0.65),
                w.roughness,
            ));
        }
        coatings.push(coat(
            &r.underpaint,
            [x, y],
            w.loss,
            if r.construction == Construction::Timber {
                srgb("#96734a")
            } else {
                srgb("#938f80")
            },
            0.88,
        ));
        coatings.push(coat(
            &r.oxide,
            [x * 3.7 + r.phase, y * 3.7],
            w.corrosion,
            [0.72, 0.8, 0.86],
            0.96,
        ));
        Ok(Sample {
            height: 0.,
            bump: Some(w.bump),
            roughness: Some(w.roughness),
            height_weight: None,
            layer_elevation_weight: None,
            layers: vec![Layer {
                material: ground.slot.clone(),
                weight: 1.,
                uv: ground.mapping.at(x, y),
                tint: Some(w.tone),
                roughness: None,
                coatings,
            }],
        })
    }
}
/// Emits the modeled face once and defers its owned raster recipe to atomic scene binding.
pub fn draw(
    d: &mut Fixture<'_, '_>,
    surface: &Surface,
    recipe: Recipe,
    density: f64,
) -> Result<(), String> {
    if !density.is_finite() || !(0. ..=4096.).contains(&density) {
        return Err("Sign raster density must be finite and bounded.".into());
    }
    Field::new(&recipe)?;
    if (surface.width - recipe.width).abs() > 1e-8 || (surface.height - recipe.height).abs() > 1e-8
    {
        return Err("Sign chart dimensions must match the modeled surface.".into());
    }
    let ground = &recipe.paints[0];
    let points: Vec<_> = ground
        .points
        .iter()
        .map(|p| surface.at(p[0], p[1], 0.))
        .collect();
    let first = d.mesh.triangle_count();
    let inverse = d.pen.matrix.inverse()?;
    let uv: Vec<_> = points
        .iter()
        .map(|p| surface.coordinates(inverse.point(d.pen.world(*p).map(|v| rounded(v, 1e5)))))
        .collect();
    let job = Job {
        recipe: RasterRecipe::Sign(Box::new(recipe.clone())),
        density: density.max(128.),
    };
    job.validate()?;
    d.pen.face_raw(
        d.mesh,
        &ground.slot,
        &points,
        surface.out,
        &Tint::Color([1.; 3]),
        1.,
        Some(&uv),
        None,
    )?;
    weathered::receive_surface(
        d.mesh,
        first,
        job,
        format!("{}/sign-face/{first}", d.pen.part),
        Some(super::super::stains::Receiver {
            frame: {
                let origin = d.pen.world(surface.at(0., 0., 0.));
                super::super::stains::Frame {
                    origin,
                    u: superspec_geometry::math::sub(d.pen.world(surface.at(1., 0., 0.)), origin),
                    v: superspec_geometry::math::sub(d.pen.world(surface.at(0., 1., 0.)), origin),
                    normal: d.pen.matrix.direction(surface.out),
                }
            },
            fallback: None,
        }),
    )
}

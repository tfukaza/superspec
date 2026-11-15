//! Bounded paper decals and seeded notice clusters, using the owned 4x4 poster atlas.
use super::{fixture::Fixture, pen::Tint};
use serde::{Deserialize, Serialize};
use superspec_geometry::math::{cos, sin};
use superspec_types::rng::SeededRng;
pub const DESIGNS: [&str; 16] = [
    "calendar",
    "roast-menu",
    "tea-ad",
    "soda-ad",
    "no-smoking",
    "repairs",
    "room-rental",
    "hiring",
    "herbal-ad",
    "opera",
    "film",
    "laundry",
    "rice-ad",
    "clinic",
    "closure",
    "electrical-ad",
];
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Options {
    pub design: String,
    pub width: f64,
    pub rotation: f64,
    pub layer: usize,
    pub wear: f64,
    pub seed: Option<String>,
}
impl Options {
    fn validate(&self) -> Result<usize, String> {
        let cell = DESIGNS
            .iter()
            .position(|name| *name == self.design)
            .ok_or("Unknown poster design.")?;
        if !self.width.is_finite()
            || !(0.05..=2.).contains(&self.width)
            || !self.rotation.is_finite()
            || self.rotation.abs() > 15.
            || self.layer > 8
            || !self.wear.is_finite()
            || !(0. ..=1.).contains(&self.wear)
            || self.seed.as_ref().is_some_and(|s| s.len() > 4096)
        {
            return Err("Poster dimensions, rotation, layer or wear exceed bounds.".into());
        }
        Ok(cell)
    }
    pub fn bounds(&self) -> Result<[f64; 2], String> {
        self.validate()?;
        let angle = self.rotation * std::f64::consts::PI / 180.;
        let (c, s) = (cos(angle).abs(), sin(angle).abs());
        Ok([self.width * (c + 1.5 * s), self.width * (1.5 * c + s)])
    }
}
fn uv(cell: usize, u: f64, v: f64) -> [f64; 2] {
    [
        ((cell % 4) as f64 * 512. + 16.5 + u * 479.) / 2048.,
        1. - ((cell / 4) as f64 * 768. + 24.5 + (1. - v) * 719.) / 3072.,
    ]
}
pub fn draw(
    drawing: &mut Fixture<'_, '_>,
    options: &Options,
    textured: bool,
) -> Result<(), String> {
    let cell = options.validate()?;
    if !textured {
        return Err("Posters require the wall-posters atlas finish.".into());
    }
    let mut rng = SeededRng::from_text(options.seed.as_deref().unwrap_or(&options.design));
    let cut = (0.006 + options.wear * 0.035) * rng.float(0.7, 1.);
    let outline = [
        [cut, 0.],
        [1. - cut * 1.6, 0.],
        [1., cut],
        [1., 1. - cut * 1.8],
        [1. - cut, 1.],
        [cut * 1.3, 1.],
        [0., 1. - cut],
        [0., cut * 1.5],
    ];
    let angle = options.rotation * std::f64::consts::PI / 180.;
    let (c, s) = (cos(angle), sin(angle));
    let points = outline.map(|[u, v]| {
        let x = (u - 0.5) * options.width;
        let z = (v - 0.5) * options.width * 1.5;
        [
            x * c - z * s,
            0.002 + options.layer as f64 * 0.0006,
            x * s + z * c,
        ]
    });
    let coordinates = if drawing.mesh.surveying() {
        None
    } else {
        Some(outline.map(|[u, v]| {
            uv(
                cell,
                if drawing.pen.matrix.determinant() > 0. {
                    1. - u
                } else {
                    u
                },
                v,
            )
        }))
    };
    drawing.pen.face(
        drawing.mesh,
        "posters",
        &points,
        [0., 1., 0.],
        &Tint::Color([1.; 3]),
        1.,
        coordinates.as_ref().map(|u| u.as_slice()),
        None,
    )
}
pub fn notices(
    drawing: &mut Fixture<'_, '_>,
    bounds: [f64; 4],
    rng: &mut SeededRng,
    textured: bool,
) -> Result<(), String> {
    let [x0, x1, z0, z1] = bounds;
    if bounds.iter().any(|v| !v.is_finite() || v.abs() > 10000.) {
        return Err("Notice region requires bounded finite coordinates.".into());
    }
    if x1 <= x0 || z1 <= z0 || !textured {
        return Ok(());
    }
    if drawing.pen.part.len() + 10 > 4096 {
        return Err("Notice identity exceeds drawing budget.".into());
    }
    let count = (((x1 - x0) * (z1 - z0) * rng.float(1.5, 4.)).round() as usize).min(9);
    for i in 0..count {
        let mut options = Options {
            design: rng.pick(&DESIGNS).into(),
            width: rng.float(0.2, 0.34),
            rotation: rng.float(-7., 7.),
            layer: i,
            wear: rng.float(0.15, 0.6),
            seed: Some(format!("{}/notice-{i}", drawing.pen.part)),
        };
        let [width, height] = options.bounds()?;
        options.width *= 1_f64.min((x1 - x0) / width).min((z1 - z0) / height);
        if options.width < 0.05 {
            continue;
        }
        let [width, height] = options.bounds()?;
        let x = rng.float(x0 + width / 2., x1 - width / 2.);
        let z = rng.float(z0 + height / 2., z1 - height / 2.);
        let pen = drawing
            .pen
            .into_part(options.seed.as_ref().unwrap())?
            .moved([x, 0.003, z])?;
        draw(
            &mut Fixture {
                pen,
                mesh: drawing.mesh,
                repeats: drawing.repeats,
            },
            &options,
            true,
        )?;
    }
    Ok(())
}

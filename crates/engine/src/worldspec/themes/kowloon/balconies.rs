//! Convex balcony slabs, mitered parapets and bolted climbing barriers.
use super::{
    facade_material,
    fixture::Fixture,
    massing::Rect,
    metal::{Binding, Kind},
    palette::shade,
    pen::{Pen, Tint},
    skin,
    steel::{self, Finish, Plate, Steel},
    wall, weathered,
};
use serde::Deserialize;
use superspec_geometry::{
    V3,
    math::{cos, hypot, sin},
};
use superspec_types::rng::SeededRng;
pub type Outline = [V3; 4];
fn validate(outline: &Outline) -> Result<(), String> {
    if outline
        .iter()
        .flatten()
        .any(|v| !v.is_finite() || v.abs() > 1000.)
    {
        return Err("Balcony outline exceeds finite local bounds.".into());
    }
    for i in 0..4 {
        let a = outline[i];
        let b = outline[(i + 1) % 4];
        let c = outline[(i + 2) % 4];
        let length = hypot(b[0] - a[0], b[1] - a[1]);
        if !(0.001..=100.).contains(&length)
            || (b[0] - a[0]) * (c[1] - b[1]) - (b[1] - a[1]) * (c[0] - b[0]) <= 1e-9
        {
            return Err("Balcony requires a strictly convex counterclockwise outline.".into());
        }
    }
    Ok(())
}
fn tint(color: V3) -> Result<(), String> {
    if color
        .iter()
        .any(|v| !v.is_finite() || !(0. ..=4.).contains(v))
    {
        Err("Balcony tint exceeds finite bounds.".into())
    } else {
        Ok(())
    }
}
fn at(p: V3, z: f64) -> V3 {
    [p[0], p[1], z]
}
/// Closed physical-thickness slab with level upper and lower faces.
pub fn slab(
    d: &mut Fixture<'_, '_>,
    outline: &Outline,
    bottom: f64,
    top: f64,
    color: V3,
    slot: &str,
) -> Result<(), String> {
    validate(outline)?;
    tint(color)?;
    if !bottom.is_finite()
        || !top.is_finite()
        || bottom.abs() > 1000.
        || top <= bottom
        || top - bottom > 10.
        || slot.is_empty()
        || slot.len() > 256
    {
        return Err("Invalid balcony slab heights or slot.".into());
    }
    d.face(slot, &outline.map(|p| at(p, top)), [0., 0., 1.], color)?;
    d.face(slot, &outline.map(|p| at(p, bottom)), [0., 0., -1.], color)?;
    for i in 0..4 {
        let a = outline[i];
        let b = outline[(i + 1) % 4];
        d.face(
            slot,
            &[at(a, bottom), at(b, bottom), at(b, top), at(a, top)],
            [b[1] - a[1], a[0] - b[0], 0.],
            color,
        )?;
    }
    Ok(())
}
pub struct Edge {
    pub pen: Pen,
    pub length: f64,
}
/// Exposed edges in left/front/right order, with outward-facing drawing frames.
pub fn edges(pen: &Pen, outline: &Outline) -> Result<Vec<Edge>, String> {
    validate(outline)?;
    let path = [outline[0], outline[3], outline[2], outline[1]];
    path.windows(2)
        .map(|p| {
            let a = p[0];
            let b = p[1];
            let length = hypot(b[0] - a[0], b[1] - a[1]);
            let tangent = [(b[0] - a[0]) / length, (b[1] - a[1]) / length, 0.];
            Ok(Edge {
                pen: pen.facing(a, tangent, [-tangent[1], tangent[0], 0.])?,
                length,
            })
        })
        .collect()
}
fn inset(outline: &Outline, distance: f64) -> Result<Outline, String> {
    let inner = std::array::from_fn(|i| {
        let p = outline[i];
        let a = outline[(i + 3) % 4];
        let b = outline[(i + 1) % 4];
        let ax = p[0] - a[0];
        let ay = p[1] - a[1];
        let bx = b[0] - p[0];
        let by = b[1] - p[1];
        let al = hypot(ax, ay);
        let bl = hypot(bx, by);
        let nx = -ay / al;
        let ny = ax / al;
        let mx = -by / bl;
        let my = bx / bl;
        let determinant = nx * my - ny * mx;
        [
            p[0] + distance * (my - ny) / determinant,
            p[1] + distance * (nx - mx) / determinant,
            p[2],
        ]
    });
    validate(&inner)?;
    if (0..4).any(|i| {
        let j = (i + 1) % 4;
        (inner[j][0] - inner[i][0]) * (outline[j][0] - outline[i][0])
            + (inner[j][1] - inner[i][1]) * (outline[j][1] - outline[i][1])
            <= 0.
    }) {
        return Err("Balcony inset consumes an entire edge.".into());
    }
    if inner.iter().any(|p| {
        (0..4).any(|i| {
            let a = outline[i];
            let b = outline[(i + 1) % 4];
            (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0]) < 0.
        })
    }) {
        return Err("Balcony inset crosses its outer footprint.".into());
    }
    Ok(inner)
}
#[derive(Clone, Deserialize)]
pub struct Settings {
    pub weathering: weathered::Settings,
    pub wall: Binding,
    pub concrete: Binding,
}
/// Open-backed parapet: one mitered cap and inner lining per exposed edge, with local runoff finishes.
#[allow(clippy::too_many_arguments)]
pub fn parapet(
    d: &mut Fixture<'_, '_>,
    outline: &Outline,
    height: f64,
    color: V3,
    trim: V3,
    settings: &Settings,
    key: &str,
) -> Result<(), String> {
    validate(outline)?;
    tint(color)?;
    tint(trim)?;
    settings.weathering.validate()?;
    if !height.is_finite()
        || !(0.03..=5.).contains(&height)
        || key.len() > 2048
        || settings.weathering.texture_density <= 0.
    {
        return Err(
            "Parapet requires positive raster density and bounded height and identity.".into(),
        );
    }
    let inner = inset(outline, 0.075)?;
    let edges = edges(&d.pen, outline)?;
    let finish = facade_material::Settings {
        concrete: false,
        coat: facade_material::Coat {
            material: settings.wall.material.clone(),
            repeat: settings.wall.repeat,
            tint: color,
        },
        backing: settings.concrete.clone(),
        wear: settings.weathering.wear,
        density: settings.weathering.texture_density,
        variant: 0,
    };
    facade_material::Facade::new(&finish)?;
    for i in 1..4 {
        let j = (i + 1) % 4;
        let a = outline[i];
        let b = outline[j];
        let c = inner[j];
        let e = inner[i];
        d.face(
            "wall",
            &[
                at(a, height + 0.035),
                at(b, height + 0.035),
                at(c, height + 0.035),
                at(e, height + 0.035),
            ],
            [0., 0., 1.],
            trim,
        )?;
        d.face(
            "wall",
            &[
                at(c, 0.02),
                at(e, 0.02),
                at(e, height + 0.035),
                at(c, height + 0.035),
            ],
            [a[1] - b[1], b[0] - a[0], 0.],
            color,
        )?;
        d.face(
            "wall",
            &[
                at(a, height),
                at(b, height),
                at(b, height + 0.035),
                at(a, height + 0.035),
            ],
            [b[1] - a[1], a[0] - b[0], 0.],
            trim,
        )?;
    }
    for i in [0, 1] {
        d.face(
            "wall",
            &[
                at(outline[i], 0.02),
                at(inner[i], 0.02),
                at(inner[i], height + 0.035),
                at(outline[i], height + 0.035),
            ],
            [0., -1., 0.],
            color,
        )?;
    }
    for (i, edge) in edges.into_iter().enumerate() {
        let seed = SeededRng::from_text(&format!("{}/{key}/parapet-{i}", settings.weathering.seed))
            .integer(1, 2147483647) as u32;
        skin::draw(
            &mut Fixture {
                pen: edge.pen,
                mesh: d.mesh,
                repeats: d.repeats,
            },
            "wall",
            &skin::Settings {
                rects: vec![Rect {
                    u0: 0.,
                    u1: edge.length,
                    v0: 0.02,
                    v1: height,
                }],
                openings: vec![],
                u_marks: vec![],
                z_marks: vec![],
                finish: facade_material::Settings {
                    variant: seed,
                    ..finish.clone()
                },
                condition: wall::Settings {
                    base: None,
                    ledges: vec![wall::Ledge {
                        u0: 0.,
                        u1: edge.length,
                        z: height,
                        flow: 0.8,
                    }],
                    openings: vec![],
                    grime: settings.weathering.grime,
                    wear: settings.weathering.wear,
                    seed: f64::from(seed),
                },
                washes: vec![],
                tiles: None,
            },
            &format!("{key}/parapet-{i}"),
        )?;
    }
    Ok(())
}
/// Semicircular steel fan in the local YZ plane, bolted to the receiving wall.
pub fn anti_climb(
    d: &mut Fixture<'_, '_>,
    x: f64,
    z: f64,
    radius: f64,
    color: V3,
    settings: &weathered::Settings,
) -> Result<(), String> {
    anti_climb_of(d, x, z, radius, color, settings, Fan::default())
}
/// The rods of a climbing barrier: the default is the facades' own eleven 14 mm spokes under a
/// 24 mm rim.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fan {
    /// The radius of the rim's tube in meters.
    pub rim: f64,
    /// The radius of each spoke in meters.
    pub spoke: f64,
    /// How many spokes fan out between the upright's two ends: 1, 2, 3, 5, 7, 11 or 23.
    pub spokes: usize,
}
impl Default for Fan {
    fn default() -> Self {
        Self {
            rim: 0.012,
            spoke: 0.007,
            spokes: 11,
        }
    }
}
/// [`anti_climb`] of other rods: fewer, bolder spokes read from further off.
pub fn anti_climb_of(
    d: &mut Fixture<'_, '_>,
    x: f64,
    z: f64,
    radius: f64,
    color: V3,
    settings: &weathered::Settings,
    fan: Fan,
) -> Result<(), String> {
    if !(0.004..=0.05).contains(&fan.rim)
        || !(0.003..=0.05).contains(&fan.spoke)
        || 24 % (fan.spokes + 1) != 0
        || fan.spokes == 0
    {
        return Err(
            "A climbing barrier's rods are 3–50 mm in radius, with 1, 2, 3, 5, 7, 11 or 23 spokes."
                .into(),
        );
    }
    if [x, z, radius]
        .iter()
        .any(|v| !v.is_finite() || v.abs() > 1000.)
        || !(0.05..=5.).contains(&radius)
    {
        return Err("Invalid climbing-barrier dimensions.".into());
    }
    tint(color)?;
    settings.validate()?;
    let pen = d.pen.with_weathering(
        d.mesh,
        settings.clone(),
        Kind::Cage,
        None,
        vec!["paint".into(), "metal".into()],
    )?;
    let mut metal = Fixture {
        pen,
        mesh: d.mesh,
        repeats: d.repeats,
    };
    let y = 0.025;
    let arc: Vec<_> = (0..25)
        .map(|i| {
            [
                x,
                y + radius * sin(i as f64 * std::f64::consts::PI / 24.),
                z + radius * cos(i as f64 * std::f64::consts::PI / 24.),
            ]
        })
        .collect();
    metal.tube("metal", &arc, fan.rim, color, 8)?;
    metal.pen.beveled_bar(
        metal.mesh,
        "metal",
        [x, y, z - radius - 0.04],
        [x, y, z + radius + 0.04],
        [1., 0., 0.],
        0.03,
        0.03,
        0.0015,
        &Tint::Color(color),
        metal.repeat("metal"),
    )?;
    let step = 24 / (fan.spokes + 1);
    for i in 1..=fan.spokes {
        metal.tube("metal", &[[x, y, z], arc[i * step]], fan.spoke, color, 6)?;
    }
    for height in [z - radius * 0.8, z + radius * 0.8] {
        steel::fixing_plate(
            &mut metal,
            Steel {
                slot: "metal",
                tint: color,
                finish: Finish::Metal(Some(settings)),
            },
            &Plate {
                center: [x, 0., height],
                across: [1., 0., 0.],
                up: [0., 0., 1.],
                out: [0., 1., 0.],
                size: [0.09, 0.13],
                bolts: 2,
            },
        )?;
    }
    metal.disc(
        "metal",
        [x + 0.014, y, z],
        0.04,
        [1., 0., 0.],
        shade(color, 0.8),
        12,
    )
}

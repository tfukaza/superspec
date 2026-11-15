//! Household fixtures placed in their owner's facade frame, in meters.
use super::pipes::{Options as PipeOptions, PipeSystem};
use super::{
    balcony_props::{self, Plant},
    fixture::Fixture,
    laundry::{self, Support},
    metal::Kind,
    palette::{mix, shade, srgb},
    pen::Pen,
    weathered,
};
use super::{
    corrugated,
    pen::Tint,
    steel::{self, Finish, Plate, Steel},
};
use superspec_geometry::V3;
use superspec_types::rng::SeededRng;

fn position(values: &[f64]) -> bool {
    values.iter().all(|v| v.is_finite() && v.abs() <= 1000.)
}
fn validate_weathering(settings: Option<&weathered::Settings>) -> Result<(), String> {
    if let Some(s) = settings {
        s.validate()?;
    }
    Ok(())
}
fn hardware(
    d: &mut Fixture<'_, '_>,
    settings: Option<&weathered::Settings>,
) -> Result<Pen, String> {
    if let Some(s) = settings {
        d.pen
            .with_weathering(d.mesh, s.clone(), Kind::Frame, None, vec!["metal".into()])
    } else {
        Ok(d.pen.clone())
    }
}
pub(super) fn metal_color(color: V3, oxidation: f64, settings: Option<&weathered::Settings>) -> V3 {
    if settings.is_some_and(|s| s.texture_density > 0.) {
        color
    } else {
        mix(color, srgb("#7a4a2a"), oxidation)
    }
}
/// Canvas uses the shared settled-cloth model; metal sheets retain their bracket geometry.
pub fn awning(
    d: &mut Fixture<'_, '_>,
    e: super::tarps::Envelope,
    style: &super::palette::Awning,
    key: &str,
    settings: Option<&weathered::Settings>,
) -> Result<(), String> {
    if !position(&[e.x0, e.x1, e.z, e.depth, e.drop])
        || !(0.1..=40.).contains(&(e.x1 - e.x0))
        || !(0.1..=5.).contains(&e.depth)
        || !(0.01..=5.).contains(&e.drop)
        || key.len() > 2048
        || [style.base, style.hem]
            .iter()
            .flatten()
            .any(|v| !v.is_finite() || !(0. ..=4.).contains(v))
    {
        return Err("Awning requires bounded dimensions, identity and colors.".into());
    }
    validate_weathering(settings)?;
    let pen = hardware(d, settings)?;
    if style.canvas {
        return super::tarps::canvas(d, e, style.base, style.hem, key, settings, None);
    }
    let super::tarps::Envelope {
        x0,
        x1,
        z,
        depth,
        drop,
        ..
    } = e;
    corrugated::draw(
        d,
        "sheet",
        [
            [x0, 0., z],
            [x1, 0., z],
            [x1, depth, z - drop],
            [x0, depth, z - drop],
        ],
        0.11,
        style.base,
        settings,
    )?;
    let d = &mut Fixture {
        pen,
        mesh: d.mesh,
        repeats: d.repeats,
    };
    let steel = shade(srgb("#a4a9aa"), 0.6);
    for x in [x0 + 0.05, x1 - 0.05] {
        d.bar(
            "metal",
            [x, 0., z - 0.03],
            [x, depth, z - drop - 0.03],
            0.025,
            steel,
        )?;
        d.bar(
            "metal",
            [x, 0., z - drop - 0.25],
            [x, depth * 0.8, z - drop * 0.8 - 0.04],
            0.02,
            steel,
        )?;
    }
    Ok(())
}
/// A seeded row preserves skipped-pot decisions and the parent's frame and material scope.
pub fn plants(
    d: &mut Fixture<'_, '_>,
    span: [f64; 2],
    y: f64,
    z: f64,
    rng: &mut SeededRng,
    textured: bool,
) -> Result<(), String> {
    if !position(&[span[0], span[1], y, z]) || span[1] < span[0] || span[1] - span[0] > 100. {
        return Err("Invalid plant row.".into());
    }
    let mut x = span[0] + 0.14;
    while x < span[1] - 0.1 {
        d.mesh.spend_work(1)?;
        if rng.chance(0.8) {
            let name = rng.pick(&[Plant::RubberPlant, Plant::TrailingPothos, Plant::SnakePlant]);
            let scale = rng.float(0.72, 1.02);
            let rotation = rng.float(0., std::f64::consts::TAU);
            balcony_props::plant(d, name, [x, y, z], scale, rotation, textured)?;
        }
        x += rng.float(0.28, 0.55);
    }
    Ok(())
}
/// Bamboo poles held by two metal arms; their cloth lies parallel to the facade.
pub fn drying_rack(
    d: &mut Fixture<'_, '_>,
    span: [f64; 2],
    z: f64,
    depth: f64,
    rng: &mut SeededRng,
    settings: Option<&weathered::Settings>,
    textured: bool,
) -> Result<(), String> {
    if !position(&[span[0], span[1], z])
        || span[1] - span[0] < 0.1
        || span[1] - span[0] > 20.
        || !depth.is_finite()
        || !(0.1..=5.).contains(&depth)
    {
        return Err("Invalid drying rack.".into());
    }
    validate_weathering(settings)?;
    let steel = metal_color(srgb("#a4a9aa"), rng.float(0.2, 0.7), settings);
    let mut frame = Fixture {
        pen: hardware(d, settings)?,
        mesh: d.mesh,
        repeats: d.repeats,
    };
    for x in span {
        frame.bar("metal", [x, 0., z], [x, depth, z + 0.04], 0.025, steel)?;
        frame.bar(
            "metal",
            [x, 0., z - 0.25],
            [x, depth * 0.7, z + 0.02],
            0.018,
            steel,
        )?;
    }
    let poles = if depth > 0.5 { 3 } else { 2 };
    for i in 0..poles {
        let y = depth * (0.35 + 0.65 * i as f64 / (poles - 1) as f64);
        d.tube(
            "paint",
            &[[span[0] - 0.15, y, z + 0.03], [span[1] + 0.15, y, z + 0.03]],
            0.013,
            srgb("#c8b888"),
            6,
        )?;
        laundry::washing(
            &mut Fixture {
                pen: d.pen.moved([0., y, 0.])?,
                mesh: d.mesh,
                repeats: d.repeats,
            },
            [span[0] - 0.1, span[1] + 0.1],
            z + 0.03,
            rng,
            &laundry::WashingOptions {
                fill: 0.75,
                drop: 0.8,
                support: Support::Pole,
            },
            textured,
        )?;
    }
    Ok(())
}
/// A socketed bamboo pole carries hangers perpendicular to its outward axis.
pub fn drying_pole(
    d: &mut Fixture<'_, '_>,
    x: f64,
    z: f64,
    length: f64,
    rng: &mut SeededRng,
    settings: Option<&weathered::Settings>,
    textured: bool,
) -> Result<(), String> {
    if !position(&[x, z]) || !length.is_finite() || !(0.35..=20.).contains(&length) {
        return Err("Invalid drying pole.".into());
    }
    validate_weathering(settings)?;
    let pen = hardware(d, settings)?;
    d.tube(
        "paint",
        &[[x, 0., z], [x, length, z + length * 0.12]],
        0.014,
        srgb("#c8b888"),
        6,
    )?;
    Fixture {
        pen,
        mesh: d.mesh,
        repeats: d.repeats,
    }
    .tube(
        "metal",
        &[[x, 0., z], [x, 0.06, z + 0.007]],
        0.024,
        shade(srgb("#a4a9aa"), 0.6),
        6,
    )?;
    laundry::hangers(
        &mut Fixture {
            pen: d.pen.facing([x, 0., z], [0., 1., 0.], [1., 0., 0.])?,
            mesh: d.mesh,
            repeats: d.repeats,
        },
        &laundry::HangerRow {
            span: [0.3, length - 0.04],
            z: 0.,
            slope: 0.12,
            reach: 0.4,
            drop: 0.85,
        },
        rng,
        textured,
    )
}
/// Three roof drying lines between T-posts; only the posts receive metal aging.
pub fn drying_lines(
    d: &mut Fixture<'_, '_>,
    span: [f64; 2],
    y: f64,
    rng: &mut SeededRng,
    settings: Option<&weathered::Settings>,
    textured: bool,
) -> Result<(), String> {
    if !position(&[span[0], span[1], y]) || span[1] - span[0] < 0.3 || span[1] - span[0] > 20. {
        return Err("Invalid drying lines.".into());
    }
    validate_weathering(settings)?;
    let mut frame = Fixture {
        pen: hardware(d, settings)?,
        mesh: d.mesh,
        repeats: d.repeats,
    };
    for x in span {
        frame.bar("metal", [x, y, 0.], [x, y, 1.9], 0.04, srgb("#a4a9aa"))?;
        frame.bar(
            "metal",
            [x, y - 0.4, 1.9],
            [x, y + 0.4, 1.9],
            0.03,
            srgb("#a4a9aa"),
        )?;
    }
    for offset in [-0.35, 0., 0.35] {
        frame.bar(
            "paint",
            [span[0], y + offset, 1.88],
            [span[1], y + offset, 1.88],
            0.008,
            srgb("#2a2927"),
        )?;
        laundry::washing(
            &mut Fixture {
                pen: frame.pen.moved([0., y + offset, 0.])?,
                mesh: frame.mesh,
                repeats: frame.repeats,
            },
            [span[0] + 0.1, span[1] - 0.1],
            1.88,
            rng,
            &laundry::WashingOptions {
                fill: 0.7,
                drop: 1.15,
                support: Support::Line,
            },
            textured,
        )?;
    }
    Ok(())
}

pub struct Cage {
    pub span: [f64; 2],
    pub heights: [f64; 2],
    pub depth: f64,
    pub color: V3,
    pub stocked: bool,
}
/// Welded enclosure, fixing hardware, optional corrugated roof and seeded household contents.
pub fn cage(
    d: &mut Fixture<'_, '_>,
    c: &Cage,
    rng: &mut SeededRng,
    settings: Option<&weathered::Settings>,
    textured: bool,
) -> Result<(), String> {
    let [x0, x1] = c.span;
    let [z0, z1] = c.heights;
    let depth = c.depth;
    if !position(&[x0, x1, z0, z1])
        || !(0.3..=20.).contains(&(x1 - x0))
        || !(0.2..=10.).contains(&(z1 - z0))
        || !depth.is_finite()
        || !(0.1..=5.).contains(&depth)
        || c.color.iter().any(|v| !v.is_finite() || *v < 0. || *v > 4.)
    {
        return Err("Invalid window cage.".into());
    }
    validate_weathering(settings)?;
    let paint = metal_color(c.color, rng.float(0., 0.35), settings);
    let pen = if let Some(s) = settings {
        d.pen.with_weathering(
            d.mesh,
            s.clone(),
            Kind::Cage,
            None,
            vec!["paint".into(), "metal".into()],
        )?
    } else {
        d.pen.clone()
    };
    let mut frame = Fixture {
        pen,
        mesh: d.mesh,
        repeats: d.repeats,
    };
    let finish = Steel {
        slot: "paint",
        tint: paint,
        finish: Finish::Metal(settings),
    };
    let width = 0.09_f64.min((x1 - x0) * 0.2);
    let height = 0.14_f64.min((z1 - z0) * 0.2);
    for x in [x0 + width / 2., x1 - width / 2.] {
        for z in [z0 + height / 2., z1 - height / 2.] {
            steel::fixing_plate(
                &mut frame,
                finish,
                &Plate {
                    center: [x, 0., z],
                    across: [1., 0., 0.],
                    up: [0., 0., 1.],
                    out: [0., 1., 0.],
                    size: [width, height],
                    bolts: 2,
                },
            )?;
            let front = if z < (z0 + z1) / 2. {
                z0 + 0.014
            } else {
                z1 - 0.014
            };
            steel::member(
                &mut frame,
                finish,
                [[x, 0.012, z], [x, depth - 0.014, front]],
                [1., 0., 0.],
                [0.025, 0.012],
            )?;
        }
    }
    let mut rail = |a: V3, b: V3, thickness: f64| {
        let side = if (b[0] - a[0]).abs() > (b[2] - a[2]).abs() {
            [0., 0., 1.]
        } else {
            [1., 0., 0.]
        };
        let repeat = frame.repeat("paint");
        frame.pen.beveled_bar(
            frame.mesh,
            "paint",
            a,
            b,
            side,
            thickness,
            thickness,
            0.001,
            &Tint::Color(paint),
            repeat,
        )
    };
    let mut y = 0.055;
    while y < depth {
        rail([x0, y, z0], [x1, y, z0], 0.009)?;
        y += 0.11;
    }
    for z in [z0, (z0 + z1) / 2., z1] {
        rail([x0, depth, z], [x1, depth, z], 0.028)?;
        for x in [x0, x1] {
            rail([x, 0., z], [x, depth, z], 0.028)?;
        }
    }
    for x in [x0, x1] {
        rail([x, depth, z0], [x, depth, z1], 0.03)?;
    }
    let mut x = x0 + 0.11;
    while x < x1 - 0.04 {
        rail([x, depth, z0], [x, depth, z1], 0.012)?;
        x += 0.11;
    }
    for x in [x0, x1] {
        let mut y = 0.11;
        while y < depth - 0.03 {
            rail([x, y, z0], [x, y, z1], 0.012)?;
            y += 0.11;
        }
    }
    let mut x = x0 + 0.2;
    while x < x1 - 0.1 {
        rail([x, 0., z0], [x, depth, z0], 0.012)?;
        x += 0.2;
    }
    if rng.chance(0.55) {
        let roof = if rng.chance(0.5) {
            srgb("#b9c79a")
        } else {
            metal_color(srgb("#a4a9aa"), rng.float(0., 0.6), settings)
        };
        corrugated::draw(
            d,
            "sheet",
            [
                [x0 - 0.05, 0., z1 + 0.14],
                [x1 + 0.05, 0., z1 + 0.14],
                [x1 + 0.05, depth + 0.1, z1 + 0.02],
                [x0 - 0.05, depth + 0.1, z1 + 0.02],
            ],
            0.09,
            roof,
            settings,
        )?;
    }
    if !c.stocked {
        return Ok(());
    }
    let content = rng.pick(&["plants", "plants", "washing", "storage", "mixed"]);
    if content == "plants" || content == "mixed" {
        plants(d, [x0, x1], depth * 0.6, z0 + 0.01, rng, textured)?;
    }
    if content == "washing" || content == "mixed" {
        d.tube(
            "paint",
            &[[x0, depth * 0.72, z1 - 0.05], [x1, depth * 0.72, z1 - 0.05]],
            0.013,
            srgb("#c8b888"),
            6,
        )?;
        laundry::washing(
            &mut Fixture {
                pen: d.pen.moved([0., depth * 0.72, 0.])?,
                mesh: d.mesh,
                repeats: d.repeats,
            },
            [x0 + 0.05, x1 - 0.05],
            z1 - 0.05,
            rng,
            &laundry::WashingOptions {
                fill: 0.85,
                drop: 0.8_f64.min(z1 - z0 - 0.12),
                support: Support::Pole,
            },
            textured,
        )?;
    }
    if content == "storage" {
        let mut x = x0 + 0.06;
        while x < x1 - 0.35 {
            let size = rng.float(0.22, 0.34);
            let high = [
                x + size,
                depth * 0.25 + size,
                z0 + size * rng.float(0.7, 1.3),
            ];
            d.cuboid(
                "paint",
                [x, depth * 0.25, z0 + 0.01],
                high,
                shade(srgb("#a8845a"), rng.float(0.7, 1.2)),
                "d",
            )?;
            x += rng.float(0.32, 0.5);
        }
    }
    Ok(())
}

fn conduit(radius: f64, settings: &weathered::Settings) -> Result<PipeSystem, String> {
    PipeSystem::new(PipeOptions {
        radius,
        color: srgb("#2a2927"),
        sides: 16,
        variant: "pipe-standard".into(),
        weathering: Some(settings.clone()),
    })
}
/// Bulkhead glass, metal guard and a rising conduit share the caller's wall frame.
pub fn wall_lamp(
    d: &mut Fixture<'_, '_>,
    x: f64,
    z: f64,
    settings: &weathered::Settings,
) -> Result<(), String> {
    if !position(&[x, z]) {
        return Err("Invalid wall lamp position.".into());
    }
    settings.validate()?;
    let pen = hardware(d, Some(settings))?;
    Fixture {
        pen: pen.clone(),
        mesh: d.mesh,
        repeats: d.repeats,
    }
    .cuboid(
        "metal",
        [x - 0.09, 0., z],
        [x + 0.09, 0.06, z + 0.13],
        shade(srgb("#a4a9aa"), 0.5),
        "b",
    )?;
    d.cuboid(
        "pane",
        [x - 0.07, 0.06, z + 0.015],
        [x + 0.07, 0.125, z + 0.115],
        srgb("#f4e8c0"),
        "b",
    )?;
    for bar in [-0.03, 0.03] {
        Fixture {
            pen: pen.clone(),
            mesh: d.mesh,
            repeats: d.repeats,
        }
        .bar(
            "metal",
            [x + bar, 0.128, z + 0.015],
            [x + bar, 0.128, z + 0.115],
            0.008,
            shade(srgb("#a4a9aa"), 0.4),
        )?;
    }
    conduit(0.012, settings)?.run(d, &[[x, 0.02, z + 0.13], [x, 0.02, z + 0.75]], Some(0.))
}
/// Utility-meter boxes with individual glasses and rising, separately charted conduits.
pub fn meter_bank(
    d: &mut Fixture<'_, '_>,
    x: f64,
    z: f64,
    count: usize,
    rng: &mut SeededRng,
    settings: &weathered::Settings,
) -> Result<(), String> {
    if !position(&[x, z]) || count > 64 {
        return Err("Invalid meter bank position or count.".into());
    }
    settings.validate()?;
    let pen = hardware(d, Some(settings))?;
    let body = shade(srgb("#a4a9aa"), rng.float(0.5, 0.9));
    for i in 0..count {
        let x0 = x + i as f64 * 0.3;
        let mut frame = Fixture {
            pen: pen.clone(),
            mesh: d.mesh,
            repeats: d.repeats,
        };
        frame.cuboid("metal", [x0, 0., z], [x0 + 0.24, 0.12, z + 0.34], body, "b")?;
        frame.cuboid(
            "metal",
            [x0 + 0.03, 0.12, z + 0.15],
            [x0 + 0.21, 0.128, z + 0.31],
            shade(body, 0.7),
            "b",
        )?;
        d.panel(
            "pane",
            [x0 + 0.05, x0 + 0.19, z + 0.17, z + 0.29],
            0.13,
            srgb("#d8d8cc"),
        )?;
        conduit(0.011, settings)?.run(
            d,
            &[
                [x0 + 0.12, 0.03, z + 0.34],
                [x0 + 0.12, 0.03, z + 1.2 + i as f64 * 0.07],
            ],
            Some(0.),
        )?;
    }
    Ok(())
}

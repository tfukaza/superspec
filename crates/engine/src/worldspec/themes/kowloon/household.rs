//! Household fixtures placed in their owner's facade frame, in meters.
use super::pipes::{Options as PipeOptions, PipeSystem};
use super::{
    balcony_props::{self, Plant},
    fixture::Fixture,
    graphics_detail::GraphicsDetail,
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
    drying_rack_with(d, span, z, depth, rng, settings, textured, 1.)
}
/// `plain` at weight 1 and `bold` at weight 2.
fn weighed(weight: f64, plain: f64, bold: f64) -> f64 {
    plain + (bold - plain) * (weight - 1.)
}
fn validate_weight(weight: f64) -> Result<(), String> {
    if weight.is_finite() && (1. ..=3.).contains(&weight) {
        Ok(())
    } else {
        Err("Drying hardware weight must lie in 1..3.".into())
    }
}
/// [`drying_rack`] with arms, poles and pegs of `weight` (1 their own size, 2 the weight of
/// a drawing; 1–3).
#[allow(clippy::too_many_arguments)]
pub fn drying_rack_with(
    d: &mut Fixture<'_, '_>,
    span: [f64; 2],
    z: f64,
    depth: f64,
    rng: &mut SeededRng,
    settings: Option<&weathered::Settings>,
    textured: bool,
    weight: f64,
) -> Result<(), String> {
    validate_weight(weight)?;
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
        frame.bar(
            "metal",
            [x, 0., z],
            [x, depth, z + 0.04],
            weighed(weight, 0.025, 0.036),
            steel,
        )?;
        // The brace under each arm goes at level 2.
        if frame.mesh.graphics_detail() != GraphicsDetail::VeryLow {
            frame.bar(
                "metal",
                [x, 0., z - 0.25],
                [x, depth * 0.7, z + 0.02],
                weighed(weight, 0.018, 0.028),
                steel,
            )?;
        }
    }
    // Heavy poles are fewer: two where three thin ones lay.
    let poles = if depth > 0.5 && weight < 1.5 { 3 } else { 2 };
    let bamboo = weighed(weight, 0.013, 0.017);
    for i in 0..poles {
        let y = depth * (0.35 + 0.65 * i as f64 / (poles - 1) as f64);
        d.tube(
            "paint",
            &[[span[0] - 0.15, y, z + 0.03], [span[1] + 0.15, y, z + 0.03]],
            bamboo,
            srgb("#c8b888"),
            6,
        )?;
        laundry::washing_with(
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
            weight,
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
    drying_pole_with(d, x, z, length, rng, settings, textured, 1.)
}
/// [`drying_pole`] with a pole, socket and hangers of `weight` (as [`drying_rack_with`]).
#[allow(clippy::too_many_arguments)]
pub fn drying_pole_with(
    d: &mut Fixture<'_, '_>,
    x: f64,
    z: f64,
    length: f64,
    rng: &mut SeededRng,
    settings: Option<&weathered::Settings>,
    textured: bool,
    weight: f64,
) -> Result<(), String> {
    validate_weight(weight)?;
    if !position(&[x, z]) || !length.is_finite() || !(0.35..=20.).contains(&length) {
        return Err("Invalid drying pole.".into());
    }
    validate_weathering(settings)?;
    let pen = hardware(d, settings)?;
    d.tube(
        "paint",
        &[[x, 0., z], [x, length, z + length * 0.12]],
        weighed(weight, 0.014, 0.018),
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
        weighed(weight, 0.024, 0.031),
        shade(srgb("#a4a9aa"), 0.6),
        6,
    )?;
    laundry::hangers_with(
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
        weight,
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
    drying_lines_with(d, span, y, rng, settings, textured, 1.)
}
/// [`drying_lines`] with posts, lines and pegs of `weight` (as [`drying_rack_with`]).
pub fn drying_lines_with(
    d: &mut Fixture<'_, '_>,
    span: [f64; 2],
    y: f64,
    rng: &mut SeededRng,
    settings: Option<&weathered::Settings>,
    textured: bool,
    weight: f64,
) -> Result<(), String> {
    validate_weight(weight)?;
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
        frame.bar(
            "metal",
            [x, y, 0.],
            [x, y, 1.9],
            weighed(weight, 0.04, 0.05),
            srgb("#a4a9aa"),
        )?;
        frame.bar(
            "metal",
            [x, y - 0.4, 1.9],
            [x, y + 0.4, 1.9],
            weighed(weight, 0.03, 0.04),
            srgb("#a4a9aa"),
        )?;
    }
    for offset in [-0.35, 0., 0.35] {
        frame.bar(
            "paint",
            [span[0], y + offset, 1.88],
            [span[1], y + offset, 1.88],
            weighed(weight, 0.008, 0.014),
            srgb("#2a2927"),
        )?;
        laundry::washing_with(
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
            weight,
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
/// What a plan fixes of a cage instead of leaving it to the seed; the default fixes nothing.
#[derive(Clone, Copy, Default)]
pub struct CagePlan {
    /// What a stocked cage holds: `plants`, `washing`, `storage`, `mixed` or `empty`.
    pub contents: Option<&'static str>,
    /// Whether it has its corrugated roof, and that roof's paint (the seed's when `None`).
    pub roof: Option<bool>,
    pub roof_color: Option<V3>,
    /// Thickness of the infill bars in meters (0.012).
    pub bar: Option<f64>,
    /// Thickness of the frame rails and corner posts in meters (0.028, the posts 0.002 more).
    pub rail: Option<f64>,
    /// Spacing of the infill bars in meters (0.11; the floor's runners stay 0.2 apart).
    pub spacing: Option<f64>,
    /// The most rust (0–1) mixed into an unbaked cage's paint; the seed picks up to it (0.35).
    pub rust: Option<f64>,
    /// The height of the middle rail above the cage's floor in meters (half its height).
    pub middle: Option<f64>,
    /// `[x0, x1]`: where the front's middle rail is left out, over what stands behind it; it
    /// stops at the infill bars either side.
    pub middle_gap: Option<[f64; 2]>,
    /// A cage of few heavy bars: the infill bars share each side evenly at about `spacing`,
    /// and the floor's runners and the wall stays are of the bars' weight instead of 9 and 12 mm.
    pub bold: bool,
    /// The cage's own colours where its stream shapes other cages too: how much rust is in
    /// its paint (0–0.35), and the draw (0–1) and rust (0–0.6) of a seeded roof's sheet.
    pub tints: Option<[f64; 3]>,
}
impl CagePlan {
    fn validate(&self) -> Result<(), String> {
        let within = |value: Option<f64>, range: std::ops::RangeInclusive<f64>| {
            value.is_none_or(|v| range.contains(&v))
        };
        if within(self.bar, 0.004..=0.05)
            && within(self.rail, 0.008..=0.08)
            && within(self.spacing, 0.04..=0.5)
            && within(self.rust, 0. ..=1.)
            && within(self.middle, 0.05..=10.)
            && self
                .middle_gap
                .is_none_or(|[a, b]| a.is_finite() && b.is_finite() && a < b)
        {
            Ok(())
        } else {
            Err("A window cage's bars are 4–50 mm, its rails 8–80 mm, its bars 0.04–0.5 m apart and its rust 0–1.".into())
        }
    }
}
/// Welded enclosure, fixing hardware, optional corrugated roof and seeded household contents.
pub fn cage(
    d: &mut Fixture<'_, '_>,
    c: &Cage,
    rng: &mut SeededRng,
    settings: Option<&weathered::Settings>,
    textured: bool,
) -> Result<(), String> {
    planned_cage(d, c, rng, settings, textured, &CagePlan::default())
}
/// [`cage`] with whatever `plan` fixes of it; an empty plan draws the seeded cage.
pub fn planned_cage(
    d: &mut Fixture<'_, '_>,
    c: &Cage,
    rng: &mut SeededRng,
    settings: Option<&weathered::Settings>,
    textured: bool,
    plan: &CagePlan,
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
    plan.validate()?;
    let oxidation = rng.float(0., 0.35);
    let oxidation = plan.tints.map_or(oxidation, |tints| tints[0]);
    let oxidation = plan.rust.map_or(oxidation, |most| oxidation / 0.35 * most);
    let paint = metal_color(c.color, oxidation, settings);
    let bar = plan.bar.unwrap_or(0.012);
    let frame_rail = plan.rail.unwrap_or(0.028);
    let post = plan.rail.map_or(0.03, |r| r + 0.002);
    let pitch = plan.spacing.unwrap_or(0.11);
    // A bold cage shares its bars evenly, so no narrow gap is left at an end.
    let even = |length: f64| length / (length / pitch).round().max(1.);
    let (pitch, side_pitch) = if plan.bold {
        (even(x1 - x0), even(depth))
    } else {
        (pitch, pitch)
    };
    let (runner, runner_pitch, cross_pitch, stay) = if plan.bold {
        (
            bar,
            side_pitch,
            pitch * (0.2 / pitch).ceil().max(1.),
            [0.04, 0.02],
        )
    } else {
        (0.009, 0.11, 0.2, [0.025, 0.012])
    };
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
    // A reduced level keeps the cage's massing: its frame, posts and a share of its infill
    // bars. The wall plates are a centimetre thick and go at both; the stays, the floor's
    // runners and its cross bars go at level 2.
    let detail = frame.mesh.graphics_detail();
    let fixed = !detail.reduced();
    let floored = detail != GraphicsDetail::VeryLow;
    let stride = if plan.bold {
        detail.pick(1, 1, 2)
    } else {
        detail.pick(1, 2, 4)
    };
    for x in [x0 + width / 2., x1 - width / 2.] {
        for z in [z0 + height / 2., z1 - height / 2.] {
            if fixed {
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
            }
            if !floored {
                continue;
            }
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
                stay,
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
    let mut y = runner_pitch / 2.;
    let mut index = 0;
    while y < depth {
        if floored && index % stride == 0 {
            rail([x0, y, z0], [x1, y, z0], runner)?;
        }
        index += 1;
        y += runner_pitch;
    }
    let middle = plan
        .middle
        .map_or((z0 + z1) / 2., |m| (z0 + m).min(z1 - 0.03));
    for z in [z0, middle, z1] {
        match plan.middle_gap {
            Some([a, b]) if z == middle => {
                // Stop either side at the nearest infill bar outside the gap.
                let left = x0 + ((a - x0) / pitch).floor().max(0.) * pitch;
                let right = x0 + ((b - x0) / pitch).ceil().max(0.) * pitch;
                if left > x0 + 0.01 {
                    rail([x0, depth, z], [left, depth, z], frame_rail)?;
                }
                if right < x1 - 0.05 {
                    rail([right, depth, z], [x1, depth, z], frame_rail)?;
                }
            }
            _ => rail([x0, depth, z], [x1, depth, z], frame_rail)?,
        }
        for x in [x0, x1] {
            rail([x, 0., z], [x, depth, z], frame_rail)?;
        }
    }
    for x in [x0, x1] {
        rail([x, depth, z0], [x, depth, z1], post)?;
    }
    let mut x = x0 + pitch;
    let mut index = 0;
    while x < x1 - 0.04 {
        // Counted from the second bar, so a cage of two bars keeps one.
        if (index + 1) % stride == 0 {
            rail([x, depth, z0], [x, depth, z1], bar)?;
        }
        index += 1;
        x += pitch;
    }
    for x in [x0, x1] {
        let mut y = side_pitch;
        let mut index = 0;
        while y < depth - 0.03 {
            if (index + 1) % stride == 0 {
                rail([x, y, z0], [x, y, z1], bar)?;
            }
            index += 1;
            y += side_pitch;
        }
    }
    let mut x = x0 + cross_pitch;
    while x < x1 - 0.1 {
        if floored && !detail.reduced() {
            rail([x, 0., z0], [x, depth, z0], bar)?;
        }
        x += cross_pitch;
    }
    // A planned roof draws from its own stream, so the contents stay the seed's.
    let roofed = match plan.roof {
        Some(roofed) => roofed,
        None => rng.chance(0.55),
    };
    if roofed {
        let roof = if plan.roof.is_some() {
            plan.roof_color.unwrap_or(srgb("#b9c79a"))
        } else if let Some([_, pick, rust]) = plan.tints {
            if pick < 0.5 {
                srgb("#b9c79a")
            } else {
                metal_color(srgb("#a4a9aa"), rust, settings)
            }
        } else if rng.chance(0.5) {
            srgb("#b9c79a")
        } else {
            metal_color(srgb("#a4a9aa"), rng.float(0., 0.6), settings)
        };
        let corners = [
            [x0 - 0.05, 0., z1 + 0.14],
            [x1 + 0.05, 0., z1 + 0.14],
            [x1 + 0.05, depth + 0.1, z1 + 0.02],
            [x0 - 0.05, depth + 0.1, z1 + 0.02],
        ];
        if detail.reduced() && settings.is_none() {
            // The ribs are a few millimetres deep: an unbaked roof is one flat sheet.
            d.sheet("sheet", &corners, roof)?;
        } else {
            corrugated::draw(d, "sheet", corners, 0.09, roof, settings)?;
        }
    }
    if !c.stocked {
        return Ok(());
    }
    let content = rng.pick(&["plants", "plants", "washing", "storage", "mixed"]);
    let content = plan.contents.unwrap_or(content);
    if content == "storage" && plan.contents.is_some() {
        // Planned storage is cartons: stacked one or two high along the cage, taped shut.
        let mut x = x0 + 0.05;
        while x < x1 - 0.3 {
            d.mesh.spend_work(1)?;
            let size = rng.float(0.3, 0.42).min(depth - 0.12);
            let mut z = z0 + 0.012;
            for _ in 0..rng.integer(1, 2) {
                let high = size * rng.float(0.75, 1.);
                if z + high > z1 - 0.1 {
                    break;
                }
                let y = depth - 0.07 - size - rng.float(0., 0.05);
                let card = shade(srgb("#b08a5c"), rng.float(0.8, 1.15));
                d.cuboid(
                    "paint",
                    [x, y, z],
                    [x + size, y + size, z + high],
                    card,
                    "d",
                )?;
                if !detail.reduced() {
                    d.cuboid(
                        "paint",
                        [x + size * 0.44, y - 0.002, z + high - 0.002],
                        [x + size * 0.56, y + size + 0.002, z + high + 0.002],
                        shade(card, 0.72),
                        "d",
                    )?;
                }
                z += high + 0.004;
            }
            x += size + rng.float(0.03, 0.12);
        }
        return Ok(());
    }
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
    guarded_wall_lamp(d, x, z, settings, None)
}
fn validate_guard(guard: Option<f64>) -> Result<(), String> {
    if guard.is_none_or(|wire| (0.004..=0.04).contains(&wire)) {
        Ok(())
    } else {
        Err("A wall lamp's guard is of 4–40 mm wire.".into())
    }
}
/// [`wall_lamp`] whose guard bars are `guard` m thick (8 mm when `None`).
pub fn guarded_wall_lamp(
    d: &mut Fixture<'_, '_>,
    x: f64,
    z: f64,
    settings: &weathered::Settings,
    guard: Option<f64>,
) -> Result<(), String> {
    if !position(&[x, z]) {
        return Err("Invalid wall lamp position.".into());
    }
    settings.validate()?;
    validate_guard(guard)?;
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
    // The guard's two bars are wire: a reduced level shows the glass.
    let guarded = !d.mesh.graphics_detail().reduced();
    for bar in [-0.03, 0.03].into_iter().filter(|_| guarded) {
        Fixture {
            pen: pen.clone(),
            mesh: d.mesh,
            repeats: d.repeats,
        }
        .bar(
            "metal",
            [x + bar, 0.128, z + 0.015],
            [x + bar, 0.128, z + 0.115],
            guard.unwrap_or(0.008),
            shade(srgb("#a4a9aa"), 0.4),
        )?;
    }
    conduit(0.012, settings)?.run(d, &[[x, 0.02, z + 0.13], [x, 0.02, z + 0.75]], Some(0.))
}
/// A round bulkhead lamp: a domed glass in a wire cage of three hoops and a ring, on a round
/// wall plate, its foot at `z`, the conduit rising from its head. The wall frame's +Y is out.
pub fn round_wall_lamp(
    d: &mut Fixture<'_, '_>,
    x: f64,
    z: f64,
    settings: &weathered::Settings,
) -> Result<(), String> {
    guarded_round_wall_lamp(d, x, z, settings, None)
}
/// [`round_wall_lamp`] in a cage of `guard` m wire: a ring and two hoops instead of three of
/// 12 mm.
pub fn guarded_round_wall_lamp(
    d: &mut Fixture<'_, '_>,
    x: f64,
    z: f64,
    settings: &weathered::Settings,
    guard: Option<f64>,
) -> Result<(), String> {
    if !position(&[x, z]) {
        return Err("Invalid wall lamp position.".into());
    }
    settings.validate()?;
    validate_guard(guard)?;
    // Sizes in units of `k` m: a plate 26 cm across under a glass 17 cm across.
    let k = 1.2;
    let (plate, centre) = (0.11 * k, z + 0.115 * k);
    // A frame whose +Z points out of the wall, centred on the lamp.
    let out =
        super::shop::forms::rotated(&d.pen, [x, 0., centre], 0, -std::f64::consts::FRAC_PI_2)?;
    let mut lamp = Fixture {
        pen: out,
        mesh: d.mesh,
        repeats: d.repeats,
    };
    let metal = hardware(&mut lamp, Some(settings))?;
    let housing = shade(srgb("#a4a9aa"), 0.5);
    let wire = shade(srgb("#a4a9aa"), 0.4);
    // A reduced level turns the plate and the glass in fewer sides and rows; level 1 bends
    // the cage of fewer chords and level 2, where its wire would be a hairline, leaves it.
    let detail = lamp.mesh.graphics_detail();
    let (plate_sides, dome_sides) = (detail.pick(24, 10, 6), detail.pick(20, 8, 6));
    let (ring_steps, hoop_steps) = (detail.pick(24, 8, 0), detail.pick(10, 4, 0));
    {
        let mut m = Fixture {
            pen: metal.clone(),
            mesh: lamp.mesh,
            repeats: lamp.repeats,
        };
        super::shop::forms::lathe(
            &mut m,
            "metal",
            [0.; 3],
            &[
                [plate, 0.],
                [plate, 0.014 * k],
                [plate - 0.012 * k, 0.026 * k],
            ],
            &super::shop::forms::Tints::Uniform(&Tint::Color(housing)),
            plate_sides,
            None,
        )?;
        m.disc(
            "metal",
            [0., 0., 0.026 * k],
            plate - 0.012 * k,
            [0., 0., 1.],
            housing,
            plate_sides,
        )?;
    }
    // The glass: a low dome from the plate, closed by a small disc at its crown.
    let dome = [
        [0.072, 0.026],
        [0.072, 0.05],
        [0.066, 0.078],
        [0.052, 0.1],
        [0.03, 0.114],
        [0.014, 0.118],
    ]
    .map(|p| p.map(|v| v * k));
    let rows: &[usize] = match detail {
        GraphicsDetail::Full => &[0, 1, 2, 3, 4, 5],
        GraphicsDetail::Low => &[0, 2, 4, 5],
        GraphicsDetail::VeryLow => &[0, 3, 5],
    };
    let dome: Vec<[f64; 2]> = rows.iter().map(|row| dome[*row]).collect();
    super::shop::forms::lathe(
        &mut lamp,
        "pane",
        [0.; 3],
        &dome,
        &super::shop::forms::Tints::Uniform(&Tint::Color(srgb("#f4e8c0"))),
        dome_sides,
        None,
    )?;
    lamp.disc(
        "pane",
        [0., 0., 0.118 * k],
        0.014 * k,
        [0., 0., 1.],
        srgb("#f4e8c0"),
        10,
    )?;
    let mut m = Fixture {
        pen: metal,
        mesh: lamp.mesh,
        repeats: lamp.repeats,
    };
    // A ring round the dome's foot and three hoops over its crown.
    let ring: Vec<V3> = (0..=ring_steps.max(1))
        .map(|i| {
            let a = i as f64 / ring_steps.max(1) as f64 * std::f64::consts::TAU;
            [
                0.084 * k * superspec_geometry::math::cos(a),
                0.084 * k * superspec_geometry::math::sin(a),
                0.034 * k,
            ]
        })
        .collect();
    let (ring_radius, hoop_radius, hoops) = match guard {
        Some(wire) => (wire / 2., wire * 0.45, 2),
        None => (0.006 * k, 0.005 * k, 3),
    };
    if ring_steps > 0 {
        m.tube("metal", &ring, ring_radius, wire, 6)?;
    }
    for hoop in (0..hoops).filter(|_| hoop_steps > 0) {
        let bearing = hoop as f64 * std::f64::consts::PI / hoops as f64;
        let (c, s) = (
            superspec_geometry::math::cos(bearing),
            superspec_geometry::math::sin(bearing),
        );
        let hoop: Vec<V3> = (0..=hoop_steps)
            .map(|i| {
                let a = i as f64 / hoop_steps as f64 * std::f64::consts::PI;
                let (r, h) = (
                    0.084 * k * superspec_geometry::math::cos(a),
                    (0.034 + 0.096 * superspec_geometry::math::sin(a)) * k,
                );
                [r * c, r * s, h]
            })
            .collect();
        m.tube("metal", &hoop, hoop_radius, wire, 6)?;
    }
    conduit(0.012, settings)?.run(
        d,
        &[[x, 0.02, centre + plate], [x, 0.02, centre + 0.75]],
        Some(0.),
    )
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
    meter_bank_with(d, x, z, count, rng, settings, 1.)
}
/// [`meter_bank`] with conduits and their clips of `weight` (as [`drying_rack_with`]).
pub fn meter_bank_with(
    d: &mut Fixture<'_, '_>,
    x: f64,
    z: f64,
    count: usize,
    rng: &mut SeededRng,
    settings: &weathered::Settings,
    weight: f64,
) -> Result<(), String> {
    validate_weight(weight)?;
    if !position(&[x, z]) || count > 64 {
        return Err("Invalid meter bank position or count.".into());
    }
    settings.validate()?;
    let pen = hardware(d, Some(settings))?;
    let body = shade(srgb("#a4a9aa"), rng.float(0.5, 0.9));
    // Meters and their conduits are street furniture of the wall's foot: level 2 draws none,
    // and level 1 the boxes with their glass and no raised door.
    let detail = d.mesh.graphics_detail();
    if detail == GraphicsDetail::VeryLow {
        return Ok(());
    }
    for i in 0..count {
        let x0 = x + i as f64 * 0.3;
        let mut frame = Fixture {
            pen: pen.clone(),
            mesh: d.mesh,
            repeats: d.repeats,
        };
        frame.cuboid("metal", [x0, 0., z], [x0 + 0.24, 0.12, z + 0.34], body, "b")?;
        if !detail.reduced() {
            frame.cuboid(
                "metal",
                [x0 + 0.03, 0.12, z + 0.15],
                [x0 + 0.21, 0.128, z + 0.31],
                shade(body, 0.7),
                "b",
            )?;
        }
        d.panel(
            "pane",
            [x0 + 0.05, x0 + 0.19, z + 0.17, z + 0.29],
            0.13,
            srgb("#d8d8cc"),
        )?;
        conduit(weighed(weight, 0.011, 0.0145), settings)?
            .weighted(weight)?
            .run(
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

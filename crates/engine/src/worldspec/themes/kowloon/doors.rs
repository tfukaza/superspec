//! Seeded closed entrances with modeled rails, hardware, glass and independently seeded repairs.
use super::{
    fixture::Fixture,
    frosted,
    metal::Kind,
    painted::{self, Surface},
    palette::{shade, srgb},
    pen::{Pen, Tint},
    pipes::{Options as PipeOptions, PipeSystem},
    posters, timber,
    wall::finish_seed,
    weathered::{self, Mapping},
};
use serde::{Deserialize, Serialize};
use superspec_geometry::{
    V3,
    math::{cos, hypot, sin},
};
use superspec_types::rng::SeededRng;
const DOORS: [&str; 8] = [
    "#2f5f4a", "#6a2c28", "#2f4f7a", "#5a4030", "#8a8f92", "#3a3c40", "#a05a2c", "#3f7f7a",
];
const WOODS: [&str; 3] = ["#705039", "#72523b", "#6e503a"];
#[derive(Clone, Deserialize)]
pub struct Settings {
    pub weathering: weathered::Settings,
    pub timber: timber::Settings,
    pub trim: V3,
    pub detail: usize,
    pub posters: bool,
}
#[derive(Clone, Deserialize, Serialize)]
pub struct Unit {
    pub u0: f64,
    pub u1: f64,
    pub height: f64,
    pub key: String,
}
impl Unit {
    fn validate(&self) -> Result<(), String> {
        if [self.u0, self.u1, self.height]
            .iter()
            .any(|v| !v.is_finite() || v.abs() > 1000.)
            || !(1.5..=100.).contains(&(self.u1 - self.u0))
            || !(2.2..=50.).contains(&self.height)
            || self.key.len() > 2048
        {
            return Err(
                "Entrance requires a bounded unit at least 1.5 m wide and 2.2 m high.".into(),
            );
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Bounds {
    pub double: bool,
    pub w: f64,
    pub h: f64,
    pub x0: f64,
    pub x1: f64,
}
/// The leaf construction a plan may name instead of leaving it to the seed.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Style {
    /// One painted panelled leaf.
    Single,
    /// Two painted panelled leaves.
    Double,
    /// One timber leaf with a glazed upper half.
    Glazed,
    /// One pressed steel leaf.
    Steel,
}
/// The light over the door a plan may name.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Transom {
    Glazed,
    /// An open grid of thin bars over the dark of the room.
    Mesh,
    None,
}
/// What a plan fixes of an entrance; the default leaves everything to the seed.
#[derive(Clone, Copy, Debug, Default)]
pub struct Design {
    pub style: Option<Style>,
    pub paint: Option<V3>,
    pub transom: Option<Transom>,
    /// The head height of a wall opening as wide as the unit, which the entrance then fills:
    /// its frame reaches the unit's edges and its transom the head.
    pub fit: Option<f64>,
    /// How many pressed panels a painted or steel leaf has, one above another (1 to 4; three
    /// when left to the door).
    pub panels: Option<usize>,
    /// The lamp over the door is a round caged bulkhead instead of the boxed one.
    pub round_lamp: bool,
}
pub fn entrance_bounds(unit: &Unit, rng: &mut SeededRng) -> Result<Bounds, String> {
    planned_bounds(unit, rng, &Design::default())
}
/// [`entrance_bounds`] under a plan; it draws from `rng` exactly as the seeded bounds do.
pub fn planned_bounds(unit: &Unit, rng: &mut SeededRng, design: &Design) -> Result<Bounds, String> {
    unit.validate()?;
    let span = unit.u1 - unit.u0;
    let double = span > 2.3 && rng.chance(0.4);
    let w = if double { 1.4 } else { rng.float(0.85, 1.) };
    let x0 = unit.u0 + 0.3_f64.max((span - w) * rng.pick(&[0.25, 0.5, 0.75]));
    let planned = design.style.map(|style| style == Style::Double);
    let (double, w, x0) = match (planned, design.fit) {
        (None, None) => (double, w, x0),
        (planned, fit) => {
            let double = planned.unwrap_or(double);
            let w = match (double, fit) {
                (true, Some(_)) => span - 0.2,
                (true, None) => 1.4_f64.min(span - 0.2),
                (false, Some(_)) => (span - 0.2).min(1.05),
                (false, None) if planned == Some(false) && w > 1. => 0.92,
                (false, None) => w,
            };
            // A planned door stands where the seed put it when the unit has room, else centred.
            let centred = fit.is_some() || x0 + w > unit.u1 - 0.1;
            (
                double,
                w,
                if centred {
                    unit.u0 + (span - w) / 2.
                } else {
                    x0
                },
            )
        }
    };
    Ok(Bounds {
        double,
        w,
        h: 2.05,
        x0,
        x1: x0 + w,
    })
}
fn rng(settings: &Settings, key: &str) -> SeededRng {
    SeededRng::from_text(&format!("{}/{key}", settings.weathering.seed))
}
fn paint_surface(
    settings: &Settings,
    group: String,
    finish: painted::Settings,
    density: f64,
) -> Surface {
    Surface {
        recipe: painted::Recipe {
            finish,
            binding: settings.weathering.paint.clone(),
        },
        group,
        density,
    }
}
/// Door front is +Y; the timber atlas's contact side is -Y.
pub fn timber_board(
    d: &mut Fixture<'_, '_>,
    low: V3,
    high: V3,
    tint: V3,
    settings: &Settings,
) -> Result<(), String> {
    if !settings.weathering.wear.is_finite()
        || !(0. ..=1.).contains(&settings.weathering.wear)
        || settings.timber.slots.len() > 64
        || settings.timber.slots.iter().map(String::len).sum::<usize>() > 16384
    {
        return Err("Door timber requires normalized wear and bounded finish identity.".into());
    }
    let mut timber = settings.timber.clone();
    timber.grain_variant = Some(1);
    timber.front_wear = Some(timber::FrontWear {
        width: 0.008,
        strength: (settings.weathering.wear * 4.).min(1.2),
        direction: None,
    });
    let pen = d
        .pen
        .facing([high[0], high[1], low[2]], [-1., 0., 0.], [0., -1., 0.])?
        .with_timber(timber)?;
    Fixture {
        pen,
        mesh: d.mesh,
        repeats: d.repeats,
    }
    .cuboid(
        "timber",
        [0.; 3],
        [high[0] - low[0], high[1] - low[1], high[2] - low[2]],
        tint,
        "f",
    )
}
pub fn pull_handle(
    d: &mut Fixture<'_, '_>,
    x: f64,
    z: f64,
    depth: f64,
    color: V3,
) -> Result<(), String> {
    if [x, z, depth]
        .iter()
        .any(|v| !v.is_finite() || v.abs() > 10000.)
        || (!d.mesh.surveying() && color.iter().any(|v| !v.is_finite() || *v < 0.))
    {
        return Err("Invalid door handle geometry or tint.".into());
    }
    for height in [z, z + 0.23] {
        d.disc(
            "metal",
            [x, depth + 0.002, height],
            0.024,
            [0., 1., 0.],
            color,
            8,
        )?;
    }
    let path = [
        [depth, z],
        [depth + 0.025, z],
        [depth + 0.04, z + 0.015],
        [depth + 0.043, z + 0.035],
        [depth + 0.043, z + 0.195],
        [depth + 0.04, z + 0.215],
        [depth + 0.025, z + 0.23],
        [depth, z + 0.23],
    ];
    let rings: Vec<_> = path
        .iter()
        .enumerate()
        .map(|(i, [y, height])| {
            let before = path[i.saturating_sub(1)];
            let after = path[(i + 1).min(7)];
            let dy = after[0] - before[0];
            let dz = after[1] - before[1];
            let length = hypot(dy, dz);
            std::array::from_fn::<_, 8, _>(|side| {
                let angle = side as f64 * std::f64::consts::PI / 4.;
                let normal = [
                    cos(angle),
                    -dz / length * sin(angle),
                    dy / length * sin(angle),
                ];
                (
                    [
                        x + normal[0] * 0.009,
                        y + normal[1] * 0.009,
                        height + normal[2] * 0.009,
                    ],
                    normal,
                )
            })
        })
        .collect();
    for ring in 0..7 {
        for side in 0..8 {
            let next = (side + 1) % 8;
            let corners = [
                rings[ring][side],
                rings[ring + 1][side],
                rings[ring + 1][next],
                rings[ring][next],
            ];
            let hint = std::array::from_fn(|axis| corners.iter().map(|c| c.1[axis]).sum());
            d.pen.face(
                d.mesh,
                "metal",
                &corners.map(|c| c.0),
                hint,
                &Tint::Color(color),
                d.repeat("metal"),
                None,
                Some(&corners.map(|c| c.1)),
            )?;
        }
    }
    Ok(())
}
fn hinges(d: &mut Fixture<'_, '_>, x: f64, inward: f64, paint: V3) -> Result<(), String> {
    let brass = srgb("#a38a57");
    for z in [0.3, 1.05, 1.83] {
        let a = x - inward * 0.023;
        let b = x + inward * 0.045;
        d.cuboid(
            "paint",
            [a.min(b), 0.061, z - 0.05],
            [a.max(b), 0.067, z + 0.05],
            shade(paint, 0.8),
            "b",
        )?;
        d.tube(
            "metal",
            &[[x, 0.071, z - 0.055], [x, 0.071, z + 0.055]],
            0.012,
            brass,
            8,
        )?;
        for dz in [-0.055_f64, 0.055] {
            d.disc(
                "metal",
                [x, 0.071, z + dz],
                0.012,
                [0., 0., dz.signum()],
                brass,
                8,
            )?;
        }
        for dz in [-0.032, 0.032] {
            d.disc(
                "metal",
                [x + inward * 0.031, 0.068, z + dz],
                0.004,
                [0., 1., 0.],
                srgb("#a4a9aa"),
                6,
            )?;
        }
    }
    Ok(())
}
fn pressed(
    d: &mut Fixture<'_, '_>,
    r: [f64; 4],
    paint: V3,
    s: Option<&Surface>,
) -> Result<(), String> {
    let [a, low, b, high] = r;
    let bevel = 0.018;
    let outer = [
        [a, 0.056, low],
        [b, 0.056, low],
        [b, 0.056, high],
        [a, 0.056, high],
    ];
    let inner = [
        [a + bevel, 0.035, low + bevel],
        [b - bevel, 0.035, low + bevel],
        [b - bevel, 0.035, high - bevel],
        [a + bevel, 0.035, high - bevel],
    ];
    for i in 0..4 {
        let n = (i + 1) % 4;
        painted::face(
            d,
            "paint",
            &[outer[i], outer[n], inner[n], inner[i]],
            paint,
            s,
        )?;
    }
    painted::panel(
        d,
        "paint",
        [a + bevel, low + bevel, b - bevel, high - bevel],
        0.035,
        paint,
        s,
    )
}
fn rail(
    d: &mut Fixture<'_, '_>,
    low: V3,
    high: V3,
    paint: V3,
    s: Option<&Surface>,
) -> Result<(), String> {
    let bevel = 0.004;
    d.cuboid(
        "paint",
        low,
        [high[0], high[1] - bevel, high[2]],
        paint,
        "bf",
    )?;
    let outer = [
        [low[0], high[1] - bevel, low[2]],
        [high[0], high[1] - bevel, low[2]],
        [high[0], high[1] - bevel, high[2]],
        [low[0], high[1] - bevel, high[2]],
    ];
    let inner = [
        [low[0] + bevel, high[1], low[2] + bevel],
        [high[0] - bevel, high[1], low[2] + bevel],
        [high[0] - bevel, high[1], high[2] - bevel],
        [low[0] + bevel, high[1], high[2] - bevel],
    ];
    for i in 0..4 {
        let n = (i + 1) % 4;
        painted::face(
            d,
            "paint",
            &[outer[i], outer[n], inner[n], inner[i]],
            paint,
            s,
        )?;
    }
    painted::face(d, "paint", &inner, paint, s)
}
fn glass(
    d: &mut Fixture<'_, '_>,
    bounds: [f64; 4],
    depth: f64,
    key: &str,
    s: &Settings,
) -> Result<(), String> {
    frosted::panel(
        d,
        bounds,
        depth,
        f64::from(finish_seed(&format!("{}/{key}", s.weathering.seed))),
        &format!("{key}/glass"),
        s.weathering.texture_density > 0.,
    )
}
fn timber_leaf(
    d: &mut Fixture<'_, '_>,
    a: f64,
    b: f64,
    paint: V3,
    key: &str,
    s: &Settings,
) -> Result<(), String> {
    d.cuboid(
        "paint",
        [a, 0.018, 0.055],
        [b, 0.028, 2.045],
        shade(paint, 0.6),
        "b",
    )?;
    for x in [a, b - 0.085] {
        timber_board(d, [x, 0.028, 0.06], [x + 0.085, 0.064, 2.045], paint, s)?;
    }
    for [z, h] in [[0.06, 0.12], [0.9, 0.12], [1.96, 0.085]] {
        timber_board(
            d,
            [a + 0.085, 0.028, z],
            [b - 0.085, 0.064, z + h],
            paint,
            s,
        )?;
    }
    let left = a + 0.085;
    let right = b - 0.085;
    let center = (a + b) / 2.;
    glass(d, [left, 1.02, right, 1.96], 0.035, key, s)?;
    timber_board(
        d,
        [center - 0.017, 0.035, 1.02],
        [center + 0.017, 0.062, 1.96],
        paint,
        s,
    )?;
    for z in [1.33, 1.65] {
        timber_board(
            d,
            [left, 0.035, z - 0.014],
            [right, 0.061, z + 0.014],
            paint,
            s,
        )?;
    }
    let boards = (((right - left) / 0.085).round() as usize).max(4);
    for i in 0..boards {
        let x = left + (right - left) * i as f64 / boards as f64;
        let end = left + (right - left) * (i + 1) as f64 / boards as f64;
        timber_board(
            d,
            [x + 0.002, 0.028, 0.18],
            [end - 0.002, 0.047, 0.9],
            shade(paint, 0.97 + (i % 3) as f64 * 0.015),
            s,
        )?;
    }
    if rng(s, &format!("{key}/repair")).chance(0.6) {
        timber_board(
            d,
            [right - 0.105, 0.048, 0.18],
            [right, 0.054, 0.39],
            shade(paint, 1.06),
            s,
        )?;
        for z in [0.2, 0.37] {
            d.disc(
                "metal",
                [right - 0.02, 0.055, z],
                0.005,
                [0., 1., 0.],
                srgb("#a4a9aa"),
                6,
            )?;
        }
    }
    Ok(())
}
pub fn wall_lamp(d: &mut Fixture<'_, '_>, x: f64, z: f64, s: &Settings) -> Result<(), String> {
    let pen = d.pen.with_weathering(
        d.mesh,
        s.weathering.clone(),
        Kind::Frame,
        None,
        vec!["metal".into()],
    )?;
    let mut hardware = Fixture {
        pen,
        mesh: d.mesh,
        repeats: d.repeats,
    };
    hardware.cuboid(
        "metal",
        [x - 0.09, 0., z],
        [x + 0.09, 0.06, z + 0.13],
        shade(srgb("#a4a9aa"), 0.5),
        "b",
    )?;
    let base = d.pen.clone();
    Fixture {
        pen: base.clone(),
        mesh: hardware.mesh,
        repeats: hardware.repeats,
    }
    .cuboid(
        "pane",
        [x - 0.07, 0.06, z + 0.015],
        [x + 0.07, 0.125, z + 0.115],
        srgb("#f4e8c0"),
        "b",
    )?;
    for bar in [-0.03, 0.03] {
        hardware.bar(
            "metal",
            [x + bar, 0.128, z + 0.015],
            [x + bar, 0.128, z + 0.115],
            0.008,
            shade(srgb("#a4a9aa"), 0.4),
        )?;
    }
    PipeSystem::new(PipeOptions {
        radius: 0.012,
        color: srgb("#2a2927"),
        sides: 16,
        variant: "pipe-standard".into(),
        weathering: Some(s.weathering.clone()),
    })?
    .run(
        &mut Fixture {
            pen: base,
            mesh: hardware.mesh,
            repeats: hardware.repeats,
        },
        &[[x, 0.02, z + 0.13], [x, 0.02, z + 0.75]],
        Some(0.),
    )
}
struct Frame<'a> {
    glazed: bool,
    steel: bool,
    paint: V3,
    surface: Option<Surface>,
    metal: Pen,
    settings: &'a Settings,
}
impl Frame<'_> {
    fn draw(
        &self,
        d: &mut Fixture<'_, '_>,
        low: V3,
        high: V3,
        omit: &str,
        tint: V3,
    ) -> Result<(), String> {
        if self.glazed {
            timber_board(d, low, high, shade(self.paint, 0.98), self.settings)
        } else if self.steel {
            Fixture {
                pen: self.metal.clone(),
                mesh: d.mesh,
                repeats: d.repeats,
            }
            .cuboid("metal", low, high, tint, omit)
        } else {
            painted::cuboid(d, "paint", low, high, tint, omit, self.surface.as_ref())
        }
    }
}
pub fn entrance(
    d: &mut Fixture<'_, '_>,
    unit: &Unit,
    s: &Settings,
    design: &mut SeededRng,
) -> Result<Bounds, String> {
    planned_entrance(d, unit, s, design, &Design::default())
}
/// [`entrance`] with whatever `plan` fixes of it; an empty plan draws the seeded entrance.
pub fn planned_entrance(
    d: &mut Fixture<'_, '_>,
    unit: &Unit,
    s: &Settings,
    design: &mut SeededRng,
    plan: &Design,
) -> Result<Bounds, String> {
    unit.validate()?;
    if plan
        .paint
        .is_some_and(|paint| paint.iter().any(|v| !v.is_finite() || *v < 0.))
        || plan
            .fit
            .is_some_and(|head| !head.is_finite() || !(2.3..=unit.height).contains(&head))
    {
        return Err("A planned entrance needs a finite paint and a head within its storey.".into());
    }
    if plan.panels.is_some_and(|count| !(1..=4).contains(&count)) {
        return Err("A planned leaf has one to four panels.".into());
    }
    let w = &s.weathering;
    if w.seed.len() > 2048
        || s.detail > 2
        || [w.wear, w.grime, w.rust]
            .iter()
            .any(|v| !v.is_finite() || !(0. ..=1.).contains(v))
        || !w.texture_density.is_finite()
        || !(0. ..=4096.).contains(&w.texture_density)
        || s.trim.iter().any(|v| !v.is_finite() || *v < 0.)
        || [&w.metal, &w.sheet, &w.paint].iter().any(|b| {
            b.material.is_empty()
                || b.material.len() > 256
                || !b.repeat.is_finite()
                || b.repeat <= 0.
                || b.repeat > 10000.
        })
        || s.timber.slots.len() > 64
        || s.timber.slots.iter().map(String::len).sum::<usize>() > 16384
    {
        return Err("Invalid entrance appearance settings.".into());
    }
    // Admit the owned finish before consuming the shared design stream.
    d.pen.with_timber(s.timber.clone())?;
    let bounds = planned_bounds(unit, design, plan)?;
    let Bounds {
        double,
        w: width,
        h,
        x0,
        x1,
    } = bounds;
    let selected = srgb(design.pick(&DOORS));
    design.float(0., 0.2);
    let steel = design.chance(0.35);
    let steel = plan.style.map_or(steel, |style| style == Style::Steel);
    let glazed = plan
        .style
        .map_or(!steel && !double, |style| style == Style::Glazed);
    let paint = if glazed {
        srgb(rng(s, &format!("{}/wood", unit.key)).pick(&WOODS))
    } else {
        selected
    };
    let paint = plan.paint.unwrap_or(paint);
    let transom = match (plan.transom, plan.fit) {
        (Some(Transom::None), _) => 0.,
        (_, Some(head)) if head - h - 0.1 >= 0.15 => (head - h - 0.1).min(0.9),
        (None, _) if unit.height - h <= 0.95 => 0.,
        _ => 0.36,
    };
    let meshed = plan.transom == Some(Transom::Mesh);
    let tint = shade(paint, 0.8);
    let frame_surface = if w.texture_density > 0. {
        Some(paint_surface(
            s,
            format!("{}/door/frame", unit.key),
            painted::Settings {
                bounds: [x0 - 0.09, 0., x1 + 0.09, h + transom + 0.09],
                paint: tint,
                substrate: srgb("#80756a"),
                seed: f64::from(finish_seed(&format!("{}/{}/frame", w.seed, unit.key))),
                wear: w.wear * 0.8,
                grime: w.grime,
                handle: None,
                panels: vec![],
            },
            w.texture_density.max(128.),
        ))
    } else {
        None
    };
    let metal = d.pen.with_weathering(
        d.mesh,
        w.clone(),
        Kind::Frame,
        None,
        vec!["paint".into(), "metal".into()],
    )?;
    let frame = Frame {
        glazed,
        steel,
        paint,
        surface: frame_surface,
        metal,
        settings: s,
    };
    d.panel(
        "paint",
        [x0 - 0.015, x1 + 0.015, 0.045, h + transom],
        0.009,
        srgb("#2a2927"),
    )?;
    for x in [x0 - 0.085, x1] {
        frame.draw(
            d,
            [x, 0., 0.05],
            [x + 0.085, 0.085, h + transom],
            "bd",
            tint,
        )?;
        frame.draw(
            d,
            [x - 0.015, 0., 0.05],
            [x + 0.1, 0.028, h + transom + 0.1],
            "bf",
            shade(tint, 0.8),
        )?;
    }
    frame.draw(
        d,
        [x0 - 0.085, 0., h + transom],
        [x1 + 0.085, 0.085, h + transom + 0.085],
        "b",
        tint,
    )?;
    d.cuboid(
        "wall",
        [x0 - 0.12, 0., 0.],
        [x1 + 0.12, 0.13, 0.045],
        shade(s.trim, 0.75),
        "bd",
    )?;
    d.cuboid(
        "metal",
        [x0, 0.015, 0.045],
        [x1, 0.085, 0.055],
        shade(srgb("#a4a9aa"), 0.7),
        "bd",
    )?;
    let leaves = if double { 2 } else { 1 };
    for leaf in 0..leaves {
        let a = x0 + leaf as f64 * width / leaves as f64 + 0.009;
        let b = x0 + (leaf + 1) as f64 * width / leaves as f64 - 0.009;
        let handle = if leaf == 0 { b - 0.065 } else { a + 0.065 };
        let key = format!("{}/door/{leaf}", unit.key);
        if glazed {
            timber_leaf(d, a, b, paint, &key, s)?;
        } else {
            // The cross rails from the foot up, and the panels between them.
            let rails: Vec<[f64; 2]> = match plan.panels {
                None => vec![[0.055, 0.2], [0.73, 0.83], [1.28, 1.39], [1.94, h - 0.005]],
                // Planned panels: the lowest a little shorter than the rest, 0.1 m rails.
                Some(count) => {
                    let clear = 1.74 - (count - 1) as f64 * 0.1;
                    let each = clear / (count as f64 - 0.18);
                    let mut rails = vec![[0.055, 0.2]];
                    for i in 1..count {
                        let top = 0.2 + each * (i as f64 - 0.18) + (i - 1) as f64 * 0.1;
                        rails.push([top, top + 0.1]);
                    }
                    rails.push([1.94, h - 0.005]);
                    rails
                }
            };
            let panels: Vec<[f64; 4]> = rails
                .windows(2)
                .map(|pair| [a + 0.095, pair[0][1], b - 0.095, pair[1][0]])
                .collect();
            let pen = if steel {
                d.pen.with_weathering(
                    d.mesh,
                    w.clone(),
                    Kind::Sheet,
                    Some(Mapping {
                        key: key.clone(),
                        bounds: [a, 0.055, b, h - 0.005],
                        axes: [[1., 0., 0.], [0., 0., 1.]],
                        offset: [0.; 2],
                        gravity: [0., -1.],
                        paint,
                    }),
                    vec!["paint".into(), "metal".into()],
                )?
            } else {
                d.pen.clone()
            };
            // A planned paint with nothing to bake it is the leaf's plain colour.
            let surface = if !steel && !(plan.paint.is_some() && w.texture_density == 0.) {
                Some(paint_surface(
                    s,
                    key.clone(),
                    painted::Settings {
                        bounds: [a, 0.055, b, h - 0.005],
                        paint,
                        substrate: srgb("#947957"),
                        seed: f64::from(finish_seed(&format!("{}/{key}", w.seed))),
                        wear: (w.wear * 1.2).min(1.),
                        grime: w.grime,
                        handle: Some([handle, 1.12]),
                        panels: panels.clone(),
                    },
                    (w.texture_density * 2.).max(256.),
                ))
            } else {
                None
            };
            let mut f = Fixture {
                pen,
                mesh: d.mesh,
                repeats: d.repeats,
            };
            painted::panel(
                &mut f,
                "paint",
                [a, 0.055, b, h - 0.005],
                0.025,
                paint,
                surface.as_ref(),
            )?;
            for [left, right] in [[a, a + 0.095], [b - 0.095, b]] {
                rail(
                    &mut f,
                    [left, 0.025, 0.055],
                    [right, 0.06, h - 0.005],
                    paint,
                    surface.as_ref(),
                )?;
            }
            for [bottom, top] in rails {
                rail(
                    &mut f,
                    [a + 0.095, 0.025, bottom],
                    [b - 0.095, 0.06, top],
                    paint,
                    surface.as_ref(),
                )?;
            }
            for panel in panels {
                pressed(&mut f, panel, paint, surface.as_ref())?;
            }
        }
        let brass = srgb("#a38a57");
        hinges(
            d,
            if leaf == 0 { a - 0.004 } else { b + 0.004 },
            if leaf == 0 { 1. } else { -1. },
            paint,
        )?;
        pull_handle(d, handle, 1.01, 0.064, brass)?;
        d.disc(
            "metal",
            [handle, 0.068, 0.92],
            0.022,
            [0., 1., 0.],
            brass,
            10,
        )?;
        d.panel(
            "paint",
            [handle - 0.003, handle + 0.003, 0.911, 0.926],
            0.069,
            srgb("#2a2927"),
        )?;
        if glazed {
            d.cuboid(
                "metal",
                [handle - 0.085, 0.065, 0.975],
                [handle + 0.035, 0.074, 1.005],
                brass,
                "b",
            )?;
            d.tube(
                "metal",
                &[[handle - 0.07, 0.084, 0.99], [handle + 0.045, 0.084, 0.99]],
                0.009,
                brass,
                6,
            )?;
        }
    }
    if transom > 0. && meshed {
        frame.draw(d, [x0, 0., h], [x1, 0.075, h + 0.045], "blr", tint)?;
        let wire = shade(paint, 0.55);
        let columns = ((width / 0.085).round() as usize).max(2);
        let rows = (((transom - 0.045) / 0.085).round() as usize).max(2);
        for i in 1..columns {
            let x = x0 + width * i as f64 / columns as f64;
            d.bar(
                "metal",
                [x, 0.04, h + 0.045],
                [x, 0.04, h + transom],
                0.009,
                wire,
            )?;
        }
        for i in 1..rows {
            let z = h + 0.045 + (transom - 0.045) * i as f64 / rows as f64;
            d.bar("metal", [x0, 0.046, z], [x1, 0.046, z], 0.009, wire)?;
        }
    } else if transom > 0. {
        glass(
            d,
            [x0, h + 0.045, x1, h + transom],
            0.029,
            &format!("{}/transom", unit.key),
            s,
        )?;
        frame.draw(d, [x0, 0., h], [x1, 0.075, h + 0.045], "blr", tint)?;
        let count = ((width / 0.28).round() as usize).max(2);
        for i in 1..count {
            let x = x0 + width * i as f64 / count as f64;
            frame.draw(
                d,
                [x - 0.009, 0.03, h + 0.045],
                [x + 0.009, 0.061, h + transom],
                "bdu",
                tint,
            )?;
        }
        frame.draw(d, [x0, 0.03, h + 0.19], [x1, 0.06, h + 0.208], "blr", tint)?;
    }
    if unit.height - h - transom > 0.5 {
        if plan.round_lamp {
            super::household::round_wall_lamp(d, (x0 + x1) / 2., h + transom + 0.1, &s.weathering)?;
        } else {
            wall_lamp(d, (x0 + x1) / 2., h + transom + 0.16, s)?;
        }
    }
    let plate = if x1 + 0.16 < unit.u1 - 0.25 {
        x1 + 0.16
    } else {
        x0 - 0.36
    };
    d.cuboid(
        "paint",
        [plate, 0., 1.55],
        [plate + 0.2, 0.012, 1.7],
        srgb("#2a4f8f"),
        "b",
    )?;
    d.panel(
        "paint",
        [plate + 0.05, plate + 0.15, 1.59, 1.66],
        0.014,
        srgb("#f0ece0"),
    )?;
    let side = if unit.u1 - x1 > x0 - unit.u0 {
        x1 + 0.45
    } else {
        unit.u0 + 0.2
    };
    if s.detail > 0 && (unit.u1 - x1).max(x0 - unit.u0) > 1.3 {
        let mut extras = Fixture {
            pen: d.pen.into_part("fixtures")?,
            mesh: d.mesh,
            repeats: d.repeats,
        };
        for i in 0..6 {
            let bx = side + (i % 3) as f64 * 0.22;
            let bz = 1.1 + (i / 3) as f64 * 0.17;
            extras.cuboid(
                "metal",
                [bx, 0., bz],
                [bx + 0.2, 0.09, bz + 0.15],
                shade(srgb("#a4a9aa"), design.float(0.6, 0.9)),
                "b",
            )?;
            extras.panel(
                "paint",
                [bx + 0.04, bx + 0.16, bz + 0.1, bz + 0.12],
                0.094,
                srgb("#2a2927"),
            )?;
        }
        posters::notices(&mut extras, [side, side + 0.7, 1.5, 2.1], design, s.posters)?;
    }
    Ok(bounds)
}

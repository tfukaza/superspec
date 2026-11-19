//! Ground-story bays; signs are returned for the scene-wide placement survey.
pub use super::doors::Unit;
use super::{
    doors,
    fixture::Fixture,
    household,
    kit::Kit,
    metal::Kind as MetalKind,
    palette::{mix, shade, srgb},
    pen::{Pen, Tint},
    posters, shutters,
    signs::{
        frontage,
        mounts::{Proposal, WallRect},
    },
    wall::Opening,
    windows,
};
use serde::{Deserialize, Serialize};
use superspec_geometry::{
    V3,
    math::{cos, sin},
};
use superspec_types::rng::SeededRng;
#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Shutter,
    Gate,
    Door,
    Shop,
    Service,
}
#[derive(Clone, Deserialize)]
pub struct Style {
    pub frame: V3,
    pub trim: V3,
    pub shutter: V3,
    pub accent: V3,
}
pub struct Settings<'a> {
    pub kit: &'a Kit,
    pub style: &'a Style,
    pub posters: bool,
}
const GOODS: [&str; 6] = [
    "#c0392b", "#e8c43c", "#3a6ea8", "#e8e4d8", "#4f8f5a", "#d98a4a",
];
const SHUTTERS: [&str; 7] = [
    "#8f969a", "#4a6f95", "#4f7a5f", "#7a4038", "#d8d2c0", "#5a6268", "#9a8a5a",
];
fn validate(unit: &Unit) -> Result<(), String> {
    if [unit.u0, unit.u1, unit.height]
        .iter()
        .any(|v| !v.is_finite() || v.abs() > 1000.)
        || !(1.5..=40.).contains(&(unit.u1 - unit.u0))
        || !(2.2..=50.).contains(&unit.height)
        || unit.key.len() > 2048
    {
        return Err("Ground bay requires bounded width, story height and identity.".into());
    }
    Ok(())
}
pub fn opening(kind: Kind, unit: &Unit, rng: &mut SeededRng) -> Result<Option<Opening>, String> {
    validate(unit)?;
    if matches!(kind, Kind::Door) {
        let d = doors::entrance_bounds(unit, rng)?;
        return Ok(Some(Opening {
            u0: d.x0,
            u1: d.x1,
            z0: 0.045,
            z1: d.h + if unit.height - d.h > 0.95 { 0.36 } else { 0. },
        }));
    }
    if matches!(kind, Kind::Service) {
        let width = unit.u1 - unit.u0;
        if width <= 1.6 || unit.height <= 2.6 {
            return Ok(None);
        }
        let x = unit.u0 + width * rng.float(0.3, 0.6);
        return Ok(Some(Opening {
            u0: x,
            u1: x + 0.9_f64.min(width - 1.),
            z0: 1.45,
            z1: 2.2,
        }));
    }
    let inset = match kind {
        Kind::Shutter => 0.22,
        Kind::Gate => 0.25,
        _ => 0.2,
    };
    let head = if matches!(kind, Kind::Shop) {
        2.5_f64.min(unit.height - 0.85)
    } else {
        (if matches!(kind, Kind::Shutter) {
            2.7_f64
        } else {
            2.6
        })
        .min(2.1_f64.max(unit.height - 0.85))
    };
    Ok(Some(Opening {
        u0: unit.u0 + inset,
        u1: unit.u1 - inset,
        z0: 0.,
        z1: head,
    }))
}
fn hardware(d: &mut Fixture<'_, '_>, s: &Settings<'_>, kind: MetalKind) -> Result<Pen, String> {
    d.pen.with_weathering(
        d.mesh,
        s.kit.settings().weathering.clone(),
        kind,
        None,
        vec!["metal".into(), "paint".into(), "sheet".into()],
    )
}
fn on<'a, 'cancel>(d: &'a mut Fixture<'_, 'cancel>, pen: &Pen) -> Fixture<'a, 'cancel> {
    Fixture {
        pen: pen.clone(),
        mesh: d.mesh,
        repeats: d.repeats,
    }
}
fn doors(s: &Settings<'_>) -> doors::Settings {
    doors::Settings {
        weathering: s.kit.settings().weathering.clone(),
        timber: s.kit.settings().timber.clone(),
        trim: s.style.trim,
        detail: usize::from(s.kit.detail()),
        posters: s.posters,
    }
}
fn fascia(
    d: &Fixture<'_, '_>,
    span: [f64; 3],
    unit: &Unit,
    kit: &Kit,
    rng: &mut SeededRng,
) -> Result<Vec<Proposal>, String> {
    let [x0, x1, head] = span;
    let h = (unit.height - head - 0.4).min(0.7).min((x1 - x0) / 4.);
    if h < 0.28 {
        return Ok(vec![]);
    }
    Ok(vec![frontage::wall_board(
        &d.pen,
        WallRect {
            x0: (x0 + x1) / 2. - h * 2.,
            z0: head + 0.28,
            width: h * 4.,
            height: h,
        },
        kit,
        rng,
    )?])
}
fn goods(d: &mut Fixture<'_, '_>, x0: f64, x1: f64, rng: &mut SeededRng) -> Result<(), String> {
    let mut x = x0 + 0.1;
    while x < x1 - 0.45 {
        d.mesh.spend_work(1)?;
        let w = rng.float(0.3, 0.5);
        let h = rng.float(0.25, 0.8);
        let tint = shade(srgb(rng.pick(&GOODS)), rng.float(0.5, 0.9));
        d.cuboid("paint", [x, 0.006, 0.], [x + w, 0.05, h], tint, "b")?;
        x += rng.float(0.4, 0.9);
    }
    Ok(())
}
fn shutter(
    d: &mut Fixture<'_, '_>,
    unit: &Unit,
    s: &Settings<'_>,
    rng: &mut SeededRng,
    plan: &Plan,
) -> Result<Vec<Proposal>, String> {
    let o = opening(Kind::Shutter, unit, rng)?.unwrap();
    let box_height = plan.shutter_head.unwrap_or(0.22);
    if !(0.1..=1.).contains(&box_height)
        || !(0. ..=1.).contains(&plan.head_rust)
        || plan
            .fit
            .is_some_and(|top| !(box_height + 1.2..=unit.height).contains(&top))
    {
        return Err(
            "A planned shutter needs a head box of 0.1 to 1 m under an opening within its storey."
                .into(),
        );
    }
    // A fitted shutter fills its wall opening: guides at the bay's edges, the box under the head.
    let (x0, x1, head) = match plan.fit {
        Some(top) => (unit.u0 + 0.06, unit.u1 - 0.06, top - box_height),
        None => (o.u0, o.u1, o.z1),
    };
    let paint = if rng.chance(0.5) {
        s.style.shutter
    } else {
        srgb(rng.pick(&SHUTTERS))
    };
    let oxidation = rng.float(0., 0.3);
    let paint = if s.kit.settings().weathering.texture_density != 0. {
        paint
    } else {
        mix(paint, srgb("#7a4a2a"), oxidation)
    };
    let options = [0., 0., 0., 0., rng.float(0.5, 1.), rng.float(1.5, 2.)];
    let raised = rng.pick(&options);
    // A fitted shutter is also as planned: closed, in the style's own paint.
    let (paint, raised) = if plan.fit.is_some() {
        (s.style.shutter, 0.)
    } else {
        (paint, raised)
    };
    d.panel("paint", [x0, x1, 0., head], 0.004, srgb("#2a2927"))?;
    if raised != 0. {
        goods(d, x0, x1, rng)?;
    }
    shutters::headed_roller_shutter(
        &d.pen,
        d.mesh,
        [x0, x1, raised, head],
        paint,
        &s.kit.settings().weathering,
        [box_height, plan.head_rust],
    )?;
    let hardware = hardware(d, s, MetalKind::Frame)?;
    let ring_x = (x0 + x1) / 2.;
    let ring_z = raised + 0.065;
    let galvanized = srgb("#a4a9aa");
    on(d, &hardware).cuboid(
        "metal",
        [ring_x - 0.02, 0.065, ring_z],
        [ring_x + 0.02, 0.08, ring_z + 0.03],
        galvanized,
        "b",
    )?;
    let ring: Vec<Vec<(V3, V3)>> = (0..12)
        .map(|segment| {
            let angle = segment as f64 * std::f64::consts::PI / 6.;
            (0..6)
                .map(|side| {
                    let cross = side as f64 * std::f64::consts::PI / 3.;
                    let normal = [cos(angle) * cos(cross), sin(cross), sin(angle) * cos(cross)];
                    (
                        [
                            ring_x + cos(angle) * 0.035 + normal[0] * 0.006,
                            0.083 + normal[1] * 0.006,
                            ring_z - 0.02 + sin(angle) * 0.035 + normal[2] * 0.006,
                        ],
                        normal,
                    )
                })
                .collect()
        })
        .collect();
    for segment in 0..12 {
        for side in 0..6 {
            let next = (segment + 1) % 12;
            let around = (side + 1) % 6;
            let corners = [
                ring[segment][side],
                ring[next][side],
                ring[next][around],
                ring[segment][around],
            ];
            let hint = std::array::from_fn(|i| corners.iter().map(|c| c.1[i]).sum());
            let repeat = d.repeat("metal");
            d.pen.face(
                d.mesh,
                "metal",
                &corners.map(|c| c.0),
                hint,
                &Tint::Color(galvanized),
                repeat,
                None,
                Some(&corners.map(|c| c.1)),
            )?;
        }
    }
    on(d, &hardware).cuboid(
        "metal",
        [x1 - 0.13, 0.065, raised + 0.025],
        [x1 - 0.06, 0.09, raised + 0.085],
        shade(galvanized, 0.7),
        "b",
    )?;
    d.panel(
        "paint",
        [x1 - 0.099, x1 - 0.091, raised + 0.04, raised + 0.065],
        0.091,
        srgb("#2a2927"),
    )?;
    fascia(d, [x0, x1, head + box_height], unit, s.kit, rng)
}
fn gate(
    d: &mut Fixture<'_, '_>,
    unit: &Unit,
    s: &Settings<'_>,
    rng: &mut SeededRng,
    lattice: Option<V3>,
) -> Result<Vec<Proposal>, String> {
    let o = opening(Kind::Gate, unit, rng)?.unwrap();
    let (x0, x1, head) = (o.u0, o.u1, o.z1);
    rng.float(0.1, 0.6);
    let steel = lattice.unwrap_or(srgb("#647a6b"));
    let drawn = rng.pick(&[1., 1., 0.55, 0.35]);
    let galvanized = srgb("#a4a9aa");
    d.panel("paint", [x0, x1, 0., head], 0.004, srgb("#2a2927"))?;
    let timber = srgb("#554030");
    let door = doors(s);
    doors::timber_board(
        d,
        [x0 + 0.06, 0.008, 0.06],
        [x1 - 0.06, 0.018, head - 0.06],
        shade(timber, 0.65),
        &door,
    )?;
    for x in [x0 + 0.06, x1 - 0.13] {
        doors::timber_board(
            d,
            [x, 0.018, 0.06],
            [x + 0.07, 0.035, head - 0.06],
            timber,
            &door,
        )?;
    }
    for z in [0.08, 0.85, 1.6, head - 0.13] {
        doors::timber_board(
            d,
            [x0 + 0.13, 0.018, z],
            [x1 - 0.13, 0.035, z + 0.07],
            timber,
            &door,
        )?;
    }
    let pen = hardware(d, s, MetalKind::Cage)?;
    let count = ((x1 - x0) / 0.14).round().max(5.) as usize;
    let end = x0 + (x1 - x0 - 0.05) * drawn;
    let pitch = (end - x0) / count as f64;
    for i in 0..=count {
        d.mesh.spend_work(1)?;
        let x = x0 + i as f64 * pitch;
        on(d, &pen).cuboid(
            "paint",
            [x, 0.065, 0.075],
            [x + 0.018, 0.085, head - 0.08],
            steel,
            "bd",
        )?;
        for z in [0.06, head - 0.07] {
            d.disc(
                "metal",
                [x + 0.009, 0.086, z],
                0.018,
                [0., 1., 0.],
                galvanized,
                6,
            )?;
        }
        let link_height = (0.52_f64.powi(2) - pitch.powi(2)).sqrt();
        for center in [0.55, head - 0.55] {
            for z in [center - link_height / 2., center + link_height / 2.] {
                d.tube(
                    "metal",
                    &[[x + 0.009, 0.082, z], [x + 0.009, 0.117, z]],
                    0.006,
                    galvanized,
                    6,
                )?;
                d.disc(
                    "metal",
                    [x + 0.009, 0.118, z],
                    0.009,
                    [0., 1., 0.],
                    galvanized,
                    6,
                )?;
            }
        }
        if i == count {
            continue;
        }
        for center in [0.55, head - 0.55] {
            let low = center - link_height / 2.;
            let high = center + link_height / 2.;
            on(d, &pen).bar(
                "paint",
                [x + 0.009, 0.094, low],
                [x + pitch + 0.009, 0.094, high],
                0.012,
                steel,
            )?;
            on(d, &pen).bar(
                "paint",
                [x + 0.009, 0.109, high],
                [x + pitch + 0.009, 0.109, low],
                0.012,
                steel,
            )?;
            d.disc(
                "metal",
                [x + pitch / 2. + 0.009, 0.117, center],
                0.012,
                [0., 1., 0.],
                galvanized,
                6,
            )?;
        }
    }
    for z in [0.015, head - 0.035] {
        on(d, &pen).cuboid(
            "metal",
            [x0 - 0.04, 0.035, z],
            [x1 + 0.04, 0.115, z + 0.025],
            shade(steel, 0.65),
            "b",
        )?;
        on(d, &pen).cuboid(
            "paint",
            [x0 - 0.04, 0.1, z + 0.025],
            [x1 + 0.04, 0.12, z + 0.05],
            steel,
            "bd",
        )?;
    }
    on(d, &pen).cuboid(
        "paint",
        [end + 0.018, 0.055, 0.065],
        [end + 0.055, 0.09, head - 0.06],
        steel,
        "bd",
    )?;
    doors::pull_handle(d, end + 0.035, 1., 0.09, galvanized)?;
    on(d, &pen).cuboid(
        "paint",
        [x0 - 0.04, 0., head],
        [x1 + 0.04, 0.1, head + 0.08],
        steel,
        "b",
    )?;
    fascia(d, [x0, x1, head + 0.08], unit, s.kit, rng)
}
fn shop(
    d: &mut Fixture<'_, '_>,
    unit: &Unit,
    s: &Settings<'_>,
    rng: &mut SeededRng,
) -> Result<Vec<Proposal>, String> {
    let o = opening(Kind::Shop, unit, rng)?.unwrap();
    let (x0, x1, head) = (o.u0, o.u1, o.z1);
    let riser = 0.55;
    let galvanized = srgb("#a4a9aa");
    let frame = if rng.chance(0.5) {
        s.style.frame
    } else {
        shade(galvanized, 0.9)
    };
    let pen = hardware(d, s, MetalKind::Frame)?;
    let door = if rng.chance(0.5) { x0 } else { x1 - 0.85 };
    d.panel("paint", [x0, x1, 0., head], 0.004, srgb("#2a2927"))?;
    let (a, b) = if door == x0 {
        (x0 + 0.85, x1)
    } else {
        (x0, x1 - 0.85)
    };
    d.cuboid(
        "paint",
        [a, 0., 0.],
        [b, 0.05, riser],
        shade(s.style.accent, 0.8),
        "bd",
    )?;
    d.panel(
        if rng.chance(0.55) {
            "interior-lit"
        } else {
            "pane"
        },
        [a, b, riser, head],
        0.02,
        srgb("#101418"),
    )?;
    let mut shelf = riser + 0.15;
    while shelf < head - 0.4 {
        let mut x = a + 0.1;
        while x < b - 0.2 {
            d.mesh.spend_work(1)?;
            let high = [x + rng.float(0.1, 0.16), 0.07, shelf + rng.float(0.15, 0.3)];
            let tint = shade(srgb(rng.pick(&GOODS)), 0.8);
            d.cuboid("paint", [x, 0.024, shelf], high, tint, "b")?;
            x += rng.float(0.16, 0.34);
        }
        shelf += 0.5;
    }
    for x in [a, (a + b) / 2. - 0.02, b - 0.04] {
        on(d, &pen).cuboid(
            "paint",
            [x, 0., riser],
            [x + 0.04, 0.06, head],
            frame,
            "bdu",
        )?;
    }
    on(d, &pen).cuboid("paint", [a, 0., head - 0.04], [b, 0.06, head], frame, "blr")?;
    d.panel(
        "pane",
        [door + 0.06, door + 0.79, 0.25, head - 0.06],
        0.02,
        srgb("#2a3438"),
    )?;
    on(d, &pen).cuboid(
        "paint",
        [door, 0., 0.],
        [door + 0.06, 0.06, head],
        frame,
        "bdu",
    )?;
    on(d, &pen).cuboid(
        "paint",
        [door + 0.79, 0., 0.],
        [door + 0.85, 0.06, head],
        frame,
        "bdu",
    )?;
    on(d, &pen).cuboid(
        "paint",
        [door + 0.06, 0., 0.],
        [door + 0.79, 0.05, 0.25],
        frame,
        "bd",
    )?;
    on(d, &pen).cuboid(
        "metal",
        [door + 0.68, 0.05, 1.],
        [door + 0.71, 0.09, 1.3],
        galvanized,
        "b",
    )?;
    fascia(d, [x0, x1, head], unit, s.kit, rng)
}
fn service(
    d: &mut Fixture<'_, '_>,
    unit: &Unit,
    s: &Settings<'_>,
    rng: &mut SeededRng,
) -> Result<(), String> {
    if let Some(o) = opening(Kind::Service, unit, rng)? {
        let look = windows::Look {
            kind: windows::Kind::Louver,
            weathering: Some(s.kit.settings().weathering.clone()),
            frame: s.style.frame,
            trim: s.style.trim,
            curtain: s.style.trim,
            reach: 0.15,
            hood: false,
            lit: false,
            boarded: false,
            fixed: false,
            recess: 0.,
            transom_ratio: None,
            lower_pane_bars: None,
            timber_textured: s.kit.settings().timber.textured,
        };
        windows::window(d, [o.u0, o.u1, 1.45, 2.2], &look, rng)?;
        windows::security_bars(
            d,
            [o.u0, o.u1, 1.45, 2.2],
            windows::Guard::Bars,
            shade(s.style.frame, 0.7),
            Some(&s.kit.settings().weathering),
        )?;
    }
    if s.kit.detail() > 0 {
        let pen = d.pen.into_part("fixtures")?;
        let d = &mut on(d, &pen);
        let width = unit.u1 - unit.u0;
        household::meter_bank(
            d,
            unit.u0 + 0.3,
            1.2,
            3.min(((width - 0.5) / 0.3).floor() as usize),
            rng,
            &s.kit.settings().weathering,
        )?;
        posters::notices(d, [unit.u0 + 0.2, unit.u1 - 0.2, 0.5, 1.15], rng, s.posters)?;
    }
    Ok(())
}
pub fn draw(
    d: &mut Fixture<'_, '_>,
    kind: Kind,
    unit: &Unit,
    s: &Settings<'_>,
    rng: &mut SeededRng,
) -> Result<Vec<Proposal>, String> {
    planned(d, kind, unit, s, rng, &Plan::default())
}
/// What a plan fixes of a bay instead of leaving it to the seed; the default fixes nothing.
#[derive(Clone, Copy, Default)]
pub struct Plan {
    /// The entrance of a door bay.
    pub door: doors::Design,
    /// The head height of a wall opening as wide as the bay: a shutter then fills it (closed,
    /// in the style's shutter paint) and a door's frame does (see [`doors::Design::fit`]).
    pub fit: Option<f64>,
    /// The height of a shutter's head box (0.22 m).
    pub shutter_head: Option<f64>,
    /// How far the head box has rusted, 0 to 1.
    pub head_rust: f64,
    /// The paint of a gate's lattice (a grey green).
    pub gate: Option<V3>,
}
/// [`draw`] with whatever `plan` fixes of the bay; an empty plan draws the seeded bay.
pub fn planned(
    d: &mut Fixture<'_, '_>,
    kind: Kind,
    unit: &Unit,
    s: &Settings<'_>,
    rng: &mut SeededRng,
    plan: &Plan,
) -> Result<Vec<Proposal>, String> {
    validate(unit)?;
    d.mesh.spend_work(1)?;
    if [s.style.frame, s.style.trim, s.style.shutter, s.style.accent]
        .iter()
        .flatten()
        .any(|v| !v.is_finite() || !(0. ..=1.).contains(v))
    {
        return Err("Ground bay pigments must be normalized linear colors.".into());
    }
    match kind {
        Kind::Shutter => shutter(d, unit, s, rng, plan),
        Kind::Gate => gate(d, unit, s, rng, plan.gate),
        Kind::Shop => shop(d, unit, s, rng),
        Kind::Door => {
            let design = doors::Design {
                fit: plan.fit,
                ..plan.door
            };
            doors::planned_entrance(d, unit, &doors(s), rng, &design)?;
            Ok(vec![])
        }
        Kind::Service => {
            service(d, unit, s, rng)?;
            Ok(vec![])
        }
    }
}

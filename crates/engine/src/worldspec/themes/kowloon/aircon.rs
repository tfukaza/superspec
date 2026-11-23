//! Window units and split-system condensers, with atlas-unwrapped cases and physical fans/hardware.
use super::{
    corrugated,
    fixture::Fixture,
    graphics_detail::GraphicsDetail,
    mesh::rounded,
    metal::Kind,
    palette::{mix, shade, srgb},
    pen::Tint,
    steel::{self, Finish, Plate, Steel},
    variants::{self, Budget},
    weathered::Settings,
};
use std::f64::consts::PI;
use superspec_geometry::{
    V3,
    math::{atan2, cos, sin},
};
use superspec_types::rng::SeededRng;
const SHADOW: V3 = [0.05, 0.05, 0.055];
const FAN_ASPECT: f64 = 1.4545;
pub const CONDENSER_ROOM: f64 = 0.06 + 0.7 + 0.15;
pub const CONDENSER_HEIGHT: f64 = 0.5;
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct Bounds {
    pub x0: f64,
    pub x1: f64,
    pub z: f64,
}
struct Cells {
    front: usize,
    left: usize,
    right: usize,
    top: usize,
    bottom: usize,
    back: Option<usize>,
}
fn atlas(cell: usize, s: f64, t: f64) -> [f64; 2] {
    [
        (cell % 8) as f64 / 8. + s.clamp(0.014, 0.986) / 8.,
        1. - ((cell / 8) as f64 + t.clamp(0.014, 0.986)) / 4.,
    ]
}
fn unwrapped(
    drawing: &mut Fixture<'_, '_>,
    cell: usize,
    points: &[V3],
    hint: V3,
    tint: V3,
    project: &dyn Fn(V3) -> [f64; 2],
) -> Result<(), String> {
    if drawing.mesh.surveying() {
        return drawing.pen.face(
            drawing.mesh,
            "aircon",
            points,
            hint,
            &Tint::Color(tint),
            1.,
            None,
            None,
        );
    }
    let uv: Vec<_> = points
        .iter()
        .map(|p| {
            let [s, t] = project(*p);
            atlas(cell, s, t)
        })
        .collect();
    drawing.pen.face(
        drawing.mesh,
        "aircon",
        points,
        hint,
        &Tint::Color(tint),
        drawing.repeat("aircon"),
        Some(&uv),
        None,
    )
}
fn on_face([x0, x1, z0, z1]: [f64; 4]) -> impl Fn(V3) -> [f64; 2] {
    move |p| [(p[0] - x0) / (x1 - x0), (z1 - p[2]) / (z1 - z0)]
}
fn casing(
    d: &mut Fixture<'_, '_>,
    lo: V3,
    hi: V3,
    b: f64,
    c: Cells,
    tint: V3,
) -> Result<(), String> {
    let [x0, y0, z0] = lo;
    let [x1, y1, z1] = hi;
    let front = on_face([x0, x1, z0, z1]);
    let left = |p: V3| [(p[1] - y0) / (y1 - y0), (z1 - p[2]) / (z1 - z0)];
    let right = |p: V3| [(y1 - p[1]) / (y1 - y0), (z1 - p[2]) / (z1 - z0)];
    let top = |p: V3| [(p[0] - x0) / (x1 - x0), (p[1] - y0) / (y1 - y0)];
    // The arrises are chamfered by a centimetre: a reduced level draws the square case.
    let (plain, b) = if d.mesh.graphics_detail().reduced() {
        (true, 0.)
    } else {
        (false, b)
    };
    unwrapped(
        d,
        c.left,
        &[
            [x0, y0, z0 + b],
            [x0, y1 - b, z0 + b],
            [x0, y1 - b, z1 - b],
            [x0, y0, z1 - b],
        ],
        [-1., 0., 0.],
        tint,
        &left,
    )?;
    unwrapped(
        d,
        c.right,
        &[
            [x1, y1 - b, z0 + b],
            [x1, y0, z0 + b],
            [x1, y0, z1 - b],
            [x1, y1 - b, z1 - b],
        ],
        [1., 0., 0.],
        tint,
        &right,
    )?;
    unwrapped(
        d,
        c.top,
        &[
            [x0 + b, y1 - b, z1],
            [x1 - b, y1 - b, z1],
            [x1 - b, y0, z1],
            [x0 + b, y0, z1],
        ],
        [0., 0., 1.],
        tint,
        &top,
    )?;
    unwrapped(
        d,
        c.bottom,
        &[
            [x0 + b, y0, z0],
            [x1 - b, y0, z0],
            [x1 - b, y1 - b, z0],
            [x0 + b, y1 - b, z0],
        ],
        [0., 0., -1.],
        shade(tint, 0.7),
        &top,
    )?;
    if let Some(back) = c.back {
        unwrapped(
            d,
            back,
            &[[x1, y0, z0], [x0, y0, z0], [x0, y0, z1], [x1, y0, z1]],
            [0., -1., 0.],
            shade(tint, 0.8),
            &front,
        )?;
    }
    if plain {
        return Ok(());
    }
    for [x, inner, side] in [[x0, x0 + b, -1.], [x1, x1 - b, 1.]] {
        for (z, flat, up, cell) in [(z1 - b, z1, 1., c.top), (z0 + b, z0, -1., c.bottom)] {
            unwrapped(
                d,
                cell,
                &[
                    [x, y0, z],
                    [x, y1 - b, z],
                    [inner, y1 - b, flat],
                    [inner, y0, flat],
                ],
                [side, 0., up],
                shade(tint, 0.94),
                &top,
            )?;
        }
    }
    for [z, flat, up] in [[z1 - b, z1, 1.], [z0 + b, z0, -1.]] {
        unwrapped(
            d,
            c.front,
            &[
                [x0 + b, y1, z],
                [x1 - b, y1, z],
                [x1 - b, y1 - b, flat],
                [x0 + b, y1 - b, flat],
            ],
            [0., 1., up],
            shade(tint, 0.96),
            &front,
        )?;
    }
    for [x, inner, side] in [[x0, x0 + b, -1.], [x1, x1 - b, 1.]] {
        unwrapped(
            d,
            c.front,
            &[
                [inner, y1, z0 + b],
                [inner, y1, z1 - b],
                [x, y1 - b, z1 - b],
                [x, y1 - b, z0 + b],
            ],
            [side, 1., 0.],
            shade(tint, 0.96),
            &front,
        )?;
        for [z, flat, up] in [[z1 - b, z1, 1.], [z0 + b, z0, -1.]] {
            unwrapped(
                d,
                c.front,
                &[[inner, y1, z], [x, y1 - b, z], [inner, y1 - b, flat]],
                [side, 1., up],
                shade(tint, 0.9),
                &front,
            )?;
        }
    }
    Ok(())
}
fn grille(
    d: &mut Fixture<'_, '_>,
    span: [f64; 4],
    y: f64,
    recess: f64,
    cell: usize,
    tint: V3,
) -> Result<(), String> {
    let [x0, x1, z0, z1] = span;
    let project = on_face(span);
    let ring = |inset: f64, depth| {
        let dx = (x1 - x0) * inset;
        let dz = (z1 - z0) * inset;
        [
            [x0 + dx, depth, z0 + dz],
            [x1 - dx, depth, z0 + dz],
            [x1 - dx, depth, z1 - dz],
            [x0 + dx, depth, z1 - dz],
        ]
    };
    let outer = ring(0., y);
    if d.mesh.graphics_detail().reduced() {
        // The pressed rim and its step are in the cell's artwork: one flat face.
        return unwrapped(d, cell, &outer, [0., 1., 0.], tint, &project);
    }
    let lip = ring(0.05, y);
    let panel = ring(0.075, y - recess);
    let inward = [[0., 0., 1.], [-1., 0., 0.], [0., 0., -1.], [1., 0., 0.]];
    for k in 0..4 {
        let next = (k + 1) % 4;
        unwrapped(
            d,
            cell,
            &[outer[k], outer[next], lip[next], lip[k]],
            [0., 1., 0.],
            tint,
            &project,
        )?;
        let facing = inward[k].map(|v| v * recess.signum());
        unwrapped(
            d,
            cell,
            &[lip[k], lip[next], panel[next], panel[k]],
            [facing[0], 0.4, facing[2]],
            shade(tint, if recess > 0. { 0.6 } else { 0.9 }),
            &project,
        )?;
    }
    unwrapped(d, cell, &panel, [0., 1., 0.], tint, &project)
}
fn louvers(
    d: &mut Fixture<'_, '_>,
    x: f64,
    side: f64,
    span: [f64; 4],
    tint: V3,
) -> Result<(), String> {
    // Slats a centimetre proud of the side: a reduced level leaves the side's own artwork.
    if d.mesh.graphics_detail().reduced() {
        return Ok(());
    }
    let [y0, y1, z0, z1] = span;
    let out = |depth| x + side * depth;
    d.face(
        "paint",
        &[
            [out(0.001), y0, z0],
            [out(0.001), y1, z0],
            [out(0.001), y1, z1],
            [out(0.001), y0, z1],
        ],
        [side, 0., 0.],
        SHADOW,
    )?;
    let slats = rounded((z1 - z0) / 0.03, 1.).max(3.) as usize;
    let pitch = (z1 - z0) / slats as f64;
    for i in 0..slats {
        let foot = z0 + i as f64 * pitch;
        let lip = foot + pitch * 0.72;
        d.face(
            "metal",
            &[
                [out(0.011), y0, foot],
                [out(0.011), y1, foot],
                [out(0.002), y1, lip],
                [out(0.002), y0, lip],
            ],
            [side, 0., -0.6],
            shade(tint, 0.92),
        )?;
    }
    Ok(())
}
fn brackets(
    drawing: &mut Fixture<'_, '_>,
    span: [f64; 2],
    depth: f64,
    z: f64,
    drop: f64,
    tint: V3,
    weathering: Option<&Settings>,
    Options { plates, weight, .. }: Options,
) -> Result<(), String> {
    // Level 1 keeps the arms and their struts and leaves the wall plates, a centimetre
    // thick, with their bolts; level 2 draws the case alone.
    let detail = drawing.mesh.graphics_detail();
    if detail == GraphicsDetail::VeryLow {
        return Ok(());
    }
    let plated = !detail.reduced();
    let heavy = |plain: f64, bold: f64| weighed(weight, plain, bold);
    let fall = drop.min(0.32);
    let pen = if let Some(s) = weathering {
        drawing.pen.with_weathering(
            drawing.mesh,
            s.clone(),
            Kind::Frame,
            None,
            vec!["paint".into(), "metal".into()],
        )?
    } else {
        drawing.pen.clone()
    };
    let mut d = Fixture {
        pen,
        mesh: drawing.mesh,
        repeats: drawing.repeats,
    };
    let steel = Steel {
        slot: "metal",
        tint,
        finish: Finish::Metal(weathering),
    };
    for x in [span[0] + 0.07, span[1] - 0.07] {
        steel::angle(
            &mut d,
            steel,
            [
                [x - heavy(0.018, 0.0225), 0.008, z - heavy(0.04, 0.05)],
                [x - heavy(0.018, 0.0225), depth, z - heavy(0.04, 0.05)],
            ],
            [[1., 0., 0.], [0., 0., 1.]],
            [heavy(0.035, 0.045), heavy(0.004, 0.008)],
        )?;
        if plates && plated {
            // A bolted wall plate where the arm meets the wall, and bolts through the arm's
            // flange where the casing sits on it.
            steel::fixing_plate(
                &mut d,
                steel,
                &Plate {
                    center: [x, 0., z - 0.045],
                    across: [1., 0., 0.],
                    up: [0., 0., 1.],
                    out: [0., 1., 0.],
                    size: [heavy(0.09, 0.115), heavy(0.13, 0.155)],
                    bolts: 4,
                },
            )?;
            for share in [0.35, 0.75] {
                steel::bolt(
                    &mut d,
                    steel,
                    [x + 0.002, depth * share, z - 0.035],
                    [0., 0., 1.],
                    heavy(0.007, 0.0105),
                )?;
            }
        }
        if fall < 0.12 {
            continue;
        }
        steel::member(
            &mut d,
            steel,
            [[x, depth - 0.05, z - 0.04], [x, 0.014, z - fall + 0.035]],
            [1., 0., 0.],
            [heavy(0.026, 0.036), heavy(0.018, 0.026)],
        )?;
        if !plated {
            continue;
        }
        steel::fixing_plate(
            &mut d,
            steel,
            &Plate {
                center: [x, 0., z - fall / 2.],
                across: [1., 0., 0.],
                up: [0., 0., 1.],
                out: [0., 1., 0.],
                size: [heavy(0.07, 0.09), fall],
                bolts: 4,
            },
        )?;
    }
    Ok(())
}
/// The shape of a unit's rain cover: a corrugated sheet, or a board held down by a brick
/// `brick` of the way along it.
#[derive(Clone, Copy, Debug)]
struct Cover {
    sheet: bool,
    brick: f64,
}
/// A rain cover's colours: its sheet's rust, its board's shade and its brick's.
#[derive(Clone, Copy, Debug)]
struct CoverPaint {
    sheet: V3,
    wood: V3,
    brick: V3,
}
impl CoverPaint {
    fn sheet(rust: f64) -> V3 {
        mix(srgb("#a4a9aa"), srgb("#7a4a2a"), rust)
    }
    fn brick(shade_by: f64) -> V3 {
        shade(srgb("#8f5a48"), shade_by)
    }
}
/// A cover and its colours as one stream draws them together.
fn seeded_cover(rng: &mut SeededRng) -> (Cover, CoverPaint) {
    let mut paint = CoverPaint {
        sheet: [0.; 3],
        wood: [0.; 3],
        brick: [0.; 3],
    };
    if rng.chance(0.6) {
        paint.sheet = CoverPaint::sheet(rng.float(0.1, 0.6));
        return (
            Cover {
                sheet: true,
                brick: 0.,
            },
            paint,
        );
    }
    paint.wood = shade([1.; 3], rng.float(0.7, 0.95));
    let brick = rng.float(0.2, 0.7);
    paint.brick = CoverPaint::brick(rng.float(0.8, 1.1));
    (
        Cover {
            sheet: false,
            brick,
        },
        paint,
    )
}
fn rain_cover(
    d: &mut Fixture<'_, '_>,
    span: [f64; 2],
    depth: f64,
    top: f64,
    cover: Cover,
    paint: CoverPaint,
) -> Result<(), String> {
    let [x0, x1] = span;
    if cover.sheet {
        let sheet = paint.sheet;
        let corners = [
            [x0 - 0.06, 0., top + 0.11],
            [x1 + 0.06, 0., top + 0.11],
            [x1 + 0.06, depth + 0.06, top + 0.02],
            [x0 - 0.06, depth + 0.06, top + 0.02],
        ];
        if d.mesh.graphics_detail().reduced() {
            // The ribs are a few millimetres deep: one flat sheet.
            return d.sheet("sheet", &corners, sheet);
        }
        return corrugated::draw(d, "sheet", corners, 0.09, sheet, None);
    }
    let detail = d.mesh.graphics_detail();
    let lo = [x0 - 0.04, 0., top + 0.002];
    let hi = [x1 + 0.04, depth + 0.05, top + 0.016];
    let flat = |p: V3| [(p[0] - lo[0]) / (hi[0] - lo[0]), p[1] / hi[1]];
    let edge = |p: V3| {
        [
            (p[0] - lo[0] + p[1]) / (hi[0] - lo[0] + hi[1]),
            (hi[2] - p[2]) / 0.2,
        ]
    };
    let wood = paint.wood;
    unwrapped(
        d,
        24,
        &[
            [lo[0], hi[1], hi[2]],
            [hi[0], hi[1], hi[2]],
            [hi[0], 0., hi[2]],
            [lo[0], 0., hi[2]],
        ],
        [0., 0., 1.],
        wood,
        &flat,
    )?;
    // The board is 14 mm thick: level 2 draws its top and no brick.
    if detail == GraphicsDetail::VeryLow {
        return Ok(());
    }
    unwrapped(
        d,
        24,
        &[
            [lo[0], hi[1], lo[2]],
            [hi[0], hi[1], lo[2]],
            [hi[0], hi[1], hi[2]],
            [lo[0], hi[1], hi[2]],
        ],
        [0., 1., 0.],
        shade(wood, 0.7),
        &edge,
    )?;
    for x in [lo[0], hi[0]] {
        unwrapped(
            d,
            24,
            &[
                [x, 0., lo[2]],
                [x, hi[1], lo[2]],
                [x, hi[1], hi[2]],
                [x, 0., hi[2]],
            ],
            [if x == lo[0] { -1. } else { 1. }, 0., 0.],
            shade(wood, 0.7),
            &edge,
        )?;
    }
    let bx = x0 + (x1 - x0) * cover.brick;
    d.cuboid(
        "paint",
        [bx, depth * 0.3, hi[2]],
        [bx + 0.2, depth * 0.3 + 0.1, hi[2] + 0.06],
        paint.brick,
        "d",
    )
}
/// Three fixed wear classes use one draw, including the reference's inclusive cumulative boundary.
fn wear(rng: &mut SeededRng, weights: [f64; 3]) -> usize {
    let mut roll = rng.next_f64() * weights.iter().sum::<f64>();
    for (i, w) in weights.iter().enumerate() {
        roll -= w;
        if roll <= 0. {
            return i;
        }
    }
    2
}
fn body(rng: &mut SeededRng) -> V3 {
    srgb(rng.pick(&["#e0ddd2", "#cfc8b4", "#b8bcc0", "#d8d0bc"]))
}
/// What a caller adds to a unit beyond the seeded one; the default adds nothing.
#[derive(Clone, Copy, Debug)]
pub struct Options {
    /// Bolted wall plates under the bracket arms and bolts through the arms.
    pub plates: bool,
    /// The outward face always shows the louvred grille (the seed's model picks it otherwise).
    pub louvred: bool,
    /// How heavy the supports, lines and guard wires are drawn: 1 is their own size, 2 the
    /// weight of a drawing (1–3).
    pub weight: f64,
    /// How many shapes the unit's part type may draw (see [`super::variants`]). Unlimited
    /// draws the shape and the colours together from the unit's own stream.
    pub variants: Budget,
    /// The catalogue entry to draw, where the unit is part of a fixture that has made its
    /// pick (a caged unit is its cage's entry); the unit's stream then draws its case paint only.
    pub entry: Option<u32>,
}
impl Default for Options {
    fn default() -> Self {
        Self {
            plates: false,
            louvred: false,
            weight: 1.,
            variants: Budget::Unlimited,
            entry: None,
        }
    }
}
/// `plain` at weight 1 and `bold` at weight 2.
fn weighed(weight: f64, plain: f64, bold: f64) -> f64 {
    plain + (bold - plain) * (weight - 1.)
}
/// Window-mounted unit. Returned facade footprint is also the source of its wall stain.
pub fn air_conditioner(
    d: &mut Fixture<'_, '_>,
    x: f64,
    z: f64,
    drop: f64,
    rng: &mut SeededRng,
    weathering: Option<&Settings>,
) -> Result<Bounds, String> {
    air_conditioner_with(d, x, z, drop, rng, weathering, Options::default())
}
/// Everything that shapes a window unit: one catalogue entry.
#[derive(Clone, Copy, Debug)]
struct Unit {
    /// Which of the four makes: its grille artwork and how far the grille is recessed.
    model: usize,
    /// The wear class of its artwork: clean, worn or rusted cells.
    wear: usize,
    w: f64,
    h: f64,
    depth: f64,
    /// The atlas cell of its top.
    top: usize,
    cover: Option<Cover>,
    /// Where the drain hose leaves the case, from its centre, and how far it hangs.
    hose: f64,
    extent: f64,
}
/// A unit's colours: the instance's own, whatever its shape.
#[derive(Clone, Copy, Debug)]
struct UnitPaint {
    body: V3,
    steel: V3,
    cover: CoverPaint,
}
fn unit_steel(rust: f64) -> V3 {
    mix(
        shade(srgb("#a4a9aa"), 0.6),
        shade(srgb("#7a4a2a"), 0.75),
        rust,
    )
}
/// A window unit's shape and colours as one stream draws them together, in the order the
/// reference draws them. `fixed` states the make and the wear class in place of the stream's.
fn seeded_unit(rng: &mut SeededRng, fixed: Option<[usize; 2]>) -> (Unit, UnitPaint) {
    let model = rng.integer(0, 3);
    let wear = wear(rng, [3., 4., 3.]);
    let [model, wear] = fixed.unwrap_or([model, wear]);
    let [narrowest, widest] = [[0.58, 0.68], [0.62, 0.72], [0.5, 0.58], [0.6, 0.7]][model];
    let w = rng.float(narrowest, widest);
    let h = w * rng.float(0.6, 0.67);
    let depth = rng.float(0.3, 0.37);
    let body = mix(
        mix(
            body(rng),
            srgb("#cdbf98"),
            wear as f64 * rng.float(0.1, 0.3),
        ),
        srgb("#7a4a2a"),
        wear as f64 * rng.float(0., 0.06),
    );
    let top = if wear > 0 && rng.chance(0.6) { 4 } else { 3 };
    let (cover, cover_paint) = if rng.chance(0.6) {
        let (cover, paint) = seeded_cover(rng);
        (Some(cover), paint)
    } else {
        (
            None,
            CoverPaint {
                sheet: [0.; 3],
                wood: [0.; 3],
                brick: [0.; 3],
            },
        )
    };
    let steel = unit_steel(rng.float(0.2, 0.85));
    let hose = rng.float(-0.2, 0.2);
    let extent = rng.float(0.3, 0.7);
    (
        Unit {
            model,
            wear,
            w,
            h,
            depth,
            top,
            cover,
            hose,
            extent,
        },
        UnitPaint {
            body,
            steel,
            cover: cover_paint,
        },
    )
}
/// One catalogue entry of the window unit. Four entries are the four makes, and the wear
/// class turns through worn, clean and rusted, so the first twelve entries are every make in
/// every wear; an entry's size, depth, cover and hose are drawn from its own stream.
fn unit_entry(entry: u32) -> (Unit, UnitPaint) {
    let fixed = [entry as usize % 4, [1, 0, 2][entry as usize % 3]];
    seeded_unit(&mut variants::stream("window-unit", entry), Some(fixed))
}
/// The window unit of `rng` under `budget`. With a catalogue the stream's first draw picks
/// the entry and its next four are the instance's one colour, its case paint: how yellowed
/// and rusted that is does not follow the entry's artwork, so a unit keeps its paint
/// whichever entry a budget gives it. Its bracket steel and its cover are the entry's.
fn unit(rng: &mut SeededRng, budget: Budget, entry: Option<u32>) -> (Unit, UnitPaint) {
    let Some(entry) = entry.or_else(|| budget.pick(rng)) else {
        return seeded_unit(rng, None);
    };
    let (unit, paint) = unit_entry(entry);
    let body = variants::instance_colour(UNIT_PAINT, unit_body(rng));
    (unit, UnitPaint { body, ..paint })
}
/// The name of the one colour a catalogued window unit or condenser takes from its instance.
pub const UNIT_PAINT: &str = "body";
/// A catalogued window unit's own case paint, from its stream after its pick.
fn unit_body(rng: &mut SeededRng) -> V3 {
    let aged = wear(rng, [3., 4., 3.]);
    mix(
        mix(
            body(rng),
            srgb("#cdbf98"),
            aged as f64 * rng.float(0.1, 0.3),
        ),
        srgb("#7a4a2a"),
        aged as f64 * rng.float(0., 0.06),
    )
}
/// The catalogue entry the window unit of `rng` is under `budget` and its own case paint, as
/// [`air_conditioner_with`] draws them from the same stream; `None` where every unit draws
/// its own shape. Units of one entry differ by that paint alone.
pub fn unit_instance(rng: &mut SeededRng, budget: Budget) -> Option<(u32, V3)> {
    let entry = budget.pick(rng)?;
    Some((entry, unit_body(rng)))
}
/// The `[width, height]` of the window unit [`air_conditioner_with`] draws from a fresh copy
/// of this stream under the same budget and entry, without drawing it.
pub fn unit_size(mut rng: SeededRng, budget: Budget, entry: Option<u32>) -> [f64; 2] {
    let (unit, _) = unit(&mut rng, budget, entry);
    [unit.w, unit.h]
}
/// [`air_conditioner`] with what `options` adds.
pub fn air_conditioner_with(
    d: &mut Fixture<'_, '_>,
    x: f64,
    z: f64,
    drop: f64,
    rng: &mut SeededRng,
    weathering: Option<&Settings>,
    options: Options,
) -> Result<Bounds, String> {
    if [x, z, drop]
        .iter()
        .any(|v| !v.is_finite() || v.abs() > 10000.)
        || !(0. ..=100.).contains(&drop)
    {
        return Err("Air conditioner needs bounded finite placement and nonnegative drop.".into());
    }
    let (
        Unit {
            model,
            wear,
            w,
            h,
            depth,
            top,
            cover,
            hose,
            extent,
        },
        paint,
    ) = unit(rng, options.variants, options.entry);
    let body = paint.body;
    let x0 = x - w / 2.;
    let x1 = x + w / 2.;
    let b = 0.012;
    let rear = if options.louvred { 16 } else { 16 + model } + if wear > 0 { 4 } else { 0 };
    let side = wear;
    casing(
        d,
        [x0, 0., z],
        [x1, depth, z + h],
        b,
        Cells {
            front: rear,
            left: side,
            right: side,
            top,
            bottom: 3,
            back: None,
        },
        body,
    )?;
    let recess = [0.006, 0.02, 0.028, -0.026][model];
    // A reduced level's square case has no chamfer for the grille to stand inside.
    let rim = if d.mesh.graphics_detail().reduced() {
        0.
    } else {
        b
    };
    grille(
        d,
        [x0 + rim, x1 - rim, z + rim, z + h - rim],
        depth,
        recess,
        rear,
        body,
    )?;
    let wire = shade(body, 0.65);
    // The guard's wires: heavier ones stand further apart.
    let heavy = |plain: f64, bold: f64| weighed(options.weight, plain, bold);
    let (gauge, pitch) = (heavy(0.0018, 0.0038), heavy(0.055, 0.1));
    let grille_y = depth - recess + heavy(0.003, 0.005);
    // Guard wires are a few millimetres thick: a reduced level leaves the grille behind them.
    let wired = !d.mesh.graphics_detail().reduced();
    let mut u = x0 + 0.055;
    while wired && u < x1 - 0.045 {
        d.tube(
            "metal",
            &[[u, grille_y, z + 0.05], [u, grille_y, z + h - 0.05]],
            gauge,
            wire,
            5,
        )?;
        u += pitch;
    }
    let mut v = z + 0.065;
    while wired && v < z + h - 0.05 {
        d.tube(
            "metal",
            &[[x0 + 0.05, grille_y, v], [x1 - 0.05, grille_y, v]],
            gauge,
            wire,
            5,
        )?;
        v += pitch;
    }
    let screws = Steel {
        slot: "metal",
        tint: shade(body, 0.7),
        finish: Finish::Metal(None),
    };
    for u in [x0 + 0.026, x1 - 0.026] {
        for v in [z + 0.026, z + h - 0.026] {
            steel::bolt(d, screws, [u, depth + 0.001, v], [0., 1., 0.], 0.004)?;
        }
    }
    for [face, direction] in [[x0, -1.], [x1, 1.]] {
        louvers(
            d,
            face,
            direction,
            [depth * 0.42, depth - 0.05, z + h * 0.24, z + h * 0.78],
            body,
        )?;
    }
    // The wall flange and the drain hose go at level 2.
    let trimmed = d.mesh.graphics_detail() != GraphicsDetail::VeryLow;
    if trimmed {
        d.cuboid(
            "metal",
            [x0 - 0.015, 0., z - 0.015],
            [x1 + 0.015, 0.035, z + h + 0.015],
            shade(body, 0.8),
            "b",
        )?;
    }
    if let Some(cover) = cover {
        rain_cover(d, [x0, x1], depth, z + h, cover, paint.cover)?;
    }
    brackets(
        d,
        [x0, x1],
        depth - 0.02,
        z,
        drop,
        paint.steel,
        weathering,
        options,
    )?;
    let hose = x + hose;
    let extent = extent.min(drop) - 0.005;
    if extent > 0.15 && trimmed {
        d.tube(
            "paint",
            &[
                [hose, depth - 0.05, z],
                [hose, depth - 0.06, z - 0.12],
                [hose + 0.03, depth - 0.1, z - extent],
            ],
            heavy(0.011, 0.0135),
            srgb("#3a3a38"),
            5,
        )?;
    }
    Ok(Bounds { x0, x1, z })
}

fn fan_rotor(
    d: &mut Fixture<'_, '_>,
    center: V3,
    radius: f64,
    blades: usize,
) -> Result<(), String> {
    let [x, y, z] = center;
    let dark = srgb("#50564f");
    let point =
        |angle: f64, r: f64, pitch: f64| [x + cos(angle) * r, y + pitch, z + sin(angle) * r];
    for blade in 0..blades {
        let start = blade as f64 / blades as f64 * PI * 2.;
        for ring in 0..4 {
            let inner = 0.18 + ring as f64 * 0.19;
            let outer = inner + 0.19;
            let a = start + inner * 0.55;
            let b = start + outer * 0.55;
            d.sheet(
                "metal",
                &[
                    point(a, inner * radius, -0.007),
                    point(b, outer * radius, -0.007),
                    point(b + 0.56, outer * radius, 0.012),
                    point(a + 0.56, inner * radius, 0.012),
                ],
                dark,
            )?;
        }
    }
    d.tube(
        "metal",
        &[[x, y - 0.025, z], [x, y + 0.018, z]],
        radius * 0.19,
        dark,
        20,
    )?;
    d.disc(
        "metal",
        [x, y + 0.018, z],
        radius * 0.19,
        [0., 1., 0.],
        dark,
        20,
    )
}
fn fan_front(
    d: &mut Fixture<'_, '_>,
    span: [f64; 4],
    y: f64,
    cell: usize,
    fan: usize,
    tint: V3,
) -> Result<(), String> {
    let [x0, x1, z0, z1] = span;
    let project = on_face(span);
    let cx = x0 + (x1 - x0) * 0.37;
    let cz = (z0 + z1) / 2.;
    let radius = (z1 - z0) * 0.4;
    let detail = d.mesh.graphics_detail();
    if detail == GraphicsDetail::VeryLow {
        // The face whole, and the fan as a dark disc on it.
        unwrapped(
            d,
            cell,
            &[[x0, y, z0], [x1, y, z0], [x1, y, z1], [x0, y, z1]],
            [0., 1., 0.],
            tint,
            &project,
        )?;
        return d.disc(
            "paint",
            [cx, y + 0.002, cz],
            radius * 0.96,
            [0., 1., 0.],
            srgb("#222724"),
            8,
        );
    }
    // Level 1 keeps the shrouded opening on a coarser rim, without rotor, guard or boss.
    let rim_sides = detail.pick(40, 12, 12);
    let mut angles: Vec<_> = (0..rim_sides)
        .map(|i| i as f64 / rim_sides as f64 * PI * 2.)
        .collect();
    for [x, z] in [[x0, z0], [x1, z0], [x1, z1], [x0, z1]] {
        angles.push((atan2(z - cz, x - cx) + PI * 2.) % (PI * 2.));
    }
    angles.sort_by(f64::total_cmp);
    let rim = |angle: f64, scale: f64, depth: f64| {
        [
            cx + cos(angle) * radius * scale,
            depth,
            cz + sin(angle) * radius * scale,
        ]
    };
    let edge = |angle: f64| {
        let c = cos(angle);
        let s = sin(angle);
        let reach_x = if c.abs() < 1e-9 {
            f64::INFINITY
        } else {
            ((if c > 0. { x1 } else { x0 }) - cx) / c
        };
        let reach_z = if s.abs() < 1e-9 {
            f64::INFINITY
        } else {
            ((if s > 0. { z1 } else { z0 }) - cz) / s
        };
        let reach = reach_x.min(reach_z);
        [cx + c * reach, y, cz + s * reach]
    };
    let shroud = y - 0.05;
    for i in 0..angles.len() {
        let a = angles[i];
        let b = if i + 1 < angles.len() {
            angles[i + 1]
        } else {
            angles[0] + PI * 2.
        };
        if (b - a).abs() < 1e-6 {
            continue;
        }
        let middle = [-cos((a + b) / 2.), 0.3, -sin((a + b) / 2.)];
        unwrapped(
            d,
            cell,
            &[rim(a, 1., y), rim(b, 1., y), edge(b), edge(a)],
            [0., 1., 0.],
            tint,
            &project,
        )?;
        d.face(
            "paint",
            &[
                rim(a, 1., y),
                rim(b, 1., y),
                rim(b, 0.96, shroud),
                rim(a, 0.96, shroud),
            ],
            middle,
            SHADOW,
        )?;
    }
    d.disc(
        "paint",
        [cx, shroud - 0.035, cz],
        radius * 0.96,
        [0., 1., 0.],
        srgb("#222724"),
        rim_sides,
    )?;
    if detail.reduced() {
        return Ok(());
    }
    fan_rotor(
        d,
        [cx, shroud + 0.01, cz],
        radius * 0.9,
        if fan == 14 { 3 } else { 5 },
    )?;
    let wire = shade(tint, 0.72);
    let proud = y + 0.014;
    for scale in [0.26, 0.42, 0.58, 0.74, 0.9, 0.99] {
        let path: Vec<_> = (0..41)
            .map(|i| {
                rim(
                    i as f64 / 40. * PI * 2.,
                    scale,
                    y + 0.006 + 0.012 * (1. - scale * scale),
                )
            })
            .collect();
        d.tube("metal", &path, 0.0019, wire, 5)?;
    }
    for i in 0..10 {
        d.tube(
            "metal",
            &[
                rim(i as f64 / 10. * PI * 2., 0.2, proud),
                rim(i as f64 / 10. * PI * 2., 0.99, y + 0.006),
            ],
            0.0018,
            wire,
            5,
        )?;
    }
    d.disc(
        "metal",
        [cx, proud + 0.002, cz],
        radius * 0.19,
        [0., 1., 0.],
        shade(tint, 0.9),
        24,
    )
}
fn line_set(
    d: &mut Fixture<'_, '_>,
    from: V3,
    rise: f64,
    white: V3,
    weight: f64,
) -> Result<(), String> {
    let [x, y, z] = from;
    let out = x + 0.07;
    let insulation = srgb("#d8d6cc");
    let hose = srgb("#3a3a38");
    let (lagged, drain) = (weighed(weight, 0.016, 0.02), weighed(weight, 0.011, 0.0135));
    // Lines and their duct are a few centimetres across: level 2 draws the unit alone.
    if d.mesh.graphics_detail() == GraphicsDetail::VeryLow {
        return Ok(());
    }
    if rise < 0.3 {
        d.tube(
            "paint",
            &[[x, y, z], [out, y, z], [out, 0.01, z + 0.04]],
            lagged,
            insulation,
            6,
        )?;
        return d.tube(
            "paint",
            &[
                [x, y, z - 0.05],
                [out + 0.03, y, z - 0.05],
                [out + 0.03, 0.01, z],
            ],
            drain,
            hose,
            5,
        );
    }
    d.tube(
        "paint",
        &[[x, y, z], [out, y, z], [out, 0.04, z + 0.05]],
        lagged,
        insulation,
        6,
    )?;
    d.tube(
        "paint",
        &[[x, y, z - 0.05], [out, y, z - 0.05], [out, 0.04, z]],
        drain,
        hose,
        5,
    )?;
    let top = z + rise;
    let half = 0.04;
    let deep = 0.055;
    let mut foot = z - 0.02;
    while foot < top - 1e-6 {
        let head = top.min(foot + 0.5);
        let share = (head - foot) / 0.5;
        let face = |p: V3| {
            [
                0.25 + ((p[0] - out + half) / (2. * half)) * 0.5,
                ((head - p[2]) / (head - foot)) * share,
            ]
        };
        let flank = |p: V3| {
            [
                (p[1] / deep) * 0.25,
                ((head - p[2]) / (head - foot)) * share,
            ]
        };
        unwrapped(
            d,
            25,
            &[
                [out - half, deep, foot],
                [out + half, deep, foot],
                [out + half, deep, head],
                [out - half, deep, head],
            ],
            [0., 1., 0.],
            white,
            &face,
        )?;
        for side in [-1., 1.] {
            unwrapped(
                d,
                25,
                &[
                    [out + side * half, 0., foot],
                    [out + side * half, deep, foot],
                    [out + side * half, deep, head],
                    [out + side * half, 0., head],
                ],
                [side, 0., 0.],
                shade(white, 0.85),
                &flank,
            )?;
        }
        foot += 0.5;
    }
    d.face(
        "paint",
        &[
            [out - half, 0., top],
            [out + half, 0., top],
            [out + half, deep, top],
            [out - half, deep, top],
        ],
        [0., 0., 1.],
        shade(white, 0.9),
    )
}
/// The widest fan front is this far inside a condenser's case edge.
const BEVEL: f64 = 0.014;
fn fan_height_of(width: f64) -> f64 {
    (width - 2. * BEVEL) / FAN_ASPECT + 2. * BEVEL
}
/// Everything that shapes a condenser in a given span: one catalogue entry.
#[derive(Clone, Copy, Debug)]
struct Condenser {
    w: f64,
    fans: usize,
    /// The wear class of its artwork: clean, worn or rusted cells.
    wear: usize,
    /// The atlas cells of its fan front, coil side and top, and which fan it turns.
    front: usize,
    coil: usize,
    top: usize,
    fan: usize,
    /// How far the case stands from the left of its room.
    offset: f64,
    /// How far the line set's duct rises up the wall.
    rise: f64,
    cover: Option<Cover>,
}
/// A condenser's colours: the instance's own, whatever its shape.
#[derive(Clone, Copy, Debug)]
struct CondenserPaint {
    body: V3,
    steel: V3,
    /// The line set's duct.
    duct: V3,
    cover: CoverPaint,
}
fn condenser_steel(rust: f64) -> V3 {
    mix(
        shade(srgb("#a4a9aa"), 0.6),
        shade(srgb("#7a4a2a"), 0.75),
        rust,
    )
}
/// A condenser's shape and colours in `span` as one stream draws them together, in the
/// order the reference draws them.
fn seeded_condenser(
    rng: &mut SeededRng,
    span: [f64; 4],
    fixed: Option<[usize; 2]>,
) -> (Condenser, CondenserPaint) {
    let [x0, x1, _, height] = span;
    let spare = x1 - x0 - 0.06 - 0.15;
    let widths: Vec<_> = [0.7, 0.78, 0.86]
        .into_iter()
        .filter(|w| *w <= spare && fan_height_of(*w) <= height)
        .collect();
    let w = if widths.is_empty() {
        0.7
    } else {
        rng.pick(&widths)
    };
    let fan_height = fan_height_of(w);
    let fans = if height >= fan_height * 2. + 0.08 && rng.chance(0.25) {
        2
    } else {
        1
    };
    let h = fan_height * fans as f64;
    let wear = wear(rng, [3., 4., 2.]);
    let front = if wear == 2 { 10 } else { rng.pick(&[8, 9, 11]) };
    let [wear, front] = fixed.unwrap_or([wear, front]);
    let body = mix(
        body(rng),
        srgb("#cdbf98"),
        wear as f64 * rng.float(0.05, 0.25),
    );
    let offset = rng.float(0., (spare - w).max(0.));
    let coil = if rng.chance(0.3) { 7 } else { 5 + wear.min(1) };
    let top = if wear > 0 && rng.chance(0.5) { 4 } else { 3 };
    let fan = rng.pick(&[14, 15]);
    let steel = condenser_steel(rng.float(0.15, 0.75));
    let rise = rng.float(0.5, 1.3).min(height - 0.2);
    let duct = if rise >= 0.3 {
        shade([1.; 3], rng.float(0.78, 0.95))
    } else {
        [0.; 3]
    };
    let (cover, cover_paint) = if wear > 0 && height >= h + 0.14 && rng.chance(0.3) {
        let (cover, paint) = seeded_cover(rng);
        (Some(cover), paint)
    } else {
        (
            None,
            CoverPaint {
                sheet: [0.; 3],
                wood: [0.; 3],
                brick: [0.; 3],
            },
        )
    };
    (
        Condenser {
            w,
            fans,
            wear,
            front,
            coil,
            top,
            fan,
            offset,
            rise,
            cover,
        },
        CondenserPaint {
            body,
            steel,
            duct,
            cover: cover_paint,
        },
    )
}
/// One catalogue entry of the condenser in `span`. Four entries are the four fan fronts
/// (the third is the rusted one, with the rusted wear class); the other fronts alternate
/// worn and clean from one four to the next. An entry's fans, coil, top, place in its room,
/// duct rise and cover are drawn from its own stream.
fn condenser_entry(entry: u32, span: [f64; 4]) -> (Condenser, CondenserPaint) {
    let front = [8, 9, 10, 11][entry as usize % 4];
    let wear = if front == 10 {
        2
    } else {
        (entry as usize / 4 + usize::from(entry % 4 == 0)) % 2
    };
    seeded_condenser(
        &mut variants::stream("condenser", entry),
        span,
        Some([wear, front]),
    )
}
/// The condenser of `rng` in `span` under `budget`. With a catalogue the stream's first draw
/// picks the entry and its next three are the instance's one colour, its case paint, as for a
/// window unit; its bracket steel, its line set's duct and its cover are the entry's. An
/// entry is one shape in one span: the room it is given still decides which widths fit, and
/// caps its duct's rise.
fn planned_condenser(
    rng: &mut SeededRng,
    span: [f64; 4],
    budget: Budget,
) -> (Option<u32>, Condenser, CondenserPaint) {
    let Some(entry) = budget.pick(rng) else {
        let (condenser, paint) = seeded_condenser(rng, span, None);
        return (None, condenser, paint);
    };
    let (condenser, paint) = condenser_entry(entry, span);
    let aged = wear(rng, [3., 4., 2.]);
    let body = mix(
        body(rng),
        srgb("#cdbf98"),
        aged as f64 * rng.float(0.05, 0.25),
    );
    (Some(entry), condenser, CondenserPaint { body, ..paint })
}
/// The catalogue entry the condenser of `rng` in `span` is under `budget` and its own case
/// paint, as [`condenser_with`] draws them from the same stream; `None` where every
/// condenser draws its own shape. Condensers of one entry in one span differ by that paint
/// alone.
pub fn condenser_instance(
    rng: &mut SeededRng,
    span: [f64; 4],
    budget: Budget,
) -> Option<(u32, V3)> {
    let (entry, _, paint) = planned_condenser(rng, span, budget);
    Some((entry?, paint.body))
}
/// Catalogue entry `entry` of the condenser in `span` with its case painted `body`: the
/// shape every condenser of that entry shares, in the entry's own steel, duct and cover.
pub fn condenser_of_entry(
    d: &mut Fixture<'_, '_>,
    span: [f64; 4],
    entry: u32,
    body: V3,
    options: Options,
) -> Result<Bounds, String> {
    let (condenser, paint) = condenser_entry(entry, span);
    drawn_condenser(
        d,
        span,
        condenser,
        CondenserPaint { body, ..paint },
        None,
        options,
    )
}
/// One- or two-fan condenser within [left,right,base,free height], including its support/line-set allowances.
pub fn condenser(
    d: &mut Fixture<'_, '_>,
    span: [f64; 4],
    rng: &mut SeededRng,
    weathering: Option<&Settings>,
) -> Result<Bounds, String> {
    condenser_with(d, span, rng, weathering, Options::default())
}
/// [`condenser`] with what `options` adds (`louvred` does not apply to a condenser).
pub fn condenser_with(
    d: &mut Fixture<'_, '_>,
    span: [f64; 4],
    rng: &mut SeededRng,
    weathering: Option<&Settings>,
    options: Options,
) -> Result<Bounds, String> {
    let [x0, x1, _, height] = span;
    if span.iter().any(|v| !v.is_finite() || v.abs() > 10000.)
        || !(CONDENSER_ROOM..=100.).contains(&(x1 - x0))
        || !(CONDENSER_HEIGHT..=100.).contains(&height)
    {
        return Err(
            "Condenser needs bounded finite placement and its declared minimum clearance.".into(),
        );
    }
    let (_, condenser, paint) = planned_condenser(rng, span, options.variants);
    drawn_condenser(d, span, condenser, paint, weathering, options)
}
/// Draws a planned condenser in its colours.
fn drawn_condenser(
    d: &mut Fixture<'_, '_>,
    span: [f64; 4],
    condenser: Condenser,
    paint: CondenserPaint,
    weathering: Option<&Settings>,
    options: Options,
) -> Result<Bounds, String> {
    let [x0, _, z, _] = span;
    let bevel = BEVEL;
    let Condenser {
        w,
        fans,
        wear,
        front,
        coil,
        top,
        fan,
        offset,
        rise,
        cover,
    } = condenser;
    let body = paint.body;
    let fan_height = fan_height_of(w);
    let h = fan_height * fans as f64;
    let depth = w * 0.36;
    let left = x0 + 0.06 + offset;
    let right = left + w;
    let y0 = 0.07;
    let y1 = y0 + depth;
    let cells = Cells {
        front,
        left: coil,
        right: 12 + wear.min(1),
        top,
        bottom: 3,
        back: Some(coil),
    };
    casing(d, [left, y0, z], [right, y1, z + h], bevel, cells, body)?;
    // A reduced level's square case has no chamfer for the fan fronts to stand inside.
    let squared = d.mesh.graphics_detail().reduced();
    let rim = if squared { 0. } else { bevel };
    for i in 0..fans {
        let foot = z + i as f64 * fan_height;
        fan_front(
            d,
            [left + rim, right - rim, foot + rim, foot + fan_height - rim],
            y1,
            front,
            fan,
            body,
        )?;
    }
    if fans == 2 && !squared {
        d.panel(
            "metal",
            [
                left + bevel,
                right - bevel,
                z + fan_height - bevel,
                z + fan_height + bevel,
            ],
            y1,
            body,
        )?;
    }
    let fasteners = Steel {
        slot: "metal",
        tint: shade(body, 0.65),
        finish: Finish::Metal(None),
    };
    for x in [left + 0.035, right - 0.035] {
        for v in [z + 0.035, z + h - 0.035] {
            steel::bolt(d, fasteners, [x, y1 + 0.001, v], [0., 1., 0.], 0.004)?;
        }
    }
    // Level 1 keeps the service box and the rails under the case; the rubber feet go.
    // Level 2 draws the case.
    let detail = d.mesh.graphics_detail();
    let trimmed = detail != GraphicsDetail::VeryLow;
    if trimmed {
        d.cuboid(
            "metal",
            [right, y0 + depth * 0.3, z + 0.05],
            [right + 0.035, y1 - depth * 0.12, z + fan_height * 0.5],
            shade(body, 0.95),
            "l",
        )?;
    }
    for rail in [left + 0.1, right - 0.1] {
        if !trimmed {
            break;
        }
        d.cuboid(
            "metal",
            [rail - 0.03, y0 + 0.02, z - 0.016],
            [rail + 0.03, y1 - 0.02, z],
            shade(body, 0.75),
            "u",
        )?;
        if detail.reduced() {
            continue;
        }
        for y in [y0 + 0.05, y1 - 0.05] {
            d.drum(
                "paint",
                [rail, y, z - 0.04],
                0.024,
                0.024,
                srgb("#3a3a38"),
                10,
            )?;
            steel::bolt(d, fasteners, [rail, y, z - 0.012], [0., 0., 1.], 0.006)?;
        }
    }
    brackets(
        d,
        [left + 0.03, right - 0.03],
        y1,
        z - 0.04,
        0.32,
        paint.steel,
        weathering,
        options,
    )?;
    line_set(
        d,
        [right + 0.035, (y0 + y1) / 2., z + 0.14],
        rise,
        paint.duct,
        options.weight,
    )?;
    if let Some(cover) = cover {
        rain_cover(d, [left, right], y1, z + h, cover, paint.cover)?;
    }
    Ok(Bounds {
        x0: left,
        x1: right,
        z,
    })
}
/// The first `entries` catalogue entries of the window unit and of the condenser in `span`,
/// one line each, for a survey.
pub fn catalogue(entries: u32, span: [f64; 4]) -> Vec<String> {
    let mut lines = Vec::new();
    for entry in 0..entries {
        let (unit, _) = unit_entry(entry);
        lines.push(format!("window-unit {entry}: {unit:?}"));
    }
    for entry in 0..entries {
        let (condenser, _) = condenser_entry(entry, span);
        lines.push(format!("condenser {entry}: {condenser:?}"));
    }
    lines
}

//! What stands against a building's party edges: the plain wall of a neighbour built hard
//! against it, or a short slice of an adjoining building with its own storeys. Conventions
//! are those of the parent module (wall plane y = 0, the fixture pen's y runs outward).
use super::{Arguments, draw, fixture, hardware, on};
use crate::itemspec::definition::{Feature, NativeOutput, Values};
use crate::worldspec::themes::kowloon::{
    aircon,
    fixture::Fixture,
    household,
    noise::value_noise,
    palette::{mix, shade, srgb},
    pen::PatchVertex,
    windows,
};
use superspec_geometry::V3;
use superspec_types::rng::SeededRng;

/// Paints neighbours are seen in, mixed into the building's own.
const PAINTS: [&str; 5] = ["#b9ad96", "#a8a294", "#c4b9a2", "#9fa39a", "#b5a48a"];

/// The tint of weathered render at `s` m across a face and `z` m up, under a top at `top`:
/// dark runs down from the top, soot at the foot and paler patches of newer render.
fn stain(s: f64, z: f64, top: f64, base: V3, seed: f64) -> V3 {
    let streak = value_noise(s * 3.1, z * 0.35, seed);
    let runs = ((streak - 0.45) * 2.).max(0.) * (1. - 0.75 * ((top - z) / top).clamp(0., 1.));
    let foot = 0.32 / (1. + (z / 0.9) * (z / 0.9));
    let mottle = value_noise(s * 0.9, z * 0.7, seed + 11.);
    let patched = value_noise(s * 0.45, z * 0.4, seed + 5.);
    let render = mix(
        base,
        shade(base, 1.12),
        ((patched - 0.62) * 6.).clamp(0., 1.),
    );
    let dark = (0.42 * runs + foot + 0.12 * (mottle - 0.5)).clamp(0., 0.65);
    std::array::from_fn(|k| render[k] * (1. - dark * [0.42, 0.48, 0.55][k]))
}

/// A stained face from `origin`, `across` (m) by `up` (m, along z): rows run across and
/// columns up, so the face looks along `across` × z, or the other way when `flipped`.
#[allow(clippy::too_many_arguments)]
fn stained_face(
    d: &mut Fixture<'_, '_>,
    origin: V3,
    across: V3,
    up: f64,
    flipped: bool,
    s0: f64,
    top: f64,
    base: V3,
    seed: f64,
) -> Result<(), String> {
    let width = (across[0] * across[0] + across[1] * across[1]).sqrt();
    let rows = ((width / 0.15).ceil() as usize).clamp(1, 400);
    let columns = ((up / 0.4).ceil() as usize).clamp(1, 120);
    let grid: Vec<Vec<PatchVertex>> = (0..=rows)
        .map(|r| {
            let t = r as f64 / rows as f64;
            (0..=columns)
                .map(|c| {
                    let z = origin[2] + up * c as f64 / columns as f64;
                    let s = s0 + t * width;
                    PatchVertex {
                        at: [origin[0] + across[0] * t, origin[1] + across[1] * t, z],
                        uv: [s, z],
                        tint: stain(s, z, top, base, seed),
                    }
                })
                .collect()
        })
        .collect();
    d.pen.patch(d.mesh, "wall", &grid, flipped)
}

/// A window drawn proud of a face: `at` maps (m across the face, m up, m out of it) to the
/// fixture's frame, which must keep the face's axes aligned with the frame's.
fn window(
    d: &mut Fixture<'_, '_>,
    at: &dyn Fn(f64, f64, f64) -> V3,
    normal: V3,
    [s0, s1, z0, z1]: [f64; 4],
    frame: V3,
    trim: V3,
) -> Result<(), String> {
    let mut block = |slot: &str, a: [f64; 3], b: [f64; 3], tint: V3| -> Result<(), String> {
        let (p, q) = (at(a[0], a[1], a[2]), at(b[0], b[1], b[2]));
        let lo = std::array::from_fn(|k| p[k].min(q[k]));
        let hi = std::array::from_fn(|k| p[k].max(q[k]));
        d.cuboid(slot, lo, hi, tint, "")
    };
    let bar = 0.045;
    block("frame", [s0, z0, 0.], [s1, z0 + bar, 0.035], frame)?;
    block("frame", [s0, z1 - bar, 0.], [s1, z1, 0.035], frame)?;
    block(
        "frame",
        [s0, z0 + bar, 0.],
        [s0 + bar, z1 - bar, 0.035],
        frame,
    )?;
    block(
        "frame",
        [s1 - bar, z0 + bar, 0.],
        [s1, z1 - bar, 0.035],
        frame,
    )?;
    let mid = (s0 + s1) / 2.;
    block(
        "frame",
        [mid - 0.015, z0 + bar, 0.],
        [mid + 0.015, z1 - bar, 0.03],
        frame,
    )?;
    block(
        "frame",
        [s0 + bar, z0 + (z1 - z0) * 0.68, 0.],
        [s1 - bar, z0 + (z1 - z0) * 0.68 + 0.03, 0.03],
        frame,
    )?;
    block(
        "trim",
        [s0 - 0.06, z0 - 0.06, 0.],
        [s1 + 0.06, z0, 0.07],
        trim,
    )?;
    let glass = [
        at(s0 + bar, z0 + bar, 0.012),
        at(s1 - bar, z0 + bar, 0.012),
        at(s1 - bar, z1 - bar, 0.012),
        at(s0 + bar, z1 - bar, 0.012),
    ];
    d.face("pane", &glass, normal, srgb("#262624"))
}

/// `kowloon/party`: what stands against a building's party edge, from its start along
/// `length` m on the wall plane. `wall` is the plain stained wall of a neighbour built hard
/// against it: 8 cm proud of the building, a little past both its ends, full height and
/// somewhat taller or shorter than its parapet `top` by seed, under a coping. `stub` is a
/// slice of the adjoining building 1.1–1.9 m deep (seeded): `own` storeys (0: a seeded few
/// about the building's `storeys`) of `ground` and `storey` m with rows of windows on its
/// faces (a few with a window unit under the sill), string courses, a drain and a roof behind a
/// parapet of its own; its front is a back lane's: barred windows in the ground storey (and
/// `bars` of the upper ones), a back door in a concrete surround under a hood and a lamp
/// (`door`), soil and vent stacks up past the parapet (`stacks`) and `cables` under its
/// courses, all from a stream of their own. Slots: `wall` (stained render, tinted per vertex),
/// `trim` (copings, courses, sills and the door's surround), `frame` (windows and the door's
/// leaf), `pane`, `metal` (pipes, cables, clips), and the units' `aircon`, `paint` (and the
/// bars'), `sheet`. Parameters: `kind` (`wall` |
/// `stub`, `wall`), `length` (m, 0.3–60, 6), `top` (m, 2–80, 12.5), `ground` (m, 2–8, 3.3),
/// `storey` (m, 2.4–6, 3), `storeys` (1–20, 4), `own` (0–20, 0), `color` (#b4a88f), `frame`
/// (#4b5257), `door` (true), `stacks` (0–4, -1: one or two), `cables` (0–6, 2), `bars`
/// (0–1, 0.3), `units` (0–1, 0.45), `seed`. Drying racks and extract vents dress a seeded few
/// upper windows.
///
/// Features ([`features`]), in item axes with the faces' outward normals (`front` faces -y):
/// `top` (the foot of the coping), `base`, and on a stub `eave` (under every string course) and
/// `sill` (under every window's sill), on each face that has them (`front`, the ends `start` and
/// `end`, and `back` where it shows above the building), and `drip` under each unit, so a part's
/// zone rules hang runoff below them as below a facade's storeys and sills.
pub(super) fn party(
    name: &str,
    values: &Values,
    cancelled: &dyn Fn() -> bool,
) -> Result<NativeOutput, String> {
    let plan = Party::read(name, values)?;
    if plan.stub {
        stub(&plan, cancelled)
    } else {
        wall(&plan, cancelled)
    }
}

/// What `kowloon/party` stands, read and seeded once for its meshes and for its features.
struct Party {
    stub: bool,
    length: f64,
    top: f64,
    ground: f64,
    storey: f64,
    frame: V3,
    paint: V3,
    trim: V3,
    noise_seed: f64,
    /// Where the coping sits.
    high: f64,
    /// A stub's storeys, roof (its deck's height), depth, bays, blank bays, units and drain.
    floors: usize,
    roof: f64,
    depth: f64,
    bays: usize,
    blank: Vec<bool>,
    units: Vec<bool>,
    pipe: f64,
    /// What a back lane shows on a stub, from a stream of its own so the rest stays the
    /// seed's: the bay of its back door, which upper windows are barred, its soil and vent
    /// stacks (m along the front) and cables (from, to, height, sag), and the seed of its units.
    door: Option<usize>,
    barred: Vec<bool>,
    stacks: Vec<f64>,
    cables: Vec<[f64; 4]>,
    lane: String,
}

/// A face of a stub: its outer face, or an end.
#[derive(Clone, Copy, PartialEq)]
enum Face {
    Front,
    Start,
    End,
}

/// One of a stub's windows: its face, its span (m across the face, m up), whether a unit
/// hangs under its sill and whether bars guard it.
struct Opening {
    face: Face,
    span: [f64; 4],
    unit: bool,
    barred: bool,
}

/// A plain party wall's back and face (fixture y, outward), and how far it runs past the
/// edge's ends.
const WALL: [f64; 3] = [-0.25, 0.08, 0.06];
/// A stub's wall thickness behind its parapet.
const STUB_WALL: f64 = 0.2;

impl Party {
    fn read(name: &str, values: &Values) -> Result<Self, String> {
        let mut a = Arguments::new(name, values);
        let stub = a.text("kind", "wall")?;
        if stub != "wall" && stub != "stub" {
            return Err(format!("{name}: kind must be one of wall, stub."));
        }
        let stub = stub == "stub";
        let length = a.bounded("length", 6., 0.3..=60.)?;
        let top = a.bounded("top", 12.5, 2. ..=80.)?;
        let ground = a.bounded("ground", 3.3, 2. ..=8.)?;
        let storey = a.bounded("storey", 3., 2.4..=6.)?;
        let storeys = a.integer("storeys", 4, 1..=20)?;
        let own = a.integer("own", 0, 0..=20)?;
        let color = a.color("color", "#b4a88f")?;
        let frame = a.color("frame", "#4b5257")?;
        // A stub's back lane: `door` (a back door in a ground bay, true), `stacks` (soil and
        // vent stacks up its front, 0–4; -1, the default, one or two by seed), `cables` (0–6,
        // 2) and `bars` (the share of upper windows barred, 0–1, 0.3; the ground storey's
        // always are).
        let door = a.boolean("door", true)?;
        let stacks = match a.bounded("stacks", -1., -1. ..=4.)? {
            count if count.fract() != 0. => {
                return Err(format!("{name}: stacks must be a whole number in -1..4."));
            }
            count => (count >= 0.).then_some(count as usize),
        };
        let cables = a.integer("cables", 2, 0..=6)?;
        let bar_share = a.bounded("bars", 0.3, 0. ..=1.)?;
        // `units`: the share of upper front windows with a unit under the sill (0–1, 0.45).
        let unit_share = a.bounded("units", 0.45, 0. ..=1.)?;
        let key = a.seed()?;
        a.finish()?;
        let mut rng = SeededRng::from_text(&key);
        let noise_seed = rng.float(0., 1000.);
        // Pale, as the render under it is bare (a facade's paint is a layer of its own).
        let paint = mix(mix(color, srgb(rng.pick(&PAINTS)), 0.45), [1.; 3], 0.22);
        let trim = shade(paint, 0.86);
        let mut plan = Party {
            stub,
            length,
            top,
            ground,
            storey,
            frame,
            paint,
            trim,
            noise_seed,
            high: 0.,
            floors: 0,
            roof: 0.,
            depth: 0.,
            bays: 0,
            blank: vec![],
            units: vec![],
            pipe: 0.,
            door: None,
            barred: vec![],
            stacks: vec![],
            cables: vec![],
            lane: format!("{key}/lane"),
        };
        if !stub {
            plan.high = (top + rng.float(-0.35, 0.9)).max(2.);
            return Ok(plan);
        }
        let floors = if own > 0 {
            own
        } else {
            let low = storeys.saturating_sub(2).max(2);
            low + (rng.float(0., 1.) * (storeys + 2 - low) as f64).floor() as usize
        };
        plan.floors = floors;
        plan.roof = ground + (floors - 1) as f64 * storey;
        let parapet = rng.float(0.4, 1.0);
        plan.high = plan.roof + parapet;
        plan.depth = rng.float(1.1, 1.9);
        plan.bays = ((length - 0.8) / 2.3).floor().max(1.) as usize;
        let bays = plan.bays;
        // Which windows stand: most do; a few bays are blank.
        plan.blank = (0..bays * floors).map(|_| rng.chance(0.15)).collect();
        // And which upper windows have a unit hung under their sill.
        plan.units = (0..bays * floors).map(|_| rng.chance(unit_share)).collect();
        plan.pipe = rng.float(0.25, 0.6);
        let mut lane = SeededRng::from_text(&plan.lane);
        let pitch = length / bays as f64;
        // The back door takes a ground bay wide enough for it and its surround.
        let door_bay = (lane.float(0., 1.) * bays as f64).floor() as usize;
        plan.door = (door && pitch >= 1.5).then_some(door_bay.min(bays - 1));
        plan.barred = (0..bays * floors).map(|_| lane.chance(bar_share)).collect();
        // Stacks stand between bays (or near the start of a single one), clear of the drain
        // and the door.
        let drain = length - plan.pipe;
        let door_x = plan.door.map(|bay| (bay as f64 + 0.5) * pitch);
        let mut spots: Vec<f64> = if bays > 1 {
            (1..bays).map(|k| k as f64 * pitch).collect()
        } else {
            vec![0.35]
        };
        spots.retain(|x| {
            (x - drain).abs() > 0.35 && door_x.is_none_or(|door| (x - door).abs() > 0.75)
        });
        let wanted = stacks.unwrap_or(1 + lane.chance(0.5) as usize);
        while plan.stacks.len() < wanted && !spots.is_empty() {
            let pick = (lane.float(0., 1.) * spots.len() as f64).floor() as usize;
            plan.stacks.push(spots.remove(pick.min(spots.len() - 1)));
        }
        // Cables hang under a string course (or the parapet) across most of the front.
        for _ in 0..cables {
            let under = if floors > 1 {
                let level = 1 + (lane.float(0., 1.) * (floors - 1) as f64).floor() as usize;
                plan.ground + (level.min(floors - 1) - 1) as f64 * plan.storey - 0.06
            } else {
                plan.roof
            };
            plan.cables.push([
                length * lane.float(0.02, 0.35),
                length * lane.float(0.65, 0.98),
                under - lane.float(0.12, 0.3),
                lane.float(0.08, 0.3),
            ]);
        }
        Ok(plan)
    }

    /// Heights of the string courses' middles, one a floor above the ground storey.
    fn courses(&self) -> impl Iterator<Item = f64> + '_ {
        (1..self.floors).map(|level| self.ground + (level - 1) as f64 * self.storey)
    }

    /// A stub's windows in the order they are drawn: a row a storey along its outer face,
    /// small ones in the ground storey, and one a storey on each end when it is deep enough.
    fn openings(&self) -> Vec<Opening> {
        let mut openings = vec![];
        let pitch = self.length / self.bays as f64;
        for level in 0..self.floors {
            let floor = if level == 0 {
                0.
            } else {
                self.ground + (level - 1) as f64 * self.storey
            };
            let (sill, tall, wide) = if level == 0 {
                (1.35, 0.8, 0.8)
            } else {
                (0.95, 1.3, 1.15_f64.min(pitch - 0.6))
            };
            let [z0, z1] = [floor + sill, floor + sill + tall];
            if z1 > self.roof - 0.25 || wide < 0.4 {
                continue;
            }
            for bay in 0..self.bays {
                if self.blank[level * self.bays + bay] || (level == 0 && self.door == Some(bay)) {
                    continue;
                }
                let middle = (bay as f64 + 0.5) * pitch;
                openings.push(Opening {
                    face: Face::Front,
                    span: [middle - wide / 2., middle + wide / 2., z0, z1],
                    unit: level > 0 && self.units[level * self.bays + bay],
                    barred: level == 0 || self.barred[level * self.bays + bay],
                });
            }
            let narrow = 0.7_f64.min(self.depth - 0.55);
            if level > 0 && narrow >= 0.45 {
                let middle = self.depth / 2.;
                let span = [middle - narrow / 2., middle + narrow / 2., z0, z1];
                for face in [Face::Start, Face::End] {
                    openings.push(Opening {
                        face,
                        span,
                        unit: false,
                        barred: false,
                    });
                }
            }
        }
        openings
    }
}

/// A plain wall: greyer than render, as a party wall left unpainted.
fn wall(plan: &Party, cancelled: &dyn Fn() -> bool) -> Result<NativeOutput, String> {
    let base = mix(plan.paint, srgb("#97928a"), 0.4);
    let (high, top, noise_seed) = (plan.high, plan.top, plan.noise_seed);
    let [back, face, past] = WALL;
    let (x0, x1) = (-past, plan.length + past);
    draw(cancelled, |mesh| {
        let d = &mut fixture(mesh, "party")?;
        d.cuboid("wall", [x0, back, 0.], [x1, face, high], base, "bfdu")?;
        stained_face(
            d,
            [x0, face, 0.],
            [x1 - x0, 0., 0.],
            high,
            false,
            0.,
            high,
            base,
            noise_seed,
        )?;
        if high > top + 0.05 {
            // Above the building's parapet its back shows too.
            stained_face(
                d,
                [x0, back, top],
                [x1 - x0, 0., 0.],
                high - top,
                true,
                0.,
                high,
                base,
                noise_seed + 3.,
            )?;
        }
        d.cuboid(
            "trim",
            [x0 - 0.03, back - 0.03, high],
            [x1 + 0.03, face + 0.03, high + 0.05],
            plan.trim,
            "d",
        )
    })
}

/// A slice of an adjoining building.
fn stub(plan: &Party, cancelled: &dyn Fn() -> bool) -> Result<NativeOutput, String> {
    let Party {
        length,
        top,
        frame,
        paint,
        trim,
        noise_seed,
        high,
        roof,
        depth,
        pipe,
        ..
    } = *plan;
    let wall = STUB_WALL;
    let openings = plan.openings();
    draw(cancelled, |mesh| {
        let d = &mut fixture(mesh, "party")?;
        // A string course at every floor, round its three faces, and a drain down one end.
        for z in plan.courses() {
            for (lo, hi) in [
                (
                    [-0.06, depth, z - 0.06],
                    [length + 0.06, depth + 0.06, z + 0.06],
                ),
                ([-0.06, 0., z - 0.06], [0., depth, z + 0.06]),
                ([length, 0., z - 0.06], [length + 0.06, depth, z + 0.06]),
            ] {
                d.cuboid("trim", lo, hi, trim, "")?;
            }
        }
        d.tube(
            "metal",
            &[
                [length - pipe, depth + 0.1, 0.05],
                [length - pipe, depth + 0.1, high - 0.1],
            ],
            0.04,
            srgb("#8f8a7c"),
            8,
        )?;
        // The outer face and both ends, stained from their parapet down.
        stained_face(
            d,
            [0., depth, 0.],
            [length, 0., 0.],
            high,
            false,
            0.,
            high,
            paint,
            noise_seed,
        )?;
        stained_face(
            d,
            [0., 0., 0.],
            [0., depth, 0.],
            high,
            false,
            length,
            high,
            paint,
            noise_seed + 1.,
        )?;
        stained_face(
            d,
            [length, 0., 0.],
            [0., depth, 0.],
            high,
            true,
            length + depth,
            high,
            paint,
            noise_seed + 2.,
        )?;
        if high > top + 0.05 {
            // Taller than the building: its back shows above the building's parapet.
            stained_face(
                d,
                [0., 0., top],
                [length, 0., 0.],
                high - top,
                true,
                0.,
                high,
                paint,
                noise_seed + 4.,
            )?;
        }
        // The roof behind the parapet, and the parapet's inner faces and coping.
        let deck = shade(paint, 0.55);
        d.face(
            "wall",
            &[
                [wall, 0., roof],
                [length - wall, 0., roof],
                [length - wall, depth - wall, roof],
                [wall, depth - wall, roof],
            ],
            [0., 0., 1.],
            deck,
        )?;
        let inner = shade(paint, 0.8);
        d.face(
            "wall",
            &[
                [wall, depth - wall, roof],
                [length - wall, depth - wall, roof],
                [length - wall, depth - wall, high],
                [wall, depth - wall, high],
            ],
            [0., -1., 0.],
            inner,
        )?;
        for (x, normal) in [(wall, 1.), (length - wall, -1.)] {
            d.face(
                "wall",
                &[
                    [x, 0., roof],
                    [x, depth - wall, roof],
                    [x, depth - wall, high],
                    [x, 0., high],
                ],
                [normal, 0., 0.],
                inner,
            )?;
        }
        for (lo, hi) in [
            (
                [-0.03, depth - wall - 0.03, high],
                [length + 0.03, depth + 0.03, high + 0.06],
            ),
            (
                [-0.03, 0., high],
                [wall + 0.03, depth - wall - 0.03, high + 0.06],
            ),
            (
                [length - wall - 0.03, 0., high],
                [length + 0.03, depth - wall - 0.03, high + 0.06],
            ),
        ] {
            d.cuboid("trim", lo, hi, trim, "d")?;
        }
        for (index, opening) in openings.iter().enumerate() {
            let span = opening.span;
            match opening.face {
                Face::Front => {
                    window(
                        d,
                        &|s, z, out| [s, depth + out, z],
                        [0., 1., 0.],
                        span,
                        frame,
                        trim,
                    )?;
                    let middle = (span[0] + span[1]) / 2.;
                    // The front face's own frame: y = 0 on its plane, outward.
                    let face = &mut on(&mut *d.mesh, d.pen.moved([0., depth, 0.])?)?;
                    if opening.barred {
                        // Welded bars over the sash, standing on the sill, as the window
                        // item's: their rods 3.5 cm off the face, on the frame.
                        let set = &mut on(&mut *face.mesh, face.pen.moved([0., -0.06, 0.])?)?;
                        windows::security_bars_sized(
                            set,
                            [
                                span[0] - 0.04,
                                span[2],
                                span[1] - span[0] + 0.08,
                                span[3] - span[2] + 0.05,
                            ],
                            windows::Guard::Bars,
                            srgb("#33363a"),
                            None,
                        )?;
                    }
                    if opening.unit {
                        // A window unit on its bracket under the sill; its foot is the drip.
                        aircon::air_conditioner_with(
                            face,
                            middle,
                            span[2] - 0.62,
                            0.3,
                            &mut SeededRng::from_text(&format!("{}/unit/{index}", plan.lane)),
                            None,
                            aircon::Options {
                                plates: true,
                                louvred: false,
                            },
                        )?;
                    }
                }
                Face::Start => window(
                    d,
                    &|s, z, out| [-out, s, z],
                    [-1., 0., 0.],
                    span,
                    frame,
                    trim,
                )?,
                Face::End => window(
                    d,
                    &|s, z, out| [length + out, s, z],
                    [1., 0., 0.],
                    span,
                    frame,
                    trim,
                )?,
            }
        }
        back_lane(d, plan, &openings)
    })
}

/// What a back lane adds to a stub's front: its back door under a hood, soil and vent stacks
/// up past its parapet, cables under its courses, drying racks over and extract vents beside
/// some upper windows, and the clips that hold its drain.
fn back_lane(d: &mut Fixture<'_, '_>, plan: &Party, openings: &[Opening]) -> Result<(), String> {
    let (length, high) = (plan.length, plan.high);
    // The front face's own frame: y = 0 on its plane, outward.
    let face = &mut on(&mut *d.mesh, d.pen.moved([0., plan.depth, 0.])?)?;
    let grey = srgb("#8f8a7c");
    let mut dress = SeededRng::from_text(&format!("{}/dress", plan.lane));
    for (index, opening) in openings.iter().enumerate() {
        let [s0, s1, z0, z1] = opening.span;
        if opening.face != Face::Front || z0 < plan.ground {
            continue;
        }
        let (racked, vented) = (dress.chance(0.3), dress.chance(0.35));
        if racked {
            let rng = &mut SeededRng::from_text(&format!("{}/rack/{index}", plan.lane));
            household::drying_rack(
                face,
                [s0 - 0.15, s1 + 0.15],
                z1 + 0.12,
                0.55,
                rng,
                None,
                true,
            )?;
        }
        // An extract vent beside the window: a louvred box in a frame.
        let x = s1 + 0.26;
        if vented && x + 0.25 < length && (x - (length - plan.pipe)).abs() > 0.3 {
            let (z, half, tall) = (z1 - 0.3, 0.17, 0.22);
            face.cuboid(
                "metal",
                [x - half, 0., z],
                [x + half, 0.035, z + tall],
                shade(grey, 1.1),
                "b",
            )?;
            for slat in 1..5 {
                let at = z + tall * slat as f64 / 5.;
                face.cuboid(
                    "metal",
                    [x - half + 0.02, 0.035, at - 0.008],
                    [x + half - 0.02, 0.05, at + 0.008],
                    shade(grey, 0.7),
                    "b",
                )?;
            }
        }
    }
    // Clips keep clear of the string courses, which stand 6 cm out.
    let courses: Vec<f64> = plan.courses().collect();
    let clear = |z: f64| courses.iter().all(|c| (z - c).abs() > 0.14) && z < high - 0.2;
    let mut z = 0.6;
    while z < high {
        if clear(z) {
            hardware::pipe_clip(face, [length - plan.pipe, 0.1, z], 0.04, shade(grey, 0.72))?;
        }
        z += 1.5;
    }
    let stack = srgb("#77746c");
    let (radius, out) = (0.055, 0.125);
    for &x in &plan.stacks {
        // Up from the lane to a cowl above the parapet, socketed a storey apart, with a
        // branch into the wall over every floor and clips between.
        let head = high + 0.75;
        face.tube(
            "metal",
            &[[x, out, 0.03], [x, out, head]],
            radius,
            stack,
            12,
        )?;
        face.tube(
            "metal",
            &[[x, out, head], [x, out, head + 0.09]],
            radius * 1.4,
            shade(stack, 0.85),
            12,
        )?;
        for &c in &courses {
            face.tube(
                "metal",
                &[[x, out, c + 0.18], [x, out, c + 0.27]],
                radius * 1.18,
                shade(stack, 0.92),
                12,
            )?;
            face.tube(
                "metal",
                &[[x, out, c + 0.42], [x, -0.02, c + 0.42]],
                radius * 0.7,
                stack,
                10,
            )?;
        }
        let mut z = 0.8;
        while z < high {
            if clear(z) {
                hardware::pipe_clip(face, [x, out, z], radius, shade(stack, 0.72))?;
            }
            z += 1.4;
        }
    }
    for &[x0, x1, z, sag] in &plan.cables {
        let points: Vec<V3> = (0..=16)
            .map(|k| {
                let t = k as f64 / 16.;
                [x0 + (x1 - x0) * t, 0.05, z - sag * 4. * t * (1. - t)]
            })
            .collect();
        face.tube("metal", &points, 0.007, srgb("#1d1d1b"), 6)?;
        for x in [x0, x1] {
            face.cuboid(
                "metal",
                [x - 0.015, 0., z - 0.02],
                [x + 0.015, 0.06, z + 0.02],
                grey,
                "b",
            )?;
        }
    }
    let Some(bay) = plan.door else {
        return Ok(());
    };
    // The back door: a concrete surround standing out of the face with the dark of the
    // passage in it, a steel leaf with pressed louvres, a step, a hood and a lamp over it.
    let x = (bay as f64 + 0.5) * length / plan.bays as f64;
    let (w, h) = (0.9, 2.05);
    let (x0, x1) = (x - w / 2., x + w / 2.);
    let surround = shade(plan.trim, 0.95);
    for (lo, hi) in [
        ([x0 - 0.12, 0., 0.], [x0, 0.12, h + 0.12]),
        ([x1, 0., 0.], [x1 + 0.12, 0.12, h + 0.12]),
        ([x0, 0., h], [x1, 0.12, h + 0.12]),
        ([x0 - 0.3, 0., h + 0.12], [x1 + 0.3, 0.42, h + 0.2]),
        ([x0 - 0.22, 0., 0.], [x1 + 0.22, 0.3, 0.12]),
    ] {
        face.cuboid("trim", lo, hi, surround, "b")?;
    }
    face.face(
        "pane",
        &[
            [x0, 0.004, 0.12],
            [x1, 0.004, 0.12],
            [x1, 0.004, h],
            [x0, 0.004, h],
        ],
        [0., 1., 0.],
        srgb("#1b2225"),
    )?;
    let leaf = mix(plan.frame, srgb("#2f4f43"), 0.5);
    let (l0, l1, z0, z1) = (x0 + 0.03, x1 - 0.03, 0.13, h - 0.02);
    face.cuboid("frame", [l0, 0.012, z0], [l1, 0.05, z1], leaf, "b")?;
    for base in [z0 + 0.22, z1 - 0.42] {
        for slat in 0..5 {
            let z = base + slat as f64 * 0.04;
            face.cuboid(
                "frame",
                [l0 + 0.12, 0.05, z],
                [l1 - 0.12, 0.062, z + 0.022],
                shade(leaf, 0.8),
                "b",
            )?;
        }
    }
    face.cuboid(
        "metal",
        [l0 + 0.02, 0.05, z0 + 0.01],
        [l1 - 0.02, 0.054, z0 + 0.17],
        srgb("#8d8a80"),
        "b",
    )?;
    face.tube(
        "metal",
        &[
            [l1 - 0.09, 0.05, 1.02],
            [l1 - 0.09, 0.1, 1.02],
            [l1 - 0.2, 0.1, 1.02],
        ],
        0.008,
        srgb("#8d8a80"),
        8,
    )?;
    face.cuboid(
        "metal",
        [x - 0.09, 0., h + 0.34],
        [x + 0.09, 0.1, h + 0.5],
        shade(grey, 0.8),
        "b",
    )?;
    face.face(
        "pane",
        &[
            [x - 0.07, 0.102, h + 0.36],
            [x + 0.07, 0.102, h + 0.36],
            [x + 0.07, 0.102, h + 0.48],
            [x - 0.07, 0.102, h + 0.48],
        ],
        [0., 1., 0.],
        srgb("#e8dcb8"),
    )
}

/// The edges `kowloon/party` reports (see [`party`]), in item axes: the fixture pen's
/// outward y is the item's -y. Parameters it refuses give none; drawing reports why.
pub(super) fn features(name: &str, values: &Values) -> Vec<Feature> {
    let Ok(plan) = Party::read(name, values) else {
        return vec![];
    };
    let mut features = vec![];
    // A level edge along a face, from `a` to `b` in fixture axes, facing `normal` (fixture).
    let mut edge = |name: &str, face: &str, a: V3, b: V3, normal: V3| {
        features.push(Feature {
            name: name.into(),
            part: String::new(),
            source: None,
            face: face.into(),
            points: [a, b].map(|p| [p[0], -p[1], p[2]]).to_vec(),
            closed: false,
            normal: [normal[0], -normal[1], normal[2]],
            width: 0.,
            reach: Default::default(),
            within: 0.,
            zone: None,
        });
    };
    if !plan.stub {
        let [back, face, past] = WALL;
        let (x0, x1) = (-past, plan.length + past);
        let out = [0., 1., 0.];
        edge(
            "top",
            "front",
            [x0, face, plan.high],
            [x1, face, plan.high],
            out,
        );
        edge("base", "front", [x0, face, 0.], [x1, face, 0.], out);
        if plan.high > plan.top + 0.05 {
            let z = plan.high;
            edge("top", "back", [x1, back, z], [x0, back, z], [0., -1., 0.]);
        }
        return features;
    }
    let (length, depth) = (plan.length, plan.depth);
    let openings = plan.openings();
    // Each face's ends: the front along x, the ends out from the building's wall.
    let faces: [(Face, &str, V3, V3, V3); 3] = [
        (
            Face::Front,
            "front",
            [0., depth, 0.],
            [length, depth, 0.],
            [0., 1., 0.],
        ),
        (
            Face::Start,
            "start",
            [0., 0., 0.],
            [0., depth, 0.],
            [-1., 0., 0.],
        ),
        (
            Face::End,
            "end",
            [length, 0., 0.],
            [length, depth, 0.],
            [1., 0., 0.],
        ),
    ];
    // A point `s` m across a face (its own measure: x on the front, y on an end), `z` up.
    let on = |face: Face, s: f64, z: f64| match face {
        Face::Front => [s, depth, z],
        Face::Start => [0., s, z],
        Face::End => [length, s, z],
    };
    for (face, label, a, b, normal) in faces {
        let level = |z: f64| ([a[0], a[1], z], [b[0], b[1], z]);
        let (p, q) = level(plan.high);
        edge("top", label, p, q, normal);
        let (p, q) = level(0.);
        edge("base", label, p, q, normal);
        for z in plan.courses() {
            // The course stands 6 cm out past each corner; its underside on the face.
            let (p, q) = level(z - 0.06);
            edge("eave", label, p, q, normal);
        }
        for opening in openings.iter().filter(|o| o.face == face) {
            let [s0, s1, z0, _] = opening.span;
            // The sill's underside: 6 cm below the window and past its sides.
            let (p, q) = (
                on(face, s0 - 0.06, z0 - 0.06),
                on(face, s1 + 0.06, z0 - 0.06),
            );
            edge("sill", label, p, q, normal);
            if opening.unit {
                let middle = (s0 + s1) / 2.;
                let z = z0 - 0.62;
                edge(
                    "drip",
                    label,
                    on(face, middle - 0.32, z),
                    on(face, middle + 0.32, z),
                    normal,
                );
            }
        }
    }
    if plan.high > plan.top + 0.05 {
        let z = plan.high;
        edge("top", "back", [length, 0., z], [0., 0., z], [0., -1., 0.]);
    }
    features
}

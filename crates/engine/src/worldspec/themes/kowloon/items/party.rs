//! What stands against a building's party edges: the plain wall of a neighbour built hard
//! against it, or a short slice of an adjoining building with its own storeys. Conventions
//! are those of the parent module (wall plane y = 0, the fixture pen's y runs outward).
use super::{Arguments, draw, fixture};
use crate::itemspec::definition::{NativeOutput, Values};
use crate::worldspec::themes::kowloon::{
    fixture::Fixture,
    noise::value_noise,
    palette::{mix, shade, srgb},
    pen::PatchVertex,
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
/// faces (a few with a unit under the sill), string courses, a drain and a roof behind a
/// parapet of its own. Slots: `wall` (stained render, tinted per vertex), `trim` (copings,
/// courses and sills), `frame`, `pane`, `metal` (the drain and units). Parameters: `kind` (`wall` |
/// `stub`, `wall`), `length` (m, 0.3–60, 6), `top` (m, 2–80, 12.5), `ground` (m, 2–8, 3.3),
/// `storey` (m, 2.4–6, 3), `storeys` (1–20, 4), `own` (0–20, 0), `color` (#b4a88f), `frame`
/// (#4b5257), `seed`.
pub(super) fn party(
    name: &str,
    values: &Values,
    cancelled: &dyn Fn() -> bool,
) -> Result<NativeOutput, String> {
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
    let key = a.seed()?;
    a.finish()?;
    let mut rng = SeededRng::from_text(&key);
    let noise_seed = rng.float(0., 1000.);
    // Pale, as the render under it is bare (a facade's paint is a layer of its own).
    let paint = mix(mix(color, srgb(rng.pick(&PAINTS)), 0.45), [1.; 3], 0.22);
    let trim = shade(paint, 0.86);
    if !stub {
        // A plain wall: greyer than render, as a party wall left unpainted.
        let base = mix(paint, srgb("#97928a"), 0.4);
        let high = (top + rng.float(-0.35, 0.9)).max(2.);
        let (x0, x1, back, face) = (-0.06, length + 0.06, -0.25, 0.08);
        return draw(cancelled, |mesh| {
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
                trim,
                "d",
            )
        });
    }
    let floors = if own > 0 {
        own
    } else {
        let low = storeys.saturating_sub(2).max(2);
        low + (rng.float(0., 1.) * (storeys + 2 - low) as f64).floor() as usize
    };
    let roof = ground + (floors - 1) as f64 * storey;
    let parapet = rng.float(0.4, 1.0);
    let high = roof + parapet;
    let depth = rng.float(1.1, 1.9);
    let wall = 0.2;
    let bays = ((length - 0.8) / 2.3).floor().max(1.) as usize;
    // Which windows stand: most do; a few bays are blank.
    let blank: Vec<bool> = (0..bays * floors).map(|_| rng.chance(0.15)).collect();
    // And which upper windows have a unit hung under their sill.
    let units: Vec<bool> = (0..bays * floors).map(|_| rng.chance(0.3)).collect();
    let pipe = rng.float(0.25, 0.6);
    draw(cancelled, |mesh| {
        let d = &mut fixture(mesh, "party")?;
        // A string course at every floor, round its three faces, and a drain down one end.
        for level in 1..floors {
            let z = ground + (level - 1) as f64 * storey;
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
        // Windows: a row a storey along its outer face, small ones in the ground storey, and
        // one a storey on each end when it is deep enough.
        let pitch = length / bays as f64;
        for level in 0..floors {
            let floor = if level == 0 {
                0.
            } else {
                ground + (level - 1) as f64 * storey
            };
            let (sill, tall, wide) = if level == 0 {
                (1.35, 0.8, 0.8)
            } else {
                (0.95, 1.3, 1.15_f64.min(pitch - 0.6))
            };
            let [z0, z1] = [floor + sill, floor + sill + tall];
            if z1 > roof - 0.25 || wide < 0.4 {
                continue;
            }
            for bay in 0..bays {
                if blank[level * bays + bay] {
                    continue;
                }
                let middle = (bay as f64 + 0.5) * pitch;
                window(
                    d,
                    &|s, z, out| [s, depth + out, z],
                    [0., 1., 0.],
                    [middle - wide / 2., middle + wide / 2., z0, z1],
                    frame,
                    trim,
                )?;
                if level > 0 && units[level * bays + bay] {
                    // A window unit on a bracket under the sill.
                    d.cuboid(
                        "metal",
                        [middle - 0.36, depth + 0.02, z0 - 0.62],
                        [middle + 0.36, depth + 0.34, z0 - 0.14],
                        srgb("#c9c6bb"),
                        "b",
                    )?;
                }
            }
            let narrow = 0.7_f64.min(depth - 0.55);
            if level > 0 && narrow >= 0.45 {
                let middle = depth / 2.;
                let span = [middle - narrow / 2., middle + narrow / 2., z0, z1];
                window(
                    d,
                    &|s, z, out| [-out, s, z],
                    [-1., 0., 0.],
                    span,
                    frame,
                    trim,
                )?;
                window(
                    d,
                    &|s, z, out| [length + out, s, z],
                    [1., 0., 0.],
                    span,
                    frame,
                    trim,
                )?;
            }
        }
        Ok(())
    })
}

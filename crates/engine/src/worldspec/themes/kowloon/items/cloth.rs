//! Hung cloth and sheet shelters: a tarpaulin or striped awning that sags between its
//! fixings, and a window canopy of loose corrugated sheet. Conventions are those of the
//! parent module (wall plane y = 0, the fixture pen's y runs outward).
use super::{Arguments, draw, fixture, hardware::wall_plate};
use crate::itemspec::definition::{NativeOutput, Values};
use crate::worldspec::themes::kowloon::{
    fixture::Fixture,
    noise::value_noise,
    palette::{mix, shade, srgb},
    pen::PatchVertex,
};
use std::f64::consts::PI;
use superspec_geometry::{
    V3,
    math::{atan2, hypot, sin},
};
use superspec_types::rng::SeededRng;

/// Both faces of a smooth grid of cloth or sheet.
fn two_sided(d: &mut Fixture<'_, '_>, slot: &str, grid: &[Vec<PatchVertex>]) -> Result<(), String> {
    d.pen.patch(d.mesh, slot, grid, false)?;
    d.pen.patch(d.mesh, slot, grid, true)
}

/// A parabola's share of its sag at `t` along a span: 0 at both fixings, 1 midway.
fn hang(t: f64) -> f64 {
    4. * t * (1. - t)
}

fn gcd(a: usize, b: usize) -> usize {
    if b == 0 { a } else { gcd(b, a % b) }
}

/// `kowloon/sagging-tarp`: a sheet of canvas hung between fixings on the wall and either
/// poles at its front edge or a light steel frame. Between its fixings every edge hangs in a
/// catenary, the field bellies below them, sharp creases run out from every fixing (shaded
/// in their troughs), a slack sheet gathers in a few seeded deep folds that deepen toward
/// its hem, and the front edge ripples; a scalloped valance has a bound hem; ropes tie the eyelets to wall hooks and the corners to the pole tops,
/// and each pole is tied back to the wall by a stay. Anchor: the centre of its rear edge on
/// the wall plane; it reaches `depth` out and its front fixings are `drop` lower. Slots:
/// `tarp` (cloth, hems and ropes, tinted per vertex), `metal`. Parameters: `width` (m,
/// 0.6–20, 3.2), `depth` (m, 0.3–5, 1.3), `drop` (m, 0–3, 0.4), `sag` (m the front edge hangs
/// midway between two fixings, 0–1, 0.18), `tilt` (m one front fixing may stand lower than
/// the next, seeded, 0–0.5, 0.06), `valance` (m, 0–0.6, 0), `scallops` (lobes along the
/// valance's hem, 0–120, 0: a straight hem), `stripes` (stripes from wall to front, 0–120,
/// 0: plain), `color` (#3f7f5f), `stripe` (#d8a030), `hem` (ropes and hems, #c9bca0), `poles`
/// (0–8, 2; with none and no frame the front corners hang from stays), `stand` (m from the
/// anchor down to the ground the poles stand on, 0–20, 0: no poles), `braces` (a stay from
/// each pole top back to the wall, true), `ties` (ropes, true), `frame` (a light steel frame
/// carries a straight front edge instead of poles, false), `steel` (#6d6f66), `seed`.
pub(super) fn sagging_tarp(
    name: &str,
    values: &Values,
    cancelled: &dyn Fn() -> bool,
) -> Result<NativeOutput, String> {
    let mut a = Arguments::new(name, values);
    let width = a.bounded("width", 3.2, 0.6..=20.)?;
    let depth = a.bounded("depth", 1.3, 0.3..=5.)?;
    let drop = a.bounded("drop", 0.4, 0. ..=3.)?;
    let sag = a.bounded("sag", 0.18, 0. ..=1.)?;
    let tilt = a.bounded("tilt", 0.06, 0. ..=0.5)?;
    let valance = a.bounded("valance", 0., 0. ..=0.6)?;
    let scallops = a.integer("scallops", 0, 0..=120)?;
    let stripes = a.integer("stripes", 0, 0..=120)?;
    let color = a.color("color", "#3f7f5f")?;
    let stripe = a.color("stripe", "#d8a030")?;
    let hem = a.color("hem", "#c9bca0")?;
    let poles = a.integer("poles", 2, 0..=8)?;
    let stand = a.bounded("stand", 0., 0. ..=20.)?;
    let braces = a.boolean("braces", true)?;
    let ties = a.boolean("ties", true)?;
    let frame = a.boolean("frame", false)?;
    let steel = a.color("steel", "#6d6f66")?;
    let key = a.seed()?;
    a.finish()?;
    let mut rng = SeededRng::from_text(&key);
    let noise_seed = rng.float(0., 1000.);
    // Front fixings: the pole tops, or the two corners.
    let supports = if frame { 2 } else { poles.max(2) };
    let lows: Vec<f64> = (0..supports).map(|_| rng.float(0., tilt)).collect();
    let phases: Vec<f64> = (0..supports).map(|_| rng.float(0., 2. * PI)).collect();
    let ripple = [rng.float(0., 2. * PI), rng.float(0.26, 0.38)];
    let trough = [rng.float(0., 2. * PI), rng.float(1.6, 2.8)];
    // Each post's foot stands a little off plumb, and out from under its top.
    let leans: Vec<[f64; 2]> = (0..supports)
        .map(|_| [rng.float(-0.05, 0.05), rng.float(0.02, 0.09)])
        .collect();
    // Columns: whole stripes and whole scallops, about 5 cm wide, six at least to a lobe.
    let unit = {
        let (s, c) = (stripes.max(1), scallops.max(1));
        s / gcd(s, c) * c
    };
    let mut per = ((width / 0.05 / unit as f64).ceil() as usize).max(1);
    if scallops > 0 {
        per = per.max((6 * scallops).div_ceil(unit));
    }
    let columns = unit * per;
    if columns > 720 {
        return Err(format!(
            "{name}: stripes and scallops need {columns} columns; at most 720."
        ));
    }
    let rows = ((depth / 0.07).ceil() as usize).clamp(8, 36);
    let eyelets = (width / 0.65).round().max(1.) as usize;
    let loose = if frame { 0.35 } else { 1. };
    // Creases run out from every fixing; a slack sheet also gathers in a few deep folds that
    // deepen toward its hem: [u of the fold, half its width in m, its share of the depth].
    let eyelet_phases: Vec<f64> = (0..=eyelets).map(|_| rng.float(0., 2. * PI)).collect();
    let gathers = if frame {
        0
    } else {
        ((width / 1.3).round() as usize).clamp(2, 6)
    };
    let deep: Vec<[f64; 3]> = (0..gathers)
        .map(|k| {
            [
                (k as f64 + rng.float(0.2, 0.8)) / gathers as f64,
                rng.float(0.14, 0.26),
                rng.float(0.7, 1.2),
            ]
        })
        .collect();
    let give = if frame { 0.6 } else { 1. };
    // A crease's profile across it: 1 in its sharp trough, falling to 0 midway to the next.
    let crease = |x: f64| {
        let s = 1. - sin(x).abs();
        s * s
    };
    // The folds at (u, v): how far they sink the cloth (in shares of its sag) and how much of
    // a trough's shadow it holds (0 to 1), to darken the cloth along its creases.
    let folds_at = |u: f64, v: f64| -> (f64, f64) {
        let (mut sink, mut shadow) = (0., 0.);
        for (p, phase) in phases.iter().enumerate() {
            let dx = (u - p as f64 / (supports - 1) as f64) * width;
            let dy = (1. - v) * depth;
            let r = hypot(dx, dy);
            let reach = (r / 0.08).min(1.) / (1. + (r / 0.95) * (r / 0.95));
            let c = crease(atan2(dy, dx) * 4.5 + phase) * reach;
            sink += 0.22 * c;
            shadow += 0.8 * c;
        }
        for (i, phase) in eyelet_phases.iter().enumerate() {
            let dx = (u - i as f64 / eyelets as f64) * width;
            let dy = v * depth;
            let r = hypot(dx, dy);
            let reach = (r / 0.05).min(1.) / (1. + (r / 0.45) * (r / 0.45));
            let c = crease(atan2(dy, dx) * 3. + phase) * reach;
            sink += 0.12 * c;
            shadow += 0.55 * c;
        }
        let toward = ((v - 0.25) / 0.75).max(0.);
        let toward = toward * toward.sqrt();
        for [at, half, share] in &deep {
            // A round-bottomed trough, so the hem hangs in festoons rather than points.
            let t = ((u - at) * width) / half;
            let f = (1. - t * t).max(0.);
            let f = f * f * toward * share;
            sink += 0.45 * f;
            shadow += f;
        }
        (sink * give, shadow.min(1.) * (sag / 0.15).min(1.))
    };
    // The cloth's surface over (u across, v from the wall to the front).
    let surface = |u: f64, v: f64| -> V3 {
        let span = (u * (supports - 1) as f64).min((supports - 1) as f64 - 1e-9);
        let (index, t) = (span.floor() as usize, span.fract());
        let low = lows[index] * (1. - t) + lows[index + 1] * t;
        let front = if frame {
            0.
        } else {
            sag * hang(t) / ((supports - 1) as f64).sqrt()
        };
        let belly = sag * 0.55 * loose * sin(PI * v) * (0.55 + 0.45 * hang(u));
        let rear = (sag * 0.35).min(0.035)
            * hang((u * eyelets as f64).fract())
            * (1. - v)
            * (1. - v)
            * (1. - v);
        // Creases run out from the fixings and gather near the hem, and the free edge ripples.
        let creases = folds_at(u, v).0;
        let mut folds = 0.8
            * sin(2. * PI * u * width / ripple[1] + ripple[0])
            * v
            * v
            * v
            * (0.4 + 0.6 * value_noise(u * width * 1.7, 0.5, noise_seed));
        // Long troughs where the slack gathers between the wall and the front.
        folds += 0.7 * sin(2. * PI * u * trough[1] + trough[0]) * sin(PI * v) * hang(u).sqrt();
        let pinch = sin(PI * v) * sag * 0.25 * loose;
        [
            -width / 2. + u * width + (0.5 - u) * 2. * pinch,
            0.03 + v * (depth - 0.03) - front * 0.35 * v,
            -(drop + low) * v - front * v * v.sqrt() - belly - rear + folds * sag * 0.3 * loose
                - creases * sag,
        ]
    };
    let soil = |u: f64, v: f64, base: V3| -> V3 {
        let edge = (u.min(1. - u) * width).min(v.min(1. - v) * depth);
        let grime = 0.3 / (1. + (edge / 0.07) * (edge / 0.07))
            + 0.16 * sin(PI * v) * hang(u) * loose
            + 0.14 * value_noise(u * width * 1.3, v * depth * 1.7, noise_seed + 7.);
        let faded = mix(
            base,
            srgb("#a99d88"),
            0.1 + 0.12 * value_noise(u * width * 0.6, v * depth * 0.8, noise_seed + 3.),
        );
        // Its creases' troughs hold a shadow and the dirt that runs into them.
        let trough = 1. - 0.3 * folds_at(u, v).1;
        std::array::from_fn(|k| faded[k] * (1. - grime * [0.42, 0.49, 0.55][k]) * trough)
    };
    // The valance hangs from the front edge, `row` (0 to 1) of the way down to its hem, which
    // is cut in round lobes; returns the point and how far it hangs.
    let lobe = if scallops > 0 { valance * 0.5 } else { 0. };
    let valance_at = |u: f64, row: f64| -> (V3, f64) {
        let top = surface(u, 1.);
        let t = (u * scallops as f64).fract();
        let round = (1. - (2. * t - 1.) * (2. * t - 1.)).max(0.).sqrt();
        // A straight hem on a slack sheet hangs unevenly.
        let uneven = if scallops == 0 && !frame {
            0.55 + 0.45 * value_noise(u * width * 1.3, 3.5, noise_seed)
        } else {
            1.
        };
        let hang = (valance - lobe + lobe * round) * uneven;
        let sway = 0.018 * sin(2. * PI * u * width / 0.9 + ripple[0]) * sin(row * PI * 0.7);
        ([top[0], top[1] + sway, top[2] - hang * row], hang)
    };
    let bands = stripes.max(1);
    let band_color = |band: usize| if band % 2 == 1 { stripe } else { color };
    let edge = shade(mix(color, hem, 0.15), 0.6);
    draw(cancelled, |mesh| {
        let d = &mut fixture(mesh, "sagging-tarp")?;
        let per_band = columns / bands;
        for band in 0..bands {
            let base = band_color(band);
            let grid: Vec<Vec<PatchVertex>> = (0..=rows)
                .map(|j| {
                    (0..=per_band)
                        .map(|i| {
                            let u = (band * per_band + i) as f64 / columns as f64;
                            let v = j as f64 / rows as f64;
                            PatchVertex {
                                at: surface(u, v),
                                uv: [u * width, v * depth],
                                tint: soil(u, v, base),
                            }
                        })
                        .collect()
                })
                .collect();
            two_sided(d, "tarp", &grid)?;
            if valance > 0. {
                let grid: Vec<Vec<PatchVertex>> = [0., 0.45, 0.8, 1.]
                    .into_iter()
                    .map(|row: f64| {
                        (0..=per_band)
                            .map(|i| {
                                let u = (band * per_band + i) as f64 / columns as f64;
                                let (at, hang) = valance_at(u, row);
                                // Its cut edge is a little darker, so the lobes read.
                                let hemmed = if row == 1. && scallops > 0 { 0.8 } else { 0.92 };
                                PatchVertex {
                                    at,
                                    uv: [u * width, depth + hang * row],
                                    tint: soil(u, 1. - 0.02 * row, shade(base, hemmed)),
                                }
                            })
                            .collect()
                    })
                    .collect();
                two_sided(d, "tarp", &grid)?;
            }
        }
        if valance > 0. && scallops > 0 {
            // The scalloped hem's bound edge.
            let path: Vec<V3> = (0..=columns)
                .map(|i| valance_at(i as f64 / columns as f64, 1.).0)
                .collect();
            d.tube("tarp", &path, 0.004, edge, 4)?;
        }
        // Hems: a rolled edge all round.
        let across = |v: f64| -> Vec<V3> {
            (0..=columns)
                .map(|i| surface(i as f64 / columns as f64, v))
                .collect()
        };
        let along = |u: f64| -> Vec<V3> {
            (0..=rows)
                .map(|j| surface(u, j as f64 / rows as f64))
                .collect()
        };
        for path in [across(0.), across(1.), along(0.), along(1.)] {
            d.tube("tarp", &path, 0.004, edge, 4)?;
        }
        let rope = shade(hem, 0.8);
        let dark = shade(steel, 0.8);
        if ties {
            // Eyelets along the wall, each tied up to a hook.
            for i in 0..=eyelets {
                let p = surface(i as f64 / eyelets as f64, 0.);
                d.tube("tarp", &[p, [p[0], 0.008, p[2] + 0.07]], 0.003, rope, 4)?;
                d.cuboid(
                    "metal",
                    [p[0] - 0.012, 0., p[2] + 0.055],
                    [p[0] + 0.012, 0.014, p[2] + 0.09],
                    dark,
                    "b",
                )?;
            }
        }
        if frame {
            // A light frame: a rail under the front edge on arms from the wall, each strutted.
            let arms = (width / 1.9).ceil().max(1.) as usize;
            let front: Vec<V3> = (0..=arms)
                .map(|i| {
                    let p = surface(i as f64 / arms as f64, 1.);
                    [p[0], p[1], p[2] - 0.016]
                })
                .collect();
            d.tube("metal", &front, 0.012, steel, 6)?;
            for end in &front {
                let wall = [end[0], 0.012, -0.03];
                d.tube("metal", &[wall, *end], 0.011, steel, 6)?;
                let foot = [end[0], 0.012, -drop - 0.42];
                let knee = [
                    end[0],
                    end[1] * 0.7,
                    wall[2] + (end[2] - wall[2]) * 0.7 - 0.01,
                ];
                d.tube("metal", &[foot, knee], 0.008, steel, 5)?;
                wall_plate(d, [end[0], -0.03], 0.05, 0.07, dark)?;
                wall_plate(d, [end[0], foot[2]], 0.04, 0.05, dark)?;
            }
            return Ok(());
        }
        for p in 0..supports {
            let u = p as f64 / (supports - 1) as f64;
            let corner = surface(u, 1.);
            let x = -width / 2. + u * width;
            let top = [x, depth + 0.03, corner[2] + 0.1];
            if poles > 0 && stand > drop + 0.3 {
                // A post on a base plate, tied back to the wall.
                let foot = [x + leans[p][0], top[1] + leans[p][1], -stand];
                d.tube("metal", &[foot, top], 0.03, steel, 8)?;
                d.disc("metal", top, 0.03, [0., 0., 1.], steel, 8)?;
                d.cuboid(
                    "metal",
                    [foot[0] - 0.08, foot[1] - 0.08, -stand],
                    [foot[0] + 0.08, foot[1] + 0.08, -stand + 0.014],
                    dark,
                    "d",
                )?;
                if braces {
                    let fixing = [x, 0.012, 0.22];
                    d.tube(
                        "metal",
                        &[[top[0], top[1], top[2] - 0.02], fixing],
                        0.008,
                        dark,
                        6,
                    )?;
                    wall_plate(d, [x, 0.22], 0.04, 0.05, dark)?;
                }
                if ties {
                    // The corner's eyelet lashed to the post in two turns.
                    for turn in [0.02, 0.05] {
                        d.tube(
                            "tarp",
                            &[corner, [top[0], top[1] - 0.024, top[2] - turn]],
                            0.004,
                            rope,
                            4,
                        )?;
                    }
                }
            } else {
                // No post: the corner hangs from a stay fixed to the wall above.
                let fixing = [x, 0.012, 0.25 + depth * 0.45];
                d.tube("tarp", &[corner, fixing], 0.004, rope, 4)?;
                wall_plate(d, [x, fixing[2]], 0.04, 0.05, dark)?;
            }
        }
        Ok(())
    })
}

/// `kowloon/sheet-canopy`: a window canopy of loose corrugated sheets on angle brackets: the
/// sheets lap, each tilts by its own seeded amount, sags between the brackets and has rusted
/// from its drip edge. Anchor: the centre of its top edge on the wall plane; it projects
/// `depth` and falls `drop`. Slots: `sheet` (tinted per vertex), `metal`. Parameters: `width`
/// (m, 0.3–12, 1.6), `depth` (m, 0.15–2, 0.5), `drop` (m, 0–1, 0.14), `sag` (m, 0–0.3,
/// 0.03), `tilt` (m a front corner may hang lower, seeded, 0–0.4, 0.06), `pitch` (m between
/// corrugations, 0.04–0.3, 0.076), `color` (#8b8a80), `rust` (#6b3f24), `rusted` (share of
/// rust, 0–1, 0.55), `steel` (brackets, #55554c), `seed`.
pub(super) fn sheet_canopy(
    name: &str,
    values: &Values,
    cancelled: &dyn Fn() -> bool,
) -> Result<NativeOutput, String> {
    let mut a = Arguments::new(name, values);
    let width = a.bounded("width", 1.6, 0.3..=12.)?;
    let depth = a.bounded("depth", 0.5, 0.15..=2.)?;
    let drop = a.bounded("drop", 0.14, 0. ..=1.)?;
    let sag = a.bounded("sag", 0.03, 0. ..=0.3)?;
    let tilt = a.bounded("tilt", 0.06, 0. ..=0.4)?;
    let pitch = a.bounded("pitch", 0.076, 0.04..=0.3)?;
    let color = a.color("color", "#8b8a80")?;
    let rust = a.color("rust", "#6b3f24")?;
    let rusted = a.bounded("rusted", 0.55, 0. ..=1.)?;
    let steel = a.color("steel", "#55554c")?;
    let key = a.seed()?;
    a.finish()?;
    let mut rng = SeededRng::from_text(&key);
    let noise_seed = rng.float(0., 1000.);
    let sheets = (width / 1.25).ceil().max(1.) as usize;
    let lap = if sheets > 1 { 0.07 } else { 0. };
    let each = (width + lap * (sheets - 1) as f64) / sheets as f64;
    let amplitude = pitch * 0.12;
    draw(cancelled, |mesh| {
        let d = &mut fixture(mesh, "sheet-canopy")?;
        for sheet in 0..sheets {
            let x0 = -width / 2. + sheet as f64 * (each - lap);
            // Its own slip at the wall and its own fall at each front corner.
            let slip = rng.float(0., tilt * 0.3);
            let fall = [rng.float(0., tilt), rng.float(0., tilt)];
            let lift = 0.012 * (sheet % 2) as f64;
            let shade_of = rng.float(0.85, 1.05);
            let columns = ((each / pitch).ceil() as usize * 4).min(640);
            let grid: Vec<Vec<PatchVertex>> = (0..=4)
                .map(|j| {
                    let v = j as f64 / 4.;
                    (0..=columns)
                        .map(|i| {
                            let u = i as f64 / columns as f64;
                            let x = x0 + u * each;
                            let z = lift
                                - slip * (1. - v)
                                - (drop + fall[0] * (1. - u) + fall[1] * u) * v
                                - sag * hang(u) * v * v
                                + amplitude * sin(2. * PI * (x + width) / pitch);
                            // Rust creeps up from the drip edge and blooms in patches.
                            let patch = value_noise(x * 2.3, v * depth * 5., noise_seed);
                            let streak = value_noise(x * 14., v * depth * 1.5, noise_seed + 5.);
                            let amount = (rusted
                                * (0.25
                                    + 1.1 * v * v
                                    + 0.9 * (patch - 0.45)
                                    + 0.5 * (streak - 0.5)))
                                .clamp(0., 1.);
                            PatchVertex {
                                at: [x, 0.012 + v * (depth - 0.012), z],
                                uv: [x + width, v * depth],
                                tint: mix(shade(color, shade_of), rust, amount),
                            }
                        })
                        .collect()
                })
                .collect();
            two_sided(d, "sheet", &grid)?;
        }
        // Angle brackets: an arm under the sheet and a strut below it, on a bolted plate.
        let brackets = (width / 1.3).ceil().max(1.) as usize + 1;
        let inset = 0.1_f64.min(width / 4.);
        for i in 0..brackets {
            let x = -width / 2. + inset + (width - 2. * inset) * i as f64 / (brackets - 1) as f64;
            let under = amplitude + 0.016;
            d.bar(
                "metal",
                [x, 0.01, -under],
                [x, depth * 0.92, -drop * 0.92 - under],
                0.022,
                steel,
            )?;
            d.bar(
                "metal",
                [x, 0.01, -drop - 0.26],
                [x, depth * 0.72, -drop * 0.72 - under - 0.012],
                0.016,
                steel,
            )?;
            wall_plate(
                d,
                [x, -(drop + 0.26 + under) / 2.],
                0.045,
                (drop + 0.36) / 2.,
                steel,
            )?;
        }
        Ok(())
    })
}

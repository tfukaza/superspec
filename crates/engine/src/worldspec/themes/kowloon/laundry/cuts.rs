//! Sample each deterministic cut once; fitting and emission share the same owned fibers.
use super::Support;
use crate::materialspec::noise::smooth_step;
use std::f64::consts::PI;
use superspec_geometry::math::{cos, sin};
use superspec_types::rng::SeededRng;
pub(super) const GRAIN: f64 = 0.075;
const FALL: f64 = 0.14;
const HALF: f64 = 0.0035;
#[derive(Clone, Copy)]
pub(super) struct Fiber {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub s: f64,
    pub t: f64,
    pub gap: f64,
    pub facing: [f64; 2],
    pub loose: f64,
}
impl Fiber {
    fn flat(x: f64, y: f64, s: f64, t: f64, gap: f64, loose: f64) -> Self {
        Self {
            x,
            y,
            z: 0.,
            s,
            t,
            gap,
            facing: [0., 1.],
            loose,
        }
    }
}
pub(super) struct Panel {
    pub grid: Vec<Vec<Fiber>>,
}
pub(super) enum Fitting {
    Pin(f64),
    Hook(f64),
    Bar([f64; 3]),
}
pub(super) struct Cut {
    pub panels: Vec<Panel>,
    pub fittings: Vec<Fitting>,
    pub follows: bool,
    pub grain: f64,
}
fn lerp(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t
}
fn steps(length: f64, grain: f64) -> usize {
    (length / grain).ceil().max(2.) as usize
}
fn panel(columns: usize, rows: usize, f: impl Fn(f64, f64) -> Fiber) -> Panel {
    Panel {
        grid: (0..=rows)
            .map(|row| {
                (0..=columns)
                    .map(|column| f(column as f64 / columns as f64, row as f64 / rows as f64))
                    .collect()
            })
            .collect(),
    }
}
#[derive(Clone, Copy, PartialEq)]
enum Hung {
    Pins,
    Hanger,
    Thread,
}
pub(super) fn shirt(rng: &mut SeededRng, support: Support, sleeveless: bool) -> Cut {
    let scale = if sleeveless { 0.9 } else { 1. };
    let hung = if support == Support::Hanger {
        Hung::Hanger
    } else if support == Support::Pole && !sleeveless {
        Hung::Thread
    } else {
        Hung::Pins
    };
    let droop = if hung == Hung::Thread {
        0.
    } else {
        rng.float(0.66, 0.9)
    };
    let body = rng.float(0.42, 0.5) * scale;
    let length = rng.float(if sleeveless { 0.62 } else { 0.6 }, 0.7) * scale;
    let sleeve = rng.float(
        if sleeveless { 0. } else { 0.17 },
        if sleeveless { 0. } else { 0.2 },
    ) * scale;
    let cuff = 0.15 * scale;
    let sleeve_share = if sleeveless { 0. } else { 0.32 };
    let half = body / 2.;
    let shoulder = 0.035 * scale;
    let armhole = 0.2 * scale;
    let neck = 0.085 * scale;
    let threaded = hung == Hung::Thread;
    let (raise, puff) = match hung {
        Hung::Pins => (-shoulder + 0.012, 0.006),
        Hung::Hanger => (0.075, 0.016),
        Hung::Thread => (-shoulder - 0.03, 0.016),
    };
    let body_gap = |side: f64, r: f64| {
        let seam = side
            * side
            * if threaded {
                smooth_step(0.1, 0.35, r)
            } else {
                1.
            };
        HALF + puff * (1. - seam) * (1. - 0.6 * r)
    };
    let mut panels = vec![panel(steps(body, GRAIN), steps(length, FALL), |c, r| {
        let side = 2. * c - 1.;
        let top = if sleeveless {
            if side.abs() < 0.56 {
                0.13 * (1. - (side / 0.56).powi(2)).sqrt()
            } else {
                0.
            }
        } else {
            shoulder * ((side.abs() * half - neck) / (half - neck)).max(0.)
        };
        let sag = if hung == Hung::Pins {
            0.022 * (1. - side * side) * (1. - r).powi(2)
        } else {
            0.
        };
        let y = lerp(top, length, r) + sag;
        let strap = if sleeveless {
            0.62 + 0.38 * smooth_step(0., armhole, y)
        } else {
            1.
        };
        Fiber::flat(
            side * half * strap,
            y + raise,
            0.5 + 0.3 * side * strap,
            y / length,
            body_gap(side, r),
            smooth_step(0.08, 0.5, r),
        )
    })];
    if !sleeveless {
        for side in [-1., 1.] {
            panels.push(panel(steps(sleeve, FALL * 0.7), 2, |c, r| {
                let out = if side < 0. { 1. - c } else { c };
                let bend = (sleeve * 0.4).max(droop * armhole * 0.75);
                let turn = |along: f64| droop * (along / bend).min(1.);
                let mut reach = 0.;
                let mut fall = 0.;
                for i in 0..12 {
                    let angle = turn((i as f64 + 0.5) / 12. * out * sleeve);
                    reach += cos(angle) * sleeve * out / 12.;
                    fall += sin(angle) * sleeve * out / 12.;
                }
                let angle = turn(out * sleeve);
                let width = lerp(armhole, cuff, out);
                let across = if threaded {
                    r * width - armhole / 2.
                } else {
                    (r - 0.5) * width
                };
                let root = body_gap(1., (shoulder + armhole / 2. + across).max(0.) / length);
                let tube = HALF
                    + if threaded {
                        0.016 * sin(PI * r).sqrt()
                    } else {
                        0.007 * sin(PI * r)
                    };
                Fiber::flat(
                    side * (half + reach - sin(angle) * across),
                    shoulder + armhole / 2. + fall + cos(angle) * across + raise,
                    if side < 0. { 0.2 * r } else { 1. - 0.2 * r },
                    sleeve_share * out,
                    lerp(root, tube, smooth_step(0., 0.25, out)),
                    if threaded { 0.15 } else { 0.25 + 0.5 * out },
                )
            }));
        }
    }
    let fittings = match hung {
        Hung::Hanger => vec![Fitting::Hook(raise + 0.01)],
        Hung::Pins => [-1., 1.]
            .map(|side| Fitting::Pin(side * half * if sleeveless { 0.5 } else { 0.9 }))
            .into(),
        Hung::Thread => vec![],
    };
    Cut {
        panels,
        fittings,
        follows: hung != Hung::Hanger,
        grain: GRAIN,
    }
}
pub(super) fn bottoms(rng: &mut SeededRng, support: Support, short: bool) -> Cut {
    let hip = rng.float(0.38, 0.45);
    let length = rng.float(
        if short { 0.3 } else { 0.4 },
        if short { 0.36 } else { 0.5 },
    );
    let rise = if short { 0.24 } else { 0.27 };
    let spread = rng.float(0.02, 0.09);
    let half = hip / 2.;
    let hem = half * 0.9;
    let hanger = support == Support::Hanger;
    let raise = if hanger { 0.075 } else { 0.005 };
    let seat_gap = |side: f64| HALF + 0.009 * (1. - side * side);
    let mut panels = vec![panel(steps(hip, GRAIN), steps(rise, FALL), |c, r| {
        let side = 2. * c - 1.;
        Fiber::flat(
            side * half * lerp(0.92, 1., r),
            r * rise + raise,
            c,
            r * rise / length,
            seat_gap(side),
            0.35 * r,
        )
    })];
    for side in [-1., 1.] {
        panels.push(panel(
            steps(half, GRAIN),
            steps(length - rise, FALL),
            |c, r| {
                let outer = if side < 0. { 1. - c } else { c };
                let top = side * half * outer;
                let foot = side * (spread / 2. + hem * outer);
                let tube = HALF + 0.008 * sin(PI * outer);
                Fiber::flat(
                    lerp(top, foot, r),
                    lerp(rise, length, r) + raise,
                    0.5 + 0.5 * side * outer,
                    lerp(rise, length, r) / length,
                    lerp(seat_gap(outer), tube, smooth_step(0., 0.3, r)),
                    0.4 + 0.6 * r,
                )
            },
        ));
    }
    let fittings = if hanger {
        vec![
            Fitting::Hook(raise - 0.012),
            Fitting::Bar([-half * 0.92, half * 0.92, raise - 0.012]),
        ]
    } else {
        [-1., 1.].map(|s| Fitting::Pin(s * half * 0.8)).into()
    };
    Cut {
        panels,
        fittings,
        follows: !hanger,
        grain: GRAIN,
    }
}
fn pinned(width: f64, length: f64) -> Cut {
    let pins = (width / 0.45).round().max(1.) as usize + 1;
    let p = panel(steps(width, GRAIN), steps(length, FALL), |c, r| {
        let between = (c * (pins - 1) as f64) % 1.;
        let sag = 0.022 * 4. * between * (1. - between);
        Fiber::flat(
            (c - 0.5) * width * lerp(1., 0.94, r * (1. - r) * 4. * 0.5 + r * 0.5),
            r * length + sag * (1. - r).powi(2) + 0.004,
            c,
            r,
            HALF,
            0.3 + 0.7 * r,
        )
    });
    Cut {
        panels: vec![p],
        fittings: (0..pins)
            .map(|i| Fitting::Pin((i as f64 / (pins - 1) as f64 - 0.5) * width * 0.97))
            .collect(),
        follows: true,
        grain: GRAIN,
    }
}
fn draped(width: f64, total: f64, share: f64, radius: f64) -> Cut {
    let bend = PI * radius;
    let back = total * (1. - share) - bend / 2.;
    let front = total * share - bend / 2.;
    let back_rows = steps(back, FALL);
    let front_rows = steps(front, FALL);
    let rows = back_rows + 3 + front_rows;
    let p = panel(steps(width, GRAIN), rows, |c, r| {
        let row = (r * rows as f64).round() as usize;
        let mut f = Fiber::flat((c - 0.5) * width, 0., c, 0., HALF, 0.);
        if row <= back_rows {
            let down = back * (1. - row as f64 / back_rows as f64);
            f.y = down;
            f.z = -radius;
            f.t = (back - down) / total;
            f.facing = [0., -1.];
            f.loose = smooth_step(0., 0.3, down);
        } else if row < back_rows + 3 {
            let angle = (row - back_rows) as f64 / 3. * PI;
            f.y = -radius * sin(angle);
            f.z = -radius * cos(angle);
            f.t = (back + bend * (row - back_rows) as f64 / 3.) / total;
            f.facing = [-sin(angle), -cos(angle)];
        } else {
            let down = front * (row - back_rows - 3) as f64 / front_rows as f64;
            f.y = down;
            f.z = radius;
            f.t = (back + bend + down) / total;
            f.loose = smooth_step(0., 0.3, down);
        }
        f
    });
    Cut {
        panels: vec![p],
        fittings: vec![],
        follows: true,
        grain: GRAIN,
    }
}
pub(super) fn flat(
    rng: &mut SeededRng,
    support: Support,
    room: super::Room,
    long: bool,
) -> Option<Cut> {
    let lo = if long { 0.36 } else { 0.32 };
    if support == Support::Hanger || room.width < lo {
        return None;
    }
    let width = rng.float(lo, if long { 0.6 } else { 0.42 }).min(room.width);
    let length = rng.float(if long { 0.7 } else { 0.36 }, if long { 1.2 } else { 0.5 });
    if support == Support::Line
        && length <= room.drop
        && rng.chance(if length < 0.8 { 0.6 } else { 0.15 })
    {
        return Some(pinned(width, length));
    }
    let share = rng.float(0.5, 0.62);
    Some(draped(
        width,
        length.min((room.drop - 0.02) / share),
        share,
        if support == Support::Pole {
            0.016
        } else {
            0.008
        },
    ))
}

//! Decorative fixture reservations; these do not establish navigation or collision.
use super::{furniture::Family, grocer_props};
use serde::Serialize;
use std::f64::consts::PI;
use superspec_geometry::V3;
use superspec_types::rng::SeededRng;
#[derive(Clone, Serialize)]
pub struct Placement {
    pub id: &'static str,
    pub origin: V3,
    pub turn: f64,
    pub size: V3,
}
#[derive(Clone, Serialize)]
pub struct Shelf {
    #[serde(flatten)]
    pub placement: Placement,
    pub levels: usize,
    pub above: f64,
    pub family: Family,
}
#[derive(Clone, Serialize)]
pub struct Hanging {
    pub x0: f64,
    pub x1: f64,
    pub y: f64,
}
#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Kind {
    FrontCounter,
    SideCounter,
    RecessedCounter,
}
#[derive(Clone, Serialize)]
pub struct Layout {
    pub kind: Kind,
    pub mirrored: bool,
    pub counter: Placement,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cooler: Option<Placement>,
    pub shelves: Vec<Shelf>,
    pub crates: Placement,
    pub sacks: Placement,
    pub stool: Placement,
    pub hanging: Hanging,
}
fn fixture(id: &'static str, x: f64, y: f64, size: V3, turn: f64, z: f64) -> Placement {
    Placement {
        id,
        origin: [x, y, z],
        turn,
        size,
    }
}
pub fn plan(width: f64, depth: f64, height: f64, rng: &mut SeededRng) -> Result<Layout, String> {
    if !width.is_finite()
        || !(1.8..=40.).contains(&width)
        || !depth.is_finite()
        || !(2.4..=20.).contains(&depth)
        || !height.is_finite()
        || !(2. ..=10.).contains(&height)
    {
        return Err("Grocer layout requires bounded usable room dimensions.".into());
    }
    let kind = if width < 2.2 {
        Kind::FrontCounter
    } else if width >= 3.8 && depth >= 4.2 {
        rng.pick(&[Kind::FrontCounter, Kind::SideCounter, Kind::RecessedCounter])
    } else {
        rng.pick(&[Kind::FrontCounter, Kind::RecessedCounter])
    };
    let mirrored = rng.chance(0.5);
    let counter_width = 0.9_f64.max(1.9_f64.min(width - 1.25));
    let counter_x = width - 0.06 - counter_width;
    let recessed = matches!(kind, Kind::RecessedCounter);
    let side = matches!(kind, Kind::SideCounter);
    let counter_y = if recessed {
        rng.float(0.75, 0.95).min(depth - 1.81)
    } else {
        rng.float(0.08, 0.17)
    };
    let counter = if side {
        fixture("counter", width - 1.35, 2.85, [1.6, 0.5, 1.3], -PI / 2., 0.)
    } else {
        fixture(
            "counter",
            counter_x,
            counter_y,
            [counter_width, 0.5, 1.3],
            0.,
            0.,
        )
    };
    let unit = (height - 0.3).min(2.3);
    let levels = (((unit - 0.1) / 0.33).floor() as usize).max(4);
    let cooled = width >= 2.5;
    let cooler_size = grocer_props::size("drinks-cooler", [0.7, 0.6, 1.85_f64.min(height - 0.25)])?;
    let cooler = cooled.then(|| {
        fixture(
            "cooler",
            if side || recessed {
                0.05
            } else {
                width - 0.05 - cooler_size[0]
            },
            if recessed {
                0.03
            } else {
                depth - 0.01 - cooler_size[1]
            },
            cooler_size,
            0.,
            0.,
        )
    });
    let back_start = if side && cooled { 0.81 } else { 0.05 };
    let back_width = width - 0.1 - if cooled && !recessed { 0.76 } else { 0. };
    let mut shelves = vec![Shelf {
        placement: fixture(
            "back-shelves",
            back_start,
            depth - 0.51,
            [
                if back_width >= 1.6 {
                    back_width * 0.68 - 0.02
                } else {
                    back_width
                },
                0.5,
                unit,
            ],
            0.,
            0.,
        ),
        levels,
        above: height - unit - 0.03,
        family: Family::Timber,
    }];
    if back_width >= 1.6 {
        let split = back_width * 0.68;
        let steel = unit.min(1.98);
        shelves.push(Shelf {
            placement: fixture(
                "back-metal",
                back_start + split + 0.02,
                depth - 0.45,
                [back_width - split - 0.02, 0.44, steel],
                0.,
                0.,
            ),
            levels: (((steel - 0.1) / 0.33).floor() as usize).max(4),
            above: 0.25,
            family: Family::Steel,
        });
    }
    let left_start = if side { 0.65 } else { counter_y + 1.25 };
    let left_end = depth - if side && cooled { 0.66 } else { 0.56 };
    if left_end - left_start >= 0.65 {
        shelves.push(Shelf {
            placement: fixture(
                "left-shelves",
                0.465,
                left_start,
                [left_end - left_start, 0.46, unit],
                PI / 2.,
                0.,
            ),
            levels,
            above: height - unit - 0.03,
            family: Family::Timber,
        });
    }
    if (recessed || side) && width >= 3. {
        shelves.push(Shelf {
            placement: fixture(
                "front-rack",
                width - 0.50,
                0.03,
                [0.44, 0.42, unit.min(2.22)],
                0.,
                0.,
            ),
            levels,
            above: 0.22,
            family: Family::Wire,
        });
    }
    let aisle_start = if recessed { counter_y + 1.3 } else { 1.4 };
    if (recessed || side) && width >= 4.4 && depth >= 4.2 && depth - 1.4 - aisle_start >= 0.3 {
        shelves.push(Shelf {
            placement: fixture(
                "aisle-shelves",
                width * 0.42,
                aisle_start,
                [depth - 1.4 - aisle_start, 0.5, 1.35],
                PI / 2.,
                0.,
            ),
            levels: 3,
            above: 0.3,
            family: Family::TimberOpen,
        });
    }
    let rack_start = if side { 3.65 } else { counter_y + 1.35 };
    let rack_end = depth
        - if cooled && !side && !recessed {
            0.66
        } else {
            0.56
        };
    if rack_end - rack_start > 0.4 {
        shelves.push(Shelf {
            placement: fixture(
                "right-rack",
                width - 0.225,
                rack_end,
                [rack_end - rack_start, 0.22, 0.7_f64.min(height - 1.45)],
                -PI / 2.,
                1.35,
            ),
            levels: 3,
            above: 0.,
            family: Family::Wire,
        });
    }
    let crates = fixture(
        "crates",
        0.05,
        if recessed { counter_y + 0.03 } else { 0.04 },
        [
            (if side { width - 0.72 } else { counter_x - 0.04 }) - 0.05,
            0.44,
            1.15,
        ],
        0.,
        0.,
    );
    let sack_x = if recessed && cooled { 0.82 } else { 0.36 };
    let sack_end = if side {
        width - 0.85
    } else if recessed {
        width - 0.06
    } else {
        back_start + back_width - 0.04
    };
    let sacks = fixture(
        "sacks",
        sack_x,
        if recessed {
            0.06
        } else if side {
            0.68
        } else {
            depth - 0.98
        },
        [sack_end - sack_x, 0.58, 0.65],
        0.,
        0.,
    );
    let center = if side {
        [width - 0.84, depth - 0.9]
    } else {
        [counter_x + counter_width * 0.5, counter_y + 0.85]
    };
    let stool = fixture(
        "stool",
        center[0] - 0.2,
        center[1] - 0.2,
        [0.4, 0.4, 0.45],
        0.,
        0.,
    );
    let hanging = if recessed {
        Hanging {
            x0: if cooled { 0.9 } else { 0.2 },
            x1: width - 0.15,
            y: 0.66,
        }
    } else {
        Hanging {
            x0: 0.2,
            x1: if side { width - 0.75 } else { counter_x - 0.15 },
            y: 0.57,
        }
    };
    Ok(Layout {
        kind,
        mirrored,
        counter,
        cooler,
        shelves,
        crates,
        sacks,
        stool,
        hanging,
    })
}

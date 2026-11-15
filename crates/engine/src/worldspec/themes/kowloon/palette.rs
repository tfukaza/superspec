use serde::Serialize;
use superspec_geometry::V3;
use superspec_types::rng::SeededRng;

pub use crate::materialspec::color::{blend, mix, mottle, shade, srgb};

fn color(rng: &mut SeededRng, choices: &str) -> V3 {
    srgb(rng.pick(&choices.split_whitespace().collect::<Vec<_>>()))
}
const WALLS: &str = "#f2ead8 #ecd5c8 #d6e4d4 #d4e0ea #eadfbd #e0e0da #cfcbc0 #f0e8de #dccfb6 #e8ccb4 #c8d8d0 #f4f0e6";
const TRIMS: &str = "#e6e2d6 #d6d2c4 #f0ece2 #c8c4b8";
const FRAMES: &str = "#2f4f43 #e4e2d8 #5a4030 #3f7f7a #2a2c2e #7a3a2c #3a5f8f #8a8f92 #d8d4c4";
pub(super) const ACCENTS: &str =
    "#5fa8a0 #6f9fd0 #5f9a6a #e8dfc0 #e9e6dc #d99a84 #a9a79c #3f8f9a #8fbfd8 #c8b070";
const CAGES: &str = "#3f6f55 #7fb0c8 #dcdad0 #7a4a30 #8a8c8a #2f5f7a #4f8f6a";
const SHUTTERS: &str = "#8f969a #4a6f95 #4f7a5f #7a4038 #d8d2c0 #5a6268 #9a8a5a";
const AWNINGS: &str = "#3f7f5f #c0392b #2f5f8f #e8dfc0 #d8a030 #7a4a30";
const CURTAINS: &str = "#e8e0c8 #c8a878 #d8b8b0 #a8c0d0 #ececec #b8c8a0 #d0a060 #8898b0 #e8d0d8";

/// Household choices use their own named stream; adjacent bays may share this state.
#[derive(Clone, Debug, Serialize)]
pub struct Flat {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub paint: Option<V3>,
    pub band: &'static str,
    pub curtain: V3,
    pub clutter: f64,
    pub vacant: bool,
    pub enclosure: &'static str,
    pub replaced: bool,
}
pub fn flat(style: &LotStyle, rng: &mut SeededRng) -> Flat {
    Flat {
        paint: if rng.chance(style.repaint) {
            Some(rng.pick(&style.repaints))
        } else {
            None
        },
        band: rng.pick(&["full", "spandrel", "spandrel"]),
        curtain: color(rng, CURTAINS),
        clutter: (rng.float(0., 1.).powf(0.8) * style.clutter).min(1.),
        vacant: rng.chance(0.025),
        enclosure: if rng.chance(0.75) {
            style.enclosure
        } else {
            rng.pick(&["open", "caged", "glazed"])
        },
        replaced: style.window != "slider" && rng.chance(0.07),
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct Awning {
    pub canvas: bool,
    pub base: V3,
    pub hem: V3,
}

/// Construction choices shared by every floor of one seeded tenement.
#[derive(Clone, Debug, Serialize)]
pub struct LotStyle {
    pub wall: V3,
    pub trim: V3,
    pub frame: V3,
    pub accent: V3,
    pub cage: V3,
    pub shutter: V3,
    pub window: &'static str,
    pub plan: &'static str,
    pub bay: f64,
    pub hoods: bool,
    pub canopy: bool,
    pub balustrade: &'static str,
    pub bars: &'static str,
    pub enclosure: &'static str,
    pub awning: Awning,
    pub repaints: [V3; 3],
    pub repaint: f64,
    pub clutter: f64,
}

/// Draw order is part of the generator identity; conditional choices consume only their own draws.
pub fn lot_style(rng: &mut SeededRng) -> LotStyle {
    let canvas = rng.chance(0.6);
    let awning = if canvas {
        color(rng, AWNINGS)
    } else {
        mix(srgb("#a4a9aa"), srgb("#7a4a2a"), rng.float(0.1, 0.6))
    };
    let mut style = LotStyle {
        wall: color(rng, WALLS),
        trim: color(rng, TRIMS),
        frame: color(rng, FRAMES),
        accent: color(rng, ACCENTS),
        cage: color(rng, CAGES),
        shutter: color(rng, SHUTTERS),
        window: rng.pick(&["casement", "casement", "slider", "timber"]),
        plan: rng.pick(&["windows", "balconies", "bands", "mixed", "mixed"]),
        bay: rng.float(2.3, 3.3),
        hoods: rng.chance(0.45),
        canopy: rng.chance(0.5),
        balustrade: rng.pick(&["solid", "solid", "rail", "half"]),
        bars: rng.pick(&["bars", "grid", "diamond"]),
        enclosure: rng.pick(&["open", "open", "caged", "glazed"]),
        awning: Awning {
            canvas,
            base: awning,
            hem: if canvas && rng.chance(0.6) {
                srgb("#ece6d4")
            } else {
                shade(awning, 0.85)
            },
        },
        repaints: [[0.; 3]; 3],
        repaint: rng.float(0.04, 0.2),
        clutter: rng.float(0.55, 1.15),
    };
    let pastel = mix([1.; 3], style.accent, 0.45);
    let peak = pastel.into_iter().fold(f64::NEG_INFINITY, f64::max) / 1.08;
    style.repaints = [
        [1.1, 1.1, 1.08],
        [1.04, 1.07, 1.1],
        pastel.map(|v| v / peak),
    ];
    style
}

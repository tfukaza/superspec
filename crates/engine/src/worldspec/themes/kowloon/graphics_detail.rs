//! How much geometry a fixture draws for the viewer preset it is published for.
//!
//! The level is a render-only choice. It may shorten what a generator draws; it never takes
//! part in a `when`, a random draw, a feature, a zone, a survey or a weathering recipe, so
//! every level places the same fixtures with the same tints. A generator reads it from the
//! mesh it draws into ([`super::mesh::Mesh::graphics_detail`]).
use serde_json::Value;
use std::cell::Cell;

/// The item and native parameter that carries the level: 0, 1 or 2.
pub const PARAMETER: &str = "graphicsDetail";
/// At a reduced level no member narrower than this is drawn at its own size.
pub const THINNEST: f64 = 0.02;
/// What a kept member narrower than [`THINNEST`] is widened to.
pub const WIDENED: f64 = 0.025;

#[derive(Clone, Copy, Debug, Default, Eq, Ord, PartialEq, PartialOrd)]
pub enum GraphicsDetail {
    /// Level 0: the authored fixture.
    #[default]
    Full,
    /// Level 1, the Low preset: fastenings gone, plain sections, coarse rounds.
    Low,
    /// Level 2, the Very Low preset: massing, openings and colour only.
    VeryLow,
}

impl GraphicsDetail {
    pub fn from_level(level: u8) -> Result<Self, String> {
        match level {
            0 => Ok(Self::Full),
            1 => Ok(Self::Low),
            2 => Ok(Self::VeryLow),
            _ => Err("Graphics detail must be 0, 1 or 2.".into()),
        }
    }
    pub fn level(self) -> u8 {
        self as u8
    }
    /// Level 1 or 2.
    pub fn reduced(self) -> bool {
        self != Self::Full
    }
    /// One value per level.
    pub fn pick<T>(self, full: T, low: T, very_low: T) -> T {
        match self {
            Self::Full => full,
            Self::Low => low,
            Self::VeryLow => very_low,
        }
    }
    /// The drawn width of a member `size` across: its own at level 0 and from [`THINNEST`]
    /// up, [`WIDENED`] below that at a reduced level. Omit a member instead where a wider
    /// one would misrepresent the fixture.
    pub fn readable(self, size: f64) -> f64 {
        if self.reduced() && size < THINNEST {
            WIDENED
        } else {
            size
        }
    }
    /// The sides of a round section of `radius` asked for with `sides`: fewer the smaller
    /// the section is, never more than asked.
    pub fn sides(self, radius: f64, sides: usize) -> usize {
        let coarse = match self {
            Self::Full => return sides,
            Self::Low if radius < 0.03 => 4,
            Self::Low if radius < 0.12 => 6,
            Self::Low if radius < 0.35 => 8,
            Self::Low => 12,
            Self::VeryLow if radius < 0.12 => 4,
            Self::VeryLow if radius < 0.35 => 6,
            Self::VeryLow => 8,
        };
        sides.min(coarse)
    }
}

thread_local! {
    static REQUESTED: Cell<GraphicsDetail> = const { Cell::new(GraphicsDetail::Full) };
}

/// The level of the native generator running on this thread; level 0 outside one.
pub(super) fn requested() -> GraphicsDetail {
    REQUESTED.with(Cell::get)
}

/// Runs `generate` as a native generator asked for `level`, restoring the caller's level.
pub(super) fn with_requested<T>(level: GraphicsDetail, generate: impl FnOnce() -> T) -> T {
    struct Restore(GraphicsDetail);
    impl Drop for Restore {
        fn drop(&mut self) {
            REQUESTED.with(|cell| cell.set(self.0));
        }
    }
    let _restore = Restore(REQUESTED.with(|cell| cell.replace(level)));
    generate()
}

/// Reads [`PARAMETER`] from a native generator's values: absent is level 0.
pub fn parse(value: Option<&Value>) -> Result<GraphicsDetail, String> {
    match value {
        None => Ok(GraphicsDetail::Full),
        Some(value) => value
            .as_f64()
            .filter(|level| level.fract() == 0. && (0. ..=2.).contains(level))
            .ok_or_else(|| format!("{PARAMETER} must be 0, 1 or 2."))
            .and_then(|level| GraphicsDetail::from_level(level as u8)),
    }
}

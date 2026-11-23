//! The variant catalogue: how many different shapes one part type may draw.
//!
//! A seeded fixture used to draw every dimension from its own stream, so sixty air
//! conditioners were sixty meshes. With a budget, a part type has a fixed catalogue of
//! shapes and each instance picks one entry by its seed; what stays the instance's own is
//! where it stands and its colours. Equal picks are then equal geometry, which a later stage
//! can draw as instances of one mesh and bake once.
//!
//! The catalogue of a part type is an ordered list of [`LARGEST`] entries, and a budget of
//! `n` uses its first `n`. The sets therefore nest: the two shapes of the Low preset are
//! among the four of Medium, and a library built for the largest budget serves every preset.
//! An instance keeps one uniform draw `u` for its pick. Its entry at budget `n` is the first
//! `log2(n)` binary digits of `u`, least significant first, so its entry at half the budget
//! is its entry at the full budget with the top bit cleared: halving the budget folds each
//! pair of entries `(k, k + n/2)` onto `k`.
//!
//! Budgets are whole powers of two; any other is rounded down to one. Zero means no
//! catalogue: every instance draws its own shape, as before the catalogue existed.
//!
//! The budget is render-only, like the graphics detail it defaults from: it never takes part
//! in a placement, a feature, a zone, a survey or collision.
use super::graphics_detail::GraphicsDetail;
use serde_json::Value;
use std::cell::Cell;
use superspec_types::rng::SeededRng;

/// The item and native parameter that carries the budget.
pub const PARAMETER: &str = "variantBudget";
/// The entries of every catalogue, and the largest budget (the Ultra preset's).
pub const LARGEST: u32 = 64;

/// How many variants each part type may draw.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Budget {
    /// Every instance draws its own shape.
    Unlimited,
    /// The first `n` catalogue entries; `n` is a power of two up to [`LARGEST`].
    Variants(u32),
}

impl Budget {
    /// `variants` rounded down to a power of two no larger than [`LARGEST`]; zero is
    /// [`Budget::Unlimited`].
    pub fn new(variants: u32) -> Self {
        match variants {
            0 => Self::Unlimited,
            n => Self::Variants(1 << (31 - n.min(LARGEST).leading_zeros())),
        }
    }
    /// The budget of a graphics detail that states none: Medium's 4 for the authored
    /// building (level 0), Low's 2 for level 1 and Very Low's 1 for level 2. High (8), Very
    /// High (16) and Ultra (64) are level 0 with the budget stated.
    pub fn of(detail: GraphicsDetail) -> Self {
        Self::Variants(detail.pick(4, 2, 1))
    }
    /// The catalogue entry of an instance whose pick draw was `u` in `0..1`, or `None` when
    /// every instance draws its own shape.
    pub fn entry(self, u: f64) -> Option<u32> {
        let Self::Variants(n) = self else {
            return None;
        };
        let mut u = u.clamp(0., 1. - f64::EPSILON);
        let mut entry = 0;
        let mut bit = 1;
        while bit < n {
            u *= 2.;
            if u >= 1. {
                u -= 1.;
                entry |= bit;
            }
            bit <<= 1;
        }
        Some(entry)
    }
    /// Draws an instance's pick from its stream and returns its entry: one draw at every
    /// budget, and none where every instance draws its own shape.
    pub fn pick(self, rng: &mut SeededRng) -> Option<u32> {
        match self {
            Self::Unlimited => None,
            Self::Variants(_) => {
                let drawn = self.entry(rng.next_f64());
                STATED.with(Cell::get).map(|(entry, _)| entry).or(drawn)
            }
        }
    }
}

thread_local! {
    /// The catalogue entry being drawn as a shared shape on this thread, and the colour that
    /// stands in for the one its instances own.
    static STATED: Cell<Option<(u32, Option<(&'static str, [f64; 3])>)>> = const { Cell::new(None) };
}

/// Runs `generate` as the drawing of catalogue entry `entry` itself, whatever seed it is
/// given: every pick on this thread is that entry, under any budget but none, and the colour
/// an instance owns under the name `tint.0` is `tint.1` (see [`instance_colour`]).
pub(super) fn with_entry<T>(
    entry: u32,
    tint: Option<(&'static str, [f64; 3])>,
    generate: impl FnOnce() -> T,
) -> T {
    struct Restore(Option<(u32, Option<(&'static str, [f64; 3])>)>);
    impl Drop for Restore {
        fn drop(&mut self) {
            STATED.with(|cell| cell.set(self.0));
        }
    }
    let _restore = Restore(STATED.with(|cell| cell.replace(Some((entry, tint)))));
    generate()
}

/// The one colour an instance of a catalogued fixture owns, by `role`: `own`, as its stream
/// drew it, unless the entry is being drawn as a shared shape in a stand-in colour.
pub(super) fn instance_colour(role: &str, own: [f64; 3]) -> [f64; 3] {
    match STATED.with(Cell::get) {
        Some((_, Some((stated, colour)))) if stated == role => colour,
        _ => own,
    }
}

/// The stream of one catalogue entry of a part type: every shape decision of that entry is
/// drawn from it, so an entry is the same shape wherever and whenever it is picked.
pub fn stream(part: &str, entry: u32) -> SeededRng {
    SeededRng::from_text(&format!("kowloon/variant/{part}/{entry}"))
}

thread_local! {
    static REQUESTED: Cell<Option<Budget>> = const { Cell::new(None) };
}

/// The budget of the native generator running on this thread, for a fixture drawn at
/// `detail`: the one it was asked for, else the detail's own.
pub(super) fn requested(detail: GraphicsDetail) -> Budget {
    REQUESTED.with(Cell::get).unwrap_or(Budget::of(detail))
}

/// Runs `generate` as a native generator asked for `budget` (`None`: the detail's own),
/// restoring the caller's.
pub(super) fn with_requested<T>(budget: Option<Budget>, generate: impl FnOnce() -> T) -> T {
    struct Restore(Option<Budget>);
    impl Drop for Restore {
        fn drop(&mut self) {
            REQUESTED.with(|cell| cell.set(self.0));
        }
    }
    let _restore = Restore(REQUESTED.with(|cell| cell.replace(budget)));
    generate()
}

/// Reads [`PARAMETER`] from a native generator's values: absent or -1 leaves the budget to
/// the graphics detail, zero is unlimited.
pub fn parse(value: Option<&Value>) -> Result<Option<Budget>, String> {
    match value {
        None => Ok(None),
        Some(value) => value
            .as_f64()
            .filter(|n| n.fract() == 0. && (-1. ..=LARGEST as f64).contains(n))
            .map(|n| (n >= 0.).then(|| Budget::new(n as u32)))
            .ok_or_else(|| format!("{PARAMETER} must be a whole number from -1 to {LARGEST}.")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn budgets_nest_and_fold() {
        for step in 0..1000 {
            let u = step as f64 / 1000.;
            let mut previous = None;
            for n in [64, 32, 16, 8, 4, 2, 1] {
                let entry = Budget::new(n).entry(u).unwrap();
                assert!(entry < n);
                if let Some(larger) = previous {
                    assert_eq!(entry, larger % n, "u {u} budget {n}");
                }
                previous = Some(entry);
            }
            assert_eq!(Budget::Unlimited.entry(u), None);
        }
        // Every entry of a budget is picked, about equally often.
        let mut picked = [0; 8];
        for step in 0..8000 {
            picked[Budget::new(8).entry((step as f64 + 0.5) / 8000.).unwrap() as usize] += 1;
        }
        assert_eq!(picked, [1000; 8]);
        assert_eq!(Budget::new(5), Budget::Variants(4));
        assert_eq!(Budget::new(1000), Budget::Variants(LARGEST));
        assert_eq!(Budget::new(0), Budget::Unlimited);
    }
}

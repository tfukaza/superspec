//! Seeded values and vector helpers that give the same bits on every target: integer
//! mixing, IEEE basic operations and the shared deterministic `sin`/`cos`.
use superspec_geometry::math::{V3, add, cos, cross, dot, scale, sin, unit};

pub(super) const TAU: f64 = std::f64::consts::TAU;

fn mix(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9e37_79b9_7f4a_7c15);
    x = (x ^ (x >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    x ^ (x >> 31)
}

/// A stable hash of a seed and integer keys, such as a grid cell.
pub(super) fn hash(seed: u64, keys: &[i64]) -> u64 {
    keys.iter()
        .fold(mix(seed), |state, key| mix(state ^ mix(*key as u64)))
}

/// A value in [0, 1) from the top 53 bits.
pub(super) fn unit_float(bits: u64) -> f64 {
    (bits >> 11) as f64 / (1u64 << 53) as f64
}

/// A sequence of values from one hash.
pub(super) struct Stream(u64);
impl Stream {
    pub(super) fn new(seed: u64, keys: &[i64]) -> Self {
        Self(hash(seed, keys))
    }
    pub(super) fn next(&mut self) -> f64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        unit_float(mix(self.0))
    }
    /// A value in [-1, 1).
    pub(super) fn signed(&mut self) -> f64 {
        self.next() * 2. - 1.
    }
    pub(super) fn between(&mut self, range: [f64; 2]) -> f64 {
        range[0] + (range[1] - range[0]) * self.next()
    }
}

/// Smooth value noise in [0, 1] over the plane, one unit per lattice cell.
pub(super) fn noise(seed: u64, x: f64, y: f64) -> f64 {
    let (cx, cy) = (x.floor(), y.floor());
    let fade = |t: f64| t * t * (3. - 2. * t);
    let (fx, fy) = (fade(x - cx), fade(y - cy));
    let corner = |dx: i64, dy: i64| unit_float(hash(seed, &[cx as i64 + dx, cy as i64 + dy]));
    let low = corner(0, 0) + (corner(1, 0) - corner(0, 0)) * fx;
    let high = corner(0, 1) + (corner(1, 1) - corner(0, 1)) * fx;
    low + (high - low) * fy
}

/// A unit vector perpendicular to `direction`, chosen the same way everywhere.
pub(super) fn perpendicular(direction: V3) -> V3 {
    let other = if direction[2].abs() < 0.9 {
        [0., 0., 1.]
    } else {
        [1., 0., 0.]
    };
    unit(cross(direction, other)).unwrap_or([1., 0., 0.])
}

/// `direction`, or `fallback` when it has no length.
pub(super) fn unit_or(direction: V3, fallback: V3) -> V3 {
    unit(direction).unwrap_or(fallback)
}

/// The unit vector at `angle` from `axis` toward the azimuth `turn` around it.
pub(super) fn tilted(axis: V3, angle: f64, turn: f64) -> V3 {
    let side = perpendicular(axis);
    let other = cross(axis, side);
    let around = add(scale(side, cos(turn)), scale(other, sin(turn)));
    unit_or(
        add(scale(axis, cos(angle)), scale(around, sin(angle))),
        axis,
    )
}

/// Removes the part of `vector` along the unit `axis`.
pub(super) fn reject(vector: V3, axis: V3) -> V3 {
    add(vector, scale(axis, -dot(vector, axis)))
}

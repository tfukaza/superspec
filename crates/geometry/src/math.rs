pub type V3 = [f64; 3];
pub fn add(a: V3, b: V3) -> V3 {
    std::array::from_fn(|i| a[i] + b[i])
}
pub fn sub(a: V3, b: V3) -> V3 {
    std::array::from_fn(|i| a[i] - b[i])
}
pub fn scale(a: V3, s: f64) -> V3 {
    a.map(|v| v * s)
}
pub fn dot(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
pub fn cross(a: V3, b: V3) -> V3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
pub fn length(a: V3) -> f64 {
    hypot(hypot(a[0], a[1]), a[2])
}
/// Scaled Euclidean norm with the same arithmetic on native and Wasm targets.
pub fn hypot(a: f64, b: f64) -> f64 {
    if a.is_infinite() || b.is_infinite() {
        return f64::INFINITY;
    }
    if a.is_nan() || b.is_nan() {
        return f64::NAN;
    }
    let largest = a.abs().max(b.abs());
    if largest == 0. {
        return 0.;
    }
    let ratio = a.abs().min(b.abs()) / largest;
    largest * (1. + ratio * ratio).sqrt()
}
pub fn unit(a: V3) -> Result<V3, String> {
    let n = length(a);
    if !n.is_finite() || n < 1e-12 {
        Err("A direction has zero or nonfinite length.".into())
    } else {
        Ok(scale(a, 1.0 / n))
    }
}
pub fn lerp(a: V3, b: V3, t: f64) -> V3 {
    add(a, scale(sub(b, a), t))
}
pub fn orient(a: V3, b: V3, c: V3) -> f64 {
    (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
}
pub fn area(points: &[V3]) -> f64 {
    points
        .iter()
        .zip(points.iter().cycle().skip(1))
        .take(points.len())
        .map(|(a, b)| a[0] * b[1] - a[1] * b[0])
        .sum::<f64>()
        / 2.0
}
pub fn inside(p: V3, ring: &[V3]) -> bool {
    let mut result = false;
    for (a, b) in ring
        .iter()
        .zip(ring.iter().cycle().skip(1))
        .take(ring.len())
    {
        if (a[1] > p[1]) != (b[1] > p[1])
            && p[0] < (b[0] - a[0]) * (p[1] - a[1]) / (b[1] - a[1]) + a[0]
        {
            result = !result;
        }
    }
    result
}
/// Proper interior crossing; shared endpoints are allowed and split separately.
pub fn crossing(a: V3, b: V3, c: V3, d: V3) -> bool {
    orient(a, b, c) * orient(a, b, d) < -1e-18 && orient(c, d, a) * orient(c, d, b) < -1e-18
}
pub fn stations(path: &[V3]) -> Vec<f64> {
    let mut result = vec![0.0];
    for pair in path.windows(2) {
        result
            .push(result.last().unwrap() + hypot(pair[1][0] - pair[0][0], pair[1][1] - pair[0][1]));
    }
    result
}
pub fn at(path: &[V3], distance: f64) -> V3 {
    let cuts = stations(path);
    let i = cuts
        .partition_point(|s| *s < distance)
        .clamp(1, path.len() - 1);
    let fraction = if cuts[i] > cuts[i - 1] {
        ((distance - cuts[i - 1]) / (cuts[i] - cuts[i - 1])).clamp(0.0, 1.0)
    } else {
        0.0
    };
    lerp(path[i - 1], path[i], fraction)
}
/// Ordered station/height profile with before/after semantics at vertical steps.
pub fn height(profile: &[V3], distance: f64, after: bool) -> Result<f64, String> {
    if profile.len() < 2
        || distance < profile[0][0] - 1e-7
        || distance > profile.last().unwrap()[0] + 1e-7
    {
        return Err("Longitudinal profile does not cover the requested station.".into());
    }
    let equal: Vec<_> = profile
        .iter()
        .filter(|p| (p[0] - distance).abs() < 1e-8)
        .collect();
    if !equal.is_empty() {
        return Ok(if after {
            equal.last().unwrap()[1]
        } else {
            equal[0][1]
        });
    }
    for pair in profile.windows(2) {
        if distance < pair[1][0] {
            return Ok(pair[0][1]
                + (pair[1][1] - pair[0][1]) * (distance - pair[0][0]) / (pair[1][0] - pair[0][0]));
        }
    }
    Ok(profile.last().unwrap()[1])
}

/// Column-major affine matrix; calculations stay f64 until an established mesh boundary.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Matrix(pub [V3; 4]);
impl Default for Matrix {
    fn default() -> Self {
        Self([[1., 0., 0.], [0., 1., 0.], [0., 0., 1.], [0., 0., 0.]])
    }
}
impl Matrix {
    pub fn translation(position: V3) -> Self {
        Self([[1., 0., 0.], [0., 1., 0.], [0., 0., 1.], position])
    }
    pub fn checked(self) -> Result<Self, String> {
        if self.0.iter().flatten().any(|v| !v.is_finite())
            || !self.determinant().is_finite()
            || self.determinant() == 0.
        {
            Err("Composed transform is nonfinite or singular.".into())
        } else {
            Ok(self)
        }
    }
    pub fn point(self, p: V3) -> V3 {
        add(self.direction(p), self.0[3])
    }
    pub fn direction(self, p: V3) -> V3 {
        add(
            add(scale(self.0[0], p[0]), scale(self.0[1], p[1])),
            scale(self.0[2], p[2]),
        )
    }
    pub fn then(self, child: Self) -> Self {
        Self([
            self.direction(child.0[0]),
            self.direction(child.0[1]),
            self.direction(child.0[2]),
            self.point(child.0[3]),
        ])
    }
    pub fn determinant(self) -> f64 {
        dot(self.0[0], cross(self.0[1], self.0[2]))
    }
    pub fn inverse(self) -> Result<Self, String> {
        let det = self.determinant();
        if det.abs() < 1e-20 {
            return Err("Singular placement.".into());
        }
        let rows = [
            scale(cross(self.0[1], self.0[2]), 1.0 / det),
            scale(cross(self.0[2], self.0[0]), 1.0 / det),
            scale(cross(self.0[0], self.0[1]), 1.0 / det),
        ];
        let mut matrix = Self(std::array::from_fn(|col| {
            if col < 3 {
                std::array::from_fn(|row| rows[row][col])
            } else {
                [0.; 3]
            }
        }));
        matrix.0[3] = scale(matrix.direction(self.0[3]), -1.0);
        Ok(matrix)
    }
    /// Extrinsic XYZ rotations, matching the existing ZYX quaternion composition.
    pub fn placement(position: V3, rotation: V3, sizes: V3) -> Result<Self, String> {
        if rotation
            .iter()
            .chain(&position)
            .chain(&sizes)
            .any(|v| !v.is_finite())
            || sizes.contains(&0.0)
        {
            return Err("Placement requires finite coordinates and nonzero scales.".into());
        }
        let [x, y, z] = rotation.map(f64::to_radians);
        let (sx, cx) = (manifold_rust::math::sin(x), manifold_rust::math::cos(x));
        let (sy, cy) = (manifold_rust::math::sin(y), manifold_rust::math::cos(y));
        let (sz, cz) = (manifold_rust::math::sin(z), manifold_rust::math::cos(z));
        Ok(Self([
            scale([cz * cy, sz * cy, -sy], sizes[0]),
            scale(
                [cz * sy * sx - sz * cx, sz * sy * sx + cz * cx, cy * sx],
                sizes[1],
            ),
            scale(
                [cz * sy * cx + sz * sx, sz * sy * cx - cz * sx, cy * cx],
                sizes[2],
            ),
            position,
        ]))
    }
}
pub use manifold_rust::math::{acos, atan2, cos, sin};

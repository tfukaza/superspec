//! Metaball canopy hulls: one smooth closed surface around a plant's leaves, made from the
//! limb balls of its `Canopy`. The hull gives the leaves their canopy shade normals, is the far
//! level of detail in place of thinned cards (an impostor), a shell behind thinned leaves, and
//! the shadow proxy the sun's shadow map draws.
//!
//! The field is a sum of Wyvill kernels `(1 - s)^3` with `s = |p - c|^2 / R^2`, one per limb
//! ball and one core ball at the balls' weighted centre, so the centre is always inside. The
//! surface is the field's level 1/8: a lone ball of reach `R` ends at `R / sqrt(2)`. A hull
//! vertex is found by marching inward along a direction from the centre, so the mesh is the
//! outermost surface seen from the centre (a shrink wrap): pockets the centre cannot see are
//! filled. Only IEEE basic operations and `sqrt`, so native and Wasm builds agree bit for bit.
use super::random::{hash, unit_float, unit_or};
use super::tree::Blob;
use super::{BEND_WEIGHT, ELEMENT, FoliageMesh, HEIGHT_RATIO, PIVOT, RANDOM, SHADE_NORMAL};
use std::collections::BTreeMap;
use superspec_geometry::math::{V3, add, cross, dot, length, scale, sub};

/// The field's level that is the surface.
const ISO: f64 = 0.125;
/// Reach of a ball whose surface (alone) is one meter away: `sqrt(2)`.
const REACH: f64 = std::f64::consts::SQRT_2;
const UP: V3 = [0., 0., 1.];

struct Ball {
    center: V3,
    /// `1 / R^2`.
    inverse: f64,
    reach: f64,
    bend: f64,
}

/// The canopy's field and where hull rays start.
pub struct CanopyField {
    balls: Vec<Ball>,
    /// Hull rays leave from here.
    pub origin: V3,
    /// Farther than any surface from `origin`.
    far: f64,
    /// March step: a sixth of the smallest reach.
    step: f64,
}

impl CanopyField {
    /// The field of a plant's limb balls. A ball's surface is `1.25` times the spread of its
    /// clusters plus half the largest leaf cluster (and at least one cluster's length). `None`
    /// without balls.
    pub fn new(blobs: &[Blob], leaf_size: f64) -> Option<Self> {
        if blobs.is_empty() {
            return None;
        }
        let mut balls: Vec<Ball> = blobs
            .iter()
            .map(|blob| {
                let surface = (1.25 * blob.spread + 0.5 * leaf_size).max(leaf_size);
                let reach = REACH * surface;
                Ball {
                    center: blob.center,
                    inverse: 1. / (reach * reach),
                    reach,
                    bend: blob.bend,
                }
            })
            .collect();
        // The core: at the balls' centre weighted by volume, half the mean ball's size.
        let weight: f64 = balls.iter().map(|b| b.reach * b.reach * b.reach).sum();
        let origin = balls.iter().fold([0.; 3], |sum, b| {
            add(sum, scale(b.center, b.reach * b.reach * b.reach / weight))
        });
        let mean = balls.iter().map(|b| b.reach).sum::<f64>() / balls.len() as f64;
        let core = 0.5 * mean;
        let bend = balls.iter().map(|b| b.bend).sum::<f64>() / balls.len() as f64;
        balls.push(Ball {
            center: origin,
            inverse: 1. / (core * core),
            reach: core,
            bend,
        });
        let far = balls
            .iter()
            .map(|b| length(sub(b.center, origin)) + b.reach)
            .fold(0., f64::max);
        let step = balls.iter().map(|b| b.reach).fold(f64::INFINITY, f64::min) / 6.;
        Some(Self {
            balls,
            origin,
            far,
            step,
        })
    }

    fn value(&self, p: V3) -> f64 {
        self.balls
            .iter()
            .map(|ball| {
                let d = sub(p, ball.center);
                let s = dot(d, d) * ball.inverse;
                if s < 1. {
                    let k = 1. - s;
                    k * k * k
                } else {
                    0.
                }
            })
            .sum()
    }

    /// The outward normal of the field at `p` (its falling gradient), or `None` where it is flat.
    pub fn normal(&self, p: V3) -> Option<V3> {
        let mut out = [0.; 3];
        for ball in &self.balls {
            let d = sub(p, ball.center);
            let s = dot(d, d) * ball.inverse;
            if s < 1. {
                let k = 1. - s;
                out = add(out, scale(d, 6. * k * k * ball.inverse));
            }
        }
        (dot(out, out) > 1e-18).then(|| unit_or(out, UP))
    }

    /// The wind weight at `p`: the balls' mean `bendWeight` weighted by a kernel of twice their
    /// reach, so every point of the hull has one.
    fn bend(&self, p: V3) -> f64 {
        let (mut sum, mut total) = (0., 0.);
        for ball in &self.balls {
            let d = sub(p, ball.center);
            let s = dot(d, d) * ball.inverse * 0.25;
            if s < 1. {
                let k = (1. - s) * (1. - s);
                sum += k * ball.bend;
                total += k;
            }
        }
        if total > 0. {
            sum / total
        } else {
            self.balls[self.balls.len() - 1].bend
        }
    }

    /// Meters from `origin` to the outermost surface along the unit `direction`.
    pub fn surface(&self, direction: V3) -> f64 {
        let at = |t: f64| self.value(add(self.origin, scale(direction, t)));
        let mut outside = self.far;
        let mut inside = outside - self.step;
        while inside > 0. && at(inside) < ISO {
            outside = inside;
            inside -= self.step;
        }
        if inside <= 0. {
            inside = 0.;
        }
        for _ in 0..32 {
            let middle = 0.5 * (inside + outside);
            if at(middle) >= ISO {
                inside = middle;
            } else {
                outside = middle;
            }
        }
        inside
    }

    /// The canopy shade normal at a point among the leaves: the hull's normal where the ray from
    /// the centre through the point leaves the hull (75%), tipped by the field's own direction at
    /// the point (25%), so leaves shade as the hull does and still show their own clump.
    pub fn shade(&self, p: V3) -> V3 {
        let direction = unit_or(sub(p, self.origin), UP);
        let exit = add(self.origin, scale(direction, self.surface(direction)));
        let hull = self.normal(exit).unwrap_or(direction);
        let local = self.normal(p).unwrap_or(hull);
        unit_or(add(scale(hull, 0.75), scale(local, 0.25)), hull)
    }

    /// The hull as a mesh: an octahedron divided `frequency` times per edge, each direction
    /// moved to the surface and then `shrink` times as far from the origin. Normals and
    /// `shadeNormal` are the field's at the surface; `heightRatio` runs from the hull's lowest
    /// point to its highest; `bendWeight` follows the limbs nearby; `pivot` is the plant's base.
    pub fn mesh(
        &self,
        id: String,
        geometry_key: String,
        frequency: usize,
        shrink: f64,
    ) -> FoliageMesh {
        let mut mesh = FoliageMesh::new(id, geometry_key, "canopy", false);
        let corners: [V3; 6] = [
            [1., 0., 0.],
            [-1., 0., 0.],
            [0., 1., 0.],
            [0., -1., 0.],
            [0., 0., 1.],
            [0., 0., -1.],
        ];
        let k = frequency.max(1);
        let mut seen = BTreeMap::<[u64; 3], u32>::new();
        let mut vertex = |mesh: &mut FoliageMesh, p: V3| -> u32 {
            // Corners are unit axes, so a point shared by two faces is the same sum on both.
            let bits = p.map(|v| v.to_bits());
            if let Some(index) = seen.get(&bits) {
                return *index;
            }
            let index = mesh.positions.len() as u32;
            let direction = unit_or(p, UP);
            let reach = self.surface(direction);
            let at = add(self.origin, scale(direction, reach));
            let normal = self.normal(at).unwrap_or(direction);
            mesh.positions
                .push(add(self.origin, scale(direction, reach * shrink)));
            mesh.normals.push(normal);
            mesh.uvs
                .push([0.5 + 0.5 * direction[0], 0.5 + 0.5 * direction[1]]);
            mesh.scalar(BEND_WEIGHT, self.bend(at));
            mesh.scalar(RANDOM, unit_float(hash(0x4011, &[index as i64])));
            mesh.scalar(ELEMENT, 0.);
            mesh.vector(SHADE_NORMAL, normal);
            mesh.vector(PIVOT, [0.; 3]);
            seen.insert(bits, index);
            index
        };
        for sx in [0, 1] {
            for sy in [2, 3] {
                for sz in [4, 5] {
                    let (a, mut b, mut c) = (corners[sx], corners[sy], corners[sz]);
                    // Wind each face outward.
                    let n = cross(sub(b, a), sub(c, a));
                    if dot(n, add(add(a, b), c)) < 0. {
                        std::mem::swap(&mut b, &mut c);
                    }
                    let mut grid = vec![vec![0u32; k + 1]; k + 1];
                    for i in 0..=k {
                        for j in 0..=k - i {
                            let p: V3 = std::array::from_fn(|axis| {
                                a[axis] * (k - i - j) as f64
                                    + b[axis] * i as f64
                                    + c[axis] * j as f64
                            });
                            grid[i][j] = vertex(&mut mesh, p);
                        }
                    }
                    for i in 0..k {
                        for j in 0..k - i {
                            mesh.indices
                                .extend([grid[i][j], grid[i + 1][j], grid[i][j + 1]]);
                            if j + 1 < k - i {
                                mesh.indices.extend([
                                    grid[i + 1][j],
                                    grid[i + 1][j + 1],
                                    grid[i][j + 1],
                                ]);
                            }
                        }
                    }
                }
            }
        }
        let (low, high) = mesh
            .positions
            .iter()
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(l, h), p| {
                (l.min(p[2]), h.max(p[2]))
            });
        let span = (high - low).max(1e-9);
        let heights: Vec<f64> = mesh.positions.iter().map(|p| (p[2] - low) / span).collect();
        mesh.scalars.insert(HEIGHT_RATIO.into(), heights);
        mesh
    }
}

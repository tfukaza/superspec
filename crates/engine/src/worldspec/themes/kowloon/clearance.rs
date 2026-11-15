//! Immutable building-placement queries. Render support and physical obstacles stay separate.
use super::pen::Pen;
use std::collections::BTreeMap;
use superspec_geometry::obstructions::{
    Bounds, Budget, Obstructions, Triangle, triangle_touches_box,
};
use superspec_geometry::{V3, math::sub};

pub struct Collision {
    pub points: Triangle,
    pub object: String,
    pub solid: Option<String>,
}
/// Preflights repeated triangle identities before producers allocate their strings.
#[derive(Default)]
pub struct CollisionStorage {
    triangles: usize,
    identity_bytes: usize,
}
impl CollisionStorage {
    pub fn admit(&mut self, count: usize, object: &str, solid: Option<&str>) -> Result<(), String> {
        if object.is_empty() || object.len() > 4096 || solid.is_some_and(|s| s.len() > 4096) {
            return Err("Building collision source identity is invalid.".into());
        }
        let triangles = self.triangles.checked_add(count);
        let bytes = count
            .checked_mul(object.len() + solid.map_or(0, str::len))
            .and_then(|n| self.identity_bytes.checked_add(n));
        let (Some(triangles), Some(bytes)) = (triangles, bytes) else {
            return Err("Building collision storage overflow.".into());
        };
        if triangles > 1_048_576 || bytes > 16 * 1024 * 1024 {
            return Err(
                "Building collision exceeds triangle or identity storage allowances.".into(),
            );
        }
        self.triangles = triangles;
        self.identity_bytes = bytes;
        Ok(())
    }
}
pub struct Volume {
    pub corners: Vec<V3>,
    pub owner: usize,
}
struct Identity {
    object: String,
    solid: Option<String>,
}
pub struct Obstacles {
    index: Obstructions,
    identities: Vec<Identity>,
    volumes: Vec<Volume>,
}
fn point(p: V3) -> bool {
    p.into_iter()
        .all(|v| v.is_finite() && v.abs() <= 1_000_000.)
}
fn corners(low: V3, high: V3) -> [V3; 8] {
    std::array::from_fn(|i| {
        [
            if i & 4 == 0 { low[0] } else { high[0] },
            if i & 2 == 0 { low[1] } else { high[1] },
            if i & 1 == 0 { low[2] } else { high[2] },
        ]
    })
}
impl Obstacles {
    pub fn new(
        triangles: Vec<Collision>,
        volumes: Vec<Volume>,
        budget: &mut Budget<'_>,
    ) -> Result<Self, String> {
        let mut storage = CollisionStorage::default();
        for triangle in &triangles {
            storage.admit(1, &triangle.object, triangle.solid.as_deref())?;
        }
        if triangles.len() > 1_048_576
            || volumes.len() > 4096
            || volumes.iter().any(|v| {
                !(4..=128).contains(&v.corners.len()) || v.corners.iter().any(|p| !point(*p))
            })
        {
            return Err("Building obstacles exceed bounded geometry or source identity.".into());
        }
        budget.spend(volumes.iter().map(|v| v.corners.len()).sum())?;
        let positions: Vec<_> = triangles.iter().map(|t| t.points).collect();
        let index = Obstructions::new(&positions, budget)?;
        let identities = triangles
            .into_iter()
            .map(|t| Identity {
                object: t.object,
                solid: t.solid.filter(|s| !s.is_empty()),
            })
            .collect();
        Ok(Self {
            index,
            identities,
            volumes,
        })
    }
    /// The nearest upward crossing of each authored solid must exit it. Open patches never establish containment.
    pub fn contains(
        &self,
        at: V3,
        exempt: Option<&str>,
        budget: &mut Budget<'_>,
    ) -> Result<bool, String> {
        budget.spend(0)?;
        if !point(at) {
            return Err("Containment requires a bounded finite world point.".into());
        }
        let candidates = self.index.near(
            Bounds {
                low: at,
                high: [at[0], at[1], 1_000_000.],
            },
            budget,
        )?;
        let mut nearest: BTreeMap<&str, (f64, bool)> = BTreeMap::new();
        for i in candidates {
            budget.spend(1)?;
            let id = &self.identities[i];
            let Some(solid) = id.solid.as_deref() else {
                continue;
            };
            if exempt == Some(id.object.as_str()) {
                continue;
            }
            let t = self.index.triangle(i).unwrap();
            let Some(height) = upward_hit(at, t) else {
                continue;
            };
            if nearest
                .get(solid)
                .is_some_and(|(before, _)| *before <= height)
            {
                continue;
            }
            let [a, b, c] = *t;
            let normal = (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]);
            nearest.insert(solid, (height, normal > 0.));
        }
        Ok(nearest.values().any(|(_, exits)| *exits))
    }
    /// Tests collision triangles in the proposed part's oriented frame and conservative foreign volumes.
    pub fn blocked(
        &self,
        pen: &Pen,
        low: V3,
        high: V3,
        owner: usize,
        exempt: Option<&str>,
        budget: &mut Budget<'_>,
    ) -> Result<bool, String> {
        budget.spend(0)?;
        if !point(low) || !point(high) || (0..3).any(|i| low[i] > high[i]) {
            return Err("Placement bounds must be finite, ordered and bounded.".into());
        }
        self.blocked_probe(pen, low, high, owner, exempt, budget)
    }
    /// Polygon facade probes retain signed half extents from the source planner,
    /// including inverted upper-story intervals on one-story lots.
    pub(super) fn blocked_probe(
        &self,
        pen: &Pen,
        low: V3,
        high: V3,
        owner: usize,
        exempt: Option<&str>,
        budget: &mut Budget<'_>,
    ) -> Result<bool, String> {
        budget.spend(0)?;
        if !point(low) || !point(high) {
            return Err("Placement probe must be finite and bounded.".into());
        }
        let inverse = pen.matrix.inverse()?;
        for volume in &self.volumes {
            budget.spend(volume.corners.len())?;
            if volume.owner == owner {
                continue;
            }
            let bounds = Bounds::points(volume.corners.iter().map(|p| inverse.point(*p)), 0.);
            if (0..3).all(|i| bounds.low[i] < high[i] && bounds.high[i] > low[i]) {
                return Ok(true);
            }
        }
        let bounds = Bounds::points(corners(low, high).map(|p| pen.world(p)), 0.);
        let center = std::array::from_fn(|i| (low[i] + high[i]) / 2.);
        let half = std::array::from_fn(|i| (high[i] - low[i]) / 2.);
        for i in self.index.near(bounds, budget)? {
            budget.spend(1)?;
            if exempt == Some(self.identities[i].object.as_str()) {
                continue;
            }
            let local = self.index.triangle(i).unwrap().map(|p| inverse.point(p));
            if triangle_touches_box(&local, center, half) {
                return Ok(true);
            }
        }
        Ok(false)
    }
}
// Watertight signed edge functions for an upward ray, matching the source renderer's ray primitive.
fn upward_hit(origin: V3, t: &Triangle) -> Option<f64> {
    let [a, b, c] = t.map(|p| sub(p, origin));
    let u = c[0] * b[1] - c[1] * b[0];
    let v = a[0] * c[1] - a[1] * c[0];
    let w = b[0] * a[1] - b[1] * a[0];
    if (u < 0. || v < 0. || w < 0.) && (u > 0. || v > 0. || w > 0.) {
        return None;
    }
    let determinant = u + v + w;
    if determinant == 0. {
        return None;
    }
    let scaled = u * a[2] + v * b[2] + w * c[2];
    if if determinant > 0. {
        scaled < 0.
    } else {
        scaled > 0.
    } {
        return None;
    }
    Some(origin[2] + scaled / determinant)
}
pub struct RenderTriangle {
    pub points: Triangle,
    pub visible: bool,
}
/// Samples authored horizontal render faces. A successful check is not navigation proof.
pub struct GroundSupport {
    index: Obstructions,
}
impl GroundSupport {
    pub fn new(triangles: &[RenderTriangle], budget: &mut Budget<'_>) -> Result<Self, String> {
        if triangles.len() > 1_048_576 {
            return Err("Ground support exceeds its triangle allowance.".into());
        }
        let mut flat = Vec::new();
        for triangle in triangles {
            budget.spend(1)?;
            if triangle.points.iter().any(|p| !point(*p)) {
                return Err("Ground support requires bounded finite coordinates.".into());
            }
            if !triangle.visible {
                continue;
            }
            let [a, b, c] = triangle.points;
            let area = (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]);
            if area > 1e-10 && a[2].max(b[2]).max(c[2]) - a[2].min(b[2]).min(c[2]) <= 0.001 {
                flat.push(triangle.points);
            }
        }
        Ok(Self {
            index: Obstructions::new(&flat, budget)?,
        })
    }
    fn supports(&self, at: V3, budget: &mut Budget<'_>) -> Result<bool, String> {
        // Preserve source cell membership even for tiny triangles whose edge tolerance
        // reaches beyond their exact bounds. The BVH stores each triangle only once.
        let cell = [(at[0] / 2.).floor(), (at[1] / 2.).floor()];
        let bounds = Bounds {
            low: [cell[0] * 2., cell[1] * 2., at[2] - 0.006],
            high: [cell[0] * 2. + 2., cell[1] * 2. + 2., at[2] + 0.006],
        };
        for i in self.index.near(bounds, budget)? {
            budget.spend(1)?;
            let [a, b, c] = *self.index.triangle(i).unwrap();
            if (0..2).any(|axis| {
                (a[axis].min(b[axis]).min(c[axis]) / 2.).floor() > cell[axis]
                    || (a[axis].max(b[axis]).max(c[axis]) / 2.).floor() < cell[axis]
            }) {
                continue;
            }
            if (a[2] - at[2]).abs() > 0.005 {
                continue;
            }
            let cross =
                |p: V3, q: V3| (q[0] - p[0]) * (at[1] - p[1]) - (q[1] - p[1]) * (at[0] - p[0]);
            if cross(a, b) >= -1e-9 && cross(b, c) >= -1e-9 && cross(c, a) >= -1e-9 {
                return Ok(true);
            }
        }
        Ok(false)
    }
    pub fn covers(
        &self,
        pen: &Pen,
        rect: [f64; 4],
        z: f64,
        budget: &mut Budget<'_>,
    ) -> Result<bool, String> {
        let [x0, x1, y0, y1] = rect;
        if rect
            .into_iter()
            .chain([z])
            .any(|v| !v.is_finite() || v.abs() > 1_000_000.)
            || x1 < x0
            || y1 < y0
            || x1 - x0 > 100.
            || y1 - y0 > 100.
        {
            return Err("Ground sampling requires a bounded ordered rectangle.".into());
        }
        let columns = ((x1 - x0) / 0.15).ceil().max(1.) as usize;
        let rows = ((y1 - y0) / 0.15).ceil().max(1.) as usize;
        budget.spend((columns + 1) * (rows + 1))?;
        for column in 0..=columns {
            for row in 0..=rows {
                let p = pen.world([
                    x0 + (x1 - x0) * column as f64 / columns as f64,
                    y0 + (y1 - y0) * row as f64 / rows as f64,
                    z,
                ]);
                if !self.supports(p, budget)? {
                    return Ok(false);
                }
            }
        }
        Ok(true)
    }
}

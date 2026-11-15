//! Stable cell traversal is part of seeded span partner selection.
use std::collections::BTreeMap;
use superspec_geometry::obstructions::Budget;
use superspec_geometry::{V3, math::hypot};
pub struct Index {
    points: Vec<V3>,
    radius: f64,
    rise: f64,
    cells: BTreeMap<[i32; 3], Vec<usize>>,
}
impl Index {
    pub fn new(
        points: &[V3],
        radius: f64,
        rise: f64,
        budget: &mut Budget<'_>,
    ) -> Result<Self, String> {
        if points.len() > 65_536
            || points
                .iter()
                .flatten()
                .any(|v| !v.is_finite() || v.abs() > 1e6)
            || !radius.is_finite()
            || !(0.01..=100.).contains(&radius)
            || !rise.is_finite()
            || !(0.01..=100.).contains(&rise)
        {
            return Err(
                "Anchor neighbors require bounded finite positions and positive reach.".into(),
            );
        }
        budget.spend(points.len() + 1)?;
        let mut index = Self {
            points: points.to_vec(),
            radius,
            rise,
            cells: BTreeMap::new(),
        };
        for (i, p) in points.iter().enumerate() {
            let cell = index.cell(*p);
            index.cells.entry(cell).or_default().push(i);
        }
        Ok(index)
    }
    fn cell(&self, p: V3) -> [i32; 3] {
        [
            (p[0] / self.radius).floor() as i32,
            (p[1] / self.radius).floor() as i32,
            (p[2] / self.rise).floor() as i32,
        ]
    }
    /// Inclusive distances, dx/dy/dz traversal, then original insertion order within each cell.
    pub fn near(&self, anchor: usize, budget: &mut Budget<'_>) -> Result<Vec<usize>, String> {
        let a = *self.points.get(anchor).ok_or("Unknown span anchor.")?;
        budget.spend(27)?;
        let cell = self.cell(a);
        let mut result = vec![];
        for dx in -1..=1 {
            for dy in -1..=1 {
                for dz in -1..=1 {
                    if let Some(ids) = self.cells.get(&[cell[0] + dx, cell[1] + dy, cell[2] + dz]) {
                        for &i in ids {
                            budget.spend(1)?;
                            let b = self.points[i];
                            if i != anchor
                                && (a[2] - b[2]).abs() <= self.rise
                                && hypot(a[0] - b[0], a[1] - b[1]) <= self.radius
                            {
                                result.push(i);
                            }
                        }
                    }
                }
            }
        }
        Ok(result)
    }
}

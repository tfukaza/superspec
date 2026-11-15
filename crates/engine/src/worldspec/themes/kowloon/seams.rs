//! Finite physical wall/floor contacts, in source traversal order, for visual weathering.
use std::collections::{BTreeMap, BTreeSet};
use superspec_geometry::obstructions::{Budget, Triangle};
use superspec_geometry::{
    V3,
    math::{dot, sub},
};

pub type Seam = [V3; 2];
#[derive(Clone, Copy)]
pub enum Kind {
    Floor,
    Stairs,
    Wall,
    Other,
}
#[derive(Clone, Copy)]
pub struct Facet<'a> {
    pub object: &'a str,
    pub kind: Kind,
    pub visible: bool,
    pub collidable: bool,
    pub points: Triangle,
    pub normal_z: f64,
}
pub struct WallContacts {
    pub host: String,
    pub seams: Vec<Seam>,
}
#[derive(Default)]
pub struct Contacts {
    pub by_object: BTreeMap<String, Vec<Seam>>,
    /// Wall order is first contact order, not lexical host order.
    pub walls_by_floor: BTreeMap<String, Vec<WallContacts>>,
}
fn distance(a: V3, b: V3) -> f64 {
    let d = sub(a, b);
    dot(d, d).sqrt()
}
fn cross(p: V3, q: V3, r: V3) -> f64 {
    (q[0] - p[0]) * (r[1] - p[1]) - (q[1] - p[1]) * (r[0] - p[0])
}
fn clip(a: V3, b: V3, floor: Triangle) -> Option<Seam> {
    let mut low: f64 = 0.;
    let mut high: f64 = 1.;
    let area = cross(floor[0], floor[1], floor[2]);
    if area == 0. {
        return None;
    }
    let sign = area.signum();
    for i in 0..3 {
        let p = floor[i];
        let q = floor[(i + 1) % 3];
        let start = sign * cross(p, q, a);
        let end = sign * cross(p, q, b);
        if start < -1e-6 && end < -1e-6 {
            return None;
        }
        if start < 0. && end > start {
            low = low.max(-start / (end - start));
        }
        if end < 0. && start > end {
            high = high.min(start / (start - end));
        }
    }
    if high - low < 1e-6 {
        return None;
    }
    Some([low, high].map(|t| std::array::from_fn(|i| a[i] + t * (b[i] - a[i]))))
}

/// The caller supplies canonical physical facets, never generated visual dressing.
pub fn contacts<'a>(
    facets: impl IntoIterator<Item = Facet<'a>>,
    budget: &mut Budget<'_>,
) -> Result<Contacts, String> {
    let mut floors = Vec::new();
    let mut walls = Vec::new();
    let mut identities = BTreeSet::new();
    let mut identity_bytes = 0;
    for (index, facet) in facets.into_iter().enumerate() {
        budget.spend(1)?;
        if index >= 1_048_576 {
            return Err("Contact triangle allowance exceeded.".into());
        }
        if facet.object.is_empty()
            || facet.object.len() > 4096
            || !facet.normal_z.is_finite()
            || facet.normal_z.abs() > 1.000001
            || facet
                .points
                .iter()
                .flatten()
                .any(|v| !v.is_finite() || v.abs() > 1_000_000.)
        {
            return Err("Contacts require identified finite physical triangles.".into());
        }
        if !facet.visible || !facet.collidable {
            continue;
        }
        if identities.insert(facet.object) {
            identity_bytes += facet.object.len();
        }
        if identities.len() > 16_384 || identity_bytes > 4 * 1024 * 1024 {
            return Err("Contact identity allowance exceeded.".into());
        }
        if matches!(facet.kind, Kind::Floor | Kind::Stairs)
            && facet.normal_z > 0.99999
            && facet
                .points
                .iter()
                .all(|p| (p[2] - facet.points[0][2]).abs() < 1e-6)
        {
            floors.push(facet);
        }
        if matches!(facet.kind, Kind::Wall) && facet.normal_z.abs() < 1e-6 {
            walls.push(facet);
        }
    }
    if walls
        .len()
        .checked_mul(floors.len())
        .is_none_or(|pairs| pairs > 2_000_000)
    {
        return Err("Dressing contact budget exceeded.".into());
    }
    let mut result = Contacts::default();
    let mut groups = BTreeMap::<(&str, &str), usize>::new();
    let mut count = 0;
    for wall in walls {
        for floor in &floors {
            budget.spend(1)?;
            let z = floor.points[0][2];
            let mut points = Vec::with_capacity(6);
            for i in 0..3 {
                let a = wall.points[i];
                let b = wall.points[(i + 1) % 3];
                if (a[2] - z).abs() < 1e-6 {
                    points.push(a);
                }
                if (a[2] - z) * (b[2] - z) < 0. {
                    let t = (z - a[2]) / (b[2] - a[2]);
                    points.push(std::array::from_fn(|j| a[j] + t * (b[j] - a[j])));
                }
            }
            // Compare against all earlier candidates, matching the source's first-occurrence rule.
            let unique: Vec<_> = points
                .iter()
                .enumerate()
                .filter_map(|(i, &p)| {
                    (!points[..i].iter().any(|&q| distance(p, q) < 1e-6)).then_some(p)
                })
                .collect();
            if unique.len() != 2 {
                continue;
            }
            let Some(seam) = clip(unique[0], unique[1], floor.points) else {
                continue;
            };
            if distance(seam[0], seam[1]) < 1e-5 {
                continue;
            }
            count += 1;
            if count > 20_000 {
                return Err("Dressing contact output allowance exceeded.".into());
            }
            let contacts = result
                .walls_by_floor
                .entry(floor.object.into())
                .or_default();
            let index = *groups
                .entry((floor.object, wall.object))
                .or_insert_with(|| {
                    contacts.push(WallContacts {
                        host: wall.object.into(),
                        seams: vec![],
                    });
                    contacts.len() - 1
                });
            contacts[index].seams.push(seam);
            for object in [wall.object, floor.object] {
                result
                    .by_object
                    .entry(object.into())
                    .or_default()
                    .push(seam);
            }
        }
    }
    Ok(result)
}

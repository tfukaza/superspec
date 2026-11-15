//! Authored axes precede finish regions and seeded joins; render triangulation cannot rotate the grid.
use super::field::{edge_distance, valid_point};
use serde::{Deserialize, Serialize};
use superspec_geometry::obstructions::Budget;
use superspec_geometry::{
    Matrix, V3,
    math::{atan2, cos, hypot, sin},
};
use superspec_types::rng::SeededRng;

pub use superspec_model::theme::ground::{Anchor, Kind};
struct Ranked {
    anchor: Anchor,
    priority: u8,
}
pub struct Layout {
    pub world: Matrix,
    pub columns: Vec<f64>,
    pub rows: Vec<f64>,
    pub zones_x: Vec<f64>,
    pub zones_y: Vec<f64>,
    pub construction_x: Vec<f64>,
    pub construction_y: Vec<f64>,
    pub anchors: Vec<Anchor>,
}
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Pour {
    pub x: usize,
    pub y: usize,
    pub end_x: usize,
    pub end_y: usize,
}

fn corners(points: &[V3], closed: bool) -> Vec<V3> {
    points
        .iter()
        .enumerate()
        .filter_map(|(i, &p)| {
            if !closed && (i == 0 || i == points.len() - 1) {
                return Some(p);
            }
            let a = points[(i + points.len() - 1) % points.len()];
            let b = points[(i + 1) % points.len()];
            let ux = p[0] - a[0];
            let uy = p[1] - a[1];
            let vx = b[0] - p[0];
            let vy = b[1] - p[1];
            let length = hypot(ux, uy) * hypot(vx, vy);
            (length > 1e-12 && (ux * vx + uy * vy) / length < cos(std::f64::consts::PI / 18.))
                .then_some(p)
        })
        .collect()
}
fn validate(
    loops: &[Vec<V3>],
    origin: V3,
    patch: f64,
    material: f64,
    budget: &mut Budget<'_>,
) -> Result<(), String> {
    budget.spend(loops.iter().map(Vec::len).sum())?;
    if loops.is_empty()
        || loops.len() > 4096
        || loops.iter().map(Vec::len).sum::<usize>() > 20_000
        || loops
            .iter()
            .any(|l| l.len() < 3 || l.iter().any(|p| !valid_point(*p)))
        || !valid_point(origin)
        || !patch.is_finite()
        || !(0.05..=1000.).contains(&patch)
        || !material.is_finite()
        || !(0.05..=1000.).contains(&material)
    {
        return Err("Ground layout requires bounded floor loops and construction spacing.".into());
    }
    Ok(())
}
fn frame(loops: &[Vec<V3>], origin: V3) -> Matrix {
    let mut longest = 0.;
    let mut angle = 0.;
    for points in loops {
        for (i, p) in points.iter().enumerate() {
            let q = points[(i + 1) % points.len()];
            let length = hypot(q[0] - p[0], q[1] - p[1]);
            if length <= longest + 1e-8 {
                continue;
            }
            longest = length;
            angle = atan2(q[1] - p[1], q[0] - p[0]);
        }
    }
    Matrix([
        [cos(angle), sin(angle), 0.],
        [-sin(angle), cos(angle), 0.],
        [0., 0., 1.],
        [origin[0], origin[1], 0.],
    ])
}
fn construction(
    anchors: &[Ranked],
    axis: usize,
    low: f64,
    high: f64,
    spacing: f64,
    budget: &mut Budget<'_>,
) -> Result<Vec<f64>, String> {
    let mut result = vec![low, high];
    let separation = 0.6_f64.min(spacing * 0.4);
    let mut sorted: Vec<_> = anchors.iter().collect();
    sorted.sort_by(|a, b| {
        b.priority
            .cmp(&a.priority)
            .then_with(|| a.anchor.point[axis].total_cmp(&b.anchor.point[axis]))
    });
    for anchor in sorted {
        budget.spend(result.len())?;
        let value = anchor.anchor.point[axis];
        if value <= low
            || value >= high
            || result.iter().any(|line| (line - value).abs() < separation)
        {
            continue;
        }
        result.push(value);
        if result.len() > 256 {
            return Err("Ground construction axis budget exceeded.".into());
        }
    }
    result.sort_by(f64::total_cmp);
    Ok(result)
}
fn finishes(anchors: &[f64], size: f64, minimum: f64) -> Result<Vec<f64>, String> {
    let end = *anchors.last().unwrap();
    let mut result = vec![anchors[0]];
    while end - result.last().unwrap() > size * 1.5 {
        let start = *result.last().unwrap();
        let desired = anchors
            .iter()
            .copied()
            .filter(|v| {
                *v >= start + size * 0.5 && *v <= (start + size * 1.5).min(end - size * 0.5)
            })
            .min_by(|a, b| {
                (a - start - size)
                    .abs()
                    .total_cmp(&(b - start - size).abs())
                    .then_with(|| a.total_cmp(b))
            })
            .unwrap_or(start + size);
        let next = anchors
            .iter()
            .find(|v| (**v - desired).abs() < minimum)
            .copied()
            .unwrap_or(desired);
        if next <= start {
            return Err("Ground finish axes cannot advance.".into());
        }
        result.push(next);
        if result.len() > 256 {
            return Err("Ground finish axis budget exceeded.".into());
        }
    }
    result.push(end);
    Ok(result)
}
fn subdivisions(construction: &[f64], finishes: &[f64], size: f64) -> Result<Vec<f64>, String> {
    let mut boundaries = [construction, finishes].concat();
    boundaries.sort_by(f64::total_cmp);
    boundaries.dedup();
    let mut result = vec![boundaries[0]];
    for pair in boundaries.windows(2) {
        let length = pair[1] - pair[0];
        let count = ((length - 1e-6) / size).ceil().max(1.) as usize;
        if result.len() + count > 256 {
            return Err("Ground pour axis budget exceeded.".into());
        }
        for n in 1..=count {
            result.push(pair[0] + length * n as f64 / count as f64);
        }
    }
    Ok(result)
}
fn grid(
    world: Matrix,
    points: &[V3],
    anchors: Vec<Ranked>,
    patch: f64,
    material: f64,
    budget: &mut Budget<'_>,
) -> Result<Layout, String> {
    let mut bounds = [
        f64::INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
    ];
    for p in points {
        for i in 0..2 {
            bounds[i] = bounds[i].min(p[i]);
            bounds[i + 2] = bounds[i + 2].max(p[i]);
        }
    }
    if bounds[2] <= bounds[0] || bounds[3] <= bounds[1] {
        return Err("Ground layout needs nonzero area.".into());
    }
    let columns = construction(&anchors, 0, bounds[0], bounds[2], patch, budget)?;
    let rows = construction(&anchors, 1, bounds[1], bounds[3], patch, budget)?;
    let minimum = 0.6_f64.min(patch * 0.4);
    let zones_x = finishes(&columns, material, minimum)?;
    let zones_y = finishes(&rows, material, minimum)?;
    Ok(Layout {
        world,
        columns: subdivisions(&columns, &zones_x, patch)?,
        rows: subdivisions(&rows, &zones_y, patch)?,
        construction_x: columns,
        construction_y: rows,
        zones_x,
        zones_y,
        anchors: anchors.into_iter().map(|a| a.anchor).collect(),
    })
}
pub fn footprint(
    loops: &[Vec<V3>],
    patch: f64,
    material: f64,
    budget: &mut Budget<'_>,
) -> Result<Layout, String> {
    validate(loops, [0.; 3], patch, material, budget)?;
    let world = frame(loops, loops[0][0]);
    let inverse = world.inverse()?;
    let points: Vec<_> = loops.iter().flatten().map(|p| inverse.point(*p)).collect();
    let anchors = points
        .iter()
        .map(|p| Ranked {
            anchor: Anchor {
                point: *p,
                kind: Kind::Floor,
            },
            priority: 3,
        })
        .collect();
    grid(world, &points, anchors, patch, material, budget)
}
/// Only visible, collidable neighbors other than the receiving host are supplied by scene resolution.
pub enum Neighbor {
    Wall {
        path: Vec<V3>,
        thickness: f64,
        contacts: Vec<[V3; 2]>,
    },
    Floor {
        loops: Vec<Vec<V3>>,
    },
}
pub struct Authored<'a> {
    pub loops: &'a [Vec<V3>],
    pub origin: V3,
    pub receiving: &'a [Vec<V3>],
    pub neighbors: &'a [Neighbor],
    /// Paths of declared connections touching this host, in resolved world meters.
    pub connections: &'a [Vec<V3>],
}
fn near(
    point: V3,
    segments: &[[V3; 2]],
    distance: f64,
    budget: &mut Budget<'_>,
) -> Result<bool, String> {
    for &[a, b] in segments {
        budget.spend(1)?;
        if edge_distance(point, a, b) <= distance {
            return Ok(true);
        }
    }
    Ok(false)
}
pub fn authored(
    request: Authored<'_>,
    patch: f64,
    material: f64,
    budget: &mut Budget<'_>,
) -> Result<Layout, String> {
    let Authored {
        loops,
        origin,
        receiving,
        neighbors,
        connections,
    } = request;
    validate(loops, origin, patch, material, budget)?;
    validate(receiving, origin, patch, material, budget)?;
    if neighbors.len() > 20_000 || connections.len() > 20_000 {
        return Err("Ground contact budget exceeded.".into());
    }
    let world = frame(loops, origin);
    let inverse = world.inverse()?;
    let local_loops: Vec<Vec<_>> = loops
        .iter()
        .map(|l| l.iter().map(|p| inverse.point(*p)).collect())
        .collect();
    let edges: Vec<_> = loops
        .iter()
        .flat_map(|l| {
            l.iter()
                .enumerate()
                .map(|(i, &p)| [p, l[(i + 1) % l.len()]])
        })
        .collect();
    let points: Vec<_> = local_loops
        .iter()
        .flatten()
        .copied()
        .chain(receiving.iter().flatten().map(|p| inverse.point(*p)))
        .collect();
    let elevation = receiving[0][0][2];
    let mut anchors: Vec<_> = local_loops
        .iter()
        .flat_map(|l| corners(l, true))
        .map(|point| Ranked {
            anchor: Anchor {
                point,
                kind: Kind::Floor,
            },
            priority: 3,
        })
        .collect();
    let mut add = |point, priority, kind| -> Result<(), String> {
        if anchors.len() >= 20_000 {
            return Err("Ground anchor budget exceeded.".into());
        }
        anchors.push(Ranked {
            anchor: Anchor {
                point: inverse.point(point),
                kind,
            },
            priority,
        });
        Ok(())
    };
    for other in neighbors {
        budget.spend(1)?;
        match other {
            Neighbor::Wall {
                path,
                thickness,
                contacts,
            } => {
                budget.spend(path.len() + contacts.len())?;
                if path.len() > 20_000
                    || contacts.len() > 20_000
                    || !thickness.is_finite()
                    || !(0. ..=1000.).contains(thickness)
                    || path
                        .iter()
                        .chain(contacts.iter().flatten())
                        .any(|p| !valid_point(*p))
                {
                    return Err("Invalid ground wall contacts.".into());
                }
                for p in corners(path, false) {
                    if p[2] <= elevation + 0.05 && near(p, contacts, thickness + 0.05, budget)? {
                        add(p, 2, Kind::Wall)?;
                    }
                }
            }
            Neighbor::Floor { loops } => {
                validate(loops, origin, patch, material, budget)?;
                for l in loops {
                    for p in corners(l, true) {
                        if (p[2] - elevation).abs() < 0.05 && near(p, &edges, 0.05, budget)? {
                            add(p, 4, Kind::FloorConnection)?;
                        }
                    }
                }
            }
        }
    }
    for path in connections {
        budget.spend(path.len())?;
        if path.len() > 20_000 || path.iter().any(|p| !valid_point(*p)) {
            return Err("Invalid ground connection path.".into());
        }
        for p in corners(path, false) {
            if (p[2] - elevation).abs() < 0.05 && near(p, &edges, 0.3, budget)? {
                add(p, 5, Kind::Connection)?;
            }
        }
    }
    grid(world, &points, anchors, patch, material, budget)
}
pub fn pours(
    layout: &Layout,
    seed: u32,
    amount: f64,
    budget: &mut Budget<'_>,
) -> Result<Vec<Pour>, String> {
    budget.spend(0)?;
    let axes = [
        &layout.columns,
        &layout.rows,
        &layout.construction_x,
        &layout.construction_y,
        &layout.zones_x,
        &layout.zones_y,
    ];
    if !amount.is_finite()
        || !(0. ..=1.).contains(&amount)
        || axes.iter().any(|a| {
            a.len() < 2
                || a.len() > 256
                || a.iter().any(|v| !v.is_finite())
                || a.windows(2).any(|p| p[0] >= p[1])
        })
    {
        return Err("Ground pours require ordered bounded axes and join probability.".into());
    }
    let width = layout.columns.len() - 1;
    let height = layout.rows.len() - 1;
    if width * height > 10_000 {
        return Err("Ground patch budget exceeded.".into());
    }
    let mut occupied = vec![false; width * height];
    let mut result = Vec::new();
    let protected_x = [layout.construction_x.as_slice(), layout.zones_x.as_slice()].concat();
    let protected_y = [layout.construction_y.as_slice(), layout.zones_y.as_slice()].concat();
    let protected = |value: f64, lines: &[f64]| lines.iter().any(|v| (v - value).abs() < 1e-6);
    for x in 0..width {
        for y in 0..height {
            budget.spend(1)?;
            if occupied[x * height + y] {
                continue;
            }
            let mut rng = SeededRng::from_text(&format!("{seed}/{x}/{y}/join"));
            let target_x = if rng.next_f64() < amount {
                rng.integer(2, 3)
            } else {
                1
            };
            let target_y = if rng.next_f64() < amount * 0.65 { 2 } else { 1 };
            let mut end_x = x + 1;
            let mut end_y = y + 1;
            while end_x < width
                && end_x - x < target_x
                && !protected(layout.columns[end_x], &protected_x)
                && !occupied[end_x * height + y]
            {
                end_x += 1;
            }
            while end_y < height
                && end_y - y < target_y
                && !protected(layout.rows[end_y], &protected_y)
            {
                if (x..end_x).any(|i| occupied[i * height + end_y]) {
                    break;
                }
                end_y += 1;
            }
            for i in x..end_x {
                for j in y..end_y {
                    occupied[i * height + j] = true;
                }
            }
            result.push(Pour { x, y, end_x, end_y });
        }
    }
    Ok(result)
}

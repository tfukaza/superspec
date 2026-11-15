//! Bounded Recast tiles use overlapping input and clipped output cores.
use super::*;
use crate::{clipping::half_plane, math::area};
use std::collections::BTreeMap;

pub(super) fn build(
    vertices: &[V3],
    triangles: &[[usize; 3]],
    mask: Option<&[bool]>,
    agent: Agent,
    cancelled: &dyn Fn() -> bool,
) -> Result<Mesh, String> {
    let cell = agent.radius / 2.;
    let side = cell * 256.;
    let margin = cell * 6.;
    let min: [f64; 3] = std::array::from_fn(|axis| {
        vertices
            .iter()
            .map(|p| p[axis])
            .fold(f64::INFINITY, f64::min)
    });
    let max: [f64; 3] = std::array::from_fn(|axis| {
        vertices
            .iter()
            .map(|p| p[axis])
            .fold(f64::NEG_INFINITY, f64::max)
    });
    let origin = [
        (min[0] / cell).floor() * cell,
        (min[1] / cell).floor() * cell,
    ];
    let width = ((max[0] - origin[0]) / side).ceil().max(1.) as usize;
    let height = ((max[1] - origin[1]) / side).ceil().max(1.) as usize;
    if width.saturating_mul(height) > 4096 {
        return Err("Navigation requires more than 4096 tiles.".into());
    }
    let mut bins: BTreeMap<(usize, usize), Vec<usize>> = BTreeMap::new();
    let mut work = 0;
    for (i, t) in triangles.iter().enumerate() {
        checkpoint(cancelled)?;
        let points = t.map(|j| vertices[j]);
        let start: [usize; 2] = std::array::from_fn(|a| {
            (((points.iter().map(|p| p[a]).fold(f64::INFINITY, f64::min) - origin[a] - margin)
                / side)
                .floor()
                .max(0.) as usize)
                .min(if a == 0 { width - 1 } else { height - 1 })
        });
        let end: [usize; 2] = std::array::from_fn(|a| {
            (((points
                .iter()
                .map(|p| p[a])
                .fold(f64::NEG_INFINITY, f64::max)
                - origin[a]
                + margin)
                / side)
                .floor()
                .max(0.) as usize)
                .min(if a == 0 { width - 1 } else { height - 1 })
        });
        for x in start[0]..=end[0] {
            for y in start[1]..=end[1] {
                work += 1;
                if work > 8_000_000 {
                    return Err("Navigation tile indexing exceeds its work allowance.".into());
                }
                bins.entry((x, y)).or_default().push(i);
            }
        }
    }
    let mut output = Mesh {
        cell_size: cell,
        cell_height: agent.radius / 4.,
        walkable_climb: (agent.step / (agent.radius / 4.)).floor() * (agent.radius / 4.),
        ..Default::default()
    };
    let mut vertex_ids = BTreeMap::new();
    let mut owners = vec![];
    for ((x, y), candidates) in bins {
        checkpoint(cancelled)?;
        let lo = [origin[0] + x as f64 * side, origin[1] + y as f64 * side];
        let hi = [lo[0] + side, lo[1] + side];
        let mut points = vec![
            [lo[0] - margin, lo[1] - margin, min[2]],
            [hi[0] + margin, hi[1] + margin, max[2]],
        ];
        let mut faces = vec![];
        let mut allowed = vec![];
        for i in candidates {
            let mut polygon = triangles[i]
                .iter()
                .map(|j| vertices[*j])
                .collect::<Vec<_>>();
            for a in 0..2 {
                polygon = half_plane(&polygon, |p| p[a] - lo[a] + margin);
                polygon = half_plane(&polygon, |p| hi[a] + margin - p[a]);
            }
            if polygon.len() < 3 {
                continue;
            }
            let offset = points.len();
            points.extend_from_slice(&polygon);
            for j in 1..polygon.len() - 1 {
                faces.push([offset, offset + j, offset + j + 1]);
                allowed.push(mask.is_none_or(|m| m[i]));
            }
        }
        if faces.is_empty() {
            continue;
        }
        let tile = build_single(&points, &faces, Some(&allowed), agent, cancelled)?;
        let owner = output.tile_count;
        output.tile_count += 1;
        let mut remap = vec![None; tile.polygons.len()];
        for (i, polygon) in tile.polygons.iter().enumerate() {
            let mut polygon = polygon
                .iter()
                .map(|j| tile.vertices[*j])
                .collect::<Vec<_>>();
            for a in 0..2 {
                polygon = half_plane(&polygon, |p| p[a] - lo[a]);
                polygon = half_plane(&polygon, |p| hi[a] - p[a]);
            }
            if polygon.len() < 3 || area(&polygon).abs() < 1e-8 {
                continue;
            }
            let indices = polygon
                .into_iter()
                .map(|p| {
                    let key = p.map(|v| (v * 1e6).round() as i64);
                    *vertex_ids.entry(key).or_insert_with(|| {
                        output.vertices.push(p);
                        output.vertices.len() - 1
                    })
                })
                .collect();
            remap[i] = Some(output.polygons.len());
            output.polygons.push(indices);
            output.neighbors.push(vec![]);
            owners.push(owner);
        }
        for (i, neighbors) in tile.neighbors.iter().enumerate() {
            let Some(a) = remap[i] else { continue };
            for n in neighbors {
                if let Some(b) = remap[*n] {
                    if output.portal(a, b).is_some() {
                        output.neighbors[a].push(b);
                    }
                }
            }
        }
        if output.vertices.len() > 2_000_000 || output.polygons.len() > 500_000 {
            return Err("Tiled navigation output exceeds its size allowance.".into());
        }
    }
    let mut seams: BTreeMap<(usize, i64), Vec<usize>> = BTreeMap::new();
    for (i, polygon) in output.polygons.iter().enumerate() {
        for j in 0..polygon.len() {
            let p = output.vertices[polygon[j]];
            let q = output.vertices[polygon[(j + 1) % polygon.len()]];
            for a in 0..2 {
                let line = ((p[a] - origin[a]) / side).round() as i64;
                if line > 0
                    && line < (if a == 0 { width } else { height }) as i64
                    && (p[a] - q[a]).abs() < 1e-4
                    && (p[a] - origin[a] - line as f64 * side).abs() < 1e-4
                {
                    seams.entry((a, line)).or_default().push(i);
                }
            }
        }
    }
    for ((axis, _), mut candidates) in seams {
        candidates.sort_unstable();
        candidates.dedup();
        let along = 1 - axis;
        let range = |i: usize| {
            output.polygons[i]
                .iter()
                .fold([f64::INFINITY, f64::NEG_INFINITY], |b, j| {
                    [
                        b[0].min(output.vertices[*j][along]),
                        b[1].max(output.vertices[*j][along]),
                    ]
                })
        };
        candidates.sort_by(|a, b| range(*a)[0].total_cmp(&range(*b)[0]));
        let ranges: Vec<_> = candidates.iter().map(|i| range(*i)).collect();
        for i in 0..candidates.len() {
            for j in i + 1..candidates.len() {
                if ranges[j][0] >= ranges[i][1] - 1e-6 {
                    break;
                }
                work += 1;
                if work > 8_000_000 {
                    return Err("Navigation seam matching exceeds its work allowance.".into());
                }
                let (a, b) = (candidates[i], candidates[j]);
                if owners[a] != owners[b] && output.portal(a, b).is_some() {
                    output.neighbors[a].push(b);
                    output.neighbors[b].push(a);
                }
            }
        }
    }
    for neighbors in &mut output.neighbors {
        neighbors.sort_unstable();
        neighbors.dedup();
    }
    label_components(&mut output);
    Ok(output)
}

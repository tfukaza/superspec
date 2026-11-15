//! Theme-independent polygon emission and local planar mapping.
#![allow(clippy::too_many_arguments)]
use super::mesh::{Mesh, Vertex};
use superspec_geometry::{
    Matrix, V3,
    math::{add, cross, dot, length, scale, sub},
};

/// Emits a convex polygon fan. Color is evaluated per emitted corner, in local/world coordinates.
/// Survey mode omits color and UV evaluation; the outward hint controls winding only.
/// The caller supplies a finite orthonormal frame; reflections are supported, affine scaling is not.
pub fn face(
    mesh: &mut Mesh<'_>,
    matrix: Matrix,
    part: &str,
    slot: &str,
    points: &[V3],
    hint: V3,
    repeat: f64,
    uv: Option<&[[f64; 2]]>,
    normals: Option<&[V3]>,
    color: &dyn Fn(V3, V3, V3) -> Result<V3, String>,
) -> Result<(), String> {
    check_frame(matrix, part)?;
    face_admitted(
        mesh, matrix, part, slot, points, hint, repeat, uv, normals, color,
    )
}

pub(crate) fn face_admitted(
    mesh: &mut Mesh<'_>,
    matrix: Matrix,
    part: &str,
    slot: &str,
    points: &[V3],
    hint: V3,
    repeat: f64,
    uv: Option<&[[f64; 2]]>,
    normals: Option<&[V3]>,
    color: &dyn Fn(V3, V3, V3) -> Result<V3, String>,
) -> Result<(), String> {
    if points.len() < 3
        || points.len() > 4096
        || points.iter().flatten().chain(&hint).any(|v| !v.is_finite())
        || !repeat.is_finite()
        || uv.is_some_and(|v| v.len() != points.len())
        || normals.is_some_and(|v| v.len() != points.len())
    {
        return Err("Invalid procedural face shape or attributes.".into());
    }
    mesh.spend_work(points.len() - 2)?;
    let positions: Vec<_> = points.iter().map(|p| matrix.point(*p)).collect();
    if positions.iter().flatten().any(|v| !v.is_finite()) {
        return Err("Procedural placement overflow.".into());
    }
    let outward = matrix.direction(hint);
    let projected;
    let coordinates = if mesh.surveying() {
        None
    } else if let Some(uv) = uv {
        Some(uv)
    } else {
        projected = planar(points, repeat)?;
        Some(projected.as_slice())
    };
    let mut shared = if points.len() > 3 && !mesh.surveying() {
        Some(vec![None; points.len()])
    } else {
        None
    };
    for i in 1..points.len() - 1 {
        if i.is_multiple_of(128) {
            mesh.check_work(0)?;
        }
        let mut normal = cross(
            sub(positions[i], positions[0]),
            sub(positions[i + 1], positions[0]),
        );
        let size = length(normal);
        if !size.is_finite() {
            return Err("Procedural face normal overflow.".into());
        }
        if size < 1e-9 {
            continue;
        }
        let order = if dot(normal, outward) < 0. {
            normal = scale(normal, -1.);
            [0, i + 1, i]
        } else {
            [0, i, i + 1]
        };
        normal = scale(normal, 1. / size);
        if mesh.surveying() {
            mesh.survey_triangle(order.map(|j| positions[j]))?;
            continue;
        }
        let coordinates = coordinates.unwrap();
        let mut vertices = [Vertex {
            position: [0.; 3],
            normal,
            uv: [0.; 2],
            color: [0.; 3],
        }; 3];
        for (corner, j) in order.into_iter().enumerate() {
            let normal = normals.map_or(normal, |n| matrix.direction(n[j]));
            vertices[corner] = Vertex {
                position: positions[j],
                normal,
                uv: coordinates[j],
                color: color(points[j], positions[j], normal)?,
            };
        }
        mesh.triangle(part, slot, vertices, order, shared.as_deref_mut())?;
    }
    Ok(())
}

/// Normalize all three components together, preserving the source chart's axis ties.
fn planar_unit(vector: V3) -> Result<V3, String> {
    let largest = vector.into_iter().map(f64::abs).fold(0., f64::max);
    if !largest.is_finite() || largest == 0. {
        return Err("A planar chart direction has zero or nonfinite length.".into());
    }
    let mut sum = 0.;
    let mut compensation = 0.;
    for component in vector {
        let normalized = component / largest;
        let square = normalized * normalized - compensation;
        let next = sum + square;
        compensation = (next - sum) - square;
        sum = next;
    }
    Ok(scale(vector, 1. / (largest * sum.sqrt())))
}
fn planar(points: &[V3], repeat: f64) -> Result<Vec<[f64; 2]>, String> {
    let mut normal = [0., 0., 1.];
    for i in 1..points.len() - 1 {
        let n = cross(sub(points[i], points[0]), sub(points[i + 1], points[0]));
        if length(n) > 1e-9 {
            normal = planar_unit(n)?;
            break;
        }
    }
    let mut dominant = 0;
    for axis in 1..3 {
        if normal[axis].abs() > normal[dominant].abs() {
            dominant = axis;
        }
    }
    if normal[dominant] < 0. {
        normal = scale(normal, -1.);
    }
    let horizontal = if dominant == 0 {
        [0., 1., 0.]
    } else {
        [1., 0., 0.]
    };
    let u = planar_unit(sub(horizontal, scale(normal, dot(horizontal, normal))))?;
    let mut v = cross(normal, u);
    let vertical = if dominant == 2 {
        [0., 1., 0.]
    } else {
        [0., 0., 1.]
    };
    if dot(v, vertical) < 0. {
        v = scale(v, -1.);
    }
    Ok(points
        .iter()
        .map(|p| [dot(*p, u) * repeat, dot(*p, v) * repeat])
        .collect())
}

#[derive(Clone, Copy)]
pub struct PatchVertex {
    pub at: V3,
    pub uv: [f64; 2],
    pub tint: V3,
}

/// Smooth grid; columns move right, rows down from its front side. No cross-cell welding.
/// The caller supplies a finite orthonormal frame; reflections are supported, affine scaling is not.
pub fn patch(
    mesh: &mut Mesh<'_>,
    matrix: Matrix,
    part: &str,
    slot: &str,
    grid: &[Vec<PatchVertex>],
    flipped: bool,
) -> Result<(), String> {
    check_frame(matrix, part)?;
    patch_admitted(mesh, matrix, part, slot, grid, flipped)
}

pub(crate) fn patch_admitted(
    mesh: &mut Mesh<'_>,
    matrix: Matrix,
    part: &str,
    slot: &str,
    grid: &[Vec<PatchVertex>],
    flipped: bool,
) -> Result<(), String> {
    let rows = grid.len();
    let columns = grid.first().map_or(0, Vec::len);
    if rows < 2
        || columns < 2
        || rows.checked_mul(columns).is_none_or(|n| n > 1_000_000)
        || grid.iter().any(|r| r.len() != columns)
        || grid
            .iter()
            .flatten()
            .flat_map(|v| v.at)
            .any(|v| !v.is_finite())
    {
        return Err("Invalid procedural patch grid.".into());
    }
    mesh.spend_work((rows - 1) * (columns - 1) * 2)?;
    let sign = if flipped { -1. } else { 1. };
    let normal_at = |r: usize, c: usize| {
        let across = sub(
            grid[r][(c + 1).min(columns - 1)].at,
            grid[r][c.saturating_sub(1)].at,
        );
        let down = sub(
            grid[(r + 1).min(rows - 1)][c].at,
            grid[r.saturating_sub(1)][c].at,
        );
        let normal = cross(across, down);
        let size = length(normal);
        matrix.direction(if size < 1e-12 {
            [0., sign, 0.]
        } else {
            scale(normal, sign / size)
        })
    };
    for r in 0..rows - 1 {
        mesh.check_work(0)?;
        for c in 0..columns - 1 {
            for corners in [
                [(r, c), (r, c + 1), (r + 1, c + 1)],
                [(r, c), (r + 1, c + 1), (r + 1, c)],
            ] {
                let positions = corners.map(|(r, c)| matrix.point(grid[r][c].at));
                let facing = cross(
                    sub(positions[1], positions[0]),
                    sub(positions[2], positions[0]),
                );
                if length(facing) < 1e-12 {
                    continue;
                }
                let normals = corners.map(|(r, c)| normal_at(r, c));
                let order = if dot(facing, add(add(normals[0], normals[1]), normals[2])) < 0. {
                    [0, 2, 1]
                } else {
                    [0, 1, 2]
                };
                if mesh.surveying() {
                    mesh.survey_triangle(order.map(|i| positions[i]))?;
                    continue;
                }
                let vertices = order.map(|i| {
                    let (r, c) = corners[i];
                    Vertex {
                        position: positions[i],
                        normal: normals[i],
                        uv: grid[r][c].uv,
                        color: grid[r][c].tint,
                    }
                });
                mesh.triangle(part, slot, vertices, [0, 1, 2], None)?;
            }
        }
    }
    Ok(())
}

/// The constructor contract shared by ItemSpec consumers and the theme pen.
pub(crate) fn check_frame(matrix: Matrix, part: &str) -> Result<(), String> {
    matrix.checked()?;
    if (0..3).any(|i| {
        (length(matrix.0[i]) - 1.).abs() > 1e-7
            || (0..i).any(|j| dot(matrix.0[i], matrix.0[j]).abs() > 1e-7)
    }) {
        return Err("Procedural pens require an orthonormal frame.".into());
    }
    if part.len() > 4096 {
        return Err("Procedural part identity exceeds 4096 bytes.".into());
    }
    Ok(())
}

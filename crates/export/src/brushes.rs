//! Exact solid partitioning for Quake brushes, with source-face material boundaries.
use crate::Options;
use superspec_geometry::{
    Solid,
    clipping::{half_plane, polygon_area},
    math::*,
};
use superspec_model::{ExportFace, ExportSolid};

#[derive(Clone)]
pub(crate) struct Plane {
    pub points: [V3; 3],
    pub normal: V3,
    pub distance: f64,
    pub face: Option<usize>,
}
pub(crate) struct Brush {
    pub planes: Vec<Plane>,
}
struct Work<'a, 'b> {
    options: &'a Options<'b>,
    count: usize,
}
impl Work<'_, '_> {
    fn spend(&mut self, amount: usize) -> Result<(), String> {
        self.count = self.count.saturating_add(amount);
        self.options.check(0)?;
        if self.count > 8_000_000 {
            return Err("Brush decomposition work allowance exceeded.".into());
        }
        Ok(())
    }
}
fn planes(solid: &Solid, work: &mut Work<'_, '_>) -> Result<(Vec<Plane>, Vec<V3>), String> {
    let mesh = solid.mesh();
    let vertices: Vec<V3> = mesh
        .vert_properties
        .chunks_exact(mesh.num_prop as usize)
        .map(|p| [p[0] as f64, p[1] as f64, p[2] as f64])
        .collect();
    let mut result: Vec<Plane> = vec![];
    for t in mesh.tri_verts.chunks_exact(3) {
        work.spend(result.len() + 1)?;
        let points = [
            vertices[t[0] as usize],
            vertices[t[1] as usize],
            vertices[t[2] as usize],
        ];
        let Ok(normal) = unit(cross(sub(points[1], points[0]), sub(points[2], points[0]))) else {
            continue;
        };
        let distance = dot(normal, points[0]);
        if !result
            .iter()
            .any(|p| dot(p.normal, normal) > 1. - 1e-10 && (p.distance - distance).abs() < 1e-4)
        {
            result.push(Plane {
                points,
                normal,
                distance,
                face: None,
            });
        }
    }
    Ok((result, vertices))
}
fn material_partition(
    planes: &mut [Plane],
    vertices: &[V3],
    faces: &[ExportFace],
    work: &mut Work<'_, '_>,
) -> Result<Option<(V3, f64)>, String> {
    for i in 0..planes.len() {
        let mut exposed = vec![];
        for (j, face) in faces.iter().enumerate() {
            work.spend(1)?;
            let normal = unit(cross(
                sub(face.points[1], face.points[0]),
                sub(face.points[2], face.points[0]),
            ))?;
            if dot(normal, planes[i].normal) < 1. - 1e-7
                || face
                    .points
                    .iter()
                    .any(|p| (dot(planes[i].normal, *p) - planes[i].distance).abs() > 1e-4)
            {
                continue;
            }
            let mut polygon = face.points.to_vec();
            for boundary in planes.iter() {
                work.spend(polygon.len())?;
                polygon = half_plane(&polygon, |p| {
                    boundary.distance - dot(boundary.normal, p) + 1e-9
                });
                if polygon.len() < 3 {
                    break;
                }
            }
            let perimeter = polygon
                .iter()
                .zip(polygon.iter().cycle().skip(1))
                .map(|(a, b)| length(sub(*a, *b)))
                .sum::<f64>();
            if polygon_area(&polygon) * 2. > (perimeter * 1e-6).max(1e-10) {
                exposed.push((j, polygon));
            }
        }
        if let Some((first, _)) = exposed.first() {
            planes[i].face = Some(*first);
            let first = &faces[*first];
            let conflict = exposed.iter().any(|(j, _)| {
                let f = &faces[*j];
                (f.material.as_str(), f.visible, f.collidable)
                    != (first.material.as_str(), first.visible, first.collidable)
            });
            if conflict {
                for (_, polygon) in &exposed {
                    for (a, b) in polygon.iter().zip(polygon.iter().cycle().skip(1)) {
                        let Ok(normal) = unit(cross(sub(*b, *a), planes[i].normal)) else {
                            continue;
                        };
                        let distance = dot(normal, *a);
                        work.spend(vertices.len())?;
                        if vertices.iter().any(|p| dot(normal, *p) - distance < -1e-7)
                            && vertices.iter().any(|p| dot(normal, *p) - distance > 1e-7)
                        {
                            return Ok(Some((normal, distance)));
                        }
                    }
                }
                return Err(
                    "Conflicting brush face materials cannot be separated at export precision."
                        .into(),
                );
            }
        }
    }
    Ok(None)
}

pub(crate) fn decompose(input: &ExportSolid, options: &Options<'_>) -> Result<Vec<Brush>, String> {
    let mut work = Work { options, count: 0 };
    let source = Solid::from_geometry(&input.geometry, Solid::reserve_owner())?;
    let mut queue = source.components()?;
    let mut brushes = vec![];
    let mut total_volume = 0.;
    let mut splits = 0;
    while let Some(part) = queue.pop() {
        work.spend(1)?;
        if splits > 8192 || queue.len() + brushes.len() >= 4096 {
            return Err("Solid exceeds 4096 convex brush pieces.".into());
        }
        let volume = part.volume();
        if volume < -1e-12 {
            return Err("Inverted export solid.".into());
        }
        if volume <= 1e-12 {
            continue;
        }
        let (mut faces, vertices) = planes(&part, &mut work)?;
        let mut candidates = vec![];
        for p in &faces {
            work.spend(vertices.len())?;
            let violation = vertices
                .iter()
                .map(|v| dot(p.normal, *v) - p.distance)
                .fold(f64::NEG_INFINITY, f64::max);
            if violation > 1e-4 {
                candidates.push((violation, p.normal, p.distance));
            }
        }
        candidates.sort_by(|a, b| b.0.total_cmp(&a.0));
        if candidates.is_empty() {
            if let Some((normal, distance)) =
                material_partition(&mut faces, &vertices, &input.faces, &mut work)?
            {
                candidates.push((0., normal, distance));
            } else {
                total_volume += volume;
                brushes.push(Brush { planes: faces });
                continue;
            }
        }
        let mut divided = false;
        for (_, normal, distance) in candidates {
            work.spend(1)?;
            let halves = part.split(normal, distance)?;
            let volumes = halves.each_ref().map(Solid::volume);
            let tolerance = (volume.abs() * 1e-6).max(1e-7);
            if volumes.iter().all(|v| *v > 1e-12 && *v < volume - 1e-12)
                && (volumes[0] + volumes[1] - volume).abs() < tolerance
            {
                for half in halves {
                    queue.extend(half.components()?);
                }
                divided = true;
                splits += 1;
                break;
            }
        }
        if !divided {
            return Err("Convex brush partition made no occupied-volume progress.".into());
        }
    }
    if (total_volume - source.volume()).abs() > (source.volume().abs() * 1e-6).max(1e-7) {
        return Err("Brush partition changed occupied volume.".into());
    }
    Ok(brushes)
}

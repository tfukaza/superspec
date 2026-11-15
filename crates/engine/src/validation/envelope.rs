//! Source-linked architectural enclosure observations. Findings never change geometry.
use super::*;
use superspec_geometry::obstructions::Budget;

struct Surface {
    id: String,
    source: SourceLocation,
    kind: String,
    points: Vec<[V3; 3]>,
    bounds: [V3; 2],
}
struct Roof {
    surface: Surface,
    tops: Vec<[V3; 3]>,
    edges: Vec<[V3; 2]>,
}
struct WallFace {
    wall: usize,
    normal: V3,
    origin: V3,
    points: Vec<V3>,
}

fn wall_alignment(walls: &[Surface], budget: &mut Budget<'_>) -> Result<Vec<Diagnostic>, String> {
    let mut groups = BTreeMap::<(usize, [i64; 3], i64), WallFace>::new();
    for (wall, source) in walls.iter().enumerate() {
        for triangle in &source.points {
            budget.spend(1)?;
            let Ok(normal) = unit(cross(
                sub(triangle[1], triangle[0]),
                sub(triangle[2], triangle[0]),
            )) else {
                continue;
            };
            if normal[2].abs() > 1e-4 {
                continue;
            }
            let key = (
                wall,
                normal.map(|n| (n * 1e5).round() as i64),
                (dot(normal, triangle[0]) * 1e4).round() as i64,
            );
            groups
                .entry(key)
                .or_insert_with(|| WallFace {
                    wall,
                    normal,
                    origin: triangle[0],
                    points: vec![],
                })
                .points
                .extend(triangle);
        }
    }
    let faces: Vec<_> = groups.into_values().collect();
    let range = |face: &WallFace, axis: V3| {
        face.points
            .iter()
            .fold([f64::INFINITY, f64::NEG_INFINITY], |mut r, p| {
                let value = dot(*p, axis);
                r[0] = r[0].min(value);
                r[1] = r[1].max(value);
                r
            })
    };
    let mut seen = BTreeSet::new();
    let mut findings = vec![];
    for (i, a) in faces.iter().enumerate() {
        for b in &faces[i + 1..] {
            budget.spend(1)?;
            if a.wall == b.wall || dot(a.normal, b.normal) < 1f64.to_radians().cos() {
                continue;
            }
            let tangent = [-a.normal[1], a.normal[0], 0.];
            let first = range(a, tangent);
            let second = range(b, tangent);
            // Wall-end caps are short across the wall thickness.
            if (first[1] - first[0]).min(second[1] - second[0]) < 0.5 {
                continue;
            }
            let gap = first[0].max(second[0]) - first[1].min(second[1]);
            if !(-1e-5..=0.25001).contains(&gap) {
                continue;
            }
            let z1 = range(a, [0., 0., 1.]);
            let z2 = range(b, [0., 0., 1.]);
            if z1[1].min(z2[1]) - z1[0].max(z2[0]) < 0.24999 {
                continue;
            }
            let station = (first[0].max(second[0]) + first[1].min(second[1])) * 0.5;
            let along = station - dot(a.origin, tangent);
            let p = [
                a.origin[0] + tangent[0] * along,
                a.origin[1] + tangent[1] * along,
                z1[0].max(z2[0]) + 0.25,
            ];
            let displacement = dot(sub(b.origin, p), b.normal) / dot(a.normal, b.normal);
            let offset = displacement.abs();
            if !(0.00201..=0.10001).contains(&offset) {
                continue;
            }
            let pair = (a.wall.min(b.wall), a.wall.max(b.wall));
            if !seen.insert(pair) {
                continue;
            }
            let first = &walls[pair.0];
            let second = &walls[pair.1];
            findings.push(finding(first.source.clone(),"WALL_ALIGNMENT",format!("Continuing faces of {} and {} step sideways by {:.1} cm. Review whether their alignment is intentional.",first.id,second.id,offset*100.),vec![first.id.clone(),second.id.clone()],&[p,add(p,scale(a.normal,displacement))],"warning"));
        }
    }
    Ok(findings)
}
fn key(p: V3) -> [i64; 3] {
    p.map(|v| (v * 1e6).round() as i64)
}
fn adjoining(a: [V3; 2], b: [V3; 2]) -> Option<[V3; 2]> {
    let d = sub(a[1], a[0]);
    let size = d[0].hypot(d[1]);
    let other = sub(b[1], b[0]);
    if size < 1.
        || d[0] * other[0] + d[1] * other[1] >= 0.
        || b.iter().any(|p| orient(a[0], a[1], *p).abs() / size > 0.02)
    {
        return None;
    }
    let station = |p: V3| ((p[0] - a[0][0]) * d[0] + (p[1] - a[0][1]) * d[1]) / size;
    let (first, last) = (station(b[0]), station(b[1]));
    let low = 0f64.max(first.min(last));
    let high = size.min(first.max(last));
    (high - low >= 1.).then(|| [lerp(a[0], a[1], low / size), lerp(a[0], a[1], high / size)])
}
fn collect(scene: &Scene, budget: &mut Budget<'_>) -> Result<Vec<Surface>, String> {
    let context = scene
        .manifest
        .architecture
        .as_ref()
        .ok_or("Missing architecture.")?;
    let mut surfaces = vec![];
    for object in context
        .objects
        .iter()
        .filter(|o| ["roof", "wall"].contains(&o.kind.as_str()))
    {
        if matches!(
            object.attributes.get("visible"),
            Some(Value::Boolean(false))
        ) || matches!(object.attributes.get("dynamic"), Some(Value::Boolean(true)))
        {
            continue;
        }
        let physical = scene
            .manifest
            .meshes
            .iter()
            .any(|m| m.object_id == object.id && scene.manifest.colliders.contains(&m.id));
        let mut triangles = vec![];
        let mut bounds = [[f64::INFINITY; 3], [f64::NEG_INFINITY; 3]];
        for mesh in scene.manifest.meshes.iter().filter(|m| {
            m.object_id == object.id
                && if physical {
                    scene.manifest.colliders.contains(&m.id)
                } else {
                    m.visible
                }
        }) {
            for t in
                scene.indices[mesh.index_start..mesh.index_start + mesh.index_count].chunks_exact(3)
            {
                budget.spend(1)?;
                let p = [
                    point(scene, mesh.vertex_start + t[0] as usize),
                    point(scene, mesh.vertex_start + t[1] as usize),
                    point(scene, mesh.vertex_start + t[2] as usize),
                ];
                for p in &p {
                    for i in 0..3 {
                        bounds[0][i] = bounds[0][i].min(p[i]);
                        bounds[1][i] = bounds[1][i].max(p[i]);
                    }
                }
                triangles.push(p);
            }
        }
        if !triangles.is_empty() {
            surfaces.push(Surface {
                id: object.id.clone(),
                source: object.source.clone(),
                kind: object.kind.clone(),
                points: triangles,
                bounds,
            });
        }
    }
    Ok(surfaces)
}
fn review(scene: &Scene, cancelled: &dyn Fn() -> bool) -> Result<Vec<Diagnostic>, String> {
    let mut budget = Budget::new(8_000_000, cancelled)?;
    let mut roofs = vec![];
    let mut walls = vec![];
    for surface in collect(scene, &mut budget)? {
        if surface.kind == "wall" {
            walls.push(surface);
            continue;
        }
        let tops: Vec<_> = surface
            .points
            .iter()
            .copied()
            .filter(|p| {
                let n = cross(sub(p[1], p[0]), sub(p[2], p[0]));
                n[2] > 0.
                    && n[0].hypot(n[1]) <= n[2] * 1e-5
                    && p.iter()
                        .all(|p| (p[2] - surface.bounds[1][2]).abs() <= 1e-5)
            })
            .collect();
        if tops.is_empty() {
            continue;
        }
        let mut edges = BTreeMap::<([i64; 3], [i64; 3]), (usize, [V3; 2])>::new();
        for triangle in &tops {
            for i in 0..3 {
                let a = triangle[i];
                let b = triangle[(i + 1) % 3];
                let (ka, kb) = (key(a), key(b));
                let entry = edges.entry((ka.min(kb), ka.max(kb))).or_insert((0, [a, b]));
                entry.0 += 1;
            }
        }
        roofs.push(Roof {
            surface,
            tops,
            edges: edges
                .into_values()
                .filter(|v| v.0 == 1)
                .map(|v| v.1)
                .collect(),
        });
    }
    let mut findings = vec![];
    for (i, first) in roofs.iter().enumerate() {
        for second in &roofs[i + 1..] {
            let (low, high) = if first.surface.bounds[1][2] < second.surface.bounds[1][2] {
                (first, second)
            } else {
                (second, first)
            };
            let gap = high.surface.bounds[0][2] - low.surface.bounds[1][2];
            if gap < 0.5 {
                continue;
            }
            let mut exposed = vec![];
            let mut span = 0.;
            for a in &low.edges {
                for b in &high.edges {
                    budget.spend(1)?;
                    let Some(shared) = adjoining(*a, *b) else {
                        continue;
                    };
                    let mut uncovered = false;
                    for station in [0.2, 0.5, 0.8] {
                        for height in [0.2, 0.5, 0.8] {
                            let mut p = lerp(shared[0], shared[1], station);
                            p[2] = low.surface.bounds[1][2] + gap * height;
                            if !super::floor_gaps::contains(scene, p, &mut budget)? {
                                uncovered = true;
                                exposed.push(p);
                            }
                        }
                    }
                    if uncovered {
                        span += length(sub(shared[1], shared[0]));
                    }
                }
            }
            if !exposed.is_empty() {
                findings.push(finding(low.surface.source.clone(),"ROOF_STEP_OPEN",format!("{} and {} adjoin across a {gap:.2} m roof step with exposed samples along {span:.2} m. Review the wall, clerestory or open-terrace intent.",low.surface.id,high.surface.id),vec![low.surface.id.clone(),high.surface.id.clone()],&exposed,"warning"));
            }
        }
    }
    for wall in &walls {
        let center = scale(add(wall.bounds[0], wall.bounds[1]), 0.5);
        let roof = roofs
            .iter()
            .filter(|r| {
                r.tops
                    .iter()
                    .any(|t| super::floor_gaps::slice(center, center, *t).is_some())
            })
            .max_by(|a, b| a.surface.bounds[1][2].total_cmp(&b.surface.bounds[1][2]));
        let Some(roof) = roof else { continue };
        let top = roof.surface.bounds[1][2];
        let excess = wall.bounds[1][2] - top;
        if wall.bounds[0][2] >= top || excess <= 1.5 {
            continue;
        }
        let mut points: Vec<_> = wall
            .points
            .iter()
            .flatten()
            .copied()
            .filter(|p| p[2] > top)
            .collect();
        points.push([center[0], center[1], top]);
        findings.push(finding(wall.source.clone(),"WALL_ABOVE_ROOF",format!("{} extends {excess:.2} m above roof {}. Review the wall height or top-floor attachment.",wall.id,roof.surface.id),vec![wall.id.clone(),roof.surface.id.clone()],&points,"warning"));
    }
    findings.extend(wall_alignment(&walls, &mut budget)?);
    Ok(findings)
}
pub(super) fn audit(
    scene: &Scene,
    cancelled: &dyn Fn() -> bool,
) -> Result<Vec<Diagnostic>, String> {
    match review(scene, cancelled) {
        Ok(findings) => Ok(findings),
        Err(error) if cancelled() => Err(error),
        Err(error) => {
            let Some(mesh) = scene.manifest.meshes.first() else {
                return Ok(vec![]);
            };
            Ok(vec![finding(
                mesh.source.clone(),
                "ARCHITECTURE_REVIEW_INCOMPLETE",
                format!("The bounded enclosure review stopped before completion. {error}"),
                vec![mesh.object_id.clone()],
                &mesh.bounds,
                "warning",
            )])
        }
    }
}

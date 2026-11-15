//! Interval-based review of adjoining floor boundaries using actual solid faces.
use super::*;
use superspec_geometry::obstructions::Budget;

const XY: f64 = 1e-5;
const HEIGHT: f64 = 0.001001;
struct Boundary {
    floor: usize,
    a: V3,
    b: V3,
    side: f64,
}
struct Face {
    points: [V3; 3],
    owner: usize,
    bounds: [f64; 4],
}
#[derive(Clone, Copy)]
pub(super) struct Piece {
    pub start: f64,
    pub end: f64,
    pub z: f64,
    pub rise: f64,
    owner: usize,
    sign: f64,
}
impl Piece {
    fn height(self, t: f64) -> f64 {
        self.z + self.rise * t
    }
    fn active(self, t: f64) -> bool {
        self.start <= t && self.end >= t
    }
}

pub(super) fn slice(a: V3, b: V3, p: [V3; 3]) -> Option<Piece> {
    let determinant = orient(p[0], p[1], p[2]);
    if determinant.abs() < 1e-10 {
        return None;
    }
    let sign = determinant.signum();
    let (mut start, mut end) = (0f64, 1f64);
    for i in 0..3 {
        let (u, v) = (p[i], p[(i + 1) % 3]);
        let f0 = sign * orient(u, v, a);
        let f1 = sign * orient(u, v, b);
        let tolerance = XY * (v[0] - u[0]).hypot(v[1] - u[1]);
        if (f1 - f0).abs() < 1e-12 || f0.abs() <= tolerance && f1.abs() <= tolerance {
            if f0 < -tolerance {
                return None;
            }
        } else {
            let t = -f0 / (f1 - f0);
            if f1 > f0 {
                start = start.max(t)
            } else {
                end = end.min(t)
            }
        }
    }
    if end - start < 1e-9 {
        return None;
    }
    let height = |q| {
        (orient(q, p[1], p[2]) * p[0][2]
            + orient(p[0], q, p[2]) * p[1][2]
            + orient(p[0], p[1], q) * p[2][2])
            / determinant
    };
    let z = height(a);
    Some(Piece {
        start,
        end,
        z,
        rise: height(b) - z,
        owner: 0,
        sign,
    })
}

fn overlap(a: &Boundary, b: &Boundary) -> Option<[V3; 2]> {
    let d = sub(a.b, a.a);
    let size = d[0].hypot(d[1]);
    if size < 0.001 {
        return None;
    }
    let direction = [d[0] / size, d[1] / size];
    let other = sub(b.b, b.a);
    if (direction[0] * other[0] + direction[1] * other[1]) * a.side * b.side >= 0.
        || orient(a.a, a.b, b.a).abs() / size > XY
        || orient(a.a, a.b, b.b).abs() / size > XY
    {
        return None;
    }
    let station = |p: V3| (p[0] - a.a[0]) * direction[0] + (p[1] - a.a[1]) * direction[1];
    let first = station(b.a);
    let last = station(b.b);
    let low = 0f64.max(first.min(last));
    let high = size.min(first.max(last));
    (high - low > 0.001)
        .then(|| [low, high].map(|t| [a.a[0] + direction[0] * t, a.a[1] + direction[1] * t, 0.]))
}

fn spans(pieces: &[Piece], t: f64) -> Vec<[f64; 2]> {
    let mut owners = BTreeMap::<usize, Vec<(f64, f64)>>::new();
    for p in pieces.iter().filter(|p| p.active(t)) {
        owners
            .entry(p.owner)
            .or_default()
            .push((p.height(t), p.sign));
    }
    let mut spans = vec![];
    for mut hits in owners.into_values() {
        hits.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut i = 0;
        let mut start = None;
        let mut top = None;
        while i < hits.len() {
            let z = hits[i].0;
            let (mut entering, mut leaving) = (false, false);
            while i < hits.len() && hits[i].0 - z < XY {
                entering |= hits[i].1 < 0.;
                leaving |= hits[i].1 > 0.;
                i += 1;
            }
            if entering && leaving {
                continue;
            }
            if entering {
                if let (Some(a), Some(b)) = (start, top) {
                    spans.push([a, b]);
                    top = None;
                }
                start = Some(z);
            } else if start.is_some() {
                top = Some(z);
            }
        }
        if let (Some(a), Some(b)) = (start, top) {
            spans.push([a, b]);
        }
    }
    spans.sort_by(|a, b| a[0].total_cmp(&b[0]));
    spans
}

fn open_height(low: f64, high: f64, spans: &[[f64; 2]]) -> f64 {
    let mut cursor = low;
    let mut gap = 0f64;
    for [a, b] in spans {
        if *a >= high {
            break;
        }
        if *a > cursor {
            gap = gap.max(*a - cursor);
        }
        cursor = cursor.max(*b);
    }
    gap.max(high - cursor)
}

fn review(scene: &Scene, cancelled: &dyn Fn() -> bool) -> Result<Vec<Diagnostic>, String> {
    let context = scene
        .manifest
        .architecture
        .as_ref()
        .ok_or("Missing architecture.")?;
    let mut budget = Budget::new(8_000_000, cancelled)?;
    let mut boundaries = vec![];
    for (index, floor) in context.floors.iter().enumerate() {
        if context
            .objects
            .iter()
            .any(|o| o.id == floor.id && o.kind == "terrain")
        {
            continue;
        }
        for (hole, ring) in floor.loops.iter().enumerate() {
            let side = area(ring).signum() * if hole == 0 { 1. } else { -1. };
            for i in 0..ring.len() {
                budget.spend(1)?;
                boundaries.push(Boundary {
                    floor: index,
                    a: ring[i],
                    b: ring[(i + 1) % ring.len()],
                    side,
                });
            }
        }
    }
    let mut owners = BTreeMap::new();
    let mut faces = vec![];
    for mesh in &scene.manifest.meshes {
        if !scene.manifest.colliders.contains(&mesh.id)
            || context.objects.iter().any(|o| {
                o.id == mesh.object_id
                    && matches!(o.attributes.get("dynamic"), Some(Value::Boolean(true)))
            })
        {
            continue;
        }
        let next = owners.len();
        let owner = *owners.entry(mesh.object_id.clone()).or_insert(next);
        for t in
            scene.indices[mesh.index_start..mesh.index_start + mesh.index_count].chunks_exact(3)
        {
            budget.spend(1)?;
            let points = [
                point(scene, mesh.vertex_start + t[0] as usize),
                point(scene, mesh.vertex_start + t[1] as usize),
                point(scene, mesh.vertex_start + t[2] as usize),
            ];
            if orient(points[0], points[1], points[2]).abs() < 1e-10 {
                continue;
            }
            let bounds = [
                points.iter().map(|p| p[0]).fold(f64::INFINITY, f64::min),
                points.iter().map(|p| p[1]).fold(f64::INFINITY, f64::min),
                points
                    .iter()
                    .map(|p| p[0])
                    .fold(f64::NEG_INFINITY, f64::max),
                points
                    .iter()
                    .map(|p| p[1])
                    .fold(f64::NEG_INFINITY, f64::max),
            ];
            faces.push(Face {
                points,
                owner,
                bounds,
            });
        }
    }
    let mut findings = BTreeMap::<(usize, usize), (f64, f64, V3)>::new();
    for (index, a) in boundaries.iter().enumerate() {
        for b in &boundaries[index + 1..] {
            budget.spend(1)?;
            if a.floor == b.floor {
                continue;
            }
            let Some([p, q]) = overlap(a, b) else {
                continue;
            };
            let floor_pieces = |index: usize| {
                let floor = &context.floors[index];
                floor
                    .triangles
                    .iter()
                    .filter_map(|t| slice(p, q, t.map(|i| floor.points[i])))
                    .collect::<Vec<_>>()
            };
            budget.spend(
                context.floors[a.floor].triangles.len() + context.floors[b.floor].triangles.len(),
            )?;
            let first = floor_pieces(a.floor);
            let second = floor_pieces(b.floor);
            let mut seals = vec![];
            for face in &faces {
                budget.spend(1)?;
                if face.bounds[0] > p[0].max(q[0]) + XY
                    || face.bounds[1] > p[1].max(q[1]) + XY
                    || face.bounds[2] < p[0].min(q[0]) - XY
                    || face.bounds[3] < p[1].min(q[1]) - XY
                {
                    continue;
                }
                if let Some(mut piece) = slice(p, q, face.points) {
                    piece.owner = face.owner;
                    seals.push(piece);
                }
            }
            let lines: Vec<_> = first.iter().chain(&second).chain(&seals).collect();
            let mut cuts = vec![0., 1.];
            for line in &lines {
                cuts.extend([line.start, line.end]);
            }
            for (i, a) in lines.iter().enumerate() {
                for b in &lines[i + 1..] {
                    budget.spend(1)?;
                    let rise = a.rise - b.rise;
                    if rise.abs() < 1e-12 {
                        continue;
                    }
                    for tolerance in [0., -HEIGHT, HEIGHT] {
                        let t = (b.z - a.z + tolerance) / rise;
                        if t > 0. && t < 1. {
                            cuts.push(t);
                            if cuts.len() > 1_000_000 {
                                return Err("Floor-boundary interval storage exceeded.".into());
                            }
                        }
                    }
                }
            }
            cuts.sort_by(f64::total_cmp);
            cuts.dedup_by(|a, b| (*a - *b).abs() < 1e-12);
            let mut missing = 0.;
            let mut largest = 0.;
            let mut at = p;
            for interval in cuts.windows(2) {
                budget.spend(seals.len() + first.len() + second.len() + 1)?;
                let t = (interval[0] + interval[1]) * 0.5;
                let top = |pieces: &[Piece]| {
                    pieces
                        .iter()
                        .filter(|s| s.active(t))
                        .map(|s| s.height(t))
                        .max_by(f64::total_cmp)
                };
                let (Some(z1), Some(z2)) = (top(&first), top(&second)) else {
                    continue;
                };
                let gap = open_height(z1.min(z2), z1.max(z2), &spans(&seals, t));
                if gap <= HEIGHT {
                    continue;
                }
                missing += (interval[1] - interval[0]) * length(sub(q, p));
                if gap > largest {
                    largest = gap;
                    at = lerp(p, q, t);
                    at[2] = z1.min(z2);
                }
            }
            if missing > 0.001 {
                let key = (a.floor.min(b.floor), a.floor.max(b.floor));
                let old = findings.entry(key).or_insert((0., 0., at));
                old.0 += missing;
                if largest > old.1 {
                    old.1 = largest;
                    old.2 = at;
                }
            }
        }
    }
    Ok(findings.into_iter().filter_map(|((a,b),(span,height,at))| {
        let first=&context.floors[a];let second=&context.floors[b];
        let source=context.objects.iter().find(|o|o.id==first.id)?.source.clone();
        Some(finding(source,"FLOOR_EDGE_GAP",format!("Floor boundaries {} and {} leave an unsealed vertical interval along {span:.3} m of boundary, observed {height:.3} m high. Review the retaining join or separate-storey intent.",first.id,second.id),vec![first.id.clone(),second.id.clone()],&[at,add(at,[0.,0.,height])],"warning"))
    }).collect())
}

/// Tests a point against actual entry/exit faces of static collision solids.
pub(super) fn contains(scene: &Scene, p: V3, budget: &mut Budget<'_>) -> Result<bool, String> {
    let context = scene
        .manifest
        .architecture
        .as_ref()
        .ok_or("Missing architecture.")?;
    let mut owners = BTreeMap::new();
    let mut pieces = vec![];
    for mesh in &scene.manifest.meshes {
        if !scene.manifest.colliders.contains(&mesh.id)
            || context.objects.iter().any(|o| {
                o.id == mesh.object_id
                    && matches!(o.attributes.get("dynamic"), Some(Value::Boolean(true)))
            })
        {
            continue;
        }
        let next = owners.len();
        let owner = *owners.entry(&mesh.object_id).or_insert(next);
        for t in
            scene.indices[mesh.index_start..mesh.index_start + mesh.index_count].chunks_exact(3)
        {
            budget.spend(1)?;
            let points = [
                point(scene, mesh.vertex_start + t[0] as usize),
                point(scene, mesh.vertex_start + t[1] as usize),
                point(scene, mesh.vertex_start + t[2] as usize),
            ];
            if let Some(mut piece) = slice(p, p, points) {
                piece.owner = owner;
                pieces.push(piece);
            }
        }
    }
    Ok(spans(&pieces, 0.5)
        .iter()
        .any(|s| p[2] >= s[0] - 1e-6 && p[2] <= s[1] + 1e-6))
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
                "GEOMETRY_REVIEW_INCOMPLETE",
                format!("The bounded floor-boundary review stopped before completion. {error}"),
                vec![mesh.object_id.clone()],
                &mesh.bounds,
                "warning",
            )])
        }
    }
}

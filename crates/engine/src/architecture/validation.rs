use super::*;
use superspec_aml::diagnostic::SpatialEvidence;
use superspec_geometry::clipping::{dominant_axis, half_plane, polygon_area, project};

fn spatial(node: &Node, code: &'static str, message: &str, points: &[V3]) -> Diagnostic {
    let mut d = node.fail(code, message);
    let mut bounds = [[f64::INFINITY; 3], [f64::NEG_INFINITY; 3]];
    for p in points {
        for i in 0..3 {
            bounds[0][i] = bounds[0][i].min(p[i]);
            bounds[1][i] = bounds[1][i].max(p[i]);
        }
    }
    if !points.is_empty() {
        d.spatial = Some(Box::new(SpatialEvidence {
            bounds,
            inspection_target: node.id.clone(),
            evidence: message.into(),
        }));
    }
    d
}
fn covered(a: V3, b: V3, faces: &[[V3; 3]], work: &mut usize) -> Result<bool, String> {
    let mut intervals = vec![];
    for p in faces {
        *work += 1;
        if *work > 8_000_000 {
            return Err("Structural query budget exceeded.".into());
        }
        let Ok(n) = unit(cross(sub(p[1], p[0]), sub(p[2], p[0]))) else {
            continue;
        };
        if [a, b].iter().any(|q| dot(n, sub(*q, p[0])).abs() > 0.0001) {
            continue;
        }
        let mut lo = 0.0_f64;
        let mut hi = 1.0_f64;
        for i in 0..3 {
            let inward = unit(cross(n, sub(p[(i + 1) % 3], p[i])))?;
            let start = dot(inward, sub(a, p[i]));
            let finish = dot(inward, sub(b, p[i]));
            let change = finish - start;
            if change.abs() < 1e-14 {
                if start < -0.0001 {
                    hi = -1.;
                }
            } else {
                let t = (-0.0001 - start) / change;
                if change > 0. {
                    lo = lo.max(t);
                } else {
                    hi = hi.min(t);
                }
            }
        }
        if lo <= hi {
            intervals.push([lo.max(0.), hi.min(1.)]);
        }
    }
    intervals.sort_by(|a, b| a[0].total_cmp(&b[0]));
    let epsilon = 1e-6 / length(sub(b, a)).max(1.);
    let mut end = 0.;
    for interval in intervals {
        if interval[0] > end + epsilon {
            return Ok(false);
        }
        end = end.max(interval[1]);
    }
    Ok(end >= 1. - epsilon)
}
fn inside(point: V3, solid: &Solid) -> bool {
    let bounds = solid.bounds();
    if (0..3).any(|i| point[i] <= bounds[0][i] || point[i] >= bounds[1][i]) {
        return false;
    }
    let faces = owned_geometry(std::slice::from_ref(solid), &[]);
    let direction = [0.8728715609439696, 0.4364357804719848, 0.2182178902359924];
    let mut hits = vec![];
    for geometry in faces.values() {
        for t in &geometry.triangles {
            let p = t.map(|i| geometry.points[i]);
            let e1 = sub(p[1], p[0]);
            let e2 = sub(p[2], p[0]);
            let h = cross(direction, e2);
            let den = dot(e1, h);
            if den.abs() < 1e-12 {
                continue;
            }
            let s = sub(point, p[0]);
            let u = dot(s, h) / den;
            let q = cross(s, e1);
            let v = dot(direction, q) / den;
            let d = dot(e2, q) / den;
            if u >= -1e-9 && v >= -1e-9 && u + v <= 1. + 1e-9 && d > 1e-8 {
                hits.push(d);
            }
        }
    }
    hits.sort_by(f64::total_cmp);
    hits.dedup_by(|a, b| (*a - *b).abs() < 1e-7);
    hits.len() % 2 == 1
}
impl Compiler<'_> {
    pub(super) fn validate_structure(
        &self,
        scene: &Scene,
        product: &Product,
    ) -> Result<(), Diagnostic> {
        let physical = owned_geometry(&product.solids, &product.patches);
        let faces = physical
            .iter()
            .filter(|(owner, _)| self.surfaces[owner].collidable)
            .flat_map(|(owner, g)| {
                g.triangles
                    .iter()
                    .map(move |t| (t.map(|i| g.points[i]), *owner))
            })
            .collect::<Vec<_>>();
        let all_faces = faces.iter().map(|(p, _)| *p).collect::<Vec<_>>();
        for source in self
            .resolution
            .document
            .metadata
            .iter()
            .filter(|n| n.kind == "connection")
        {
            for key in ["from", "to"] {
                let id = source.text(key).unwrap();
                let index = self.resolution.ids[id];
                let node = &self.resolution.nodes[index];
                let has_floor = self.floors.get(&index).is_some_and(|floor| {
                    node.source.boolean("collidable", true)
                        && (!["roof", "canopy"].contains(&node.source.kind.as_str())
                            || node.source.boolean("playable", false))
                        && floor.triangles.iter().any(|t| {
                            let p = t.map(|i| floor.points[i]);
                            let Ok(n) = unit(cross(sub(p[1], p[0]), sub(p[2], p[0]))) else {
                                return false;
                            };
                            if n[2] <= 1e-7 {
                                return false;
                            }
                            faces.iter().any(|(q, owner)| {
                                if self.surfaces[owner].node != index {
                                    return false;
                                }
                                let Ok(m) = unit(cross(sub(q[1], q[0]), sub(q[2], q[0]))) else {
                                    return false;
                                };
                                dot(n, m) > 1. - 1e-7
                                    && q.iter().all(|q| dot(n, sub(*q, p[0])).abs() < 1e-6)
                                    && superspec_geometry::grading::overlap_area(p, *q) > 1e-8
                            })
                        })
                });
                if !has_floor {
                    let mut d = source.fail(
                        "AML_CONNECTION_SUPPORT",
                        "Declared connection is missing final physical support.",
                    );
                    d.objects = vec![
                        source.id.clone(),
                        source.string("from", ""),
                        source.string("to", ""),
                    ];
                    return Err(d);
                }
            }
        }
        let mut work = 0usize;
        for (index, node) in self
            .resolution
            .nodes
            .iter()
            .enumerate()
            .filter(|(_, n)| n.source.kind == "shared-edge")
        {
            let source = &node.source;
            let path = self
                .path(source, "path")?
                .iter()
                .map(|p| node.matrix.point(*p))
                .collect::<Vec<_>>();
            let mut hosts = vec![];
            for key in ["from", "to"] {
                let id = self
                    .resolution
                    .reference(index, source.text(key).unwrap())?;
                let host = &self.resolution.nodes[id];
                if host.source.kind == "collision-surface" {
                    return Err(source.fail(
                        "AML_SHARED_EDGE",
                        "Collision surfaces cannot be shared-edge hosts.",
                    ));
                }
                let product = &self.cache[&id];
                let geometry = owned_geometry(&product.solids, &product.patches);
                let faces = geometry
                    .iter()
                    .filter(|(o, _)| self.surfaces[o].collidable)
                    .flat_map(|(_, g)| g.triangles.iter().map(|t| t.map(|i| g.points[i])))
                    .collect::<Vec<_>>();
                hosts.push((id, faces));
            }
            for pair in path.windows(2) {
                let mut valid = covered(pair[0], pair[1], &all_faces, &mut work).at(source)?;
                for (_, faces) in &hosts {
                    valid &= covered(pair[0], pair[1], faces, &mut work).at(source)?;
                }
                if !valid {
                    let mut d = spatial(
                        source,
                        "AML_SHARED_EDGE",
                        "Declared shared edge lacks continuous native contact on both hosts or final physical geometry.",
                        &path,
                    );
                    d.objects.extend(
                        hosts
                            .iter()
                            .map(|(i, _)| self.resolution.nodes[*i].id.clone()),
                    );
                    return Err(d);
                }
            }
        }
        for volume in self.volumes.iter().filter(|v| v.kind == "clearance") {
            let node = &self.resolution.nodes[self.resolution.ids[&volume.id]].source;
            let mut offenders = BTreeSet::new();
            for t in &volume.triangles {
                let mut base = t.map(|i| volume.points[i]);
                let mut n = unit(cross(sub(base[1], base[0]), sub(base[2], base[0]))).at(node)?;
                if dot(n, volume.extrusion) < 0. {
                    base.reverse();
                    n = scale(n, -1.);
                }
                let mut planes = vec![
                    (n, dot(n, base[0])),
                    (scale(n, -1.), -dot(n, add(base[0], volume.extrusion))),
                ];
                for i in 0..3 {
                    let inward =
                        unit(cross(volume.extrusion, sub(base[(i + 1) % 3], base[i]))).at(node)?;
                    planes.push((inward, dot(inward, base[i])));
                }
                for (points, owner) in &faces {
                    work += 1;
                    if work > 8_000_000 {
                        return Err(
                            node.fail("AML_COMPLEXITY", "Structural query budget exceeded.")
                        );
                    }
                    let mut clipped = points.to_vec();
                    for (normal, offset) in &planes {
                        clipped = half_plane(&clipped, |p| dot(*normal, p) - offset - 1e-6);
                        if clipped.is_empty() {
                            break;
                        }
                    }
                    if polygon_area(&clipped) > 1e-10 {
                        offenders.insert(self.surfaces[owner].node);
                    }
                }
                let center = add(
                    scale(add(add(base[0], base[1]), base[2]), 1. / 3.),
                    scale(volume.extrusion, 0.5),
                );
                if offenders.is_empty() {
                    for solid in &product.solids {
                        if inside(center, solid) {
                            for owner in solid.mesh().run_original_id {
                                offenders.insert(self.surfaces[&owner].node);
                            }
                        }
                    }
                }
            }
            if !offenders.is_empty() {
                let mut d = spatial(
                    node,
                    "AML_CLEARANCE",
                    "Declared clearance intersects actual collision geometry.",
                    &[volume.bounds[0], volume.bounds[1]],
                );
                d.objects.extend(
                    offenders
                        .iter()
                        .map(|i| self.resolution.nodes[*i].id.clone()),
                );
                return Err(d);
            }
            if volume.require_support {
                let first = volume.triangles[0].map(|i| volume.points[i]);
                let n = unit(cross(sub(first[1], first[0]), sub(first[2], first[0]))).at(node)?;
                let axis = dominant_axis(n);
                let project_triangle = |p: [V3; 3]| {
                    let mut p = p.map(|p| project(p, axis)).to_vec();
                    if area(&p) < 0. {
                        p.reverse();
                    }
                    p
                };
                let footprint = volume
                    .triangles
                    .iter()
                    .map(|t| project_triangle(t.map(|i| volume.points[i])))
                    .collect::<Vec<_>>();
                let supporting = faces
                    .iter()
                    .filter_map(|(p, _)| {
                        let m = unit(cross(sub(p[1], p[0]), sub(p[2], p[0]))).ok()?;
                        if m[2] <= 1e-7
                            || dot(n, m).abs() < 1. - 1e-7
                            || p.iter().any(|p| dot(n, sub(*p, first[0])).abs() > 1e-6)
                        {
                            None
                        } else {
                            Some(project_triangle(*p))
                        }
                    })
                    .collect::<Vec<_>>();
                if supporting.is_empty() || Solid::uncovered_area(&footprint, &supporting) > 1e-8 {
                    return Err(spatial(
                        node,
                        "AML_CLEARANCE_SUPPORT",
                        "Declared clearance base lacks complete native physical support.",
                        &[volume.bounds[0], volume.bounds[1]],
                    ));
                }
            }
        }
        let _ = scene;
        Ok(())
    }
}

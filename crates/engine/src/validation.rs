//! Source-linked physical and sampled navigation findings shared by native and Wasm callers.
use crate::Scene;
use std::collections::{BTreeMap, BTreeSet};
use superspec_aml::{
    architecture::{Node, Value},
    diagnostic::{Diagnostic, SourceLocation, SpatialEvidence},
};
use superspec_geometry::{
    math::*,
    navigation::{self, Agent},
};
mod envelope;
mod floor_gaps;
pub(crate) mod standing;

pub(crate) fn review_structure(
    scene: &Scene,
    cancelled: &(dyn Fn() -> bool + Sync),
) -> Result<Vec<Diagnostic>, String> {
    #[cfg(not(target_arch = "wasm32"))]
    let (mut floors, enclosure) = std::thread::scope(|scope| {
        let floors = scope.spawn(|| floor_gaps::audit(scene, cancelled));
        let enclosure = envelope::audit(scene, cancelled);
        let floors = floors
            .join()
            .map_err(|_| "Floor review worker panicked.".to_string())?;
        Ok::<_, String>((floors?, enclosure?))
    })?;
    #[cfg(all(target_arch = "wasm32", feature = "wasm-threads"))]
    let (mut floors, enclosure) = {
        let (floors, enclosure) = rayon::join(
            || floor_gaps::audit(scene, cancelled),
            || envelope::audit(scene, cancelled),
        );
        (floors?, enclosure?)
    };
    #[cfg(all(target_arch = "wasm32", not(feature = "wasm-threads")))]
    let (mut floors, enclosure) = (
        floor_gaps::audit(scene, cancelled)?,
        envelope::audit(scene, cancelled)?,
    );
    floors.extend(enclosure);
    Ok(floors)
}
mod physical;
mod route_bounds;
mod water;

pub use superspec_model::validation::{Coverage, Report, Route};

fn finding(
    source: SourceLocation,
    code: &'static str,
    message: String,
    objects: Vec<String>,
    points: &[V3],
    severity: &'static str,
) -> Diagnostic {
    let spatial = if points.is_empty() {
        None
    } else {
        let mut bounds = [[f64::INFINITY; 3], [f64::NEG_INFINITY; 3]];
        for p in points {
            for i in 0..3 {
                bounds[0][i] = bounds[0][i].min(p[i]);
                bounds[1][i] = bounds[1][i].max(p[i]);
            }
        }
        Some(Box::new(SpatialEvidence {
            bounds,
            inspection_target: objects.first().cloned().unwrap_or_default(),
            evidence: message.clone(),
        }))
    };
    Diagnostic {
        source,
        code,
        message: message.into_boxed_str(),
        objects,
        severity,
        spatial,
    }
}
fn node_finding(
    node: &Node,
    code: &'static str,
    message: &str,
    points: &[V3],
    severity: &'static str,
) -> Diagnostic {
    finding(
        node.source.clone(),
        code,
        message.into(),
        vec![node.id.clone()],
        points,
        severity,
    )
}

/// A segment intersects canonical collision strictly between its endpoints.
pub fn ray_blocked(scene: &Scene, a: V3, b: V3) -> bool {
    let direction = sub(b, a);
    for mesh in &scene.manifest.meshes {
        if !scene.manifest.colliders.contains(&mesh.id) {
            continue;
        }
        for t in
            scene.indices[mesh.index_start..mesh.index_start + mesh.index_count].chunks_exact(3)
        {
            let p = [
                point(scene, mesh.vertex_start + t[0] as usize),
                point(scene, mesh.vertex_start + t[1] as usize),
                point(scene, mesh.vertex_start + t[2] as usize),
            ];
            let e1 = sub(p[1], p[0]);
            let e2 = sub(p[2], p[0]);
            let h = cross(direction, e2);
            let determinant = dot(e1, h);
            if determinant.abs() < 1e-12 {
                continue;
            }
            let relative = sub(a, p[0]);
            let u = dot(relative, h) / determinant;
            let q = cross(relative, e1);
            let v = dot(direction, q) / determinant;
            let distance = dot(e2, q) / determinant;
            if u >= 0. && v >= 0. && u + v <= 1. && distance > 1e-5 && distance < 1. - 1e-5 {
                return true;
            }
        }
    }
    false
}
fn point(scene: &Scene, index: usize) -> V3 {
    std::array::from_fn(|axis| scene.positions[index * 3 + axis] as f64)
}

/// Audits the supplied physical scene. Navigation findings explicitly retain their sampled status.
pub fn audit(scene: &mut Scene, cancelled: &dyn Fn() -> bool) -> Result<(), String> {
    let context = scene
        .manifest
        .architecture
        .as_ref()
        .ok_or("Validation needs architectural context.")?;
    let player = context.metadata.iter().find(|n| n.kind == "player");
    let agent = player
        .map(|p| Agent {
            radius: p.number("radius", 0.35),
            height: p.number("height", 1.8),
            step: p.number("step", 0.45),
            slope: p.number("slope", 42.),
        })
        .unwrap_or_default();
    let mut diagnostics = vec![];
    let mut vertices = vec![];
    let mut triangles = vec![];
    let mut duplicate = BTreeMap::new();
    let colliders: BTreeSet<_> = scene.manifest.colliders.iter().collect();
    for mesh in &scene.manifest.meshes {
        if cancelled() {
            return Err("Validation cancelled.".into());
        }
        if !colliders.contains(&mesh.id) {
            continue;
        }
        let offset = vertices.len();
        vertices.extend((0..mesh.vertex_count).map(|i| point(scene, mesh.vertex_start + i)));
        let mut degenerate = 0;
        let mut overlapping = 0;
        for t in
            scene.indices[mesh.index_start..mesh.index_start + mesh.index_count].chunks_exact(3)
        {
            let t = [
                t[0] as usize + offset,
                t[1] as usize + offset,
                t[2] as usize + offset,
            ];
            let p = t.map(|i| vertices[i]);
            if length(cross(sub(p[1], p[0]), sub(p[2], p[0]))) < 1e-10 {
                degenerate += 1;
                continue;
            }
            let mut key = p.map(|p| p.map(|v| (v * 1e6).round() as i64));
            key.sort();
            if duplicate
                .insert(key, mesh.object_id.clone())
                .is_some_and(|owner| owner != mesh.object_id)
            {
                overlapping += 1;
            }
            triangles.push(t);
        }
        if degenerate > 0 {
            diagnostics.push(finding(
                mesh.source.clone(),
                "GEOMETRY_DEGENERATE",
                format!("{degenerate} collision triangles have negligible area."),
                vec![mesh.object_id.clone()],
                &mesh.bounds,
                "warning",
            ));
        }
        if overlapping > 0 {
            diagnostics.push(finding(mesh.source.clone(),"GEOMETRY_COINCIDENT",format!("{overlapping} collision triangles coincide with another object. Review whether the overlap is intentional."),vec![mesh.object_id.clone()],&mesh.bounds,"warning"));
        }
    }
    let nav = water::land(&vertices, &triangles, context, agent, cancelled)?;
    let water = water::build(&vertices, &triangles, context, cancelled)?;
    if let Some(report) = &water {
        diagnostics.extend(report.diagnostics.iter().cloned());
    }
    let network = physical::Network::build(scene, &nav, agent, cancelled)?;
    let route_bounds = route_bounds::Bounds::build(context, agent, cancelled)?;
    let links: Vec<_> = network
        .links
        .iter()
        .map(|link| link.navigation.clone())
        .collect();
    diagnostics.extend(network.diagnostics.iter().cloned());
    let mut markers = BTreeMap::new();
    for node in context.metadata.iter().filter(|n| n.kind == "marker") {
        let p = node.vector("position", [0.; 3]);
        markers.insert(node.id.as_str(), (node, p));
        if node.text("movement") == Some("water") {
            if let Some((code, message)) = water
                .as_ref()
                .and_then(|w| water::marker_issue(&vertices, &triangles, context, p, w.boat))
            {
                diagnostics.push(node_finding(node, code, &message, &[p], "error"));
            } else if water.as_ref().is_none_or(|w| {
                w.navigation
                    .nearest(p, w.boat.radius, w.navigation.cell_height * 2. + 0.01)
                    .is_none()
            }) {
                diagnostics.push(node_finding(node,"NAV_WATER_MARKER_UNVERIFIED","The marker has no nearby boat navigation surface with known draft and headroom.",&[p],"warning"));
            }
        } else if nav
            .nearest(p, agent.radius, agent.step.max(nav.cell_height * 2.))
            .is_none()
        {
            diagnostics.push(node_finding(
                node,
                "NAV_MARKER_UNVERIFIED",
                "The marker has no nearby sampled standing navigation surface.",
                &[p],
                "warning",
            ));
        }
    }
    let mut routes = vec![];
    for node in context.metadata.iter().filter(|n| n.kind == "route") {
        if cancelled() {
            return Err("Validation cancelled.".into());
        }
        let Some((from, a)) = markers.get(node.text("from").unwrap_or("")) else {
            continue;
        };
        let Some((to, b)) = markers.get(node.text("to").unwrap_or("")) else {
            continue;
        };
        let standing_land = [node, *from, *to]
            .iter()
            .all(|n| n.text("movement").is_none_or(|m| m == "land"));
        let water_route = [node, *from, *to]
            .iter()
            .all(|n| n.text("movement") == Some("water"));
        let found = if standing_land {
            nav.path_with_links(
                *a,
                *b,
                agent.radius,
                agent.step.max(nav.cell_height * 2.),
                &links,
            )
        } else if water_route {
            water.as_ref().and_then(|w| {
                w.navigation.path_with_links(
                    *a,
                    *b,
                    w.boat.radius,
                    w.navigation.cell_height * 2. + 0.01,
                    &[],
                )
            })
        } else {
            diagnostics.push(node_finding(
                node,
                "NAV_MOVEMENT_UNVERIFIED",
                "The route requests a movement mode outside this standing-land navigation pass.",
                &[*a, *b],
                "warning",
            ));
            None
        };
        let traversals = found
            .as_ref()
            .map(|path| {
                path.links
                    .iter()
                    .map(|i| network.links[*i].id.clone())
                    .collect()
            })
            .unwrap_or_default();
        let path = found.map(|path| path.points).unwrap_or_default();
        let reachable = !path.is_empty();
        let distance = reachable.then(|| {
            path.windows(2)
                .map(|p| length(sub(p[1], p[0])))
                .sum::<f64>()
        });
        let requested = node.number("min-routes", 1.) as usize;
        if !reachable {
            diagnostics.push(node_finding(node,"NAV_ROUTE_UNVERIFIED","No path was found in the sampled standing navigation mesh. Inspect clearance, steps and movement requirements.",&[*a,*b],"warning"));
        }
        let verified_routes = if reachable && standing_land {
            network
                .independent(
                    context,
                    from,
                    to,
                    agent,
                    agent.step + nav.cell_height * 2.,
                    cancelled,
                )?
                .max(1)
        } else if reachable {
            1
        } else {
            0
        };
        let maximum_routes = if standing_land {
            route_bounds
                .maximum(context, from, to, agent, cancelled)?
                .filter(|maximum| *maximum >= verified_routes)
        } else {
            None
        };
        if let Some(maximum) = maximum_routes.filter(|maximum| requested > *maximum) {
            diagnostics.push(node_finding(
                node,
                "ROUTE_CAPACITY",
                &format!("The declared entrance network permits at most {maximum} independent routes. This route requires {requested}. Unverified mouths were retained as possible crossings."),
                &[*a, *b],
                "error",
            ));
        } else if requested > verified_routes {
            diagnostics.push(node_finding(node,"NAV_INDEPENDENCE_UNVERIFIED",&format!("{verified_routes} independent physical entrance routes were witnessed; {requested} are requested. Sampling may undercount usable passages."),&[*a,*b],"warning"));
        }
        if let Some(distance) = distance {
            if node
                .optional_number("min-distance")
                .is_some_and(|min| distance < min)
            {
                diagnostics.push(node_finding(
                    node,
                    "ROUTE_DISTANCE",
                    &format!(
                        "A {:.3} m witnessed path is shorter than the required minimum.",
                        distance
                    ),
                    &path,
                    "error",
                ));
            }
            if node
                .optional_number("max-distance")
                .is_some_and(|max| length(sub(*b, *a)) > max)
            {
                diagnostics.push(node_finding(
                    node,
                    "ROUTE_DISTANCE",
                    "Even the straight-line distance exceeds the declared maximum.",
                    &[*a, *b],
                    "error",
                ));
            } else if node
                .optional_number("max-distance")
                .is_some_and(|max| distance > max)
            {
                diagnostics.push(node_finding(node,"NAV_DISTANCE_UNVERIFIED","The witnessed corridor exceeds the maximum distance. A shorter path may exist.",&path,"warning"));
            }
        }
        routes.push(Route {
            id: node.id.clone(),
            reachable,
            distance,
            straight_distance: length(sub(*b, *a)),
            path,
            requested_routes: requested,
            verified_routes,
            maximum_routes,
            traversals,
        });
    }
    for node in context.metadata.iter().filter(|n| n.kind == "no-spawn-los") {
        let ids: Vec<_> = node
            .text("markers")
            .unwrap_or("")
            .split_whitespace()
            .collect();
        if ids.len() != 2 {
            continue;
        }
        if let (Some((_, a)), Some((_, b))) = (markers.get(ids[0]), markers.get(ids[1])) {
            let a = add(*a, [0., 0., agent.height * 0.8]);
            let b = add(*b, [0., 0., agent.height * 0.8]);
            if !ray_blocked(scene, a, b) {
                diagnostics.push(node_finding(
                    node,
                    "SPAWN_LINE_OF_SIGHT",
                    "The declared spawn pair has an unobstructed eye-height collision ray.",
                    &[a, b],
                    "error",
                ));
            }
        }
    }
    let mut coverage = vec![];
    for floor in &context.floors {
        let playable = context
            .objects
            .iter()
            .find(|o| o.id == floor.id)
            .is_some_and(|o| matches!(o.attributes.get("playable"), Some(Value::Boolean(true))));
        if !playable {
            continue;
        }
        let stride = (floor.triangles.len() / 1024).max(1);
        let mut samples = 0;
        let mut covered = 0;
        let mut missing = vec![];
        for tri in floor.triangles.iter().step_by(stride) {
            let p = scale(
                tri.iter().fold([0.; 3], |p, i| add(p, floor.points[*i])),
                1. / 3.,
            );
            samples += 1;
            if nav
                .nearest(p, agent.radius, agent.step.max(nav.cell_height * 2.))
                .is_some()
            {
                covered += 1;
            } else {
                missing.push(p);
            }
        }
        if !missing.is_empty() {
            if let Some(object) = context.objects.iter().find(|o| o.id == floor.id) {
                diagnostics.push(finding(object.source.clone(),"NAV_COVERAGE_UNVERIFIED",format!("{} of {samples} sampled floor locations lack nearby standing navigation. This check is inconclusive between samples.",missing.len()),vec![floor.id.clone()],&missing,"warning"));
            }
        }
        coverage.push(Coverage {
            object: floor.id.clone(),
            samples,
            covered,
        });
    }
    for inherited in &scene.manifest.diagnostics {
        if !diagnostics.iter().any(|d| {
            d.code == inherited.code
                && d.objects == inherited.objects
                && d.message == inherited.message
                && d.source.offset == inherited.source.offset
        }) {
            diagnostics.push(inherited.clone());
        }
    }
    let success = !diagnostics.iter().any(|d| d.severity == "error");
    scene.manifest.geometry_validation = "checked";
    scene.manifest.navigation_validation = "sampled";
    scene.manifest.diagnostics = diagnostics.clone();
    scene.manifest.success &= success;
    scene.manifest.validation = Some(Report {
        schema_version: 1,
        success,
        geometry_status: "checked",
        navigation_status: "sampled",
        agent,
        navigation: nav,
        routes,
        coverage,
        diagnostics,
        reviews: scene.manifest.reviews.clone(),
        physical: network.evidence,
        water,
    });
    Ok(())
}

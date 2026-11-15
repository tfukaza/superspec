use super::Proposal;
use crate::Scene;
use serde::Serialize;
use std::{collections::BTreeSet, fmt::Write};
use superspec_geometry::math::*;

#[derive(Clone)]
struct Placement {
    point: V3,
    area: String,
}
#[derive(Serialize)]
pub struct Pickup {
    id: String,
    kind: &'static str,
    variant: Option<&'static str>,
    at: V3,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Run {
    pub spawn: V3,
    pub exit: V3,
    pub spawn_facing: f64,
    pub pickups: Vec<Pickup>,
    pub enemy_posts: Vec<V3>,
}

fn supported(scene: &Scene, p: V3) -> Option<Placement> {
    let mut best = None;
    let mut distance = 0.25;
    for floor in &scene.manifest.architecture.as_ref()?.floors {
        for triangle in &floor.triangles {
            let [a, b, c] = triangle.map(|i| floor.points[i]);
            let denominator = orient(a, b, c);
            if denominator.abs() < 1e-16 {
                continue;
            }
            let u = orient(b, c, p) / denominator;
            let v = orient(c, a, p) / denominator;
            let w = 1. - u - v;
            if u.min(v).min(w) < -1e-7 {
                continue;
            }
            let z = u * a[2] + v * b[2] + w * c[2];
            let delta = (z - p[2]).abs();
            if delta < distance {
                distance = delta;
                best = Some(Placement {
                    point: [p[0], p[1], z],
                    area: floor.area.clone(),
                });
            }
        }
    }
    best
}

pub fn populate(
    proposal: &mut Proposal,
    scene: &Scene,
    depth: usize,
    cancelled: &dyn Fn() -> bool,
) -> Result<Run, String> {
    let context = scene
        .manifest
        .architecture
        .as_ref()
        .ok_or("Run placement needs architecture metadata.")?;
    let spawn = context
        .metadata
        .iter()
        .find(|n| n.kind == "marker" && n.text("kind") == Some("attacker_spawn"))
        .ok_or("Generated map lacks its spawn.")?
        .vector("position", [0.; 3]);
    let exit = context
        .metadata
        .iter()
        .find(|n| n.kind == "marker" && n.text("kind") == Some("objective"))
        .ok_or("Generated map lacks its exit.")?
        .vector("position", [0.; 3]);
    let nav = &scene
        .manifest
        .validation
        .as_ref()
        .ok_or("Run placement requires navigation validation.")?
        .navigation;
    let (start, _) = nav
        .nearest(spawn, 1., 1.)
        .ok_or("Spawn lacks navigation support.")?;
    let component = nav.components[start];
    let mut points = vec![];
    let mut seen = BTreeSet::new();
    let n = ((8192. / nav.polygons.len().max(1) as f64).sqrt().floor() as usize).clamp(1, 12);
    let mut add_point = |p| {
        if let Some(placed) = supported(scene, p) {
            let key = placed.point.map(|v| (v * 100000.).round() as i64);
            if seen.insert(key) {
                points.push(placed);
            }
        }
    };
    for (i, polygon) in nav.polygons.iter().enumerate() {
        if cancelled() {
            return Err("Run placement cancelled.".into());
        }
        if nav.components[i] != component {
            continue;
        }
        for edge in polygon[1..].windows(2) {
            let [a, b, c] = [
                nav.vertices[polygon[0]],
                nav.vertices[edge[0]],
                nav.vertices[edge[1]],
            ];
            add_point(scale(add(add(a, b), c), 1. / 3.));
            for x in 0..n {
                for y in 0..n - x {
                    let u = (x + 1) as f64 / (n + 2) as f64;
                    let v = (y + 1) as f64 / (n + 2) as f64;
                    add_point(add(add(scale(a, u), scale(b, v)), scale(c, 1. - u - v)));
                }
            }
        }
    }
    let rng = &mut proposal.rng;
    let mut kinds = vec![];
    let health = rng.int(2, 3) + usize::from(depth > 3);
    let ammo = rng.int(3, 5);
    let guns = if depth == 1 { 1 } else { rng.int(1, 2) };
    kinds.extend(std::iter::repeat_n(("health", None), health));
    kinds.extend(std::iter::repeat_n(("ammo", None), ammo));
    if rng.bool(0.7) {
        kinds.push(("armor", None));
    }
    for _ in 0..guns {
        kinds.push(("weapon", Some(["smg", "shotgun", "rifle"][rng.int(0, 2)])));
    }
    for i in (1..kinds.len()).rev() {
        let j = rng.int(0, i);
        kinds.swap(i, j);
    }
    let mut shuffled = points.clone();
    for i in (1..shuffled.len()).rev() {
        let j = rng.int(0, i);
        shuffled.swap(i, j);
    }
    let mut selected = vec![];
    for spacing in [12., 8., 5., 3., 2., 0.735] {
        selected.clear();
        for p in &shuffled {
            if length(sub(p.point, spawn)) > 0.7
                && selected
                    .iter()
                    .all(|q: &Placement| length(sub(p.point, q.point)) >= spacing)
            {
                selected.push(p.clone());
                if selected.len() == kinds.len() {
                    break;
                }
            }
        }
        if selected.len() == kinds.len() {
            break;
        }
    }
    if selected.len() != kinds.len() {
        return Err("Insufficient reachable space for generated pickups.".into());
    }
    let mut additions = String::new();
    let mut pickups = vec![];
    for (i, ((kind, variant), p)) in kinds.into_iter().zip(selected).enumerate() {
        let id = format!("run.pickup.{}", i + 1);
        let label = variant.map_or(kind.to_owned(), |v| format!("{kind}:{v}"));
        writeln!(additions,"<marker id=\"{id}\" kind=\"poi\" label=\"{label}\" group=\"layout\" element=\"{}\" position=\"{} {} {}\"/>",p.area,p.point[0],p.point[1],p.point[2]).unwrap();
        pickups.push(Pickup {
            id,
            kind,
            variant,
            at: p.point,
        });
    }
    let far = points
        .iter()
        .filter(|p| length(sub(p.point, spawn)) > 10.)
        .collect::<Vec<_>>();
    let stride = far.len().div_ceil(96).max(1);
    let mut enemy_posts = vec![];
    for (i, p) in far.into_iter().step_by(stride).enumerate() {
        writeln!(additions,"<marker id=\"run.enemy.{}\" kind=\"enemy_post\" group=\"layout\" element=\"{}\" position=\"{} {} {}\"/>",i+1,p.area,p.point[0],p.point[1],p.point[2]).unwrap();
        enemy_posts.push(p.point);
    }
    let mut facing = points
        .iter()
        .filter(|p| length(sub(p.point, spawn)) < 35.)
        .collect::<Vec<_>>();
    facing.sort_by(|a, b| length(sub(b.point, spawn)).total_cmp(&length(sub(a.point, spawn))));
    let eye = |p: V3| [p[0], p[1], p[2] + 1.62];
    let target = facing
        .into_iter()
        .take(100)
        .find(|p| !crate::validation::ray_blocked(scene, eye(spawn), eye(p.point)));
    let spawn_facing = target.map_or(0., |p| (p.point[1] - spawn[1]).atan2(p.point[0] - spawn[0]));
    let end = proposal
        .aml
        .rfind("</architecture>")
        .ok_or("Generated source lost its root.")?;
    proposal.aml.insert_str(end, &additions);
    superspec_profiles::parse(&proposal.aml).map_err(|e| e.to_string())?;
    Ok(Run {
        spawn,
        exit,
        spawn_facing,
        pickups,
        enemy_posts,
    })
}

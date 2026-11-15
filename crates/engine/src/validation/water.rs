//! Movement-specific surfaces are derived without changing rendered or physical scene geometry.
use super::*;
use crate::architecture::Context;
use superspec_geometry::{Solid, clipping::half_plane};

pub use superspec_model::validation::water::{Boat, Depth, Report};

#[derive(Default)]
struct Input {
    vertices: Vec<V3>,
    triangles: Vec<[usize; 3]>,
    walkable: Vec<bool>,
}
impl Input {
    fn polygon(&mut self, p: &[V3], walkable: bool) -> Result<(), String> {
        if p.len() < 3 {
            return Ok(());
        }
        if self.vertices.len() + p.len() > 2_000_000 {
            return Err("Movement surface budget exceeded.".into());
        }
        let offset = self.vertices.len();
        self.vertices.extend_from_slice(p);
        for i in 1..p.len() - 1 {
            if length(cross(sub(p[i], p[0]), sub(p[i + 1], p[0]))) < 1e-10 {
                continue;
            }
            self.triangles.push([offset, offset + i, offset + i + 1]);
            self.walkable.push(walkable);
        }
        Ok(())
    }
}
fn wet_triangles(context: &Context) -> Vec<[V3; 3]> {
    context
        .water
        .iter()
        .flat_map(|w| w.triangles.iter().map(|t| t.map(|i| w.points[i])))
        .collect()
}
fn overlapping(a: &[V3], b: &[V3]) -> bool {
    (0..2).all(|axis| {
        a.iter().map(|p| p[axis]).fold(f64::INFINITY, f64::min)
            <= b.iter().map(|p| p[axis]).fold(f64::NEG_INFINITY, f64::max)
            && b.iter().map(|p| p[axis]).fold(f64::INFINITY, f64::min)
                <= a.iter().map(|p| p[axis]).fold(f64::NEG_INFINITY, f64::max)
    })
}

pub fn land(
    vertices: &[V3],
    triangles: &[[usize; 3]],
    context: &Context,
    agent: Agent,
    cancelled: &dyn Fn() -> bool,
) -> Result<navigation::Mesh, String> {
    if context.water.is_empty() {
        return navigation::build(vertices, triangles, agent, cancelled);
    }
    let waters = wet_triangles(context);
    let mut input = Input::default();
    let mut work = 0;
    for triangle in triangles {
        if cancelled() {
            return Err("Movement preparation cancelled.".into());
        }
        let tri = triangle.map(|i| vertices[i]);
        if cross(sub(tri[1], tri[0]), sub(tri[2], tri[0]))[2] <= 1e-10 {
            input.polygon(&tri, true)?;
            continue;
        }
        let mut dry = vec![tri.to_vec()];
        for water in &waters {
            if !overlapping(&tri, water) {
                continue;
            }
            let sign = orient(water[0], water[1], water[2]).signum();
            let mut next = vec![];
            for mut inside in dry {
                for side in 0..4 {
                    work += inside.len();
                    if work > 8_000_000 {
                        return Err("Water clipping work budget exceeded.".into());
                    }
                    let distance = |p: V3| {
                        if side == 3 {
                            water[0][2] - p[2] - 1e-7
                        } else {
                            sign * orient(water[side], water[(side + 1) % 3], p)
                        }
                    };
                    let outside = half_plane(&inside, |p| -distance(p));
                    if outside.len() >= 3 {
                        next.push(outside);
                    }
                    inside = half_plane(&inside, distance);
                    if inside.len() < 3 {
                        break;
                    }
                }
                if inside.len() >= 3 {
                    input.polygon(&inside, false)?;
                }
            }
            dry = next;
        }
        for p in dry {
            input.polygon(&p, true)?;
        }
    }
    navigation::build_masked(
        &input.vertices,
        &input.triangles,
        Some(&input.walkable),
        agent,
        cancelled,
    )
}

pub fn build(
    vertices: &[V3],
    triangles: &[[usize; 3]],
    context: &Context,
    cancelled: &dyn Fn() -> bool,
) -> Result<Option<Report>, String> {
    if context.water.is_empty() {
        return Ok(None);
    }
    let settings = context.metadata.iter().find(|n| n.kind == "boat");
    let boat = settings
        .map(|n| Boat {
            radius: n.number("radius", 2.),
            height: n.number("height", 2.),
            draft: n.number("draft", 0.5),
        })
        .unwrap_or(Boat {
            radius: 2.,
            height: 2.,
            draft: 0.5,
        });
    if !boat.draft.is_finite() || boat.draft <= 0. {
        return Err("Boat draft must be positive and finite.".into());
    }
    let mut input = Input::default();
    let mut depth = vec![];
    let mut diagnostics = vec![];
    let mut work = 0;
    for surface in &context.water {
        if cancelled() {
            return Err("Water navigation cancelled.".into());
        }
        let level = surface.points[0][2];
        let mut known = vec![];
        let mut deep = vec![];
        let footprints: Vec<Vec<V3>> = surface
            .triangles
            .iter()
            .map(|t| t.iter().map(|i| surface.points[*i]).collect())
            .collect();
        for water in &footprints {
            for t in triangles {
                let p = t.map(|i| vertices[i]);
                if cross(sub(p[1], p[0]), sub(p[2], p[0]))[2] <= 1e-10 || !overlapping(water, &p) {
                    continue;
                }
                work += p.len();
                if work > 8_000_000 {
                    return Err("Water support budget exceeded.".into());
                }
                let mut p = p.to_vec();
                let sign = orient(water[0], water[1], water[2]).signum();
                for side in 0..3 {
                    p = half_plane(&p, |q| sign * orient(water[side], water[(side + 1) % 3], q));
                }
                let supported = half_plane(&p, |p| level - p[2]);
                let navigable = half_plane(&p, |p| level - boat.draft - p[2]);
                if supported.len() >= 3 {
                    known.push(supported);
                }
                if navigable.len() >= 3 {
                    deep.push(navigable);
                }
            }
        }
        let blocked = context
            .water_obstacles
            .get(&surface.id)
            .cloned()
            .unwrap_or_default();
        let unknown = Solid::uncovered_area(&footprints, &known).max(0.);
        let unavailable = Solid::uncovered_area(&footprints, &deep).max(0.);
        let mut geometry = Solid::planar_surface(&deep, &blocked)?;
        for p in &mut geometry.points {
            p[2] = level;
        }
        for triangle in geometry.triangles {
            input.polygon(&triangle.map(|i| geometry.points[i]), true)?;
        }
        let object = context.objects.iter().find(|o| o.id == surface.id);
        if unknown > 1e-7 {
            if let Some(object) = object {
                diagnostics.push(super::finding(object.source.clone(),"WATER_DEPTH_UNKNOWN",format!("{unknown:.3} m² of water has no supporting seabed evidence and is excluded from boat navigation."),vec![surface.id.clone()],&surface.points,"warning"));
            }
        }
        depth.push(Depth {
            id: surface.id.clone(),
            unknown_area: unknown,
            shallow_area: (unavailable - unknown).max(0.),
        });
    }
    let navigation = navigation::build_masked(
        &input.vertices,
        &input.triangles,
        Some(&input.walkable),
        Agent {
            radius: boat.radius,
            height: boat.height,
            step: 0.,
            slope: 0.1,
        },
        cancelled,
    )?;
    Ok(Some(Report {
        boat,
        navigation,
        depth,
        diagnostics,
    }))
}

fn within(p: V3, triangle: [V3; 3]) -> bool {
    let values = [
        orient(triangle[0], triangle[1], p),
        orient(triangle[1], triangle[2], p),
        orient(triangle[2], triangle[0], p),
    ];
    values.iter().all(|v| *v >= -1e-7) || values.iter().all(|v| *v <= 1e-7)
}

/// Reports definite depth or hull-clearance failures at an authored water marker.
pub fn marker_issue(
    vertices: &[V3],
    triangles: &[[usize; 3]],
    context: &Context,
    p: V3,
    boat: Boat,
) -> Option<(&'static str, String)> {
    let water = context.water.iter().find(|w| {
        (w.points[0][2] - p[2]).abs() < 1e-4
            && w.triangles
                .iter()
                .any(|t| within(p, t.map(|i| w.points[i])))
    })?;
    let mut highest = f64::NEG_INFINITY;
    for t in triangles {
        let q = t.map(|i| vertices[i]);
        let normal = cross(sub(q[1], q[0]), sub(q[2], q[0]));
        if normal[2] <= 1e-10 || !within(p, q) {
            continue;
        }
        let z = q[0][2] - (normal[0] * (p[0] - q[0][0]) + normal[1] * (p[1] - q[0][1])) / normal[2];
        if z <= p[2] + 1e-6 {
            highest = highest.max(z);
        }
    }
    let depth = p[2] - highest;
    if depth.is_finite() && depth < boat.draft - 1e-6 {
        return Some((
            "WATER_DRAFT",
            format!(
                "Water depth is {depth:.3} m at this marker; the boat requires {:.3} m draft.",
                boat.draft
            ),
        ));
    }
    if let Some(loops) = context.water_obstacles.get(&water.id) {
        let winding: f64 = loops
            .iter()
            .filter(|ring| inside(p, ring))
            .map(|ring| area(ring).signum())
            .sum();
        if winding > 0. {
            return Some((
                "WATER_CLEARANCE",
                format!(
                    "Collision occupies the boat's {:.3} m draft / {:.3} m headroom envelope at this marker.",
                    boat.draft, boat.height
                ),
            ));
        }
    }
    None
}

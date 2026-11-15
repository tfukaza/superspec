//! Standing-position admission for compiler visibility and navigation checks.
use superspec_geometry::obstructions::{Bounds, Budget, Obstructions};
use superspec_geometry::{
    math::*,
    navigation::{self, Agent},
};

pub(crate) struct Standing {
    navigation: navigation::Mesh,
    collision: Obstructions,
    agent: Agent,
}

impl Standing {
    pub fn new(coordinates: &[f32], agent: Agent) -> Result<Self, String> {
        if coordinates.len() % 9 != 0
            || coordinates.len() > 9 * 1_048_576
            || coordinates.iter().any(|v| !v.is_finite())
            || !agent.height.is_finite()
            || agent.height < agent.radius * 2.
            || !agent.step.is_finite()
            || agent.step < 0.
            || !agent.slope.is_finite()
            || !(0. ..89.).contains(&agent.slope)
        {
            return Err("Invalid inspection geometry or standing agent.".into());
        }
        let vertices: Vec<V3> = coordinates
            .chunks_exact(3)
            .map(|p| [p[0] as f64, p[1] as f64, p[2] as f64])
            .collect();
        let triangles: Vec<[usize; 3]> = (0..vertices.len())
            .step_by(3)
            .map(|i| [i, i + 1, i + 2])
            .collect();
        let navigation = navigation::build(&vertices, &triangles, agent, &|| false)?;
        let faces: Vec<_> = triangles.iter().map(|t| t.map(|i| vertices[i])).collect();
        let collision = Obstructions::new(&faces, &mut Budget::new(32_000_000, &|| false)?)?;
        Ok(Self {
            navigation,
            collision,
            agent,
        })
    }

    pub fn resolve(&self, requested: V3) -> Result<V3, String> {
        if requested.iter().any(|v| !v.is_finite()) {
            return Err("Invalid inspection position.".into());
        }
        let (_, point) = self
            .navigation
            .nearest(requested, self.agent.radius, self.agent.step + 0.1)
            .ok_or("Select a supported walkable floor.")?;
        let mut budget = Budget::new(1_000_000, &|| false)?;
        let feet = self
            .support(point, point, &mut budget)?
            .ok_or("Standing body has no clear physical support.")?;

        Ok(feet)
    }

    fn support(
        &self,
        candidate: V3,
        previous: V3,
        budget: &mut Budget<'_>,
    ) -> Result<Option<V3>, String> {
        let radius = self.agent.radius;
        let window = self.agent.step + radius + 0.1;
        let bounds = Bounds {
            low: [
                candidate[0] - radius,
                candidate[1] - radius,
                candidate[2] - window,
            ],
            high: [
                candidate[0] + radius,
                candidate[1] + radius,
                candidate[2] + window,
            ],
        };
        let mut support = f64::NEG_INFINITY;
        for index in self.collision.near(bounds, budget)? {
            let tri = *self.collision.triangle(index).unwrap();
            let Ok(normal) = unit(cross(sub(tri[1], tri[0]), sub(tri[2], tri[0]))) else {
                continue;
            };
            if normal[2] < self.agent.slope.to_radians().cos() {
                continue;
            }
            let plane = |p: V3| {
                tri[0][2]
                    - (normal[0] * (p[0] - tri[0][0]) + normal[1] * (p[1] - tri[0][1])) / normal[2]
            };
            if (plane(previous) - previous[2]).abs() > self.agent.step + 0.03 {
                continue;
            }
            let height = sphere_support(tri, normal, candidate, radius);
            if (height - candidate[2]).abs() <= window {
                support = support.max(height);
            }
        }
        if !support.is_finite() {
            return Ok(None);
        }
        let feet = [candidate[0], candidate[1], support + 0.01];
        Ok(self
            .clearance(feet, self.agent.height, budget)?
            .then_some(feet))
    }

    fn clearance(&self, feet: V3, height: f64, budget: &mut Budget<'_>) -> Result<bool, String> {
        let radius = self.agent.radius;
        let a = add(feet, [0., 0., radius]);
        let b = add(feet, [0., 0., height - radius]);
        if self.collision.blocked(a, b, budget)? {
            return Ok(false);
        }
        for index in self
            .collision
            .near(Bounds::points([a, b], radius), budget)?
        {
            let tri = *self.collision.triangle(index).unwrap();
            let mut distance = length(sub(
                a,
                navigation::closest_triangle(a, tri[0], tri[1], tri[2]),
            ))
            .min(length(sub(
                b,
                navigation::closest_triangle(b, tri[0], tri[1], tri[2]),
            )));
            for i in 0..3 {
                distance = distance.min(segment_distance(a, b, tri[i], tri[(i + 1) % 3]));
            }
            if distance < radius - 1e-5 {
                return Ok(false);
            }
        }
        Ok(true)
    }
}

fn sphere_support(tri: [V3; 3], normal: V3, feet: V3, radius: f64) -> f64 {
    let gradient = [-normal[0] / normal[2], -normal[1] / normal[2]];
    let factor = radius / (1. + gradient[0] * gradient[0] + gradient[1] * gradient[1]).sqrt();
    let mut contact = [
        feet[0] + factor * gradient[0],
        feet[1] + factor * gradient[1],
        0.,
    ];
    let plane =
        |p: V3| tri[0][2] + gradient[0] * (p[0] - tri[0][0]) + gradient[1] * (p[1] - tri[0][1]);
    contact[2] = plane(contact);
    let mut height = f64::NEG_INFINITY;
    if length(sub(
        contact,
        navigation::closest_triangle(contact, tri[0], tri[1], tri[2]),
    )) < 1e-8
    {
        height = plane(feet)
            + radius * (1. + gradient[0] * gradient[0] + gradient[1] * gradient[1]).sqrt()
            - radius;
    }
    for i in 0..3 {
        let a = tri[i];
        let b = tri[(i + 1) % 3];
        let edge = sub(b, a);
        let size = edge[0].hypot(edge[1]);
        if size < 1e-10 {
            continue;
        }
        let along = ((feet[0] - a[0]) * edge[0] + (feet[1] - a[1]) * edge[1]) / size;
        let across = ((feet[0] - a[0]) * edge[1] - (feet[1] - a[1]) * edge[0]) / size;
        let remaining = radius * radius - across * across;
        if remaining < 0. {
            continue;
        }
        let horizontal = remaining.sqrt();
        let low = 0f64.max(along - horizontal);
        let high = size.min(along + horizontal);
        if high < low {
            continue;
        }
        let grade = edge[2] / size;
        let station = (along + horizontal * grade / (1. + grade * grade).sqrt()).clamp(low, high);
        height = height.max(
            a[2] + grade * station + (remaining - (station - along).powi(2)).max(0.).sqrt()
                - radius,
        );
    }
    height
}

fn segment_distance(p: V3, q: V3, a: V3, b: V3) -> f64 {
    let d = sub(q, p);
    let e = sub(b, a);
    let r = sub(p, a);
    let dd = dot(d, d);
    let ee = dot(e, e);
    let er = dot(e, r);
    let dr = dot(d, r);
    let de = dot(d, e);
    let (mut s, mut t) = (0., 0.);
    if dd <= 1e-16 && ee <= 1e-16 {
        return length(r);
    }
    if dd <= 1e-16 {
        t = (er / ee).clamp(0., 1.);
    } else if ee <= 1e-16 {
        s = (-dr / dd).clamp(0., 1.);
    } else {
        let denominator = dd * ee - de * de;
        if denominator > 1e-16 {
            s = ((de * er - dr * ee) / denominator).clamp(0., 1.);
        }
        t = (de * s + er) / ee;
        if t < 0. {
            t = 0.;
            s = (-dr / dd).clamp(0., 1.);
        } else if t > 1. {
            t = 1.;
            s = ((de - dr) / dd).clamp(0., 1.);
        }
    }
    length(sub(add(p, scale(d, s)), add(a, scale(e, t))))
}

//! Ordered sign placement and sampled readability. This is not a gameplay visibility proof.
use serde::{Deserialize, Serialize};
use superspec_geometry::obstructions::{Bounds, Budget, Obstructions, triangle_touches_box};
use superspec_geometry::{
    V3,
    math::{cos, cross, dot, length, sin, sub},
};
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Solid {
    pub center: V3,
    pub axes: [V3; 3],
    pub half: V3,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Face {
    pub center: V3,
    pub right: V3,
    pub up: V3,
    pub normal: V3,
    pub width: f64,
    pub height: f64,
    pub look_down: f64,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Sign {
    pub priority: f64,
    pub solids: Vec<Solid>,
    pub faces: Vec<Face>,
}
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Verdict {
    Clear,
    Crowded,
    Obstructed,
}
fn along(origin: V3, steps: &[(V3, f64)]) -> V3 {
    let mut p = origin;
    for (direction, distance) in steps {
        for i in 0..3 {
            p[i] += direction[i] * distance
        }
    }
    p
}
pub fn solid_corners(s: &Solid) -> Vec<V3> {
    let mut points = Vec::with_capacity(8);
    for x in [-1., 1.] {
        for y in [-1., 1.] {
            for z in [-1., 1.] {
                points.push(along(
                    s.center,
                    &[
                        (s.axes[0], x * s.half[0]),
                        (s.axes[1], y * s.half[1]),
                        (s.axes[2], z * s.half[2]),
                    ],
                ))
            }
        }
    }
    points
}
fn local(s: &Solid, p: V3) -> V3 {
    let p = sub(p, s.center);
    s.axes.map(|axis| dot(p, axis))
}
fn frame(axes: [V3; 3]) -> bool {
    axes.iter()
        .all(|p| p.iter().all(|v| v.is_finite()) && (length(*p) - 1.).abs() < 1e-7)
        && (0..3).all(|i| (0..i).all(|j| dot(axes[i], axes[j]).abs() < 1e-7))
}
fn point(p: V3) -> bool {
    p.into_iter().all(|v| v.is_finite() && v.abs() <= 10000.)
}
impl Sign {
    pub fn validate(&self) -> Result<(), String> {
        if !self.priority.is_finite()
            || self.priority.abs() > 1e12
            || self.solids.is_empty()
            || self.solids.len() > 256
            || self.faces.is_empty()
            || self.faces.len() > 512
            || self.solids.iter().any(|s| {
                !point(s.center)
                    || !frame(s.axes)
                    || s.half
                        .iter()
                        .any(|v| !v.is_finite() || *v <= 0. || *v > 50.)
            })
            || self.faces.iter().any(|f| {
                !point(f.center)
                    || !frame([f.right, f.up, f.normal])
                    || [f.width, f.height]
                        .iter()
                        .any(|v| !v.is_finite() || *v <= 0. || *v > 100.)
                    || !f.look_down.is_finite()
                    || !(0. ..=1.5).contains(&f.look_down)
            })
        {
            return Err(
                "Sign survey requires bounded finite solids, face frames and priorities.".into(),
            );
        }
        Ok(())
    }
}
fn touches(s: &Solid, scenery: &Obstructions, budget: &mut Budget<'_>) -> Result<bool, String> {
    let bounds = Bounds::points(solid_corners(s), 0.);
    let half = s.half.map(|v| (v - 0.005).max(0.));
    for i in scenery.near(bounds, budget)? {
        budget.spend(1)?;
        let triangle = scenery.triangle(i).unwrap().map(|p| local(s, p));
        if triangle_touches_box(&triangle, [0.; 3], half) {
            return Ok(true);
        }
    }
    Ok(false)
}
fn solids_within(a: &Solid, b: &Solid, gap: f64) -> bool {
    let offset = sub(b.center, a.center);
    let mut axes = Vec::from(a.axes);
    axes.extend(b.axes);
    for u in a.axes {
        for v in b.axes {
            let axis = cross(u, v);
            let size = length(axis);
            if size > 1e-9 {
                axes.push(axis.map(|v| v / size))
            }
        }
    }
    let reach = |s: &Solid, axis| {
        s.half[0] * dot(s.axes[0], axis).abs()
            + s.half[1] * dot(s.axes[1], axis).abs()
            + s.half[2] * dot(s.axes[2], axis).abs()
    };
    axes.into_iter()
        .all(|axis| dot(offset, axis).abs() <= reach(a, axis) + reach(b, axis) + gap)
}
fn crosses(a: V3, b: V3, s: &Solid) -> bool {
    let a = local(s, a);
    let b = local(s, b);
    let mut enter: f64 = 0.;
    let mut leave: f64 = 1.;
    for i in 0..3 {
        let run = b[i] - a[i];
        if run.abs() < 1e-12 {
            if a[i].abs() > s.half[i] {
                return false;
            }
            continue;
        }
        let first = (-s.half[i] - a[i]) / run;
        let second = (s.half[i] - a[i]) / run;
        enter = enter.max(first.min(second));
        leave = leave.min(first.max(second));
        if enter > leave {
            return false;
        }
    }
    true
}
fn readers(f: &Face) -> Vec<V3> {
    let back = 6f64.min(0.8f64.max(0.8 * f.width.max(f.height)));
    let tan = |v| sin(v) / cos(v);
    [0., 25., -25., 45., -45.]
        .into_iter()
        .map(|angle| {
            along(
                f.center,
                &[
                    (f.normal, back),
                    (f.right, back * tan(angle * std::f64::consts::PI / 180.)),
                    (f.up, -back * tan(f.look_down)),
                ],
            )
        })
        .collect()
}
fn samples(f: &Face) -> Vec<V3> {
    let count = |v: f64| ((v / 0.35).ceil() as usize + 1).clamp(3, 7);
    let columns = count(f.width);
    let rows = count(f.height);
    let mut points = Vec::with_capacity(columns * rows);
    for i in 0..columns {
        for j in 0..rows {
            points.push(along(
                f.center,
                &[
                    (
                        f.right,
                        (0.1 + 0.8 * i as f64 / (columns - 1) as f64 - 0.5) * f.width,
                    ),
                    (
                        f.up,
                        (0.1 + 0.8 * j as f64 / (rows - 1) as f64 - 0.5) * f.height,
                    ),
                    (f.normal, 0.06),
                ],
            ))
        }
    }
    points
}
struct Subject<'a> {
    sign: &'a Sign,
    body: Bounds,
    view: Bounds,
}
fn readable(
    subject: &Subject<'_>,
    scenery: &Obstructions,
    others: &[&Solid],
    budget: &mut Budget<'_>,
) -> Result<bool, String> {
    for face in &subject.sign.faces {
        let samples = samples(face);
        let allowed = (samples.len() as f64 * (1. - 0.6) + 1e-9).floor() as usize;
        for reader in readers(face) {
            let mut hidden = 0;
            for &sample in &samples {
                budget.spend(1)?;
                let mut blocked = scenery.blocked(sample, reader, budget)?;
                if !blocked {
                    for other in others {
                        budget.spend(1)?;
                        if crosses(sample, reader, other) {
                            blocked = true;
                            break;
                        }
                    }
                }
                if blocked {
                    hidden += 1
                }
                if hidden > allowed {
                    break;
                }
            }
            if hidden <= allowed {
                return Ok(true);
            }
        }
    }
    Ok(false)
}
fn in_view<'a>(
    viewer: &Subject<'_>,
    kept: &[Subject<'a>],
    budget: &mut Budget<'_>,
) -> Result<Vec<&'a Solid>, String> {
    let mut result = vec![];
    for other in kept {
        budget.spend(1)?;
        if !std::ptr::eq(other.sign, viewer.sign) && other.body.overlaps(viewer.view) {
            result.extend(&other.sign.solids)
        }
    }
    Ok(result)
}
/// Returns source-ordered verdicts; priorities and exact ties determine placement order.
pub fn survey(
    signs: &[Sign],
    scenery: &Obstructions,
    budget: &mut Budget<'_>,
) -> Result<Vec<Verdict>, String> {
    if signs.len() > 4096
        || signs
            .iter()
            .map(|s| s.solids.len() + s.faces.len())
            .sum::<usize>()
            > 16384
    {
        return Err("Sign survey exceeds its scene allowance.".into());
    }
    budget.spend(signs.len() + 1)?;
    for sign in signs {
        sign.validate()?
    }
    let mut verdicts = vec![Verdict::Clear; signs.len()];
    let mut order: Vec<_> = (0..signs.len()).collect();
    order.sort_by(|a, b| {
        signs[*b]
            .priority
            .partial_cmp(&signs[*a].priority)
            .unwrap()
            .then(a.cmp(b))
    });
    let mut kept = vec![];
    for i in order {
        let sign = &signs[i];
        let corners: Vec<_> = sign.solids.iter().flat_map(solid_corners).collect();
        let subject = Subject {
            sign,
            body: Bounds::points(corners.iter().copied(), 0.06),
            view: Bounds::points(
                corners.into_iter().chain(
                    sign.faces
                        .iter()
                        .flat_map(|f| samples(f).into_iter().chain(readers(f))),
                ),
                0.06,
            ),
        };
        let mut crowded = false;
        for solid in &sign.solids {
            if touches(solid, scenery, budget)? {
                crowded = true;
                break;
            }
        }
        if crowded {
            verdicts[i] = Verdict::Crowded;
            continue;
        }
        for other in &kept {
            budget.spend(1)?;
            let other: &Subject<'_> = other;
            if other.body.overlaps(subject.body) {
                for theirs in &other.sign.solids {
                    for ours in &sign.solids {
                        budget.spend(1)?;
                        if solids_within(ours, theirs, 0.06) {
                            crowded = true;
                            break;
                        }
                    }
                    if crowded {
                        break;
                    }
                }
            }
            if crowded {
                break;
            }
        }
        if crowded {
            verdicts[i] = Verdict::Crowded;
            continue;
        }
        if !readable(
            &subject,
            scenery,
            &in_view(&subject, &kept, budget)?,
            budget,
        )? {
            verdicts[i] = Verdict::Obstructed;
            continue;
        }
        for other in &kept {
            budget.spend(1)?;
            if other.view.overlaps(subject.body) {
                let mut solids = in_view(other, &kept, budget)?;
                solids.extend(&sign.solids);
                if !readable(other, scenery, &solids, budget)? {
                    crowded = true;
                    break;
                }
            }
        }
        if crowded {
            verdicts[i] = Verdict::Crowded;
            continue;
        }
        kept.push(subject);
    }
    Ok(verdicts)
}

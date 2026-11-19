//! The `arc` generator: a section swept round a plan arc, for a wall, a band or a moulding that
//! turns a round corner as one surface.
//!
//! Local frame: the arc starts at the part origin and leaves along +X; it turns to the left
//! (toward +Y) about an upright axis at (0, radius), and its outside faces -Y at the start, as
//! a panel's front does. A section point `[out, up]` lies `out` meters outside the arc of
//! `radius` at height `up`. Normals turn with the arc, so its chords shade as one round; the
//! section's own corners stay sharp unless `bevel` rounds them.
//!
//! Texture coordinates are pattern coordinates in meters: `u` is the distance along the chords
//! of the arc of `radius` (the same for every section point, so a band stays on its wall's
//! pattern) and `v` the section's height at its first point run on along the section, both
//! measured from the pattern anchor. A wall before the arc and one after it that measure their
//! patterns round the same outline therefore meet it without a seam.
use super::{
    Feature, ItemDiagnostic, PatternAnchor,
    parts::Context,
    schema::ArcDefinition,
    surfaces::{P2, Surfaces},
};
use serde::Serialize;
use std::collections::BTreeMap;
use superspec_geometry::{
    V3,
    math::{cos, hypot, sin},
    triangulate,
};

pub(super) const GROUPS: &[&str] = &["side", "end"];

#[derive(Clone, Debug, Serialize)]
pub(super) struct Arc {
    radius: f64,
    /// Radians.
    turn: f64,
    segments: usize,
    section: Vec<P2>,
    closed: bool,
    bevel: f64,
    bevel_segments: usize,
    step: f64,
    anchor: P2,
    rings: Vec<(String, P2)>,
    /// Section edges (by their first point) that run across a recess's floor or head: the
    /// texture's `v` does not advance over them, so the face above a recess keeps its height.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    level: Vec<usize>,
}

/// One straight piece of the swept profile, with the outward normal at each of its ends.
struct Piece {
    a: P2,
    b: P2,
    na: P2,
    nb: P2,
    /// Distance along the profile to `a` and to `b`.
    along: [f64; 2],
}

pub(super) fn resolve(
    context: &Context<'_>,
    definition: &ArcDefinition,
    pointer: &str,
) -> Result<Arc, ItemDiagnostic> {
    let geometry = |field: &str, message: &str| {
        context.fail(&format!("{pointer}/{field}"), "ITEM_GEOMETRY", message)
    };
    let radius = context.dimension(&definition.radius, &format!("{pointer}/radius"))?;
    let turn = context.scalar(&definition.turn, &format!("{pointer}/turn"))?;
    if !(1. ..=180.).contains(&turn) {
        return Err(geometry("turn", "An arc turns by 1 to 180 degrees."));
    }
    let segments = context.scalar(&definition.segments, &format!("{pointer}/segments"))?;
    if segments.fract() != 0. || !(1. ..=64.).contains(&segments) {
        return Err(geometry("segments", "An arc has 1 to 64 segments."));
    }
    let count = definition.section.len();
    if !(2..=32).contains(&count) || (definition.closed && count < 3) {
        return Err(geometry(
            "section",
            "A section has 2 to 32 points, and at least 3 when it is closed.",
        ));
    }
    let mut section = Vec::with_capacity(count);
    for (index, point) in definition.section.iter().enumerate() {
        let mut resolved = [0.; 2];
        for axis in 0..2 {
            resolved[axis] =
                context.scalar(&point[axis], &format!("{pointer}/section/{index}/{axis}"))?;
        }
        if resolved.iter().any(|v| v.abs() > 10_000.) || radius + resolved[0] < 1e-3 {
            return Err(geometry(
                "section",
                "Section points are bounded and lie outside the arc's axis.",
            ));
        }
        section.push(resolved);
    }
    if definition.closed {
        let area: f64 = (0..count)
            .map(|i| {
                let (a, b) = (section[i], section[(i + 1) % count]);
                a[0] * b[1] - a[1] * b[0]
            })
            .sum();
        if area.abs() < 1e-12 {
            return Err(geometry("section", "A closed section has area."));
        }
        if area < 0. {
            section.reverse();
        }
    }
    let edges = if definition.closed { count } else { count - 1 };
    let shortest = (0..edges)
        .map(|i| {
            let (a, b) = (section[i], section[(i + 1) % count]);
            hypot(b[0] - a[0], b[1] - a[1])
        })
        .fold(f64::INFINITY, f64::min);
    if shortest < 1e-6 {
        return Err(geometry("section", "Section points are distinct."));
    }
    let (bevel, bevel_segments) = context.bevel(
        pointer,
        &definition.bevel,
        &definition.bevel_segments,
        (shortest / 2. - 1e-6).max(0.),
    )?;
    let step = context.scalar(&definition.step, &format!("{pointer}/step"))?;
    if step < 0. || (step > 0. && step < 0.01) {
        return Err(geometry("step", "A step is 0 or at least a centimeter."));
    }
    let anchor = match &definition.pattern_anchor {
        Some(anchor) => [
            context.scalar(&anchor[0], &format!("{pointer}/patternAnchor/0"))?,
            context.scalar(&anchor[1], &format!("{pointer}/patternAnchor/1"))?,
        ],
        None => [0., 0.],
    };
    if definition.rings.len() > 16 {
        return Err(context.fail(
            &format!("{pointer}/rings"),
            "ITEM_LIMIT",
            "An arc has at most 16 rings.",
        ));
    }
    let mut rings = Vec::with_capacity(definition.rings.len());
    for (index, ring) in definition.rings.iter().enumerate() {
        let at = format!("{pointer}/rings/{index}");
        if !super::schema::bounded_name(&ring.name) {
            return Err(context.fail(
                &format!("{at}/name"),
                "ITEM_FEATURE",
                "A ring has a bounded name.",
            ));
        }
        let point = [
            context.scalar(&ring.at[0], &format!("{at}/at/0"))?,
            context.scalar(&ring.at[1], &format!("{at}/at/1"))?,
        ];
        if point.iter().any(|v| v.abs() > 10_000.) || radius + point[0] < 1e-3 {
            return Err(context.fail(
                &format!("{at}/at"),
                "ITEM_FEATURE",
                "A ring is bounded and lies outside the arc's axis.",
            ));
        }
        rings.push((ring.name.clone(), point));
    }
    let recesses = resolve_recesses(context, definition, pointer)?;
    if !recesses.is_empty() && bevel > 0. {
        return Err(geometry("recesses", "A bevelled section has no recesses."));
    }
    let (section, level) = recess(&section, definition.closed, recesses, radius)
        .map_err(|message| geometry("recesses", message))?;
    Ok(Arc {
        radius,
        turn: turn * (std::f64::consts::PI / 180.),
        segments: segments as usize,
        section,
        closed: definition.closed,
        bevel,
        bevel_segments,
        step,
        anchor,
        rings,
        level,
    })
}

/// Every recess as `[from, to, depth]`, its copies expanded and those whose `when` fails left out.
fn resolve_recesses(
    context: &Context<'_>,
    definition: &ArcDefinition,
    pointer: &str,
) -> Result<Vec<[f64; 3]>, ItemDiagnostic> {
    let mut found = Vec::new();
    for (index, recess) in definition.recesses.iter().enumerate() {
        let at = format!("{pointer}/recesses/{index}");
        let copies = match &recess.repeat {
            None => vec![context.derived.clone()],
            Some(axis) => {
                let identifier = !axis.name.is_empty()
                    && axis.name.len() <= 64
                    && axis
                        .name
                        .bytes()
                        .all(|c| c.is_ascii_alphanumeric() || c == b'_')
                    && !axis.name.as_bytes()[0].is_ascii_digit()
                    && !context.definition.parameters.contains_key(&axis.name)
                    && !context.derived.contains_key(&axis.name);
                let count = context.scalar(&axis.count, &format!("{at}/repeat/count"))?;
                if !identifier || count.fract() != 0. || !(0. ..=16.).contains(&count) {
                    return Err(context.fail(
                        &format!("{at}/repeat"),
                        "ITEM_REPEAT",
                        "A recess repeats up to 16 times, indexed by a new identifier.",
                    ));
                }
                (0..count as usize)
                    .map(|copy| {
                        let mut derived = context.derived.clone();
                        derived.insert(axis.name.clone(), copy as f64);
                        derived.insert(format!("{}_count", axis.name), count);
                        derived
                    })
                    .collect()
            }
        };
        for derived in copies {
            let copy = Context {
                definition: context.definition,
                parameters: context.parameters,
                derived: &derived,
            };
            if let Some(condition) = &recess.when {
                if copy.scalar(condition, &format!("{at}/when"))? <= 0. {
                    continue;
                }
            }
            let from = copy.scalar(&recess.from, &format!("{at}/from"))?;
            let to = copy.scalar(&recess.to, &format!("{at}/to"))?;
            let depth = copy.scalar(&recess.depth, &format!("{at}/depth"))?;
            if !(to - from >= 1e-3) || !(depth >= 1e-3) || to.abs().max(from.abs()) > 10_000. {
                return Err(context.fail(
                    &at,
                    "ITEM_GEOMETRY",
                    "A recess rises from `from` to a higher `to` and is at least a millimeter deep.",
                ));
            }
            found.push([from, to, depth]);
        }
    }
    if found.len() > 16 {
        return Err(context.fail(
            &format!("{pointer}/recesses"),
            "ITEM_LIMIT",
            "An arc has at most 16 recesses, counting every copy.",
        ));
    }
    Ok(found)
}

/// The section with its recesses cut into its upright outer faces, and the edges that run
/// across their floors and heads. A recess lies inside one such face, at least a millimeter
/// from its ends and from the next recess, except where it reaches an open section's end: the
/// section then starts (or ends) at the back of the recess.
fn recess(
    section: &[P2],
    closed: bool,
    mut recesses: Vec<[f64; 3]>,
    radius: f64,
) -> Result<(Vec<P2>, Vec<usize>), &'static str> {
    if recesses.is_empty() {
        return Ok((section.to_vec(), Vec::new()));
    }
    recesses.sort_by(|a, b| a[0].total_cmp(&b[0]));
    let count = section.len();
    let edges = if closed { count } else { count - 1 };
    let upright = |i: usize| {
        let (a, b) = (section[i], section[(i + 1) % count]);
        (a[0] - b[0]).abs() < 1e-9 && b[1] > a[1]
    };
    let mut used = vec![false; recesses.len()];
    let (mut points, mut level) = (Vec::new(), Vec::new());
    for i in 0..count {
        let a = section[i];
        let first = i == 0 && !closed;
        if i >= edges || !upright(i) {
            points.push(a);
            continue;
        }
        let b = section[(i + 1) % count];
        let last = !closed && i + 1 == count - 1;
        let mut floor = a[1];
        let mut start = true;
        for (k, &[from, to, depth]) in recesses.iter().enumerate() {
            if used[k] || from < a[1] - 1e-9 || to > b[1] + 1e-9 {
                continue;
            }
            let (at_start, at_end) = (from < a[1] + 1e-9, to > b[1] - 1e-9);
            if (at_start && (!first || !start))
                || (at_end && !last)
                || (!at_start && from < floor + 1e-3)
                || radius + a[0] - depth < 1e-3
            {
                return Err(
                    "Recesses lie apart inside an upright outer face, or reach an open section's end, and keep outside the arc's axis.",
                );
            }
            used[k] = true;
            let back = a[0] - depth;
            if at_start {
                // The open section starts at the back of the recess.
                points.push([back, from]);
            } else {
                if start {
                    points.push(a);
                }
                points.push([a[0], from]);
                level.push(points.len() - 1);
                points.push([back, from]);
            }
            start = false;
            points.push([back, to]);
            if at_end {
                // ... or ends there, without its head.
                floor = f64::INFINITY;
            } else {
                level.push(points.len() - 1);
                points.push([a[0], to]);
                floor = to;
            }
        }
        if start {
            points.push(a);
        } else if floor.is_infinite() {
            // The section's last point was the recess's back: skip `b`.
            for point in section.iter().skip(i + 2) {
                points.push(*point);
            }
            return finish(points, level, &used);
        }
    }
    finish(points, level, &used)
}

fn finish(
    points: Vec<P2>,
    level: Vec<usize>,
    used: &[bool],
) -> Result<(Vec<P2>, Vec<usize>), &'static str> {
    if used.iter().any(|used| !used) {
        return Err("Every recess lies on one upright outer face of the section.");
    }
    Ok((points, level))
}

fn unit(v: P2) -> P2 {
    let size = hypot(v[0], v[1]);
    [v[0] / size, v[1] / size]
}

impl Arc {
    /// The swept profile: the section with its corners rounded and its long edges split.
    fn profile(&self) -> Vec<Piece> {
        let count = self.section.len();
        let edges = if self.closed { count } else { count - 1 };
        // The visible side is on the right of the way a section runs.
        let normal = |a: P2, b: P2| unit([b[1] - a[1], a[0] - b[0]]);
        // What rounding leaves of each corner: where it starts and ends, and its middle.
        let corners: Vec<Option<(P2, P2, P2)>> = (0..count)
            .map(|i| {
                if self.bevel <= 0. || (!self.closed && (i == 0 || i == count - 1)) {
                    return None;
                }
                let point = self.section[i];
                let before = self.section[(i + count - 1) % count];
                let after = self.section[(i + 1) % count];
                let back = unit([before[0] - point[0], before[1] - point[1]]);
                let onward = unit([after[0] - point[0], after[1] - point[1]]);
                if (back[0] * onward[1] - back[1] * onward[0]).abs() < 1e-9 {
                    return None;
                }
                let a = [
                    point[0] + back[0] * self.bevel,
                    point[1] + back[1] * self.bevel,
                ];
                let b = [
                    point[0] + onward[0] * self.bevel,
                    point[1] + onward[1] * self.bevel,
                ];
                // The middle of the round that touches both edges at `a` and `b`, by the half
                // angle between them: only square roots, so every platform agrees.
                let cosine = back[0] * onward[0] + back[1] * onward[1];
                let half_cos = ((1. + cosine) / 2.).max(0.).sqrt();
                let half_sin = ((1. - cosine) / 2.).max(0.).sqrt();
                let toward = unit([back[0] + onward[0], back[1] + onward[1]]);
                let reach = self.bevel * (1. - half_sin) / half_cos;
                Some((
                    a,
                    b,
                    [point[0] + toward[0] * reach, point[1] + toward[1] * reach],
                ))
            })
            .collect();
        let mut pieces = Vec::new();
        let mut along = 0.;
        let mut push = |pieces: &mut Vec<Piece>, a: P2, b: P2, na: P2, nb: P2, measured: bool| {
            let length = hypot(b[0] - a[0], b[1] - a[1]);
            if length < 1e-12 {
                return;
            }
            // A recess's floor and head keep `v` where it is.
            let length = if measured { length } else { 0. };
            // Flat faces longer than the step are split evenly.
            let rows = if self.step > 0. && na == nb {
                (length / self.step - 1e-9).ceil().max(1.) as usize
            } else {
                1
            };
            for row in 0..rows {
                let at = |k: usize| {
                    if k == 0 {
                        a
                    } else if k == rows {
                        b
                    } else {
                        let t = k as f64 / rows as f64;
                        [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]
                    }
                };
                pieces.push(Piece {
                    a: at(row),
                    b: at(row + 1),
                    na,
                    nb,
                    along: [
                        along + length * row as f64 / rows as f64,
                        along + length * (row + 1) as f64 / rows as f64,
                    ],
                });
            }
            along += length;
        };
        for i in 0..edges {
            let j = (i + 1) % count;
            let face = normal(self.section[i], self.section[j]);
            let from = corners[i].map_or(self.section[i], |corner| corner.1);
            let to = corners[j].map_or(self.section[j], |corner| corner.0);
            push(&mut pieces, from, to, face, face, !self.level.contains(&i));
            if let Some((a, b, middle)) = corners[j] {
                let next = normal(self.section[j], self.section[(j + 1) % count]);
                if self.bevel_segments >= 2 {
                    let between = unit([face[0] + next[0], face[1] + next[1]]);
                    push(&mut pieces, a, middle, face, between, true);
                    push(&mut pieces, middle, b, between, next, true);
                } else {
                    push(&mut pieces, a, b, face, next, true);
                }
            }
        }
        pieces
    }

    /// The angle of each step round the arc, with its sine and cosine.
    fn steps(&self) -> Vec<[f64; 2]> {
        (0..=self.segments)
            .map(|k| {
                let angle = self.turn * k as f64 / self.segments as f64;
                [sin(angle), cos(angle)]
            })
            .collect()
    }
    /// The point `[out, up]` of the section at a step of the arc.
    fn at(&self, [s, c]: [f64; 2], [out, up]: P2) -> V3 {
        let reach = self.radius + out;
        [reach * s, self.radius - reach * c, up]
    }
    /// The length of one chord of the arc of `radius`.
    fn chord(&self) -> f64 {
        2. * self.radius * sin(self.turn / (2. * self.segments as f64))
    }

    pub(super) fn build(&self, surfaces: &mut Surfaces<'_>) -> Result<(), String> {
        let steps = self.steps();
        let chord = self.chord();
        let pieces = self.profile();
        let v0 = self.section[0][1] - self.anchor[1];
        let mut positions: Vec<V3> = Vec::new();
        let mut normals: Vec<V3> = Vec::new();
        let mut uvs: Vec<P2> = Vec::new();
        let mut triangles: Vec<[usize; 3]> = Vec::new();
        // Vertices equal in every attribute are named once, so the surface is one mesh.
        let mut named: BTreeMap<[u64; 8], usize> = BTreeMap::new();
        let mut vertex = |position: V3, normal: V3, uv: P2| {
            let clean = |v: f64| if v == 0. { 0. } else { v };
            let key = [
                position[0],
                position[1],
                position[2],
                normal[0],
                normal[1],
                normal[2],
                uv[0],
                uv[1],
            ]
            .map(|v| clean(v).to_bits());
            *named.entry(key).or_insert_with(|| {
                positions.push(position);
                normals.push(normal);
                uvs.push(uv);
                positions.len() - 1
            })
        };
        for piece in &pieces {
            let mut previous: Option<[usize; 2]> = None;
            for (k, step) in steps.iter().enumerate() {
                let [s, c] = *step;
                let turned = |n: P2| [n[0] * s, -n[0] * c, n[1]];
                let u = k as f64 * chord - self.anchor[0];
                let pair = [
                    vertex(
                        self.at(*step, piece.a),
                        turned(piece.na),
                        [u, v0 + piece.along[0]],
                    ),
                    vertex(
                        self.at(*step, piece.b),
                        turned(piece.nb),
                        [u, v0 + piece.along[1]],
                    ),
                ];
                if let Some(before) = previous {
                    // Seen from outside the section runs to the left of the way the arc turns.
                    triangles.push([before[0], pair[0], pair[1]]);
                    triangles.push([before[0], pair[1], before[1]]);
                }
                previous = Some(pair);
            }
        }
        surfaces.indexed("side", &positions, &normals, &uvs, &triangles, false)?;
        if self.closed {
            let outline: Vec<P2> = pieces.iter().map(|piece| piece.a).collect();
            let ring: Vec<V3> = outline.iter().map(|p| [p[0], p[1], 0.]).collect();
            let profile = triangulate(vec![ring], &[], &[])?;
            let points: Vec<P2> = profile.points.iter().map(|p| [p[0], p[1]]).collect();
            for (step, facing) in [(steps[0], -1.), (steps[self.segments], 1.)] {
                let [s, c] = step;
                surfaces.indexed(
                    "end",
                    &points.iter().map(|p| self.at(step, *p)).collect::<Vec<_>>(),
                    &vec![[c * facing, s * facing, 0.]; points.len()],
                    &points,
                    &profile.triangles,
                    // The section is counterclockwise seen from before the arc's start.
                    facing > 0.,
                )?;
            }
        }
        Ok(())
    }

    /// `base` and `top` (the arc at the section's lowest and highest points) and the named
    /// rings, as polylines; `start` and `end`, the section where the arc begins and ends (where
    /// it meets what stands before and after it); and the pattern anchor, in part coordinates.
    pub(super) fn features(&self, part: &str) -> (Vec<Feature>, PatternAnchor) {
        let steps = self.steps();
        let [s, c] = {
            let half = self.turn / 2.;
            [sin(half), cos(half)]
        };
        let lowest = self
            .section
            .iter()
            .copied()
            .reduce(|a, b| if b[1] < a[1] { b } else { a })
            .unwrap_or_default();
        let highest = self
            .section
            .iter()
            .copied()
            .reduce(|a, b| if b[1] > a[1] { b } else { a })
            .unwrap_or_default();
        let mut features: Vec<Feature> = [("base".to_owned(), lowest), ("top".to_owned(), highest)]
            .into_iter()
            .chain(self.rings.iter().cloned())
            .map(|(name, point)| Feature {
                name,
                part: part.into(),
                source: None,
                face: "side".into(),
                points: steps.iter().map(|step| self.at(*step, point)).collect(),
                closed: false,
                normal: [s, -c, 0.],
                width: 0.,
                reach: Default::default(),
                within: 0.,
                zone: None,
            })
            .collect();
        for (name, step, facing) in [("start", steps[0], -1.), ("end", steps[self.segments], 1.)] {
            features.push(Feature {
                name: name.into(),
                part: part.into(),
                source: None,
                face: "side".into(),
                points: self
                    .section
                    .iter()
                    .map(|point| self.at(step, *point))
                    .collect(),
                closed: self.closed,
                normal: [step[1] * facing, step[0] * facing, 0.],
                width: 0.,
                reach: Default::default(),
                within: 0.,
                zone: None,
            });
        }
        (
            features,
            PatternAnchor {
                part: part.into(),
                origin: [self.anchor[0], 0., self.anchor[1]],
                u: [1., 0., 0.],
                v: [0., 0., 1.],
                normal: [0., -1., 0.],
            },
        )
    }
}

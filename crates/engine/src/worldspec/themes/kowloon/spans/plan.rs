//! Signs, washing, then utility cables. Owned obstruction reservations are discarded on failure.
use super::{
    anchors::{Anchor, Plane, Rect},
    catenary::hanging,
    neighbors::Index,
    walking::Walking,
};
use crate::worldspec::themes::kowloon::{laundry, palette::srgb};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use superspec_geometry::obstructions::{Budget, Obstructions};
use superspec_geometry::{V3, math::hypot};
use superspec_types::rng::SeededRng;
pub const RACK_STANDOFF: f64 = 0.11;
pub const RACK_SLOTS: [f64; 9] = [0., 0.065, -0.065, 0.13, -0.13, 0.195, -0.195, 0.26, -0.26];
pub const LINE_STANDOFF: f64 = 0.09;
pub const LINE_DROP: f64 = 0.14;
pub const TRUSS_DEPTH: f64 = 0.22;
pub const CONDUIT_BOX: f64 = 0.22;
pub const CONDUIT_STANDOFF: f64 = 0.03;
#[derive(Clone, Copy, Deserialize)]
pub struct Rule {
    pub cables: f64,
    pub clotheslines: f64,
    pub signs: f64,
    pub clearance: f64,
    pub reach: f64,
}
#[derive(Serialize)]
pub struct Strand {
    pub slots: [usize; 2],
    pub radius: f64,
    pub tint: V3,
    pub points: Vec<V3>,
}
#[derive(Serialize)]
pub struct Cable {
    pub from: usize,
    pub to: usize,
    pub strands: Vec<Strand>,
}
pub struct Hung {
    pub garment: laundry::Garment,
    pub left: V3,
    pub right: V3,
    pub sway: f64,
}
impl Serialize for Hung {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        serde_json::json!({"garment":{"width":self.garment.width(),"drop":self.garment.drop()},"left":self.left,"right":self.right,"sway":self.sway}).serialize(s)
    }
}
#[derive(Serialize)]
pub struct Clothesline {
    pub from: usize,
    pub to: usize,
    pub upper: Vec<V3>,
    pub lower: Vec<V3>,
    pub garments: Vec<Hung>,
}
#[derive(Serialize)]
pub struct Panel {
    pub s0: f64,
    pub s1: f64,
    pub z0: f64,
    pub z1: f64,
}
#[derive(Clone, Copy, Serialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum SignKind {
    Banner,
    Gantry,
}
#[derive(Serialize)]
pub struct Sign {
    pub kind: SignKind,
    pub from: usize,
    pub to: usize,
    pub start: V3,
    pub direction: V3,
    pub length: f64,
    pub top: f64,
    pub mount: f64,
    pub panels: Vec<Panel>,
}
impl Sign {
    pub fn point(&self, s: f64, z: f64) -> V3 {
        [
            self.start[0] + self.direction[0] * s,
            self.start[1] + self.direction[1] * s,
            z,
        ]
    }
}
#[derive(Serialize)]
pub struct Rack {
    pub anchor: usize,
    pub slots: usize,
    pub conduit: Option<f64>,
}
#[derive(Serialize)]
pub struct Plan {
    pub cables: Vec<Cable>,
    pub racks: Vec<Rack>,
    pub clotheslines: Vec<Clothesline>,
    pub signs: Vec<Sign>,
}
pub fn in_front(a: &Anchor, planes: &[Plane], out: f64, up: f64) -> V3 {
    let n = planes[a.plane].normal;
    [
        a.position[0] + n[0] * out,
        a.position[1] + n[1] * out,
        a.position[2] + up,
    ]
}
fn distance(a: V3, b: V3) -> f64 {
    hypot(b[0] - a[0], b[1] - a[1])
}
fn weighted(rng: &mut SeededRng, entries: &[(usize, f64)]) -> usize {
    let mut roll = rng.next_f64() * entries.iter().map(|e| e.1).sum::<f64>();
    for &(id, w) in entries {
        roll -= w;
        if roll <= 0. {
            return id;
        }
    }
    entries.last().unwrap().0
}
fn shuffle(ids: &mut [usize], rng: &mut SeededRng) {
    for i in (1..ids.len()).rev() {
        let j = rng.integer(0, i);
        ids.swap(i, j);
    }
}
struct Planning<'a, 'b, 'c> {
    rule: Rule,
    seed: &'a str,
    planes: &'a [Plane],
    anchors: &'a [Anchor],
    floors: &'a Walking,
    obstacles: Obstructions,
    budget: &'b mut Budget<'c>,
    taken: BTreeSet<usize>,
}
impl Planning<'_, '_, '_> {
    fn front(&self, id: usize, out: f64, up: f64) -> V3 {
        in_front(&self.anchors[id], self.planes, out, up)
    }
    fn facing(&self, a: usize, b: usize, minimum: f64) -> bool {
        let a = &self.anchors[a];
        let b = &self.anchors[b];
        let length = distance(a.position, b.position);
        if length < 1e-6 {
            return false;
        }
        let dx = (b.position[0] - a.position[0]) / length;
        let dy = (b.position[1] - a.position[1]) / length;
        let na = self.planes[a.plane].normal;
        let nb = self.planes[b.plane].normal;
        dx * na[0] + dy * na[1] >= minimum && -(dx * nb[0] + dy * nb[1]) >= minimum
    }
    fn clears(&mut self, points: &[V3], clearance: f64) -> Result<bool, String> {
        if points.len() == 1 {
            return self.floors.clears(points[0], clearance, self.budget);
        }
        for (i, pair) in points.windows(2).enumerate() {
            let [a, b] = [pair[0], pair[1]];
            let steps = (distance(a, b) / 0.25).ceil().max(1.) as usize;
            self.budget.spend(steps)?;
            for step in usize::from(i > 0)..=steps {
                let t = step as f64 / steps as f64;
                let at = std::array::from_fn(|k| a[k] + (b[k] - a[k]) * t);
                if !self.floors.clears(at, clearance, self.budget)? {
                    return Ok(false);
                }
            }
        }
        Ok(true)
    }
    fn passes(&mut self, points: &[V3]) -> Result<bool, String> {
        Ok(self.clears(points, self.rule.clearance)?
            && !self.obstacles.blocked_path(points, self.budget)?)
    }
    fn hang_sign(
        &mut self,
        from: usize,
        to: usize,
        rng: &mut SeededRng,
    ) -> Result<Option<Sign>, String> {
        let top = self.anchors[from].z.min(self.anchors[to].z);
        let start = self.front(from, LINE_STANDOFF, top - self.anchors[from].z);
        let end = self.front(to, LINE_STANDOFF, top - self.anchors[to].z);
        let length = distance(start, end);
        let direction = [
            (end[0] - start[0]) / length,
            (end[1] - start[1]) / length,
            0.,
        ];
        let mut sign = Sign {
            kind: SignKind::Banner,
            from,
            to,
            start,
            direction,
            length,
            top,
            mount: TRUSS_DEPTH,
            panels: vec![],
        };
        if length <= 7. && rng.chance(0.5) {
            sign.kind = SignKind::Gantry;
            let hung = top - TRUSS_DEPTH - 0.12;
            if length >= 3.2 && rng.chance(0.5) {
                let width = (4. * (length * 0.15).clamp(0.45, 0.85)).min(length - 0.7);
                let s0 = (length - width) / 2.;
                sign.panels.push(Panel {
                    s0,
                    s1: s0 + width,
                    z0: hung - width / 4.,
                    z1: hung,
                });
            } else {
                let count = (length / 1.4).floor().clamp(1., 3.) as usize;
                let width = rng.float(0.4, 0.55);
                for i in 0..count {
                    let center = length * (i + 1) as f64 / (count + 1) as f64;
                    sign.panels.push(Panel {
                        s0: center - width / 2.,
                        s1: center + width / 2.,
                        z0: hung - width * 4.,
                        z1: hung,
                    });
                }
            }
        } else {
            let width = (4. * (length * 0.14).clamp(0.5, 1.15)).min(length - 0.9);
            if width < 1.6 {
                return Ok(None);
            }
            let s0 = (length - width) / 2. + rng.float(-0.2, 0.2) * (length - width - 0.6);
            sign.panels.push(Panel {
                s0,
                s1: s0 + width,
                z0: top - 0.14 - width / 4.,
                z1: top - 0.14,
            });
            sign.mount = width / 4. + 0.09;
        }
        for id in [from, to] {
            let a = &self.anchors[id];
            if !self.planes[a.plane].covers(
                Rect {
                    u0: a.u - 0.08,
                    u1: a.u + 0.08,
                    z0: top - sign.mount - 0.08,
                    z1: top + 0.08,
                },
                self.budget,
            )? {
                return Ok(None);
            }
        }
        let overhead = sign.point(
            length / 2.,
            sign.panels
                .iter()
                .map(|p| p.z0)
                .fold(f64::INFINITY, f64::min)
                - 0.05,
        );
        let Some(street) = self.floors.below(overhead, self.budget)? else {
            return Ok(None);
        };
        if top - street > 7.5
            || self.obstacles.blocked(
                overhead,
                [overhead[0], overhead[1], street + 0.1],
                self.budget,
            )?
            || self.obstacles.blocked(start, end, self.budget)?
        {
            return Ok(None);
        }
        for p in &sign.panels {
            let corners = [
                sign.point(p.s0, p.z0),
                sign.point(p.s1, p.z0),
                sign.point(p.s1, p.z1),
                sign.point(p.s0, p.z1),
            ];
            if !self.passes(&[
                corners[0], corners[1], corners[2], corners[3], corners[0], corners[2],
            ])? || self
                .obstacles
                .blocked(corners[1], corners[3], self.budget)?
                || self.obstacles.pierced(corners, self.budget)?
            {
                return Ok(None);
            }
        }
        Ok(Some(sign))
    }
    fn signs(&mut self, utility: &[usize]) -> Result<Vec<Sign>, String> {
        if self.rule.signs <= 0. || utility.is_empty() {
            return Ok(vec![]);
        }
        let mut rng = SeededRng::from_text(&format!("{}/signs", self.seed));
        let target = (self.rule.signs * utility.len() as f64 * 0.02)
            .round()
            .max(1.) as usize;
        let index = Index::new(
            &utility
                .iter()
                .map(|&i| self.anchors[i].position)
                .collect::<Vec<_>>(),
            self.rule.reach.min(12.),
            0.3,
            self.budget,
        )?;
        let mut order: Vec<_> = (0..utility.len()).collect();
        shuffle(&mut order, &mut rng);
        let mut signs: Vec<Sign> = vec![];
        for from_index in order {
            self.budget.spend(1)?;
            if signs.len() >= target {
                break;
            }
            let from = utility[from_index];
            if self.taken.contains(&from) {
                continue;
            }
            let partners = index
                .near(from_index, self.budget)?
                .into_iter()
                .map(|j| utility[j])
                .filter(|&to| {
                    !self.taken.contains(&to)
                        && self.anchors[to].plane != self.anchors[from].plane
                        && distance(self.anchors[from].position, self.anchors[to].position) >= 3.
                        && (self.anchors[to].z - self.anchors[from].z).abs() <= 0.3
                        && self.facing(from, to, 0.95)
                })
                .collect::<Vec<_>>();
            if partners.is_empty() {
                continue;
            }
            let to = rng.pick(&partners);
            let Some(sign) = self.hang_sign(from, to, &mut rng)? else {
                continue;
            };
            let here = sign.point(sign.length / 2., sign.top);
            if signs.iter().any(|s| {
                let there = s.point(s.length / 2., s.top);
                hypot(distance(here, there), here[2] - there[2]) < 5.
            }) {
                continue;
            }
            self.taken.insert(from);
            self.taken.insert(to);
            for p in &sign.panels {
                self.obstacles.reserve([
                    sign.point(p.s0, p.z0),
                    sign.point(p.s1, p.z0),
                    sign.point(p.s1, p.z1),
                    sign.point(p.s0, p.z1),
                ])?;
            }
            signs.push(sign);
        }
        Ok(signs)
    }
    fn hang_line(
        &mut self,
        from: usize,
        to: usize,
        rng: &mut SeededRng,
    ) -> Result<Option<Clothesline>, String> {
        let upper = hanging(
            self.front(from, LINE_STANDOFF, 0.),
            self.front(to, LINE_STANDOFF, 0.),
            0.003,
            8,
            self.budget,
        )?;
        let lower = hanging(
            self.front(from, LINE_STANDOFF, -LINE_DROP),
            self.front(to, LINE_STANDOFF, -LINE_DROP),
            rng.float(0.015, 0.035),
            16,
            self.budget,
        )?;
        if !self.passes(&lower)? || self.obstacles.blocked_path(&upper, self.budget)? {
            return Ok(None);
        }
        let length = distance(lower[0], *lower.last().unwrap());
        let mut garments = vec![];
        let mut along = 0.4 + rng.float(0., 0.3);
        while along < length - 0.75 {
            self.budget.spend(1)?;
            let Some(garment) = laundry::choose(
                rng,
                laundry::Room {
                    width: length - 0.4 - along,
                    drop: 1.15,
                },
                laundry::Support::Line,
            )?
            else {
                break;
            };
            let left = along_curve(&lower, along, length);
            let right = along_curve(&lower, along + garment.width(), length);
            let sway = rng.float(-0.05, 0.05);
            let hem = [left, right].map(|p| [p[0], p[1], p[2] - garment.drop()]);
            along += garment.width()
                + if rng.chance(0.15) {
                    rng.float(0.5, 1.1)
                } else {
                    rng.float(0.05, 0.22)
                };
            if !self.passes(&[left, hem[0], hem[1], right])?
                || self
                    .obstacles
                    .pierced([left, right, hem[1], hem[0]], self.budget)?
            {
                continue;
            }
            garments.push(Hung {
                garment,
                left,
                right,
                sway,
            });
        }
        Ok((garments.len() >= 2).then_some(Clothesline {
            from,
            to,
            upper,
            lower,
            garments,
        }))
    }
    fn clotheslines(
        &mut self,
        windows: &[usize],
        utility: &[usize],
    ) -> Result<Vec<Clothesline>, String> {
        if self.rule.clotheslines <= 0. || windows.is_empty() {
            return Ok(vec![]);
        }
        let mut rng = SeededRng::from_text(&format!("{}/clotheslines", self.seed));
        let target = (self.rule.clotheslines * windows.len() as f64 * 0.4)
            .round()
            .max(1.) as usize;
        let all: Vec<_> = windows.iter().chain(utility).copied().collect();
        let index = Index::new(
            &all.iter()
                .map(|&i| self.anchors[i].position)
                .collect::<Vec<_>>(),
            self.rule.reach.min(9.),
            0.45,
            self.budget,
        )?;
        let mut order: Vec<_> = (0..windows.len()).collect();
        shuffle(&mut order, &mut rng);
        let mut lines = vec![];
        for from_index in order {
            self.budget.spend(1)?;
            if lines.len() >= target {
                break;
            }
            let from = all[from_index];
            if self.taken.contains(&from) {
                continue;
            }
            let mut partners = vec![];
            for j in index.near(from_index, self.budget)? {
                let to = all[j];
                let a = &self.anchors[from];
                let b = &self.anchors[to];
                let distance = distance(a.position, b.position);
                if !self.taken.contains(&to)
                    && b.plane != a.plane
                    && distance >= 1.8
                    && (b.z - a.z).abs() <= 0.45
                    && self.facing(from, to, 0.82)
                {
                    partners.push((
                        to,
                        (if b.window.is_some() { 3. } else { 1. }) * (-distance / 5.).exp(),
                    ));
                }
            }
            if partners.is_empty() {
                continue;
            }
            let to = weighted(&mut rng, &partners);
            let Some(line) = self.hang_line(from, to, &mut rng)? else {
                continue;
            };
            self.taken.insert(from);
            self.taken.insert(to);
            for g in &line.garments {
                self.obstacles.reserve([
                    g.left,
                    g.right,
                    [g.right[0], g.right[1], g.right[2] - g.garment.drop()],
                    [g.left[0], g.left[1], g.left[2] - g.garment.drop()],
                ])?;
            }
            lines.push(line);
        }
        Ok(lines)
    }
    fn cables(&mut self, utility: &[usize]) -> Result<Vec<Cable>, String> {
        let free = utility
            .iter()
            .copied()
            .filter(|i| !self.taken.contains(i))
            .collect::<Vec<_>>();
        if self.rule.cables <= 0. || free.is_empty() {
            return Ok(vec![]);
        }
        let mut rng = SeededRng::from_text(&format!("{}/cables", self.seed));
        let target = (self.rule.cables * free.len() as f64 * 0.22)
            .round()
            .max(1.) as usize;
        let index = Index::new(
            &free
                .iter()
                .map(|&i| self.anchors[i].position)
                .collect::<Vec<_>>(),
            self.rule.reach,
            self.rule.reach * 0.35,
            self.budget,
        )?;
        let mut total = 0.;
        let cumulative = free
            .iter()
            .map(|&i| {
                let h = self.anchors[i].height;
                total += if h < 9. { 1. } else { (-(h - 9.) / 6.).exp() };
                total
            })
            .collect::<Vec<_>>();
        let mut used = vec![0usize; free.len()];
        let mut hubs = vec![];
        let mut linked = BTreeSet::new();
        let mut bundles = vec![];
        let mut run: Option<(usize, f64, usize)> = None;
        let colors = [
            "#1b1b1b", "#1b1b1b", "#242424", "#242424", "#2c2925", "#33332f", "#3d3a35", "#4a4640",
            "#5a5852", "#6a2f26", "#b8b4a6",
        ]
        .map(srgb);
        for _ in 0..target * 16 {
            self.budget.spend(1)?;
            if bundles.len() >= target {
                break;
            }
            let from = if let Some((at, _, _)) = run {
                at
            } else if !hubs.is_empty() && rng.chance(0.4) {
                rng.pick(&hubs)
            } else {
                let roll = rng.next_f64() * total;
                let (mut low, mut high) = (0, free.len() - 1);
                while low < high {
                    let middle = (low + high) / 2;
                    if roll <= cumulative[middle] {
                        high = middle;
                    } else {
                        low = middle + 1;
                    }
                }
                low
            };
            let heading = run.map_or(0., |r| r.1);
            let wanted = if let Some((_, _, strands)) = run {
                (strands as i32 + rng.integer(0, 2) as i32 - 1).clamp(1, 6) as usize
            } else {
                weighted(&mut rng, &[(1, 5.), (2, 3.), (3, 1.5), (4, 0.8), (6, 0.3)])
            };
            run = None;
            let room = RACK_SLOTS.len() - used[from];
            if room == 0 {
                continue;
            }
            let a = &self.anchors[free[from]];
            let mut partners = vec![];
            for to in index.near(from, self.budget)? {
                let b = &self.anchors[free[to]];
                let d = distance(a.position, b.position);
                let rise = (b.z - a.z).abs();
                if a.plane == b.plane
                    || d < 1.5
                    || rise > d * 0.35
                    || used[to] >= RACK_SLOTS.len()
                    || linked.contains(&(from.min(to), from.max(to)))
                    || !self.facing(free[from], free[to], 0.3)
                {
                    continue;
                }
                let tangent = self.planes[a.plane].tangent;
                let along = ((b.position[0] - a.position[0]) * tangent[0]
                    + (b.position[1] - a.position[1]) * tangent[1])
                    * heading;
                let onward = if heading == 0. {
                    1.
                } else if along > 0.3 {
                    2.5
                } else {
                    0.3
                };
                partners.push((
                    to,
                    (-d / 5.5 - rise / 1.2).exp() * if used[to] > 0 { 2.5 } else { 1. } * onward,
                ));
            }
            if partners.is_empty() {
                continue;
            }
            let to = weighted(&mut rng, &partners);
            let count = wanted.min(room).min(RACK_SLOTS.len() - used[to]);
            let trunk = count >= 3 || rng.chance(0.25);
            let gauge = if trunk {
                rng.float(0.011, 0.017)
            } else {
                rng.float(0.006, 0.01)
            };
            let slack = 0.008 + 0.05 * rng.next_f64().powi(2);
            let pieces = (6. + slack * 120.).round() as usize;
            let tint = rng.pick(&colors);
            let mut strands = vec![];
            for i in 0..count {
                let slots = [used[from] + i, used[to] + i];
                let radius = gauge * rng.float(0.8, 1.15);
                let tint = if rng.chance(0.7) {
                    tint
                } else {
                    rng.pick(&colors)
                };
                let points = hanging(
                    self.front(free[from], RACK_STANDOFF, RACK_SLOTS[slots[0]]),
                    self.front(free[to], RACK_STANDOFF, RACK_SLOTS[slots[1]]),
                    slack * rng.float(0.75, 1.4),
                    pieces,
                    self.budget,
                )?;
                strands.push(Strand {
                    slots,
                    radius,
                    tint,
                    points,
                });
            }
            let mut clear = true;
            for strand in &strands {
                if !self.passes(&strand.points)? {
                    clear = false;
                    break;
                }
            }
            if !clear {
                continue;
            }
            bundles.push(Cable {
                from: free[from],
                to: free[to],
                strands,
            });
            for id in [from, to] {
                if used[id] == 0 {
                    hubs.push(id);
                }
                used[id] += count;
            }
            linked.insert((from.min(to), from.max(to)));
            if rng.chance(0.55) {
                let b = &self.anchors[free[to]];
                let tangent = self.planes[b.plane].tangent;
                let traveled = (b.position[0] - a.position[0]) * tangent[0]
                    + (b.position[1] - a.position[1]) * tangent[1];
                run = Some((
                    to,
                    if traveled == 0. {
                        0.
                    } else {
                        traveled.signum()
                    },
                    count,
                ));
            }
        }
        Ok(bundles)
    }
    fn racks(&mut self, cables: &[Cable]) -> Result<Vec<Rack>, String> {
        let mut slots: Vec<(usize, usize)> = vec![];
        for cable in cables {
            for strand in &cable.strands {
                for (anchor, count) in [
                    (cable.from, strand.slots[0] + 1),
                    (cable.to, strand.slots[1] + 1),
                ] {
                    self.budget.spend(slots.len() + 1)?;
                    if let Some(s) = slots.iter_mut().find(|s| s.0 == anchor) {
                        s.1 = s.1.max(count);
                    } else {
                        slots.push((anchor, count));
                    }
                }
            }
        }
        let mut result = vec![];
        for (index, (anchor, count)) in slots.into_iter().enumerate() {
            let mut rack = Rack {
                anchor,
                slots: count,
                conduit: None,
            };
            if count >= 2
                && !SeededRng::from_text(&format!("{}/conduit/{index}", self.seed)).chance(0.4)
            {
                let a = &self.anchors[anchor];
                let plane = &self.planes[a.plane];
                let top = a.z
                    + RACK_SLOTS[..count]
                        .iter()
                        .copied()
                        .fold(f64::INFINITY, f64::min)
                    - 0.07;
                for length in [2.1, 1.4, 0.8] {
                    let foot = top - length - CONDUIT_BOX;
                    if !plane.covers(
                        Rect {
                            u0: a.u - 0.1,
                            u1: a.u + 0.1,
                            z0: foot - 0.08,
                            z1: top,
                        },
                        self.budget,
                    )? || self.obstacles.blocked(
                        plane.point(a.u, top, CONDUIT_STANDOFF),
                        plane.point(a.u, foot, CONDUIT_STANDOFF),
                        self.budget,
                    )? {
                        continue;
                    }
                    let mut level = foot;
                    let mut clear = true;
                    while level < top {
                        if self.obstacles.blocked(
                            plane.point(a.u - 0.09, level, CONDUIT_STANDOFF),
                            plane.point(a.u + 0.09, level, CONDUIT_STANDOFF),
                            self.budget,
                        )? {
                            clear = false;
                            break;
                        }
                        level += 0.25;
                    }
                    if clear {
                        rack.conduit = Some(length);
                        break;
                    }
                }
            }
            result.push(rack);
        }
        Ok(result)
    }
}
fn along_curve(curve: &[V3], distance: f64, length: f64) -> V3 {
    let position =
        (distance / length * (curve.len() - 1) as f64).clamp(0., (curve.len() - 1) as f64);
    let index = (position.floor() as usize).min(curve.len() - 2);
    let t = position - index as f64;
    std::array::from_fn(|axis| {
        curve[index][axis] + (curve[index + 1][axis] - curve[index][axis]) * t
    })
}
/// Returned anchor indices refer to utilities first, followed by window fixings.
#[allow(clippy::too_many_arguments)]
pub fn build(
    rule: Rule,
    seed: &str,
    planes: &[Plane],
    utility: &[Anchor],
    windows: &[Anchor],
    floors: &Walking,
    obstacles: Obstructions,
    budget: &mut Budget<'_>,
) -> Result<Plan, String> {
    budget.spend(1)?;
    if seed.len() > 8192
        || utility.len() + windows.len() > 65_536
        || planes.len() > 8192
        || [rule.cables, rule.clotheslines, rule.signs]
            .iter()
            .any(|v| !v.is_finite() || !(0. ..=1.).contains(v))
        || !rule.clearance.is_finite()
        || !(2. ..=10.).contains(&rule.clearance)
        || !rule.reach.is_finite()
        || !(2. ..=40.).contains(&rule.reach)
    {
        return Err("Span plan exceeds rule, identity or anchor allowances.".into());
    }
    let anchors: Vec<_> = utility.iter().chain(windows).cloned().collect();
    if anchors.iter().any(|a| {
        a.plane >= planes.len()
            || a.position
                .iter()
                .chain([a.u, a.z, a.height].iter())
                .any(|v| !v.is_finite() || v.abs() > 999_998.)
            || (a.z - a.position[2]).abs() > 1e-8
    }) {
        return Err("Span anchors require finite positions on known planes.".into());
    }
    let utility: Vec<_> = (0..utility.len()).collect();
    let windows: Vec<_> = (utility.len()..anchors.len()).collect();
    let mut p = Planning {
        rule,
        seed,
        planes,
        anchors: &anchors,
        floors,
        obstacles,
        budget,
        taken: BTreeSet::new(),
    };
    let signs = p.signs(&utility)?;
    let clotheslines = p.clotheslines(&windows, &utility)?;
    let cables = p.cables(&utility)?;
    let racks = p.racks(&cables)?;
    Ok(Plan {
        cables,
        racks,
        clotheslines,
        signs,
    })
}

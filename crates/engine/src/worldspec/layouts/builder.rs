use super::{Rng, Theme};
use std::{collections::BTreeMap, fmt::Write};
use superspec_aml::format::escape;
use superspec_geometry::math::*;

#[derive(Clone)]
pub(super) struct Room {
    pub id: String,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    pub z: f64,
    pub wall: f64,
}
impl Room {
    fn center(&self) -> V3 {
        [self.x + self.w / 2., self.y + self.h / 2., self.z]
    }
}
struct Surface {
    id: String,
    points: Vec<V3>,
    kind: &'static str,
    wall: f64,
}
pub(super) struct Builder {
    id: String,
    scale: f64,
    pub wall: f64,
    thickness: f64,
    surfaces: Vec<Surface>,
    rooms: Vec<Room>,
}
fn snap(n: f64) -> f64 {
    superspec_aml::architecture::quantize(n)
}
fn points(p: &[V3]) -> String {
    p.iter()
        .map(|p| format!("{} {} {}", p[0], p[1], p[2]))
        .collect::<Vec<_>>()
        .join("; ")
}
fn key(a: V3, b: V3) -> String {
    let mut p = [points(&[a]), points(&[b])];
    p.sort();
    p.join("|")
}
impl Builder {
    pub fn new(id: &str, theme: &Theme, rng: &mut Rng, scale: f64) -> Self {
        Self {
            id: id.into(),
            scale,
            wall: snap(rng.float(theme.ceiling[0], theme.ceiling[1])),
            thickness: theme.wall_thickness,
            surfaces: vec![],
            rooms: vec![],
        }
    }
    fn world(&self, p: V3) -> V3 {
        [snap(p[0] * self.scale), snap(p[1] * self.scale), snap(p[2])]
    }
    pub fn room(
        &mut self,
        id: impl Into<String>,
        x: f64,
        y: f64,
        w: f64,
        h: f64,
        z: f64,
        wall: f64,
    ) -> Room {
        let r = Room {
            id: id.into(),
            x,
            y,
            w,
            h,
            z,
            wall,
        };
        self.rooms.push(r.clone());
        self.surface(
            &r.id,
            vec![[x, y, z], [x + w, y, z], [x + w, y + h, z], [x, y + h, z]],
            "floor",
            wall,
        );
        r
    }
    pub fn surface(&mut self, id: &str, points: Vec<V3>, kind: &'static str, wall: f64) {
        self.surfaces.push(Surface {
            id: id.into(),
            points: points.into_iter().map(|p| p.map(snap)).collect(),
            kind,
            wall,
        });
    }
    pub fn join(&mut self, id: &str, a: &Room, b: &Room, width: f64) {
        let ac = a.center();
        let bc = b.center();
        let kind = if a.z == b.z { "floor" } else { "ramp" };
        if ac[1] == bc[1] {
            let (l, r) = if a.x < b.x { (a, b) } else { (b, a) };
            let (x, xx, y) = (l.x + l.w, r.x, ac[1] - width / 2.);
            self.surface(
                id,
                vec![
                    [x, y, l.z],
                    [xx, y, r.z],
                    [xx, y + width, r.z],
                    [x, y + width, l.z],
                ],
                kind,
                1.15,
            );
        } else {
            let (s, n) = if a.y < b.y { (a, b) } else { (b, a) };
            let (x, y, yy) = (ac[0] - width / 2., s.y + s.h, n.y);
            self.surface(
                id,
                vec![
                    [x, y, s.z],
                    [x + width, y, s.z],
                    [x + width, yy, n.z],
                    [x, yy, n.z],
                ],
                kind,
                1.15,
            );
        }
    }
    pub fn switchback(&mut self, id: &str, l: &Room, u: &Room) {
        let (x, y, z, zz) = (l.x + (l.w - 8.5) / 2., l.y + l.h, l.z, u.z);
        let m = (z + zz) / 2.;
        self.surface(
            &format!("{id}.out"),
            vec![
                [x, y, z],
                [x + 4., y, z],
                [x + 4., y + 8., m],
                [x, y + 8., m],
            ],
            "stairs",
            1.15,
        );
        self.surface(
            &format!("{id}.landing"),
            vec![
                [x, y + 8., m],
                [x + 8.5, y + 8., m],
                [x + 8.5, y + 12., m],
                [x, y + 12., m],
            ],
            "floor",
            1.15,
        );
        self.surface(
            &format!("{id}.back"),
            vec![
                [x + 4.5, y, zz],
                [x + 8.5, y, zz],
                [x + 8.5, y + 8., m],
                [x + 4.5, y + 8., m],
            ],
            "stairs",
            1.15,
        );
    }
    pub fn finish(
        self,
        ends: (Room, Room),
        routes: usize,
        theme: &Theme,
        rng: &mut Rng,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<String, String> {
        let mut out = format!(
            "<architecture version=\"1\" id=\"{}\" name=\"{}\" validation=\"playable\">\n<player radius=\"0.35\" height=\"1.8\" step=\"0.45\" crouch=\"1.1\" vault=\"0\"/>\n",
            escape(&self.id),
            escape(&self.id)
        );
        let roughness = theme.surface.as_ref().map_or(0.8, |s| s.roughness);
        let metalness = theme.surface.as_ref().map_or(0., |s| s.metalness);
        for (id, role) in [
            ("floor", "floor"),
            ("wall", "static_hard"),
            ("cover", "cover"),
        ] {
            writeln!(out,"<material id=\"{id}\" color=\"#{:06x}\" roughness=\"{roughness}\" metalness=\"{metalness}\"/>",theme.palette.roles.get(role).copied().unwrap_or(0x82909a)).unwrap();
        }
        let all = self
            .surfaces
            .iter()
            .flat_map(|s| s.points.iter().copied())
            .collect::<Vec<_>>();
        let mut edges: BTreeMap<String, (V3, V3, Vec<String>, f64)> = BTreeMap::new();
        let mut body = String::new();
        for s in &self.surfaces {
            if cancelled() {
                return Err("Generation cancelled.".into());
            }
            let id = format!("plan/{}", s.id);
            let mut ring = vec![];
            for (a, b) in s
                .points
                .iter()
                .zip(s.points.iter().cycle().skip(1))
                .take(s.points.len())
            {
                let direction = sub(*b, *a);
                let length2 = dot(direction, direction);
                let mut split = vec![(0., *a)];
                for p in &all {
                    let t = dot(sub(*p, *a), direction) / length2;
                    if t > 1e-7 && t < 1. - 1e-7 && length(sub(*p, lerp(*a, *b, t))) < 1e-7 {
                        split.push((t, *p));
                    }
                }
                split.sort_by(|a, b| a.0.total_cmp(&b.0));
                split.dedup_by(|a, b| (a.0 - b.0).abs() < 1e-7);
                ring.extend(split.into_iter().map(|(_, p)| self.world(p)));
            }
            if s.kind == "floor" {
                writeln!(
                    out,
                    "<profile id=\"{id}:profile\" dimensions=\"3\" points=\"{}\"/>",
                    points(&ring)
                )
                .unwrap();
                writeln!(body,"<slab id=\"{id}\" profile=\"{id}:profile\" thickness=\"0.22\" material=\"floor\" area=\"{id}\"/>").unwrap();
            } else {
                let low = s.points.iter().map(|p| p[2]).fold(f64::INFINITY, f64::min);
                let start = s
                    .points
                    .iter()
                    .enumerate()
                    .position(|(i, p)| p[2] == low && s.points[(i + 1) % 4][2] == low)
                    .ok_or("Flight lacks a horizontal entrance.")?;
                let p = (0..4)
                    .map(|i| self.world(s.points[(start + i) % 4]))
                    .collect::<Vec<_>>();
                writeln!(
                    out,
                    "<path id=\"{id}:left\" points=\"{}\"/><path id=\"{id}:right\" points=\"{}\"/>",
                    points(&[p[0], p[3]]),
                    points(&[p[1], p[2]])
                )
                .unwrap();
                let extra = if s.kind == "stairs" {
                    " maximum-riser=\"0.18\" underside=\"sloped\""
                } else {
                    ""
                };
                writeln!(body,"<{} id=\"{id}\" left=\"{id}:left\" right=\"{id}:right\" material=\"floor\" thickness=\"0.22\" area=\"{id}\"{extra}/>",s.kind).unwrap();
            }
            for (a, b) in ring
                .iter()
                .zip(ring.iter().cycle().skip(1))
                .take(ring.len())
            {
                let e = edges.entry(key(*a, *b)).or_insert((*a, *b, vec![], s.wall));
                e.2.push(id.clone());
            }
        }
        for (i, (_, (a, b, owners, height))) in edges.iter().enumerate() {
            let id = format!("plan/edge-{i}");
            writeln!(
                out,
                "<path id=\"{id}:path\" points=\"{}\"/>",
                points(&[*a, *b])
            )
            .unwrap();
            if owners.len() == 1 {
                writeln!(body,"<wall id=\"{id}\" path=\"{id}:path\" height=\"{height}\" thickness=\"{}\" material=\"wall\"/>",self.thickness).unwrap();
            }
            if owners.len() == 2 {
                writeln!(
                    out,
                    "<connection id=\"{id}:join\" from=\"{}\" to=\"{}\" path=\"{id}:path\"/>",
                    owners[0], owners[1]
                )
                .unwrap();
            }
        }
        for r in &self.rooms {
            if !rng.bool((theme.decor.clutter + theme.decor.centre_cover).min(0.9)) {
                continue;
            }
            let height = snap(rng.float(theme.decor.crate_height[0], theme.decor.crate_height[1]));
            writeln!(body,"<box id=\"decor.{}\" position=\"{}\" width=\"{}\" depth=\"{}\" height=\"{height}\" material=\"cover\"/>",r.id,points(&[self.world([r.x+2.,r.y+2.,r.z])]),1.2*self.scale,1.2*self.scale).unwrap();
            if rng.bool(theme.decor.beams) && r.w >= 10. && r.h >= 10. {
                let z = r.z + 2.6f64.min(r.wall - 0.5);
                if z - r.z > 1.8 {
                    writeln!(body,"<box id=\"beam.{}\" position=\"{}\" width=\"{}\" depth=\"{}\" height=\"0.25\" material=\"cover\"/>",r.id,points(&[self.world([r.x+2.,r.y+r.h-3.,z])]),(r.w-4.)*self.scale,0.5*self.scale).unwrap();
                }
            }
        }
        writeln!(out, "<assembly id=\"layout\">{body}</assembly>").unwrap();
        for (id, r, kind) in [
            ("spawn", &ends.0, "attacker_spawn"),
            ("exit", &ends.1, "objective"),
        ] {
            writeln!(
                out,
                "<marker id=\"{id}\" kind=\"{kind}\" position=\"{}\" element=\"plan/{}\"/>",
                points(&[self.world(r.center())]),
                r.id
            )
            .unwrap();
        }
        writeln!(out,"<route id=\"run\" from=\"spawn\" to=\"exit\" min-routes=\"{routes}\"/><sky color=\"#{:06x}\" intensity=\"{}\"/></architecture>",theme.palette.sky,theme.palette.hemi_intensity).unwrap();
        superspec_profiles::parse(&out).map_err(|e| e.to_string())?;
        Ok(out)
    }
}

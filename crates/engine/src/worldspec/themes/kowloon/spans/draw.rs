//! World-space decorative spans with explicit host/part ownership and contributor records.
use super::{
    anchors::{Anchor, Plane},
    catenary::hanging,
    plan::{self, Clothesline, Plan, Rack, Sign, SignKind},
};
use crate::worldspec::themes::kowloon::{
    fixture::Fixture,
    kit::Kit,
    laundry,
    mesh::Mesh,
    metal::Kind,
    palette::{mix, shade, srgb},
    pen::{Pen, Tint},
    pipes::{Options, PipeSystem},
    signs::{
        Construction, copy, designs,
        panel::{self, Sides},
    },
};
use std::collections::BTreeMap;
use superspec_geometry::obstructions::Budget;
use superspec_geometry::{
    Matrix, V3,
    math::{cos, cross, hypot, sin, sub},
};
use superspec_types::rng::SeededRng;
fn unit(a: V3, b: V3) -> Result<V3, String> {
    let d = sub(b, a);
    let length = hypot(hypot(d[0], d[1]), d[2]);
    if length < 1e-12 {
        return Err("Span cable needs distinct neighboring points.".into());
    }
    Ok(d.map(|v| v / length))
}
/// Shared rings prevent gaps at the bends of a planar cable; each corner retains its smooth normal.
pub fn cable(
    d: &mut Fixture<'_, '_>,
    points: &[V3],
    radius: f64,
    tint: V3,
    sides: usize,
) -> Result<(), String> {
    if !(2..=4097).contains(&points.len())
        || !(3..=16).contains(&sides)
        || !radius.is_finite()
        || !(0.0001..=0.1).contains(&radius)
        || points
            .iter()
            .flatten()
            .any(|v| !v.is_finite() || v.abs() > 1e6)
    {
        return Err("Cable drawing requires bounded finite planar points and radius.".into());
    }
    d.mesh.spend_work(points.len() * sides)?;
    let overall = unit(points[0], *points.last().unwrap())?;
    let plan = hypot(overall[0], overall[1]);
    let side = if plan > 1e-6 {
        [-overall[1] / plan, overall[0] / plan, 0.]
    } else {
        [1., 0., 0.]
    };
    let rings = points
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let axis = unit(
                points[i.saturating_sub(1)],
                points[(i + 1).min(points.len() - 1)],
            )?;
            let other = cross(axis, side);
            Ok((0..sides)
                .map(|k| {
                    let angle = k as f64 / sides as f64 * std::f64::consts::PI * 2.;
                    let normal: V3 =
                        std::array::from_fn(|j| side[j] * cos(angle) + other[j] * sin(angle));
                    let at = std::array::from_fn(|j| p[j] + normal[j] * radius);
                    (at, normal)
                })
                .collect::<Vec<_>>())
        })
        .collect::<Result<Vec<_>, String>>()?;
    for i in 0..rings.len() - 1 {
        for k in 0..sides {
            let [a, b, c, e] = [
                rings[i][k],
                rings[i + 1][k],
                rings[i + 1][(k + 1) % sides],
                rings[i][(k + 1) % sides],
            ];
            let repeat = d.repeat("paint");
            d.pen.face(
                d.mesh,
                "paint",
                &[a.0, b.0, c.0, e.0],
                std::array::from_fn(|j| a.1[j] + e.1[j]),
                &Tint::Color(tint),
                repeat,
                None,
                Some(&[a.1, b.1, c.1, e.1]),
            )?;
        }
    }
    Ok(())
}
/// One detached host drawing and its local material sampling definitions.
pub struct Output<'a, 'b> {
    pub host: &'a str,
    pub mesh: &'a mut Mesh<'b>,
    pub kit: &'a Kit,
    pub cloth: bool,
    pub repeats: &'a dyn Fn(&str) -> f64,
}
#[derive(Clone)]
struct RoutedPen {
    destination: usize,
    pen: Pen,
}
impl RoutedPen {
    fn facing(&self, origin: V3, tangent: V3, outward: V3) -> Result<Self, String> {
        Ok(Self {
            destination: self.destination,
            pen: self.pen.facing(origin, tangent, outward)?,
        })
    }
    fn moved(&self, offset: V3) -> Result<Self, String> {
        Ok(Self {
            destination: self.destination,
            pen: self.pen.moved(offset)?,
        })
    }
}
struct Painter<'a, 'b, 'c> {
    outputs: Vec<Output<'a, 'c>>,
    destinations: BTreeMap<String, usize>,
    combined: bool,
    anchors: &'b [Anchor],
    planes: &'b [Plane],
    contributors: BTreeMap<String, Vec<String>>,
}
impl<'a, 'b, 'c> Painter<'a, 'b, 'c> {
    fn pen(&self, anchor: usize, part: &str) -> Result<RoutedPen, String> {
        let a = self
            .anchors
            .get(anchor)
            .ok_or("Unknown span drawing anchor.")?;
        let p = self
            .planes
            .get(a.plane)
            .ok_or("Unknown span drawing wall.")?;
        let destination = *self
            .destinations
            .get(&p.host)
            .ok_or("Missing span host destination.")?;
        let part = if self.combined {
            format!("{}/$dressing/{part}", p.host)
        } else {
            part.into()
        };
        Ok(RoutedPen {
            destination,
            pen: Pen::new(Matrix::default(), &part)?,
        })
    }
    fn wall(&self, anchor: usize) -> Result<RoutedPen, String> {
        let a = &self.anchors[anchor];
        let p = &self.planes[a.plane];
        self.pen(anchor, "span-fittings")?
            .facing(a.position, p.tangent, p.normal)
    }
    fn fixture(&mut self, pen: RoutedPen) -> Fixture<'_, 'c> {
        let output = &mut self.outputs[pen.destination];
        Fixture {
            pen: pen.pen,
            mesh: output.mesh,
            repeats: output.repeats,
        }
    }
    fn hardware(&mut self, pen: &RoutedPen) -> Result<RoutedPen, String> {
        let output = &mut self.outputs[pen.destination];
        Ok(RoutedPen {
            destination: pen.destination,
            pen: pen.pen.with_weathering(
                output.mesh,
                output.kit.settings().weathering.clone(),
                Kind::Frame,
                None,
                vec!["metal".into()],
            )?,
        })
    }
    fn kit(&self, pen: &RoutedPen) -> &Kit {
        self.outputs[pen.destination].kit
    }
    fn steel(&self, pen: &RoutedPen, oxidation: f64) -> V3 {
        let color = srgb("#a4a9aa");
        if self.kit(pen).settings().weathering.texture_density > 0. {
            color
        } else {
            mix(color, srgb("#7a4a2a"), oxidation)
        }
    }
    fn checkpoint(&self, charged: &mut usize, budget: &mut Budget<'_>) -> Result<(), String> {
        budget.spend(self.outputs.len())?;
        let triangles: usize = self.outputs.iter().map(|o| o.mesh.triangle_count()).sum();
        let work: usize = self.outputs.iter().map(|o| o.mesh.work_spent()).sum();
        if triangles > 2_000_000 {
            return Err("Combined span drawings exceed triangle allowance.".into());
        }
        budget.spend(work - *charged)?;
        *charged = work;
        Ok(())
    }
    fn credit(&mut self, from: usize, to: usize, part: &str) {
        let a = &self.planes[self.anchors[from].plane].host;
        let b = &self.planes[self.anchors[to].plane].host;
        let ids = self
            .contributors
            .entry(format!("{a}/$dressing/{part}"))
            .or_insert_with(|| vec![a.clone()]);
        if !ids.contains(b) {
            ids.push(b.clone());
        }
    }
    fn rack(&mut self, r: &Rack, rng: &mut SeededRng) -> Result<(), String> {
        if !(1..=plan::RACK_SLOTS.len()).contains(&r.slots) {
            return Err("Span rack exceeds its insulator slots.".into());
        }
        let pen = self.wall(r.anchor)?;
        let hardware = self.hardware(&pen)?;
        let steel = self.steel(&pen, rng.float(0.15, 0.75));
        let heights = &plan::RACK_SLOTS[..r.slots];
        let low = heights.iter().copied().fold(f64::INFINITY, f64::min);
        let high = heights.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        let porcelain = rng.pick(&[srgb("#d8d2c0"), srgb("#6a3a28"), srgb("#8a8f8a")]);
        for &z in heights {
            self.fixture(pen.clone()).cuboid(
                "paint",
                [-0.024, plan::RACK_STANDOFF - 0.03, z - 0.022],
                [0.024, plan::RACK_STANDOFF + 0.02, z + 0.022],
                porcelain,
                "",
            )?;
        }
        for z in if r.slots > 1 {
            vec![low - 0.04, high + 0.04]
        } else {
            vec![low]
        } {
            self.fixture(hardware.clone()).bar(
                "metal",
                [0., 0., z],
                [0., plan::RACK_STANDOFF - 0.02, z],
                0.026,
                steel,
            )?;
        }
        if r.slots > 1 {
            let mut d = self.fixture(hardware.clone());
            d.bar(
                "metal",
                [0., plan::RACK_STANDOFF - 0.03, low - 0.05],
                [0., plan::RACK_STANDOFF - 0.03, high + 0.05],
                0.03,
                steel,
            )?;
            d.cuboid(
                "metal",
                [-0.04, 0., low - 0.07],
                [0.04, 0.012, high + 0.07],
                shade(steel, 0.8),
                "b",
            )?;
        }
        if let Some(length) = r.conduit
            && self.kit(&pen).detail() > 0
        {
            let foot = low - 0.07 - length;
            let pipe = PipeSystem::new(Options {
                radius: 0.014,
                color: srgb("#3a3a36"),
                sides: 16,
                variant: "pipe-standard".into(),
                weathering: Some(self.kit(&pen).settings().weathering.clone()),
            })?;
            pipe.run(
                &mut self.fixture(pen),
                &[
                    [0., plan::CONDUIT_STANDOFF, low - 0.07],
                    [0., plan::CONDUIT_STANDOFF, foot],
                ],
                Some(0.),
            )?;
            self.fixture(hardware).cuboid(
                "metal",
                [-0.08, 0., foot - plan::CONDUIT_BOX],
                [0.08, 0.07, foot],
                shade(steel, 0.7),
                "b",
            )?;
        }
        Ok(())
    }
    fn clothesline(&mut self, line: &Clothesline) -> Result<(), String> {
        let pen = self.pen(line.from, "span-washing")?;
        cable(
            &mut self.fixture(pen.clone()),
            &line.upper,
            0.004,
            srgb("#c9c2aa"),
            3,
        )?;
        cable(
            &mut self.fixture(pen.clone()),
            &line.lower,
            0.004,
            srgb("#c9c2aa"),
            3,
        )?;
        for g in &line.garments {
            let cloth = self.outputs[pen.destination].cloth;
            laundry::hang(
                &mut self.fixture(pen.clone()),
                &g.garment,
                g.left,
                g.right,
                g.sway,
                cloth,
            )?;
        }
        self.credit(line.from, line.to, "span-washing");
        for id in [line.from, line.to] {
            let wall = self.wall(id)?;
            let wall = self.hardware(&wall)?;
            let steel = shade(srgb("#a4a9aa"), 0.6);
            let mut d = self.fixture(wall);
            d.bar(
                "metal",
                [0., 0., 0.02],
                [0., plan::LINE_STANDOFF, 0.02],
                0.02,
                steel,
            )?;
            d.bar(
                "metal",
                [0., 0., -plan::LINE_DROP - 0.3],
                [0., plan::LINE_STANDOFF - 0.01, -plan::LINE_DROP / 2.],
                0.014,
                steel,
            )?;
            d.cuboid(
                "metal",
                [-0.012, plan::LINE_STANDOFF - 0.05, -plan::LINE_DROP - 0.04],
                [0.012, plan::LINE_STANDOFF + 0.05, 0.04],
                steel,
                "",
            )?;
        }
        Ok(())
    }
    fn panel(
        &mut self,
        plane: &RoutedPen,
        p: &plan::Panel,
        construction: Construction,
        rng: &mut SeededRng,
    ) -> Result<(), String> {
        let settings = self.kit(plane).settings().clone();
        let look = designs::draw_look(construction, &settings.weathering, rng)?;
        let depth = if construction == Construction::Cloth {
            0.004
        } else {
            0.045
        };
        let business = copy::invent(rng, &copy::Brief::default())?;
        let design =
            designs::title_panel(&business, [p.s1 - p.s0, p.z1 - p.z0, depth], &look, rng)?;
        let pen = plane.moved([p.s0, depth / 2., p.z0])?;
        panel::draw(
            &mut self.fixture(pen),
            &design,
            Sides::Both,
            &look.steel,
            &settings,
        )
    }
    fn sign(
        &mut self,
        sign: &Sign,
        rng: &mut SeededRng,
        budget: &mut Budget<'_>,
    ) -> Result<(), String> {
        let pen = self.pen(sign.from, "span-signs")?;
        let plane = pen.facing(
            sign.point(0., 0.),
            sign.direction,
            [-sign.direction[1], sign.direction[0], 0.],
        )?;
        let end = sign.point(sign.length, sign.top);
        if sign.kind == SignKind::Banner {
            let p = sign.panels.first().ok_or("Span banner needs a panel.")?;
            self.panel(&plane, p, Construction::Cloth, rng)?;
            for (from, to) in [
                (sign.start, sign.point(p.s0, p.z1)),
                (sign.point(p.s1, p.z1), end),
                (
                    sign.point(0., sign.top - sign.mount),
                    sign.point(p.s0, p.z0),
                ),
                (
                    sign.point(p.s1, p.z0),
                    sign.point(sign.length, sign.top - sign.mount),
                ),
            ] {
                let points = hanging(from, to, 0.004, 3, budget)?;
                cable(
                    &mut self.fixture(pen.clone()),
                    &points,
                    0.005,
                    srgb("#b9a97f"),
                    3,
                )?;
            }
            for z in [p.z0, p.z1] {
                self.fixture(pen.clone()).bar(
                    "timber",
                    sign.point(p.s0, z),
                    sign.point(p.s1, z),
                    0.022,
                    srgb("#8a7a58"),
                )?;
            }
        } else {
            let hardware = self.hardware(&pen)?;
            let steel = self.steel(&pen, rng.float(0.25, 0.8));
            let chord = sign.top - plan::TRUSS_DEPTH;
            self.fixture(hardware.clone())
                .bar("metal", sign.start, end, 0.045, steel)?;
            self.fixture(hardware.clone()).bar(
                "metal",
                sign.point(0., chord),
                sign.point(sign.length, chord),
                0.045,
                steel,
            )?;
            let bays = (sign.length / 0.55).round().max(2.) as usize;
            if bays > 128 {
                return Err("Span gantry exceeds its bay allowance.".into());
            }
            for bay in 0..=bays {
                let at = sign.length * bay as f64 / bays as f64;
                self.fixture(hardware.clone()).bar(
                    "metal",
                    sign.point(at, chord),
                    sign.point(at, sign.top),
                    0.022,
                    steel,
                )?;
                if bay < bays {
                    self.fixture(hardware.clone()).bar(
                        "metal",
                        sign.point(at, if bay % 2 == 1 { sign.top } else { chord }),
                        sign.point(
                            at + sign.length / bays as f64,
                            if bay % 2 == 1 { chord } else { sign.top },
                        ),
                        0.018,
                        steel,
                    )?;
                }
            }
            for p in &sign.panels {
                self.panel(&plane, p, Construction::Sheet, rng)?;
                for at in [p.s0 + 0.06, p.s1 - 0.06] {
                    self.fixture(hardware.clone()).bar(
                        "metal",
                        sign.point(at, p.z1),
                        sign.point(at, chord),
                        0.016,
                        steel,
                    )?;
                }
            }
        }
        self.credit(sign.from, sign.to, "span-signs");
        for anchor in [sign.from, sign.to] {
            let wall = self.wall(anchor)?;
            let wall = self.hardware(&wall)?;
            let level = sign.top - self.anchors[anchor].position[2];
            let steel = shade(srgb("#a4a9aa"), 0.55);
            let mut d = self.fixture(wall);
            d.cuboid(
                "metal",
                [-0.07, 0., level - sign.mount - 0.06],
                [0.07, 0.014, level + 0.06],
                steel,
                "b",
            )?;
            for z in [level, level - sign.mount] {
                d.bar(
                    "metal",
                    [0., 0., z],
                    [0., plan::LINE_STANDOFF, z],
                    0.03,
                    steel,
                )?;
            }
        }
        Ok(())
    }
}
pub struct Settings<'a> {
    pub kit: &'a Kit,
    pub cloth: bool,
}
/// Draws into an unpublished bounded mesh; discard it if any validation, work or cancellation error occurs.
#[allow(clippy::too_many_arguments)]
pub fn draw(
    mesh: &mut Mesh<'_>,
    plan: &Plan,
    seed: &str,
    settings: &Settings<'_>,
    planes: &[Plane],
    anchors: &[Anchor],
    repeats: &dyn Fn(&str) -> f64,
    budget: &mut Budget<'_>,
) -> Result<BTreeMap<String, Vec<String>>, String> {
    draw_outputs(
        vec![Output {
            host: "",
            mesh,
            kit: settings.kit,
            cloth: settings.cloth,
            repeats,
        }],
        true,
        plan,
        seed,
        planes,
        anchors,
        budget,
    )
}
/// Routes parts and endpoint fittings to separate host meshes in one deterministic traversal.
/// On failure all supplied drawings must be discarded. They share the aggregate work/triangle limits.
pub fn draw_hosts(
    outputs: Vec<Output<'_, '_>>,
    plan: &Plan,
    seed: &str,
    planes: &[Plane],
    anchors: &[Anchor],
    budget: &mut Budget<'_>,
) -> Result<BTreeMap<String, Vec<String>>, String> {
    draw_outputs(outputs, false, plan, seed, planes, anchors, budget)
}
fn draw_outputs(
    outputs: Vec<Output<'_, '_>>,
    combined: bool,
    plan: &Plan,
    seed: &str,
    planes: &[Plane],
    anchors: &[Anchor],
    budget: &mut Budget<'_>,
) -> Result<BTreeMap<String, Vec<String>>, String> {
    budget.spend(1)?;
    if seed.len() > 8192
        || anchors.len() > 65_536
        || planes.len() > 8192
        || plan.cables.len() + plan.racks.len() + plan.clotheslines.len() + plan.signs.len()
            > 65_536
        || anchors.iter().any(|a| a.plane >= planes.len())
    {
        return Err("Span drawing exceeds bounded ownership or identities.".into());
    }
    for (a, b) in plan
        .cables
        .iter()
        .map(|c| (c.from, c.to))
        .chain(plan.clotheslines.iter().map(|c| (c.from, c.to)))
        .chain(plan.signs.iter().map(|s| (s.from, s.to)))
        .chain(plan.racks.iter().map(|r| (r.anchor, r.anchor)))
    {
        if a >= anchors.len() || b >= anchors.len() {
            return Err("Span drawing references an unknown anchor.".into());
        }
    }
    if outputs.len() > 512 {
        return Err("Span drawing exceeds host allowance.".into());
    }
    let mut destinations = BTreeMap::new();
    if combined {
        for p in planes {
            destinations.insert(p.host.clone(), 0);
        }
    } else {
        let mut bytes = 0;
        for (i, output) in outputs.iter().enumerate() {
            bytes += output.host.len();
            if output.host.is_empty()
                || output.host.len() > 4096
                || bytes > 4 * 1024 * 1024
                || destinations.insert(output.host.to_owned(), i).is_some()
            {
                return Err("Span drawing requires distinct bounded host identities.".into());
            }
        }
        if planes.iter().any(|p| !destinations.contains_key(&p.host)) {
            return Err("Missing span host destination.".into());
        }
    }
    let mut painter = Painter {
        outputs,
        destinations,
        combined,
        anchors,
        planes,
        contributors: BTreeMap::new(),
    };
    let mut charged = 0;
    painter.checkpoint(&mut charged, budget)?;
    for (i, sign) in plan.signs.iter().enumerate() {
        painter.sign(
            sign,
            &mut SeededRng::from_text(&format!("{seed}/sign/{i}")),
            budget,
        )?;
        painter.checkpoint(&mut charged, budget)?;
    }
    for line in &plan.clotheslines {
        painter.clothesline(line)?;
        painter.checkpoint(&mut charged, budget)?;
    }
    for bundle in &plan.cables {
        let pen = painter.pen(bundle.from, "span-cables")?;
        for strand in &bundle.strands {
            let sides = if strand.radius < 0.01 {
                3
            } else {
                [3, 4, 6][painter.kit(&pen).detail() as usize]
            };
            cable(
                &mut painter.fixture(pen.clone()),
                &strand.points,
                strand.radius,
                strand.tint,
                sides,
            )?;
        }
        painter.credit(bundle.from, bundle.to, "span-cables");
        painter.checkpoint(&mut charged, budget)?;
    }
    for (i, rack) in plan.racks.iter().enumerate() {
        painter.rack(rack, &mut SeededRng::from_text(&format!("{seed}/rack/{i}")))?;
        painter.checkpoint(&mut charged, budget)?;
    }
    Ok(painter.contributors)
}

//! Highest-detail roof reservations keep lower-detail fixtures in the same places.
use super::{
    fixture::Fixture,
    household,
    kit::Kit,
    metal::{Binding, Kind},
    palette::{blend, mix, shade, srgb},
    pipes::{Options, PipeSystem},
    roof_access::{self, Plot},
    signs::{frontage, mounts::Proposal},
    water_tanks,
};
use serde::Deserialize;
use superspec_geometry::{
    V3,
    math::{cos, sin},
};
use superspec_types::rng::SeededRng;
#[derive(Clone, Deserialize)]
pub struct Request {
    pub lot_key: String,
    pub tenement: bool,
    pub wall_tint: V3,
    pub width: f64,
    pub depth: f64,
    pub parapet: f64,
    pub ceiling: f64,
    pub perimeter: bool,
}
pub struct Settings<'a> {
    pub kit: &'a Kit,
    pub wall: Binding,
    pub concrete: Binding,
    pub brushed: Binding,
    pub plants: bool,
    pub cloth: bool,
}
struct Deck<'a> {
    request: &'a Request,
    kit: &'a Kit,
    plots: Vec<Plot>,
}
impl Deck<'_> {
    fn stream(&self, key: &str) -> Result<SeededRng, String> {
        self.kit
            .rng(&format!("{}/roof/{key}", self.request.lot_key))
    }
    fn claim(
        &mut self,
        name: &str,
        width: f64,
        depth: f64,
        rear: bool,
    ) -> Result<Option<Plot>, String> {
        let r = self.request;
        if width > r.width - 0.9 || depth > r.depth - 0.9 {
            return Ok(None);
        }
        let mut rng = self.stream(&format!("{name}/plot"))?;
        for _ in 0..14 {
            let x0 = rng.float(0.45, r.width - 0.45 - width);
            let y0 = if rear {
                r.depth - 0.45 - depth - rng.float(0., 0.6_f64.min(r.depth - 0.9 - depth))
            } else {
                rng.float(0.45, r.depth - 0.45 - depth)
            };
            let p = Plot {
                x0,
                x1: x0 + width,
                y0,
                y1: y0 + depth,
            };
            if self.plots.iter().all(|q| {
                p.x1 + 0.3 < q.x0 || p.x0 - 0.3 > q.x1 || p.y1 + 0.3 < q.y0 || p.y0 - 0.3 > q.y1
            }) {
                self.plots.push(p);
                return Ok(Some(p));
            }
        }
        Ok(None)
    }
}
pub(super) fn aerial(
    d: &mut Fixture<'_, '_>,
    base: V3,
    height: f64,
    rng: &mut SeededRng,
    kit: &Kit,
) -> Result<(), String> {
    let pen = d.pen.with_weathering(
        d.mesh,
        kit.settings().weathering.clone(),
        Kind::Frame,
        None,
        vec!["metal".into()],
    )?;
    let d = &mut Fixture {
        pen,
        mesh: d.mesh,
        repeats: d.repeats,
    };
    let steel = shade(srgb("#a4a9aa"), rng.float(0.6, 0.95));
    let top = [base[0], base[1], base[2] + height];
    d.bar("metal", base, top, 0.03, steel)?;
    let mut array = 0;
    // The source draws a fresh condition on every loop test, including the final one.
    loop {
        let count = if height > 1.8 && rng.chance(0.5) {
            2
        } else {
            1
        };
        if array >= count {
            break;
        }
        let z = top[2] - 0.08 - array as f64 * 0.55;
        let yaw = rng.float(0., std::f64::consts::PI);
        let reach = rng.float(0.7, 1.1);
        let along = [cos(yaw), sin(yaw), 0.];
        let across = [-along[1], along[0], 0.];
        let at = |a: f64, c: f64| {
            [
                base[0] + along[0] * a + across[0] * c,
                base[1] + along[1] * a + across[1] * c,
                z,
            ]
        };
        d.bar(
            "metal",
            at(-reach * 0.35, 0.),
            at(reach * 0.65, 0.),
            0.018,
            steel,
        )?;
        for i in 0..5 {
            let a = -reach * 0.3 + i as f64 * reach * 0.9 / 4.;
            let half = 0.3 - i as f64 * 0.04;
            d.bar("metal", at(a, -half), at(a, half), 0.012, steel)?;
        }
        array += 1;
    }
    Ok(())
}
pub(super) fn shack(
    d: &mut Fixture<'_, '_>,
    p: Plot,
    height: f64,
    rng: &mut SeededRng,
    kit: &Kit,
) -> Result<(), String> {
    let color = srgb(rng.pick(&["#a4a9aa", "#4f7a6a", "#5a7a9a", "#8a6a4a"]));
    let base = household::metal_color(color, rng.float(0.2, 0.6), Some(&kit.settings().weathering));
    let walls = [
        ([p.x0, p.y0, 0.], [1., 0., 0.], [0., -1., 0.]),
        ([p.x1, p.y0, 0.], [0., 1., 0.], [1., 0., 0.]),
        ([p.x1, p.y1, 0.], [-1., 0., 0.], [0., 1., 0.]),
        ([p.x0, p.y1, 0.], [0., -1., 0.], [-1., 0., 0.]),
    ];
    for (index, (origin, tangent, out)) in walls.into_iter().enumerate() {
        let pen = d.pen.facing(origin, tangent, out)?.with_weathering(
            d.mesh,
            kit.settings().weathering.clone(),
            Kind::Sheet,
            None,
            vec!["sheet".into()],
        )?;
        let face = &mut Fixture {
            pen,
            mesh: d.mesh,
            repeats: d.repeats,
        };
        let length = if index % 2 == 1 {
            p.y1 - p.y0
        } else {
            p.x1 - p.x0
        };
        let sheets = (length / 0.8).round().max(2.) as usize;
        for i in 0..sheets {
            face.panel(
                "sheet",
                [
                    i as f64 * length / sheets as f64,
                    (i + 1) as f64 * length / sheets as f64,
                    0.,
                    height,
                ],
                0.,
                shade(base, rng.float(0.7, 1.15)),
            )?;
        }
        if index == 0 {
            let x = length * rng.float(0.15, 0.5);
            face.panel("paint", [x, x + 0.75, 0., 1.85], 0.006, srgb("#2a2927"))?;
            face.panel(
                "pane",
                [x + 1., (length - 0.2).min(x + 1.7), 1., 1.6],
                0.006,
                srgb("#2a3a40"),
            )?;
        }
    }
    let pen = d.pen.with_weathering(
        d.mesh,
        kit.settings().weathering.clone(),
        Kind::Sheet,
        None,
        vec!["sheet".into()],
    )?;
    Fixture {
        pen,
        mesh: d.mesh,
        repeats: d.repeats,
    }
    .sheet(
        "sheet",
        &[
            [p.x0 - 0.2, p.y0 - 0.3, height - 0.25],
            [p.x1 + 0.2, p.y0 - 0.3, height - 0.25],
            [p.x1 + 0.2, p.y1 + 0.2, height + 0.2],
            [p.x0 - 0.2, p.y1 + 0.2, height + 0.2],
        ],
        shade(base, 0.8),
    )
}
/// Equipment is visual dressing; the building owner retains the canonical roof collision.
pub fn draw(
    d: &mut Fixture<'_, '_>,
    r: &Request,
    s: &Settings<'_>,
) -> Result<Vec<Proposal>, String> {
    d.mesh.spend_work(1)?;
    if r.lot_key.len() > 1024
        || r.lot_key.is_empty()
        || [r.width, r.depth, r.parapet, r.ceiling]
            .iter()
            .any(|v| !v.is_finite())
        || !(1. ..=40.).contains(&r.width)
        || !(1. ..=44.).contains(&r.depth)
        || !(0. ..=3.).contains(&r.parapet)
        || !(0. ..=20.).contains(&r.ceiling)
        || r.wall_tint
            .iter()
            .any(|v| !v.is_finite() || !(0. ..=1.).contains(v))
    {
        return Err("Rooftop requires bounded deck dimensions and lot identity.".into());
    }
    for b in [&s.wall, &s.concrete, &s.brushed] {
        if b.material.is_empty()
            || b.material.len() > 256
            || !b.repeat.is_finite()
            || b.repeat <= 0.
        {
            return Err("Rooftop finish binding is invalid.".into());
        }
    }
    if !r.tenement {
        return Ok(vec![]);
    }
    let kit = s.kit;
    let mut deck = Deck {
        request: r,
        kit,
        plots: vec![],
    };
    let fine = kit.detail() > 0;
    let mut signs = Vec::new();
    let mut hut_rng = deck.stream("hut")?;
    let access = deck.claim(
        "hut",
        hut_rng.float(2., 2.6),
        hut_rng.float(2.4, 3.) + 0.85,
        true,
    )?;
    let hut = access.map(|p| Plot {
        y0: p.y0 + 0.85,
        ..p
    });
    if let Some(p) = hut {
        roof_access::stair_hut(
            d,
            p,
            2.3,
            &roof_access::Settings {
                weathering: kit.settings().weathering.clone(),
                wall: s.wall.clone(),
                concrete: s.concrete.clone(),
                wall_tint: r.wall_tint,
                lot_key: r.lot_key.clone(),
                masonry: None,
            },
        )?;
    }
    let tanks = deck.stream("tanks")?.integer(1, 3);
    let tank_settings = water_tanks::Settings {
        weathering: kit.settings().weathering.clone(),
        brushed: s.brushed.clone(),
    };
    for i in 0..tanks {
        d.mesh.spend_work(1)?;
        let mut rng = deck.stream(&format!("tank-{i}"))?;
        let round = rng.chance(0.6);
        let Some(p) = deck.claim(&format!("tank-{i}"), 2., 2., false)? else {
            break;
        };
        if i > 0 && !fine {
            continue;
        }
        let center = [(p.x0 + p.x1) / 2., (p.y0 + p.y1) / 2., 0.];
        if round {
            water_tanks::round_tank(d, center, &mut rng, &tank_settings)?;
        } else {
            water_tanks::box_tank(d, center, &mut rng, &tank_settings)?;
        }
    }
    let squatting = kit.option(&format!("{}/shack", r.lot_key), 0.4)?;
    if squatting.planned {
        let mut rng = deck.stream("shack")?;
        let plot = deck.claim("shack", rng.float(2.4, 3.4), rng.float(2.2, 3.), false)?;
        if let Some(p) = plot
            && squatting.shown
        {
            shack(d, p, rng.float(2., 2.3), &mut rng, kit)?;
        }
    }
    let drying = kit.option(&format!("{}/lines", r.lot_key), 0.45)?;
    if drying.planned {
        let mut rng = deck.stream("lines")?;
        let plot = deck.claim("lines", rng.float(2.4, 3.6), 1.2, false)?;
        if let Some(p) = plot
            && fine
            && drying.shown
        {
            household::drying_lines(
                d,
                [p.x0, p.x1],
                (p.y0 + p.y1) / 2.,
                &mut rng,
                Some(&kit.settings().weathering),
                s.cloth,
            )?;
        }
    }
    if fine && kit.has(&format!("{}/roof-plants", r.lot_key), 0.5)? {
        let mut rng = deck.stream("plants")?;
        let end = (r.width - 0.3).min(rng.float(1.5, 4.));
        household::plants(d, [0.3, end], 0.4, 0.01, &mut rng, s.plants)?;
    }
    let galvanized = srgb("#a4a9aa");
    if fine {
        let mut rng = deck.stream("vents")?;
        let vents = rng.integer(1, 3);
        for _ in 0..vents {
            d.mesh.spend_work(1)?;
            let p = [
                rng.float(0.4, r.width - 0.4),
                rng.float(0.4, r.depth - 0.4),
                0.,
            ];
            if deck.plots.iter().any(|q| {
                p[0] > q.x0 - 0.15 && p[0] < q.x1 + 0.15 && p[1] > q.y0 - 0.15 && p[1] < q.y1 + 0.15
            }) {
                continue;
            }
            PipeSystem::new(Options {
                radius: 0.05,
                color: shade(galvanized, 0.6),
                sides: 16,
                variant: "pipe-standard".into(),
                weathering: Some(kit.settings().weathering.clone()),
            })?
            .run(d, &[p, [p[0], p[1], 0.55]], None)?;
            d.drum(
                "paint",
                [p[0], p[1], 0.55],
                0.09,
                0.05,
                shade(galvanized, 0.5),
                5,
            )?;
        }
    }
    let shown = [1, 3, 6][kit.detail() as usize];
    for i in 0..8 {
        d.mesh.spend_work(1)?;
        if if i < 6 {
            i >= shown
        } else {
            !kit.has(&format!("{}/more-aerials", r.lot_key), 0.5)?
        } {
            continue;
        }
        let mut rng = deck.stream(&format!("aerial-{i}"))?;
        let edge = rng.integer(0, 3);
        let t = rng.float(0.1, 0.9);
        let edges = [
            [0.08, t * r.depth, r.parapet],
            [r.width - 0.08, t * r.depth, r.parapet],
            [t * r.width, 0.08, r.parapet],
            [t * r.width, r.depth - 0.08, r.parapet],
        ];
        let on_hut = i == 0 && hut.is_some();
        let mut base = if on_hut {
            let p = hut.unwrap();
            [p.x0 + 0.5, p.y0 + 0.5, 2.42]
        } else {
            edges[edge]
        };
        if !r.perimeter && !on_hut {
            base[2] = 0.;
        }
        let height = rng.float(1.6, 3.2).min(r.ceiling - base[2] - 0.05);
        if height > 0.9 {
            aerial(d, base, height, &mut rng, kit)?;
        }
    }
    let key = format!("{}/hoarding", r.lot_key);
    let hoarding = kit.option(&key, 0.3)?;
    if r.perimeter && r.width >= 4.6 && hoarding.planned {
        signs.extend(frontage::roof_hoarding(
            &d.pen.facing([0.; 3], [1., 0., 0.], [0., -1., 0.])?,
            [0.3, r.width - 0.3],
            [r.parapet, r.ceiling],
            kit,
            &key,
            fine && hoarding.shown,
        )?);
    }
    if r.perimeter && fine && r.parapet < 0.75 && kit.has(&format!("{}/fence", r.lot_key), 0.4)? {
        let steel = blend(shade(galvanized, 0.8), mix([1.; 3], srgb("#7a4a2a"), 0.4));
        let step = (r.width - 0.16) / (r.width / 1.3).round().max(2.);
        let mut x = 0.08;
        while x <= r.width - 0.07 {
            d.mesh.spend_work(1)?;
            d.bar(
                "metal",
                [x, 0.08, r.parapet],
                [x, 0.08, r.parapet + 0.75],
                0.03,
                steel,
            )?;
            x += step;
        }
        for z in [0.4, 0.74] {
            d.bar(
                "metal",
                [0.08, 0.08, r.parapet + z],
                [r.width - 0.08, 0.08, r.parapet + z],
                0.025,
                steel,
            )?;
        }
    }
    Ok(signs)
}

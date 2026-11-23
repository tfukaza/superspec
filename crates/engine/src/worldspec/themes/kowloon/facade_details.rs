//! Permanent facade construction and its matching runoff sources, planned before emission.
pub mod corners;
use super::{
    facade_material,
    fixture::Fixture,
    massing::{Rect, subtract_rect},
    metal::Binding,
    molding::{self, Strip},
    palette::{shade, srgb},
    pen::Tint,
    skin, wall, weathered,
};
use serde::{Deserialize, Serialize};
use superspec_geometry::V3;
use superspec_types::rng::SeededRng;
#[derive(Clone, Deserialize)]
pub struct Request {
    pub key: String,
    pub ground: f64,
    pub roof: f64,
    pub top: f64,
    pub stories: usize,
    pub story_height: f64,
    pub body_span: [f64; 2],
    pub walls: Vec<Rect>,
    pub openings: Vec<Rect>,
}
#[derive(Default, Serialize, Deserialize)]
pub struct Plan {
    pub bands: Vec<Rect>,
    pub vents: Vec<Rect>,
    pub outlets: Vec<Rect>,
    pub ledges: Vec<wall::Ledge>,
}
fn rect(r: &Rect) -> bool {
    [r.u0, r.u1, r.v0, r.v1]
        .iter()
        .all(|v| v.is_finite() && v.abs() <= 10000.)
        && r.u1 > r.u0
        && r.v1 > r.v0
}
fn inside(walls: &[Rect], r: Rect) -> bool {
    let covered: f64 = walls
        .iter()
        .map(|w| {
            (w.u1.min(r.u1) - w.u0.max(r.u0)).max(0.) * (w.v1.min(r.v1) - w.v0.max(r.v0)).max(0.)
        })
        .sum();
    covered >= (r.u1 - r.u0) * (r.v1 - r.v0) - 1e-4
}
/// `reach` measures outward clearance against the complete scene; planning owns no drawing state.
pub fn plan(
    request: &Request,
    seed: &str,
    reach: &dyn Fn(Rect, f64) -> Result<f64, String>,
    cancelled: &dyn Fn() -> bool,
) -> Result<Plan, String> {
    let r = request;
    if r.key.len() > 2048
        || seed.len() > 4096
        || r.walls.len() > 256
        || r.openings.len() > 256
        || r.stories > 128
        || [
            r.ground,
            r.roof,
            r.top,
            r.story_height,
            r.body_span[0],
            r.body_span[1],
        ]
        .iter()
        .any(|v| !v.is_finite() || v.abs() > 10000.)
        || r.story_height < 0.
        || r.body_span[1] < r.body_span[0]
        || r.walls.iter().chain(r.openings.iter()).any(|r| !rect(r))
    {
        return Err("Facade detail plan exceeds finite geometry or identity bounds.".into());
    }
    if cancelled() {
        return Err("Facade detail planning cancelled.".into());
    }
    let mut result = Plan::default();
    if r.top <= r.ground + 0.05 {
        return Ok(result);
    }
    let clear = |candidate: Rect, depth: f64| -> Result<bool, String> {
        if !inside(&r.walls, candidate)
            || r.openings.iter().any(|o| {
                candidate.u0 < o.u1 + 0.06
                    && candidate.u1 > o.u0 - 0.06
                    && candidate.v0 < o.v1 + 0.08
                    && candidate.v1 > o.v0 - 0.08
            })
        {
            return Ok(false);
        }
        let room = reach(candidate, depth)?;
        if !room.is_finite() || room < 0. || room > depth + 1e-6 {
            return Err("Facade clearance callback returned an invalid depth.".into());
        }
        Ok(room >= depth - 1e-6)
    };
    let mut work = 0;
    let mut check = || -> Result<(), String> {
        work += 1;
        if work > 262144 {
            return Err("Facade detail planning work exceeded.".into());
        }
        if cancelled() {
            return Err("Facade detail planning cancelled.".into());
        }
        Ok(())
    };
    for story in 1..r.stories {
        let z = r.ground + (story - 1) as f64 * r.story_height;
        for wall in &r.walls {
            check()?;
            let band = Rect {
                u0: wall.u0,
                u1: wall.u1,
                v0: z + 0.015,
                v1: z + 0.19,
            };
            let mut segments = vec![band];
            for opening in &r.openings {
                check()?;
                if opening.v0 >= band.v1 + 0.08 || opening.v1 <= band.v0 - 0.08 {
                    continue;
                }
                segments = segments
                    .into_iter()
                    .flat_map(|segment| {
                        subtract_rect(
                            segment,
                            Rect {
                                u0: opening.u0 - 0.065,
                                u1: opening.u1 + 0.065,
                                v0: band.v0,
                                v1: band.v1,
                            },
                            0.002,
                        )
                    })
                    .collect();
            }
            for segment in segments {
                check()?;
                if segment.u1 - segment.u0 > 0.2 && clear(segment, 0.12)? {
                    result.bands.push(segment);
                    result.ledges.push(wall::Ledge {
                        u0: segment.u0,
                        u1: segment.u1,
                        z: segment.v0,
                        flow: 0.7,
                    });
                }
            }
        }
        let mut rng = SeededRng::from_text(&format!("{seed}/{}/vent-{story}", r.key));
        let u = r.body_span[0] + (r.body_span[1] - r.body_span[0]) * rng.float(0.35, 0.65);
        let vent = Rect {
            u0: u - 0.19,
            u1: u + 0.19,
            v0: z + 0.35,
            v1: z + 0.58,
        };
        if clear(vent, 0.09)? {
            result.vents.push(vent);
            result.ledges.push(wall::Ledge {
                u0: vent.u0,
                u1: vent.u1,
                z: vent.v0,
                flow: 0.25,
            });
        }
    }
    let u = (r.body_span[0] + r.body_span[1]) / 2.;
    let outlet = Rect {
        u0: u - 0.14,
        u1: u + 0.14,
        v0: r.roof + 0.025,
        v1: (r.top - 0.035).min(r.roof + 0.24),
    };
    if outlet.v1 - outlet.v0 >= 0.1
        && clear(
            Rect {
                v0: outlet.v0 - 0.018,
                ..outlet
            },
            0.14,
        )?
    {
        result.outlets.push(outlet);
        result.ledges.push(wall::Ledge {
            u0: u - 0.07,
            u1: u + 0.07,
            z: outlet.v0,
            flow: 1.,
        });
    }
    check()?;
    Ok(result)
}
#[derive(Clone, Deserialize)]
pub struct Settings {
    pub weathering: weathered::Settings,
    pub concrete: Binding,
    pub detail: usize,
    pub trim: V3,
}
impl Settings {
    fn finish(&self, color: V3, variant: u32) -> facade_material::Settings {
        facade_material::Settings {
            concrete: true,
            coat: facade_material::Coat {
                material: self.concrete.material.clone(),
                repeat: self.concrete.repeat,
                tint: color,
            },
            backing: self.concrete.clone(),
            wear: self.weathering.wear,
            density: self.weathering.texture_density,
            variant,
        }
    }
    fn validate(&self) -> Result<(), String> {
        self.weathering.validate()?;
        if self.detail > 2
            || self.weathering.texture_density <= 0.
            || self
                .trim
                .iter()
                .any(|v| !v.is_finite() || !(0. ..=4.).contains(v))
        {
            return Err("Facade details require finite tints, supported detail and positive raster density.".into());
        }
        facade_material::Facade::new(&self.finish(self.trim, 0))?;
        Ok(())
    }
    fn seed(&self, key: &str) -> u32 {
        SeededRng::from_text(&format!("{}/{key}", self.weathering.seed)).integer(1, 2147483647)
            as u32
    }
}
fn stain(
    d: &mut Fixture<'_, '_>,
    r: Rect,
    y: f64,
    color: V3,
    s: &Settings,
    flow: f64,
    key: &str,
) -> Result<(), String> {
    let seed = s.seed(key);
    skin::draw(
        &mut Fixture {
            pen: d.pen.moved([0., y, 0.])?,
            mesh: d.mesh,
            repeats: d.repeats,
        },
        "concrete",
        &skin::Settings {
            rects: vec![r],
            openings: vec![],
            u_marks: vec![],
            z_marks: vec![],
            finish: s.finish(color, seed),
            condition: wall::Settings {
                base: None,
                ledges: vec![wall::Ledge {
                    u0: r.u0,
                    u1: r.u1,
                    z: r.v1,
                    flow,
                }],
                openings: vec![],
                grime: s.weathering.grime,
                wear: s.weathering.wear,
                seed: f64::from(seed),
            },
            washes: vec![],
            tiles: None,
        },
        key,
    )
}
/// Emission uses preplanned bands/openings and caller-supplied corner miters for the same revision.
pub fn draw(
    d: &mut Fixture<'_, '_>,
    plan: &Plan,
    s: &Settings,
    key: &str,
    miters: &dyn Fn(Rect) -> Result<[Option<f64>; 2], String>,
) -> Result<(), String> {
    s.validate()?;
    if key.len() > 2048
        || plan.bands.len() + plan.vents.len() + plan.outlets.len() > 4096
        || plan
            .bands
            .iter()
            .chain(plan.vents.iter())
            .chain(plan.outlets.iter())
            .any(|r| !rect(r))
    {
        return Err("Invalid facade detail geometry plan.".into());
    }
    for r in &plan.bands {
        d.mesh.spend_work(1)?;
        let miters = miters(*r)?;
        let repeat = d.repeat("concrete");
        let edges = molding::profiled_strip(
            &d.pen,
            d.mesh,
            "concrete",
            &Strip {
                span: [r.u0, r.u1],
                section: &[
                    [0., r.v0],
                    [0.085, r.v0],
                    [0.12, r.v0 + 0.035],
                    [0.12, r.v1 - 0.04],
                    [0.07, r.v1],
                    [0., r.v1],
                ],
                omitted_edges: &[2],
                miters,
                setback: 0.008,
            },
            &Tint::Color(s.trim),
            repeat,
        )?;
        let face = Rect {
            u0: r.u0 + miters[0].unwrap_or(0.) * 0.12,
            u1: r.u1 + miters[1].unwrap_or(0.) * 0.12,
            v0: edges[2].a[1],
            v1: edges[2].b[1],
        };
        stain(
            d,
            face,
            0.12,
            s.trim,
            s,
            0.45,
            &format!("{key}/band-{}", ryu_js::Buffer::new().format(r.v0)),
        )?;
        // A band's joints are 12 mm lines.
        if s.detail > 0 && !d.mesh.graphics_detail().reduced() {
            let mut u = r.u0 + 2.4;
            while u < r.u1 - 0.1 {
                d.panel(
                    "paint",
                    [u - 0.006, u + 0.006, face.v0, face.v1],
                    0.122,
                    shade(s.trim, 0.3),
                )?;
                u += 2.4;
            }
        }
    }
    for r in &plan.vents {
        let rim = shade(srgb("#a4a9aa"), 0.65);
        d.cuboid("metal", [r.u0, 0., r.v0], [r.u1, 0.035, r.v1], rim, "b")?;
        d.panel(
            "paint",
            [r.u0 + 0.025, r.u1 - 0.025, r.v0 + 0.025, r.v1 - 0.025],
            0.036,
            srgb("#2a2927"),
        )?;
        // Louvres 8 mm thick, 45 mm apart: level 1 draws every second one and level 2 the
        // frame round its dark opening. Neither draws the screw heads.
        let detail = d.mesh.graphics_detail();
        let every = detail.pick(1, 2, usize::MAX);
        let mut z = r.v0 + 0.045;
        let mut index = 0;
        while z < r.v1 - 0.02 {
            if every != usize::MAX && index % every == 0 {
                d.molding(
                    "metal",
                    [r.u0 + 0.02, r.u1 - 0.02],
                    &[
                        [0.04, z + 0.028],
                        [0.085, z],
                        [0.085, z - 0.008],
                        [0.04, z + 0.02],
                    ],
                    rim,
                )?;
            }
            index += 1;
            z += 0.045;
        }
        if s.detail > 0 && !detail.reduced() {
            for u in [r.u0 + 0.012, r.u1 - 0.012] {
                for z in [r.v0 + 0.012, r.v1 - 0.012] {
                    d.disc(
                        "metal",
                        [u, 0.037, z],
                        0.006,
                        [0., 1., 0.],
                        srgb("#2a2927"),
                        6,
                    )?;
                }
            }
        }
    }
    for r in &plan.outlets {
        let rim = shade(srgb("#a4a9aa"), 0.6);
        d.panel("paint", [r.u0, r.u1, r.v0, r.v1], 0.003, srgb("#2a2927"))?;
        // Level 2 keeps the dark mouth, its hood and its lip.
        if d.mesh.graphics_detail() != super::graphics_detail::GraphicsDetail::VeryLow {
            for u in [r.u0, r.u1 - 0.025] {
                d.cuboid("metal", [u, 0., r.v0], [u + 0.025, 0.1, r.v1], rim, "b")?;
            }
        }
        d.molding(
            "metal",
            [r.u0, r.u1],
            &[
                [0., r.v0 + 0.035],
                [0.14, r.v0],
                [0.14, r.v0 - 0.018],
                [0., r.v0 + 0.017],
            ],
            rim,
        )?;
        d.cuboid(
            "metal",
            [r.u0, 0., r.v1 - 0.025],
            [r.u1, 0.11, r.v1],
            rim,
            "b",
        )?;
    }
    Ok(())
}
/// Concrete base sections stop at thresholds and retain the exposed corner's miter.
#[allow(clippy::too_many_arguments)]
pub fn plinth(
    d: &mut Fixture<'_, '_>,
    walls: &[Rect],
    thresholds: &[[f64; 2]],
    ground: f64,
    s: &Settings,
    key: &str,
    reach: &dyn Fn(Rect, f64) -> Result<f64, String>,
    miters: &dyn Fn(Rect) -> Result<[Option<f64>; 2], String>,
) -> Result<(), String> {
    s.validate()?;
    if !ground.is_finite()
        || ground <= 0.
        || ground > 10000.
        || walls.len() > 256
        || thresholds.len() > 256
        || walls.iter().any(|r| !rect(r))
        || thresholds
            .iter()
            .any(|t| t.iter().any(|v| !v.is_finite() || v.abs() > 10000.) || t[1] <= t[0])
        || key.len() > 2048
    {
        return Err("Invalid facade plinth plan.".into());
    }
    let height = 0.42_f64.min(ground * 0.18);
    let mut pieces: Vec<_> = walls
        .iter()
        .map(|r| Rect {
            v0: r.v0.max(0.005),
            v1: r.v1.min(height),
            ..*r
        })
        .filter(|r| r.v1 > r.v0)
        .collect();
    for t in thresholds {
        d.mesh.spend_work(pieces.len())?;
        pieces = pieces
            .into_iter()
            .flat_map(|r| {
                subtract_rect(
                    r,
                    Rect {
                        u0: t[0],
                        u1: t[1],
                        v0: 0.,
                        v1: height + 0.01,
                    },
                    0.002,
                )
            })
            .collect();
        if pieces.len() > 4096 {
            return Err("Plinth fragmentation exceeds 4096 pieces.".into());
        }
    }
    let mut drawing = Fixture {
        pen: d.pen.into_part("fixtures")?,
        mesh: d.mesh,
        repeats: d.repeats,
    };
    let color = srgb("#96968b");
    for r in pieces {
        drawing.mesh.spend_work(1)?;
        if r.v1 - r.v0 < 0.04 || r.u1 - r.u0 < 0.04 {
            continue;
        }
        let room = reach(r, 0.065)?;
        if !room.is_finite() || room < 0. || room > 0.065 + 1e-6 {
            return Err("Invalid plinth clearance result.".into());
        }
        if room < 0.065 - 1e-6 {
            continue;
        }
        let miters = miters(r)?;
        let repeat = drawing.repeat("concrete");
        let edges = molding::profiled_strip(
            &drawing.pen,
            drawing.mesh,
            "concrete",
            &Strip {
                span: [r.u0, r.u1],
                section: &[
                    [0.002, r.v0],
                    [0.06, r.v0],
                    [0.06, r.v1 - 0.035],
                    [0.025, r.v1],
                    [0.002, r.v1],
                ],
                omitted_edges: &[1],
                miters,
                setback: 0.008,
            },
            &Tint::Color(color),
            repeat,
        )?;
        stain(
            &mut drawing,
            Rect {
                u0: r.u0 + miters[0].unwrap_or(0.) * 0.06,
                u1: r.u1 + miters[1].unwrap_or(0.) * 0.06,
                v0: edges[1].a[1],
                v1: edges[1].b[1],
            },
            0.06,
            color,
            s,
            0.35,
            &format!("{key}/base"),
        )?;
    }
    Ok(())
}

use super::footprint::Footprint;
use crate::worldspec::themes::kowloon::{
    clearance::{Collision, GroundSupport, Obstacles, RenderTriangle, Volume},
    facade_cells::Role,
    facade_plan::{self, Zone},
    massing::{self, Block, Lot, Rect, SiteBudget},
    pen::Pen,
};
use serde::Serialize;
use std::collections::BTreeSet;
use superspec_geometry::obstructions::Budget;
use superspec_geometry::{Matrix, V3, math::hypot};

const SLIVER: f64 = 0.002;
const FLUSH: f64 = 0.15;
const CLEAR_HEIGHT: f64 = 2.5;
#[derive(Clone)]
pub enum Shape {
    Box {
        width: f64,
        depth: f64,
        height: f64,
        bottom_center: bool,
    },
    Polygon(Footprint),
}
pub struct Request {
    pub host: String,
    pub matrix: Matrix,
    pub shape: Shape,
    pub stories: Option<usize>,
    pub shops: Option<f64>,
    pub base_wall: Option<String>,
}
#[derive(Serialize)]
pub struct Warning {
    pub code: &'static str,
    pub host: String,
    pub corners: Vec<V3>,
}
#[derive(Serialize)]
pub struct OwnedBlock {
    pub block: Block,
    pub lot: usize,
}
pub struct Site {
    pub host: String,
    pub matrix: Matrix,
    inverse: Matrix,
    pub width: f64,
    pub depth: f64,
    pub height: f64,
    pub lots: Vec<Lot>,
    pub shops: f64,
    pub blocks: Vec<OwnedBlock>,
    pub foreign: Vec<Block>,
    pub footprint: Option<Footprint>,
    base_wall: Option<String>,
}
pub struct Plan {
    sites: Vec<Site>,
    pub warnings: Vec<Warning>,
    obstacles: Obstacles,
    ground: GroundSupport,
}
/// Derived frames and exposed regions belong to the plan that created this facade.
pub struct Facade {
    pub pen: Pen,
    pub role: Role,
    pub key: String,
    pub width: f64,
    pub shift: f64,
    pub body_span: [f64; 2],
    pub ground: Vec<Rect>,
    pub body: Vec<Rect>,
    site: usize,
    lot: usize,
    frontal: bool,
    polygon: bool,
}
impl Facade {
    pub(super) fn request<'a>(&'a self, site: &'a Site) -> facade_plan::Request<'a> {
        facade_plan::Request {
            lot: &site.lots[self.lot],
            role: self.role,
            key: &self.key,
            width: self.width,
            shift: self.shift,
            body_span: self.body_span,
            ground: &self.ground,
            body: &self.body,
            shops: site.shops,
        }
    }
}
fn corners(b: Block) -> [V3; 8] {
    std::array::from_fn(|i| {
        [
            if i & 4 == 0 { b.x0 } else { b.x1 },
            if i & 2 == 0 { b.y0 } else { b.y1 },
            if i & 1 == 0 { b.z0 } else { b.z1 },
        ]
    })
}
fn bounding(points: &[V3]) -> Block {
    let low: V3 =
        std::array::from_fn(|a| points.iter().map(|p| p[a]).fold(f64::INFINITY, f64::min));
    let high: V3 = std::array::from_fn(|a| {
        points
            .iter()
            .map(|p| p[a])
            .fold(f64::NEG_INFINITY, f64::max)
    });
    Block {
        x0: low[0],
        x1: high[0],
        y0: low[1],
        y1: high[1],
        z0: low[2],
        z1: high[2],
    }
}
impl Plan {
    /// Plans all hosts before any drawing, so insertion order cannot hide a neighboring obstacle.
    pub fn new(
        requests: Vec<Request>,
        seed: &str,
        physical: Vec<Collision>,
        render: &[RenderTriangle],
        budget: &mut Budget<'_>,
    ) -> Result<Self, String> {
        budget.spend(0)?;
        if requests.len() > 512 || seed.len() > 1024 || physical.len() > 1_048_576 {
            return Err("Building planning exceeds source allowance.".into());
        }
        let mut sites = Vec::new();
        let mut warnings = Vec::new();
        let mut ids = BTreeSet::new();
        for r in requests {
            budget.spend(1)?;
            if r.host.is_empty()
                || r.host.len() > 512
                || !ids.insert(r.host.clone())
                || r.base_wall.as_ref().is_some_and(|s| s.len() > 4096)
                || r.shops
                    .is_some_and(|v| !v.is_finite() || !(0. ..=1.).contains(&v))
            {
                return Err(
                    "Building requests require unique bounded identities and a valid shop share."
                        .into(),
                );
            }
            let root = Pen::new(r.matrix, "envelope")?;
            if (0..3).any(|i| (r.matrix.0[2][i] - [0., 0., 1.][i]).abs() > 1e-7) {
                return Err("Building frames must preserve world Z up.".into());
            }
            let (width, depth, height, offset, footprint) = match r.shape {
                Shape::Box {
                    width,
                    depth,
                    height,
                    bottom_center,
                } => (
                    width,
                    depth,
                    height,
                    if bottom_center {
                        [-width / 2., -depth / 2., 0.]
                    } else {
                        [0.; 3]
                    },
                    None,
                ),
                Shape::Polygon(f) => (f.width, f.depth, f.height, [0., 0., f.elevation], Some(f)),
            };
            if [width, depth, height].iter().any(|v| !v.is_finite()) {
                return Err("Building dimensions must be finite.".into());
            }
            let matrix = root.moved(offset)?.matrix;
            let inverse = matrix.inverse()?;
            let stories = r.stories.unwrap_or(massing::default_stories(height));
            let mut base_height = 0.0_f64;
            if let Some(base) = &r.base_wall {
                for t in &physical {
                    budget.spend(1)?;
                    if &t.object == base {
                        for p in t.points {
                            base_height = base_height.max(inverse.point(p)[2]);
                        }
                    }
                }
            }
            let identity = format!("{seed}/{}", r.host);
            let lots = if let Some(f) = &footprint {
                vec![f.lot(stories, &identity)?]
            } else {
                massing::plan_lots(
                    SiteBudget {
                        width,
                        depth,
                        height,
                        stories,
                        base_height,
                    },
                    &identity,
                )?
            };
            let large = width >= 4.8 && depth >= 5. && stories >= 1;
            let shops = if footprint.is_none() && large {
                r.shops.unwrap_or(0.15)
            } else {
                0.
            };
            if footprint.is_none() && !large && r.shops.unwrap_or(0.) > 0. {
                warnings.push(Warning {
                    code: "DRESSING_NO_SHOP_FIT",
                    host: r.host.clone(),
                    corners: corners(Block {
                        x0: 0.,
                        x1: width,
                        y0: 0.,
                        y1: depth,
                        z0: 0.,
                        z1: height,
                    })
                    .map(|p| matrix.point(p))
                    .to_vec(),
                });
            }
            let blocks = if footprint.is_some() {
                vec![]
            } else {
                lots.iter()
                    .enumerate()
                    .flat_map(|(i, l)| {
                        massing::lot_blocks(l, depth)
                            .into_iter()
                            .map(move |block| OwnedBlock { block, lot: i })
                    })
                    .collect()
            };
            sites.push(Site {
                host: r.host,
                matrix,
                inverse,
                width,
                depth,
                height,
                lots,
                shops,
                blocks,
                foreign: vec![],
                footprint,
                base_wall: r.base_wall,
            });
        }
        let mut volumes = Vec::new();
        for (index, site) in sites.iter().enumerate() {
            if let Some(f) = &site.footprint {
                volumes.push(Volume {
                    owner: index,
                    corners: f
                        .points
                        .iter()
                        .flat_map(|p| [0., f.height].map(|z| site.matrix.point([p[0], p[1], z])))
                        .collect(),
                });
            }
            for b in &site.blocks {
                volumes.push(Volume {
                    owner: index,
                    corners: corners(b.block).map(|p| site.matrix.point(p)).to_vec(),
                });
            }
        }
        for i in 0..sites.len() {
            let mut foreign = Vec::new();
            for (j, other) in sites.iter().enumerate() {
                budget.spend(1)?;
                if i == j {
                    continue;
                }
                let relative = sites[i].inverse.then(other.matrix);
                if ![
                    relative.0[0][0],
                    relative.0[0][1],
                    relative.0[1][0],
                    relative.0[1][1],
                ]
                .iter()
                .all(|v| v.abs() < 1e-6 || (v.abs() - 1.).abs() < 1e-6)
                {
                    continue;
                }
                for b in &other.blocks {
                    budget.spend(8)?;
                    foreign.push(bounding(&corners(b.block).map(|p| relative.point(p))));
                }
                if foreign.len() > 4096 {
                    return Err("Building neighbor collection exceeds allowance.".into());
                }
            }
            sites[i].foreign = foreign;
        }
        let obstacles = Obstacles::new(physical, volumes, budget)?;
        let ground = GroundSupport::new(render, budget)?;
        Ok(Self {
            sites,
            warnings,
            obstacles,
            ground,
        })
    }
    pub fn sites(&self) -> &[Site] {
        &self.sites
    }
    fn site(&self, index: usize) -> Result<&Site, String> {
        self.sites
            .get(index)
            .ok_or_else(|| "Unknown building site.".into())
    }
    /// All four rectangular faces, or each actual polygon edge; no bounding-box substitution.
    pub fn facades(
        &self,
        index: usize,
        lot: usize,
        budget: &mut Budget<'_>,
    ) -> Result<Vec<Facade>, String> {
        budget.spend(0)?;
        let site = self.site(index)?;
        let l = site.lots.get(lot).ok_or("Unknown building lot.")?;
        let root = Pen::new(site.matrix, "envelope")?;
        if let Some(f) = &site.footprint {
            return f
                .points
                .iter()
                .enumerate()
                .map(|(i, &a)| {
                    let b = f.points[(i + 1) % f.points.len()];
                    let width = hypot(b[0] - a[0], b[1] - a[1]);
                    let tangent = [(b[0] - a[0]) / width, (b[1] - a[1]) / width, 0.];
                    let pen = root.facing(a, tangent, [tangent[1], -tangent[0], 0.])?;
                    let ground = self.polygon_exposure(
                        index,
                        &pen,
                        Rect {
                            u0: 0.,
                            u1: width,
                            v0: 0.,
                            v1: l.ground,
                        },
                        false,
                        budget,
                    )?;
                    let body = self.polygon_exposure(
                        index,
                        &pen,
                        Rect {
                            u0: 0.,
                            u1: width,
                            v0: l.ground,
                            v1: l.top,
                        },
                        false,
                        budget,
                    )?;
                    Ok(Facade {
                        pen,
                        role: if width < 2. {
                            Role::Party
                        } else if i == 0 {
                            Role::Front
                        } else {
                            Role::Side
                        },
                        key: format!("{}/edge-{i}", l.key),
                        width,
                        shift: 0.,
                        body_span: [0.5, width - 0.5],
                        ground,
                        body,
                        site: index,
                        lot,
                        frontal: false,
                        polygon: true,
                    })
                })
                .collect();
        }
        (0..4)
            .map(|side| {
                let frontal = side < 2;
                let forward = side == 0 || side == 3;
                let outward = if side == 0 || side == 2 { -1. } else { 1. };
                let ground_at = [0., site.depth, l.x0, l.x1][side];
                let body_at = [-l.front, site.depth + l.back, l.x0, l.x1][side];
                let span = if frontal {
                    [l.x0, l.x1]
                } else {
                    [0., site.depth]
                };
                let body_span = if frontal {
                    [l.x0, l.x1]
                } else {
                    [-l.front, site.depth + l.back]
                };
                let origin = span[usize::from(!forward)];
                let to_u = |t: f64| if forward { t - origin } else { origin - t };
                let tangent = if frontal {
                    [if forward { 1. } else { -1. }, 0., 0.]
                } else {
                    [0., if forward { 1. } else { -1. }, 0.]
                };
                let normal = if frontal {
                    [0., outward, 0.]
                } else {
                    [outward, 0., 0.]
                };
                let corner = if frontal {
                    [origin, ground_at, 0.]
                } else {
                    [ground_at, origin, 0.]
                };
                let pen = root.facing(corner, tangent, normal)?;
                let shift = if frontal {
                    if side == 0 { l.front } else { l.back }
                } else {
                    0.
                };
                let body_pen = pen.moved([0., shift, 0.])?;
                let mut exposed =
                    |at: f64, span: [f64; 2], z0: f64, z1: f64| -> Result<Vec<Rect>, String> {
                        let mut visible = vec![Rect {
                            u0: span[0],
                            u1: span[1],
                            v0: z0,
                            v1: z1,
                        }];
                        for b in site.blocks.iter().map(|b| &b.block).chain(&site.foreign) {
                            budget.spend(visible.len())?;
                            let (n0, n1, t0, t1) = if frontal {
                                (b.y0, b.y1, b.x0, b.x1)
                            } else {
                                (b.x0, b.x1, b.y0, b.y1)
                            };
                            let covers = if outward > 0. {
                                n0 <= at + SLIVER && n1 >= at + 0.02
                            } else {
                                n1 >= at - SLIVER && n0 <= at - 0.02
                            };
                            if covers {
                                visible = visible
                                    .into_iter()
                                    .flat_map(|r| {
                                        massing::subtract_rect(
                                            r,
                                            Rect {
                                                u0: t0,
                                                u1: t1,
                                                v0: b.z0,
                                                v1: b.z1,
                                            },
                                            SLIVER,
                                        )
                                    })
                                    .collect();
                            }
                            if visible.len() > 256 {
                                return Err("Building wall exposure exceeds allowance.".into());
                            }
                        }
                        let mut result = Vec::new();
                        for r in visible {
                            let (a, b) = (to_u(r.u0), to_u(r.u1));
                            result.extend(self.polygon_exposure(
                                index,
                                if at == ground_at { &pen } else { &body_pen },
                                Rect {
                                    u0: a.min(b),
                                    u1: a.max(b),
                                    ..r
                                },
                                true,
                                budget,
                            )?);
                        }
                        Ok(result)
                    };
                let ground = exposed(ground_at, span, 0., l.ground)?;
                let body = if l.top - l.ground > 0.05 {
                    exposed(body_at, body_span, l.ground, l.top)?
                } else {
                    vec![]
                };
                let role = if l.kind == "well" {
                    if side == 0 { Role::Well } else { Role::Party }
                } else if side == 0 {
                    Role::Front
                } else if side == 1 {
                    Role::Back
                } else if (side == 2 && lot == 0) || (side == 3 && lot + 1 == site.lots.len()) {
                    Role::Side
                } else {
                    Role::Party
                };
                let (a, b) = (to_u(body_span[0]), to_u(body_span[1]));
                Ok(Facade {
                    pen,
                    role,
                    key: format!("{}/side-{side}", l.key),
                    width: span[1] - span[0],
                    shift,
                    body_span: [a.min(b), a.max(b)],
                    ground,
                    body,
                    site: index,
                    lot,
                    frontal,
                    polygon: false,
                })
            })
            .collect()
    }
    fn polygon_exposure(
        &self,
        index: usize,
        pen: &Pen,
        rect: Rect,
        polygons_only: bool,
        budget: &mut Budget<'_>,
    ) -> Result<Vec<Rect>, String> {
        let inverse = pen.matrix.inverse()?;
        let mut visible = vec![rect];
        for (j, other) in self.sites.iter().enumerate() {
            budget.spend(1)?;
            if index == j || (polygons_only && other.footprint.is_none()) {
                continue;
            }
            let prisms = if let Some(f) = &other.footprint {
                vec![(f.points.clone(), 0., f.height)]
            } else {
                other
                    .blocks
                    .iter()
                    .map(|b| {
                        let b = b.block;
                        (
                            vec![
                                [b.x0, b.y0, 0.],
                                [b.x1, b.y0, 0.],
                                [b.x1, b.y1, 0.],
                                [b.x0, b.y1, 0.],
                            ],
                            b.z0,
                            b.z1,
                        )
                    })
                    .collect()
            };
            let relative = inverse.then(other.matrix);
            for (points, bottom, top) in prisms {
                budget.spend(points.len() * 2 + visible.len())?;
                let points: Vec<_> = points.iter().map(|p| relative.point(*p)).collect();
                let interval = |depth: f64| -> Option<[f64; 2]> {
                    let mut crossings = Vec::new();
                    for (i, a) in points.iter().enumerate() {
                        let b = points[(i + 1) % points.len()];
                        if (a[1] <= depth && b[1] > depth) || (b[1] <= depth && a[1] > depth) {
                            crossings.push(a[0] + (b[0] - a[0]) * (depth - a[1]) / (b[1] - a[1]));
                        }
                    }
                    (crossings.len() >= 2).then(|| {
                        [
                            crossings.iter().copied().fold(f64::INFINITY, f64::min),
                            crossings.iter().copied().fold(f64::NEG_INFINITY, f64::max),
                        ]
                    })
                };
                if let (Some(contact), Some(occupied)) = (interval(SLIVER), interval(0.02)) {
                    let hole = Rect {
                        u0: contact[0].max(occupied[0]),
                        u1: contact[1].min(occupied[1]),
                        v0: points[0][2] + bottom,
                        v1: points[0][2] + top,
                    };
                    visible = visible
                        .into_iter()
                        .flat_map(|r| massing::subtract_rect(r, hole, SLIVER))
                        .collect();
                    if visible.len() > 256 {
                        return Err("Polygon wall exposure exceeds allowance.".into());
                    }
                }
            }
        }
        Ok(visible)
    }
    fn crowded(
        &self,
        f: &Facade,
        pen: &Pen,
        low: V3,
        high: V3,
        budget: &mut Budget<'_>,
    ) -> Result<bool, String> {
        let site = self.site(f.site)?;
        let local = site.inverse.then(pen.matrix);
        let b = bounding(
            &corners(Block {
                x0: low[0],
                x1: high[0],
                y0: low[1],
                y1: high[1],
                z0: low[2],
                z1: high[2],
            })
            .map(|p| local.point(p)),
        );
        for other in &site.blocks {
            budget.spend(1)?;
            let a = other.block;
            if other.lot != f.lot
                && a.x0 < b.x1 - 1e-6
                && a.x1 > b.x0 + 1e-6
                && a.y0 < b.y1 - 1e-6
                && a.y1 > b.y0 + 1e-6
                && a.z0 < b.z1 - 1e-6
                && a.z1 > b.z0 + 1e-6
            {
                return Ok(true);
            }
        }
        Ok(false)
    }
    pub fn reach(
        &self,
        f: &Facade,
        zone: Zone,
        r: Rect,
        limit: f64,
        budget: &mut Budget<'_>,
    ) -> Result<f64, String> {
        budget.spend(0)?;
        let bounded = [r.u0, r.u1, r.v0, r.v1, limit]
            .iter()
            .all(|v| v.is_finite() && v.abs() <= 1000.)
            && limit >= 0.;
        // Rectangular one-story lots reject empty probes. Polygon facades retain
        // the source planner's signed extents when testing their roof fixtures.
        if bounded && !f.polygon && (r.u1 <= r.u0 || r.v1 <= r.v0) {
            return Ok(0.);
        }
        if !f.polygon || !bounded {
            query(r, limit)?;
        }
        let cap = if r.v0 < CLEAR_HEIGHT - if f.polygon { 0. } else { 1e-6 } {
            limit.min(FLUSH)
        } else {
            limit
        };
        let pen = if matches!(zone, Zone::Ground) {
            f.pen.clone()
        } else {
            f.pen.moved([0., f.shift, 0.])?
        };
        for depth in [cap, 0.9, 0.6, 0.35, FLUSH] {
            if depth > cap + if f.polygon { 0. } else { 1e-9 } {
                continue;
            }
            let low = [r.u0 + 0.02, 0.02, r.v0 + 0.02];
            let high = [r.u1 - 0.02, depth, r.v1 - 0.02];
            if !f.polygon && (high[0] <= low[0] || high[2] <= low[2] || high[1] < low[1]) {
                return Ok(0.);
            }
            if self
                .obstacles
                .blocked_probe(&pen, low, high, f.site, None, budget)?
            {
                continue;
            }
            if !f.polygon && self.crowded(f, &pen, low, high, budget)? {
                continue;
            }
            return Ok(depth);
        }
        Ok(0.)
    }
    pub fn hollow(
        &self,
        f: &Facade,
        span: [f64; 2],
        height: f64,
        limit: f64,
        budget: &mut Budget<'_>,
    ) -> Result<f64, String> {
        budget.spend(0)?;
        query(
            Rect {
                u0: span[0],
                u1: span[1],
                v0: 0.05,
                v1: height,
            },
            limit,
        )?;
        if f.polygon {
            return Ok(0.);
        }
        let site = self.site(f.site)?;
        let l = &site.lots[f.lot];
        let clear = limit.min(if f.frontal { site.depth } else { l.x1 - l.x0 } * 0.49);
        if clear < 0.02 {
            return Ok(0.);
        }
        if self.obstacles.blocked(
            &f.pen,
            [span[0], -clear, 0.05],
            [span[1], -0.02, height],
            f.site,
            site.base_wall.as_deref(),
            budget,
        )? {
            Ok(0.)
        } else {
            Ok(clear)
        }
    }
    pub fn frontage(
        &self,
        f: &Facade,
        span: [f64; 2],
        display_depth: f64,
        height: f64,
        budget: &mut Budget<'_>,
    ) -> Result<bool, String> {
        budget.spend(0)?;
        query(
            Rect {
                u0: span[0],
                u1: span[1],
                v0: 0.,
                v1: height,
            },
            display_depth,
        )?;
        if f.polygon {
            return Ok(false);
        }
        let site = self.site(f.site)?;
        let ground_depth = display_depth + 1.2;
        let low = [span[0], 0.025, 0.02];
        let high = [span[1], ground_depth + 0.65, height.max(2.2)];
        let center = std::array::from_fn(|i| (low[i] + high[i]) / 2.);
        Ok(self
            .ground
            .covers(&f.pen, [span[0], span[1], 0.025, ground_depth], 0., budget)?
            && !self.obstacles.blocked(
                &f.pen,
                low,
                high,
                f.site,
                site.base_wall.as_deref(),
                budget,
            )?
            && !self
                .obstacles
                .contains(f.pen.world(center), site.base_wall.as_deref(), budget)?
            && !self.crowded(f, &f.pen, low, high, budget)?)
    }
}
fn query(r: Rect, depth: f64) -> Result<(), String> {
    if [r.u0, r.u1, r.v0, r.v1, depth]
        .iter()
        .any(|v| !v.is_finite() || v.abs() > 1000.)
        || r.u1 <= r.u0
        || r.v1 <= r.v0
        || depth < 0.
    {
        Err("Building placement query requires bounded ordered extents.".into())
    } else {
        Ok(())
    }
}

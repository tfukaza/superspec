//! Finite pavement finish fields. Ceramic microrelief stays in normals; discrete repair plates own geometry height.
use super::{
    coverage::{self, Bounds, Footprint},
    field::{edge_distance, valid_point},
};
use crate::worldspec::themes::kowloon::{
    noise::{lattice_hash, smooth_step, value_noise, value_noise_range},
    surface::{Layer, Sample},
    tile,
};
use serde::{Deserialize, Serialize};
use std::{cell::RefCell, collections::BTreeMap};
use superspec_geometry::obstructions::Budget;
use superspec_geometry::{
    V3,
    math::{cos, sin},
};

pub use superspec_model::theme::ground::Finish;
fn plate(finish: Finish) -> f64 {
    match finish {
        Finish::Asphalt => 0.0035,
        Finish::Joint | Finish::Crack | Finish::Repair => -0.0015,
        Finish::Spall => -0.004,
        Finish::Chip => -0.0025,
        _ => 0.,
    }
}
fn fractured(finish: Finish) -> bool {
    matches!(finish, Finish::Spall | Finish::Chip)
}
#[derive(Clone, Deserialize, Serialize)]
pub struct Source {
    pub id: String,
    pub repeat: f64,
    pub roughness: Option<f64>,
}
#[derive(Clone, Deserialize, Serialize)]
pub struct Deposit {
    pub polygon: Vec<V3>,
    pub material: Finish,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Recipe {
    pub seed: u32,
    pub kind: Finish,
    pub polygon: Vec<V3>,
    #[serde(default)]
    pub asphalt: Vec<Vec<V3>>,
    pub deposits: Vec<Deposit>,
    pub tile_size: f64,
    pub tile_aspect: Option<f64>,
    pub tile_wear: Option<f64>,
    pub tile_grime: Option<f64>,
    pub worn_edges: Vec<bool>,
    pub angle: f64,
    pub phase: f64,
    pub relief: bool,
    pub receiving_edges: Vec<[V3; 2]>,
    pub sources: BTreeMap<Finish, Source>,
}
pub struct Field<'a> {
    recipe: &'a Recipe,
    finishes: Vec<Finish>,
    sources: Vec<Source>,
    deposits: Vec<(Deposit, Footprint)>,
    asphalt: Vec<(Vec<V3>, Footprint)>,
    receiving_edges: Vec<[V3; 2]>,
    polygon: Vec<V3>,
    pub materials: Vec<String>,
    pub elevations: Option<Vec<f64>>,
    tiles: Option<(tile::Tile, tile::Materials)>,
    cosine: f64,
    sine: f64,
    timber: RefCell<BTreeMap<(i64, i64), bool>>,
    asphalt_sample: RefCell<Option<(f64, f64, f64)>>,
}
impl<'a> Field<'a> {
    pub fn new(recipe: &'a Recipe, budget: &mut Budget<'_>) -> Result<Self, String> {
        let r = recipe;
        let count = r.polygon.len()
            + r.asphalt.iter().map(Vec::len).sum::<usize>()
            + r.deposits.iter().map(|d| d.polygon.len()).sum::<usize>()
            + r.receiving_edges.len() * 2;
        budget.spend(count)?;
        let polygons = std::iter::once(&r.polygon)
            .chain(&r.asphalt)
            .chain(r.deposits.iter().map(|d| &d.polygon));
        if count > 65_536
            || r.deposits.len() > 4096
            || r.asphalt.len() > 4096
            || r.receiving_edges.len() > 20_000
            || polygons
                .clone()
                .any(|p| p.len() < 3 || p.len() > 4096 || p.iter().any(|p| !valid_point(*p)))
            || r.receiving_edges.iter().flatten().any(|p| !valid_point(*p))
            || r.worn_edges.len() != r.polygon.len()
            || !r.angle.is_finite()
            || r.angle.abs() > 1000.
            || !r.phase.is_finite()
            || r.phase.abs() > 1e6
            || !r.tile_size.is_finite()
            || !(0.01..=10.).contains(&r.tile_size)
            || [r.tile_wear, r.tile_grime]
                .into_iter()
                .flatten()
                .any(|v| !v.is_finite() || !(0. ..=1.).contains(&v))
            || r.sources.values().any(|s| {
                s.id.is_empty()
                    || s.id.len() > 4096
                    || !s.repeat.is_finite()
                    || !(0.0001..=10000.).contains(&s.repeat)
                    || s.roughness
                        .is_some_and(|v| !v.is_finite() || !(0. ..=1.).contains(&v))
            })
        {
            return Err("Pavement field exceeds finite geometry or material limits.".into());
        }
        // Derived halfplanes check every owned vertex; charge this work before allocating them.
        budget.spend(polygons.map(|p| p.len() * p.len()).sum())?;
        let mut finishes = vec![r.kind];
        let extras = (!r.asphalt.is_empty())
            .then_some(Finish::Asphalt)
            .into_iter()
            .chain(if matches!(r.kind, Finish::Tile | Finish::Timber) {
                vec![Finish::Substrate, Finish::Grout]
            } else {
                vec![]
            })
            .chain(r.deposits.iter().map(|d| d.material));
        for f in extras {
            if !finishes.contains(&f) {
                finishes.push(f);
            }
        }
        let sources: Vec<_> = finishes
            .iter()
            .map(|f| {
                r.sources
                    .get(f)
                    .cloned()
                    .ok_or_else(|| format!("Missing pavement source {f:?}."))
            })
            .collect::<Result<_, _>>()?;
        let mut materials: Vec<_> = sources.iter().map(|s| s.id.clone()).collect();
        let elevations = matches!(r.kind, Finish::Concrete | Finish::Repair).then(|| {
            finishes
                .iter()
                .enumerate()
                .map(|(i, f)| if i == 0 { 0. } else { plate(*f) })
                .collect()
        });
        let tiles = if r.kind == Finish::Tile {
            let ceramic = &sources[0];
            let bed = &sources[finishes
                .iter()
                .position(|f| *f == Finish::Substrate)
                .unwrap()];
            let grime = r
                .sources
                .get(&Finish::TileGrime)
                .ok_or("Missing tile grime source.")?;
            let roughness = ceramic.roughness.unwrap_or(0.66);
            let binding = tile::Materials {
                ceramic: ceramic.id.clone(),
                bed: bed.id.clone(),
                grime: grime.id.clone(),
                repeat: ceramic.repeat,
                bed_repeat: bed.repeat,
                bed_roughness: bed.roughness.unwrap_or(0.96),
                roughness: roughness + (1. - roughness) * r.tile_grime.unwrap_or(0.) * 0.45,
            };
            binding.validate()?;
            materials.push(grime.id.clone());
            Some((
                tile::Tile::new(&tile::Settings {
                    seed: r.seed,
                    share: 1.,
                    size: Some(r.tile_size),
                    aspect: Some(r.tile_aspect.unwrap_or(1.)),
                    wear: r.tile_wear.unwrap_or(0.9),
                    grime: r.tile_grime.unwrap_or(0.),
                })?,
                binding,
            ))
        } else {
            None
        };
        let deposits = r
            .deposits
            .iter()
            .map(|d| {
                let polygon = coverage::ccw(&d.polygon);
                let footprint = Footprint::new(&polygon);
                (
                    Deposit {
                        polygon,
                        material: d.material,
                    },
                    footprint,
                )
            })
            .collect();
        let asphalt = r
            .asphalt
            .iter()
            .map(|p| {
                let p = coverage::ccw(p);
                let f = Footprint::new(&p);
                (p, f)
            })
            .collect();
        let region = coverage::bounds(&r.polygon);
        let receiving_edges = r
            .receiving_edges
            .iter()
            .filter(|e| coverage::overlaps(region, coverage::bounds(&e[..]), 0.04))
            .copied()
            .collect();
        Ok(Self {
            recipe: r,
            finishes,
            sources,
            deposits,
            asphalt,
            receiving_edges,
            polygon: coverage::ccw(&r.polygon),
            materials,
            elevations,
            tiles,
            cosine: cos(r.angle),
            sine: sin(r.angle),
            timber: RefCell::new(BTreeMap::new()),
            asphalt_sample: RefCell::new(None),
        })
    }
    fn index(&self, finish: Finish) -> usize {
        self.finishes
            .iter()
            .position(|f| *f == finish)
            .expect("admitted finish")
    }
    fn timber_missing(&self, ix: f64, iy: f64) -> bool {
        let key = (ix as i64, iy as i64);
        if let Some(v) = self.timber.borrow().get(&key) {
            return *v;
        }
        let r = self.recipe;
        let center = [(ix + 0.5) * 0.16, (iy + 0.5) * 1.15, 0.];
        let exposure = r
            .polygon
            .iter()
            .enumerate()
            .filter(|(i, _)| r.worn_edges[*i])
            .map(|(i, &a)| edge_distance(center, a, r.polygon[(i + 1) % r.polygon.len()]))
            .fold(f64::INFINITY, f64::min);
        let wear = (1. - exposure / 0.65).max(0.);
        let seed = f64::from(r.seed);
        let loss = value_noise(center[0] * 1.8, center[1] * 1.8, seed + 31.) * 0.88
            + lattice_hash(ix, iy, seed + 31.) * 0.12;
        let missing = loss < 0.12 + wear * 0.53;
        let mut cache = self.timber.borrow_mut();
        if cache.len() < 65_536 {
            cache.insert(key, missing);
        }
        missing
    }
    fn asphalt_distance(&self, x: f64, y: f64) -> f64 {
        if let Some((a, b, d)) = *self.asphalt_sample.borrow()
            && x == a
            && y == b
        {
            return d;
        }
        if self.asphalt.is_empty() {
            return f64::NEG_INFINITY;
        }
        let seed = f64::from(self.recipe.seed);
        let noise = |frequency, offset, amplitude| {
            (value_noise(x * frequency, y * frequency, seed + offset) - 0.5) * amplitude
        };
        let dx = noise(4.2, 401., 0.14) + noise(19., 402., 0.038);
        let dy = noise(4.2, 403., 0.14) + noise(19., 404., 0.038);
        let distance = self
            .asphalt
            .iter()
            .map(|(p, _)| coverage::signed_distance(x + dx, y + dy, p))
            .fold(f64::NEG_INFINITY, f64::max)
            + noise(67., 405., 0.018)
            + noise(157., 406., 0.006);
        *self.asphalt_sample.borrow_mut() = Some((x, y, distance));
        distance
    }
    fn uniform_asphalt(&self, area: Bounds, budget: &mut Budget<'_>) -> Result<bool, String> {
        for (polygon, f) in &self.asphalt {
            budget.spend(polygon.len())?;
            if f.uniform(area, 0.15, 0.15) {
                continue;
            }
            if !f.convex() {
                return Ok(false);
            }
            let mut noise =
                |frequency: f64, offset: f64, amplitude: f64| -> Result<[f64; 2], String> {
                    let bounds = area.map(|v| v * frequency);
                    let work = (bounds[2].floor() - bounds[0].floor() + 2.)
                        * (bounds[3].floor() - bounds[1].floor() + 2.);
                    budget.spend(if work > 4096. { 1 } else { work as usize })?;
                    Ok(
                        value_noise_range(bounds, f64::from(self.recipe.seed) + offset)?
                            .map(|v| (v - 0.5) * amplitude),
                    )
                };
            let cx = noise(4.2, 401., 0.14)?;
            let fx = noise(19., 402., 0.038)?;
            let cy = noise(4.2, 403., 0.14)?;
            let fy = noise(19., 404., 0.038)?;
            let warped = [
                area[0] + cx[0] + fx[0],
                area[1] + cy[0] + fy[0],
                area[2] + cx[1] + fx[1],
                area[3] + cy[1] + fy[1],
            ];
            let grain = noise(67., 405., 0.018)?;
            let chips = noise(157., 406., 0.006)?;
            if !f.uniform(
                warped,
                (0.0025 - grain[0] - chips[0]).max(0.),
                (0.0025 + grain[1] + chips[1]).max(0.),
            ) {
                return Ok(false);
            }
        }
        Ok(true)
    }
    pub fn feature_scale(&self, area: Bounds) -> f64 {
        if self.deposits.iter().any(|(d, f)| {
            matches!(d.material, Finish::Crack | Finish::Joint)
                && coverage::overlaps(area, f.bounds, 0.006)
        }) {
            0.004
        } else if matches!(self.recipe.kind, Finish::Tile | Finish::Timber) {
            0.008
        } else {
            0.02
        }
    }
    fn bounds_valid(area: Bounds) -> Result<(), String> {
        if area.iter().any(|v| !v.is_finite() || v.abs() > 1e6)
            || area[0] > area[2]
            || area[1] > area[3]
        {
            Err("Pavement query needs finite ordered bounds.".into())
        } else {
            Ok(())
        }
    }
    pub fn construction_cuts(
        &self,
        area: Bounds,
        budget: &mut Budget<'_>,
    ) -> Result<Option<[Vec<f64>; 2]>, String> {
        budget.spend(0)?;
        Self::bounds_valid(area)?;
        if self.recipe.kind != Finish::Timber {
            return Ok(None);
        }
        let perimeter = coverage::bounds(&self.recipe.polygon);
        let rectangular = self.recipe.polygon.len() == 4
            && self.recipe.polygon.iter().all(|p| {
                [perimeter[0], perimeter[2]]
                    .iter()
                    .any(|v| (v - p[0]).abs() < 1e-9)
                    && [perimeter[1], perimeter[3]]
                        .iter()
                        .any(|v| (v - p[1]).abs() < 1e-9)
            });
        let mut result: [Vec<f64>; 2] = Default::default();
        for (axis, span) in [0.16, 1.15].into_iter().enumerate() {
            let lower = area[axis];
            let upper = area[axis + 2];
            let bevel = if self.recipe.relief {
                vec![0.006, 0.00675, 0.0075, 0.00825, 0.009]
            } else {
                vec![0.0075]
            };
            let offsets = std::iter::once(0.)
                .chain(bevel.iter().copied())
                .chain(bevel.iter().map(|v| span - v))
                .collect::<Vec<_>>();
            let low = (lower / span).floor() as i64;
            let high = (upper / span).floor() as i64;
            let count = (high - low + 1) as usize;
            if count > 65_536 {
                return Err("Pavement construction cut budget exceeded.".into());
            }
            budget.spend(count * offsets.len())?;
            for unit in low..=high {
                for offset in &offsets {
                    let p = unit as f64 * span + offset;
                    if p > lower + 1e-9 && p < upper - 1e-9 {
                        result[axis].push(p);
                    }
                }
            }
            if self.recipe.relief && rectangular {
                for d in [0.00625, 0.0125, 0.01875, 0.025] {
                    for p in [perimeter[axis] + d, perimeter[axis + 2] - d] {
                        if p > lower + 1e-9 && p < upper - 1e-9 {
                            result[axis].push(p);
                        }
                    }
                }
            }
            result[axis].sort_by(f64::total_cmp);
            result[axis].dedup();
        }
        Ok(Some(result))
    }
    pub fn uniform_coverage(&self, area: Bounds, budget: &mut Budget<'_>) -> Result<bool, String> {
        Self::bounds_valid(area)?;
        budget.spend(
            self.deposits
                .iter()
                .map(|(d, _)| d.polygon.len())
                .sum::<usize>()
                + 1,
        )?;
        let r = self.recipe;
        if self.tiles.is_some() || !self.uniform_asphalt(area, budget)? {
            return Ok(false);
        }
        if r.kind == Finish::Timber {
            let ix = (area[0] / 0.16).floor();
            let iy = (area[1] / 1.15).floor();
            if ix != (area[2] / 0.16).floor() || iy != (area[3] / 1.15).floor() {
                return Ok(false);
            }
            if !self.timber_missing(ix, iy) {
                let left = area[0] - ix * 0.16;
                let right = (ix + 1.) * 0.16 - area[2];
                let bottom = area[1] - iy * 1.15;
                let top = (iy + 1.) * 1.15 - area[3];
                let midpoint = 0.0075;
                let roundoff = f64::EPSILON * area.iter().map(|v| v.abs()).fold(1., f64::max) * 32.;
                let grout = area[2] - ix * 0.16 <= midpoint + roundoff
                    || (ix + 1.) * 0.16 - area[0] <= midpoint + roundoff
                    || area[3] - iy * 1.15 <= midpoint + roundoff
                    || (iy + 1.) * 1.15 - area[1] <= midpoint + roundoff;
                if !grout && left.min(right).min(bottom).min(top) < midpoint - roundoff {
                    return Ok(false);
                }
            }
        }
        for (d, f) in &self.deposits {
            let margin = if fractured(d.material) { 0.016 } else { 0.0035 };
            if coverage::overlaps(area, f.bounds, margin) && !f.uniform(area, margin, margin) {
                return Ok(false);
            }
        }
        Ok(true)
    }
    pub fn sample(&self, x: f64, y: f64, footprint: f64) -> Result<Sample, String> {
        if !valid_point([x, y, footprint]) || !(0. ..=100.).contains(&footprint) {
            return Err("Invalid pavement sample coordinates.".into());
        }
        let r = self.recipe;
        let seed = f64::from(r.seed);
        let mut weights = vec![0.; self.finishes.len()];
        weights[0] = 1.;
        let mut height = 0.;
        let apply = |weights: &mut Vec<f64>, finish: Finish, coverage: f64| {
            if coverage > 0. {
                for w in weights.iter_mut() {
                    *w *= 1. - coverage;
                }
                weights[self.index(finish)] += coverage;
            }
        };
        if r.kind == Finish::Timber {
            let ix = (x / 0.16).floor();
            let iy = (y / 1.15).floor();
            let dx = x - ix * 0.16;
            let dy = y - iy * 1.15;
            let edge = dx.min(0.16 - dx).min(dy).min(1.15 - dy);
            let intact = if footprint > 0. {
                tile::coverage(x, 0.16, 0.0075, footprint)
                    * tile::coverage(y, 1.15, 0.0075, footprint)
            } else {
                smooth_step(0.006, 0.009, edge)
            };
            let retained = if self.timber_missing(ix, iy) { 0. } else { 1. };
            apply(&mut weights, Finish::Grout, 1. - intact);
            apply(&mut weights, Finish::Substrate, 1. - retained);
            let body = intact * retained;
            height += 0.0035 * body - 0.001 * (1. - body);
        }
        let tile_sample = self
            .tiles
            .as_ref()
            .map(|(t, m)| t.surface(x, y, footprint, m, [1.; 3]))
            .transpose()?;
        if let Some(t) = &tile_sample {
            weights.fill(0.);
            weights[0] = t.layers[1].weight;
            weights[self.index(Finish::Substrate)] = t.layers[0].weight;
            height = 0.;
        }
        for (d, f) in &self.deposits {
            if !coverage::overlaps([x, y, x, y], f.bounds, 0.06) {
                continue;
            }
            let distance = coverage::signed_distance(x, y, &d.polygon)
                + (value_noise(x * 48., y * 48., seed + 430.) - 0.5)
                    * if fractured(d.material) { 0.025 } else { 0.0018 }
                + (value_noise(x * 131., y * 131., seed + 431.) - 0.5) * 0.002;
            let coverage = smooth_step(-0.0015, 0.0015, distance);
            apply(&mut weights, d.material, coverage);
            if self.elevations.is_none() {
                height += coverage
                    * if d.material == Finish::Chip {
                        0.0015
                    } else if fractured(d.material) {
                        -0.004
                    } else {
                        -0.0015
                    };
            }
        }
        if !self.asphalt.is_empty() {
            let distance = self.asphalt_distance(x, y);
            let coverage = smooth_step(-0.0025, 0.0025, distance);
            apply(&mut weights, Finish::Asphalt, coverage);
            if self.elevations.is_none() {
                let edge = (1. - distance.abs() / 0.025).max(0.);
                height = height * (1. - coverage)
                    + coverage
                        * (0.0035
                            + edge * (value_noise(x * 72., y * 72., seed + 407.) - 0.5) * 0.0012);
            }
        }
        let asphalt_weight = self
            .finishes
            .iter()
            .position(|f| *f == Finish::Asphalt)
            .map_or(0., |i| weights[i]);
        let mut boundary_weight = 0.;
        if r.relief {
            let native = self
                .receiving_edges
                .iter()
                .map(|e| edge_distance([x, y, 0.], e[0], e[1]))
                .fold(f64::INFINITY, f64::min)
                .max(0.);
            let boundary = coverage::signed_distance(x, y, &self.polygon)
                .min(native)
                .max(0.);
            boundary_weight = smooth_step(0., 0.025, boundary) * (1. - asphalt_weight)
                + smooth_step(0., 0.025, native) * asphalt_weight;
        }
        height *= boundary_weight;
        let layers = self
            .sources
            .iter()
            .enumerate()
            .map(|(i, s)| {
                let mut layer = tile_sample
                    .as_ref()
                    .and_then(|t| t.layers.iter().find(|l| l.material == s.id))
                    .cloned()
                    .unwrap_or_else(|| Layer {
                        material: s.id.clone(),
                        weight: 0.,
                        uv: if self.finishes[i] == Finish::Asphalt && !self.asphalt.is_empty() {
                            [x * s.repeat, y * s.repeat]
                        } else {
                            [
                                (x * self.cosine - y * self.sine) * s.repeat + r.phase,
                                (x * self.sine + y * self.cosine) * s.repeat + r.phase * 0.37,
                            ]
                        },
                        tint: None,
                        roughness: None,
                        coatings: vec![],
                    });
                layer.weight = weights[i];
                layer
            })
            .collect();
        let bump = tile_sample.map(|t| {
            t.bump * (weights[0] + weights[self.index(Finish::Substrate)]) + asphalt_weight * 0.0035
        });
        Ok(Sample {
            layers,
            bump,
            height: if r.relief { height } else { 0. },
            roughness: None,
            height_weight: Some(if r.relief { boundary_weight } else { 0. }),
            layer_elevation_weight: self.elevations.as_ref().map(|_| boundary_weight),
        })
    }
    fn height(&self, sample: &Sample) -> f64 {
        let selected = sample
            .layers
            .iter()
            .enumerate()
            .max_by(|(a, x), (b, y)| x.weight.total_cmp(&y.weight).then_with(|| b.cmp(a)))
            .unwrap()
            .0;
        sample.height
            + self.elevations.as_ref().map_or(0., |v| {
                v[selected] * sample.layer_elevation_weight.unwrap_or(1.)
            })
    }
    pub fn rendered_height(&self, x: f64, y: f64, footprint: f64) -> Result<f64, String> {
        Ok(self.height(&self.sample(x, y, footprint)?))
    }
    pub fn tint(
        &self,
        x: f64,
        y: f64,
        layer: usize,
        color: &dyn Fn(V3, Finish) -> V3,
    ) -> Result<V3, String> {
        let finish = *self.finishes.get(layer).ok_or("Invalid pavement layer.")?;
        let soil = if self.asphalt.is_empty() {
            0.
        } else {
            (1. - smooth_step(0.004, 0.065, self.asphalt_distance(x, y).abs()))
                * (0.08 + value_noise(x * 17., y * 17., f64::from(self.recipe.seed) + 408.) * 0.12)
        };
        let color = if self.tiles.is_some()
            && matches!(finish, Finish::Tile | Finish::Substrate | Finish::Grout)
        {
            [1.; 3]
        } else {
            color([x, y, 0.], finish)
        };
        if color.iter().any(|v| !v.is_finite() || *v < 0.) {
            return Err("Pavement pigment must be finite nonnegative RGB.".into());
        }
        Ok(color.map(|v| v * (1. - soil)))
    }
    pub fn raster_sample(
        &self,
        x: f64,
        y: f64,
        footprint: f64,
        color: &dyn Fn(V3, Finish) -> V3,
    ) -> Result<Sample, String> {
        let mut s = self.sample(x, y, footprint)?;
        let bump = s.bump.unwrap_or_else(|| self.height(&s));
        for (i, l) in s.layers.iter_mut().enumerate() {
            if l.weight > 0. {
                let tint = self.tint(x, y, i, color)?;
                let prior = l.tint.unwrap_or([1.; 3]);
                l.tint = Some(std::array::from_fn(|k| prior[k] * tint[k]));
            }
        }
        s.height = 0.;
        s.bump = Some(bump);
        Ok(s)
    }
}

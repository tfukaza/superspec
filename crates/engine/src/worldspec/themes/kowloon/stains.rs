//! Ordered fixture deposits on finite, coplanar surfaces. No decal geometry is emitted.
mod runoff;
use super::noise::{smooth_step, value_noise};
use super::{
    bake_jobs::{Job, Recipe as RasterRecipe},
    mesh::Mesh,
    pen::Pen,
    surface::Sample,
};
pub(crate) use runoff::Settings as RunoffSettings;
use serde::Serialize;
use superspec_geometry::{
    V3,
    math::{dot, sub},
};

#[derive(Clone, Serialize)]
pub(super) struct Frame {
    pub origin: V3,
    pub u: V3,
    pub v: V3,
    pub normal: V3,
}
impl Frame {
    pub fn pen(pen: &Pen, depth: f64) -> Self {
        Self {
            origin: pen.world([0., depth, 0.]),
            u: pen.matrix.direction([1., 0., 0.]),
            v: pen.matrix.direction([0., 0., 1.]),
            normal: pen.matrix.direction([0., 1., 0.]),
        }
    }
    fn at(&self, u: f64, v: f64) -> V3 {
        std::array::from_fn(|i| self.origin[i] + self.u[i] * u + self.v[i] * v)
    }
    fn coordinates(&self, p: V3) -> V3 {
        let p = sub(p, self.origin);
        [dot(p, self.u), dot(p, self.v), dot(p, self.normal)]
    }
}
#[derive(Clone, Serialize)]
pub(super) struct Deposit {
    frame: Frame,
    bounds: [f64; 4],
    seed: u32,
    wash: V3,
    depth: f64,
    #[serde(
        skip_serializing_if = "Option::is_none",
        serialize_with = "serialize_runoff"
    )]
    runoff: Option<std::sync::Arc<runoff::Runoff>>,
}
fn serialize_runoff<S: serde::Serializer>(
    value: &Option<std::sync::Arc<runoff::Runoff>>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    value.as_deref().serialize(serializer)
}
#[derive(Clone, Serialize)]
struct Mapped {
    deposit: Deposit,
    receiver: Frame,
}
#[derive(Clone, Serialize)]
enum Base {
    Facade(super::skin::Deposits),
    Tile(super::tile::Recipe),
    TiledSkin(Box<super::tiled_skin::Recipe>),
    Sign(Box<super::signs::weathering::Recipe>),
}
#[derive(Clone, Serialize)]
pub struct Recipe {
    base: Base,
    deposits: Vec<Mapped>,
}
impl Recipe {
    pub(super) fn conforms_edges(&self) -> bool {
        matches!(self.base, Base::Sign(_))
    }
    pub(super) fn bounds(&self) -> [f64; 4] {
        match &self.base {
            Base::Facade(r) => r.bounds,
            Base::Tile(r) => r.bounds,
            Base::TiledSkin(r) => r.bounds,
            Base::Sign(r) => [0., 0., r.width, r.height],
        }
    }
    pub(super) fn source(&self) -> &str {
        match &self.base {
            Base::Facade(r) => &r.finish.coat.material,
            Base::Tile(r) => &r.materials.bed,
            Base::TiledSkin(r) => &r.finish.coat.material,
            Base::Sign(r) => &r.paints[0].slot,
        }
    }
    pub(super) fn owned_bytes(&self) -> usize {
        let base = match &self.base {
            Base::Facade(r) => {
                r.finish.coat.material.capacity()
                    + r.finish.backing.material.capacity()
                    + r.condition.ledges.capacity() * std::mem::size_of::<super::wall::Ledge>()
                    + r.condition.openings.capacity() * std::mem::size_of::<super::wall::Opening>()
            }
            Base::Tile(r) => {
                r.materials.ceramic.capacity()
                    + r.materials.bed.capacity()
                    + r.materials.grime.capacity()
            }
            Base::TiledSkin(r) => r.owned_bytes(),
            Base::Sign(r) => r.owned_bytes(),
        };
        base + self.deposits.capacity() * std::mem::size_of::<Mapped>()
    }
}
enum Intrinsic<'a> {
    Facade(super::skin::Deposited<'a>),
    Tile(super::tile::Field<'a>),
    TiledSkin(Box<super::tiled_skin::Field<'a>>),
    Sign(super::signs::weathering::Field<'a>),
}
pub struct Field<'a> {
    recipe: &'a Recipe,
    intrinsic: Intrinsic<'a>,
}
impl<'a> Field<'a> {
    pub fn new(recipe: &'a Recipe) -> Result<Self, String> {
        let intrinsic = match &recipe.base {
            Base::Facade(r) => Intrinsic::Facade(super::skin::Deposited::new(r)?),
            Base::Tile(r) => Intrinsic::Tile(super::tile::Field::new(r)?),
            Base::TiledSkin(r) => Intrinsic::TiledSkin(Box::new(super::tiled_skin::Field::new(r)?)),
            Base::Sign(r) => Intrinsic::Sign(super::signs::weathering::Field::new(r)?),
        };
        Ok(Self { recipe, intrinsic })
    }
    pub fn structure(&self) -> Sample {
        match &self.intrinsic {
            Intrinsic::Facade(f) => f.structure(),
            Intrinsic::Tile(f) => f.structure(),
            Intrinsic::TiledSkin(f) => f.structure(),
            Intrinsic::Sign(f) => f.structure(),
        }
    }
    pub fn sample(&self, u: f64, v: f64) -> Result<Sample, String> {
        let (mut sample, retention) = match &self.intrinsic {
            Intrinsic::Facade(f) => (f.sample(u, v)?, 1.),
            Intrinsic::Tile(f) => (f.sample(u, v)?, 1.),
            Intrinsic::TiledSkin(f) => f.sample_with_retention(u, v)?,
            Intrinsic::Sign(f) => (f.sample(u, v)?, 1.),
        };
        for mapped in &self.recipe.deposits {
            let d = &mapped.deposit;
            let [x, y, _] = d.frame.coordinates(mapped.receiver.at(u, v));
            let [x0, bottom, x1, top] = d.bounds;
            if x < x0 || x > x1 || y < bottom || y > top {
                continue;
            }
            let width = x1 - x0;
            let height = top - bottom;
            let seed = f64::from(d.seed);
            let fall = (top - y) / height;
            let center = (x0 + x1) / 2. + (value_noise(y * 8., 0., seed) - 0.5) * width * 0.12;
            let reach = width * (0.46 - fall * 0.3);
            let sides = 1. - smooth_step(reach * 0.3, reach, (x - center).abs());
            let ends =
                smooth_step(bottom, bottom + height * 0.35, y) * smooth_step(top, top - 0.012, y);
            let threads = 0.45 + value_noise(x * 65., y * 2., seed + 17.) * 0.55;
            let (strength, pigment, roughness) = if let Some(runoff) = &d.runoff {
                let (strength, pigment) = runoff.sample((x - (x0 + x1) / 2.) / width, fall);
                (strength * retention, pigment, 0.9)
            } else {
                (
                    ((d.depth * 1.6).min(1.) * sides * ends * threads).clamp(0., 1.) * retention,
                    d.wash.map(|channel| 0.38 + channel * 0.24),
                    0.94,
                )
            };
            if strength == 0. {
                continue;
            }
            for layer in &mut sample.layers {
                let tint = layer.tint.get_or_insert([1.; 3]);
                for (i, c) in tint.iter_mut().enumerate() {
                    *c *= 1. + (pigment[i] - 1.) * strength;
                }
            }
            sample.roughness =
                Some(sample.roughness.unwrap_or(0.9) * (1. - strength) + roughness * strength);
        }
        Ok(sample)
    }
}
/// Bounds are [left, bottom, right, top] in the attaching pen's local X/Z frame.
pub fn stain(
    mesh: &mut Mesh<'_>,
    pen: &Pen,
    bounds: [f64; 4],
    wash: V3,
    depth: f64,
) -> Result<(), String> {
    mesh.check_work(0)?;
    if bounds.iter().any(|v| !v.is_finite() || v.abs() > 10000.)
        || wash
            .iter()
            .any(|v| !v.is_finite() || !(0. ..=1.).contains(v))
        || !depth.is_finite()
        || !(0. ..=1.).contains(&depth)
    {
        return Err("Fixture stain requires bounded coordinates, color and depth.".into());
    }
    if bounds[0] >= bounds[2] || bounds[1] >= bounds[3] || mesh.surveying() {
        return Ok(());
    }
    let number = |v| ryu_js::Buffer::new().format(v).to_string();
    let seed = super::wall::finish_seed(&format!(
        "{}/{}/{}/{}",
        pen.part,
        number(bounds[0]),
        number(bounds[2]),
        number(bounds[3])
    ));
    super::weathered::deposit(
        mesh,
        Deposit {
            frame: Frame::pen(pen, 0.),
            bounds,
            seed,
            wash,
            depth,
            runoff: None,
        },
    )
}
pub(crate) fn runoff(
    mesh: &mut Mesh<'_>,
    pen: &Pen,
    bounds: [f64; 4],
    settings: RunoffSettings<'_>,
) -> Result<(), String> {
    mesh.check_work(0)?;
    if bounds.iter().any(|v| !v.is_finite() || v.abs() > 10000.)
        || bounds[0] >= bounds[2]
        || bounds[1] >= bounds[3]
    {
        return Err("Runoff needs finite, positive wall-local bounds.".into());
    }
    if mesh.surveying() {
        return Ok(());
    }
    let runoff = std::sync::Arc::new(runoff::Runoff::new(settings)?);
    super::weathered::deposit(
        mesh,
        Deposit {
            frame: Frame::pen(pen, 0.),
            bounds,
            seed: 0,
            wash: [1.; 3],
            depth: 0.,
            runoff: Some(runoff),
        },
    )
}
pub(super) struct Receiver {
    pub frame: Frame,
    pub fallback: Option<super::skin::Deposits>,
}
impl Receiver {
    pub fn owned_bytes(&self) -> usize {
        self.fallback
            .as_ref()
            .map_or(0, super::skin::Deposits::owned_bytes)
    }
    pub fn apply(
        self,
        mut job: Job,
        deposits: &[Deposit],
        mesh: &mut Mesh<'_>,
        bytes: &mut usize,
    ) -> Result<Job, String> {
        let bounds = self
            .fallback
            .as_ref()
            .map_or_else(|| job.bounds(), |r| r.bounds);
        let mut mapped = Vec::new();
        for d in deposits {
            mesh.spend_work(1)?;
            if dot(self.frame.normal, d.frame.normal) < 0.99999 {
                continue;
            }
            let [x0, y0, x1, y1] = d.bounds;
            let corners = [[x0, y0], [x1, y0], [x1, y1], [x0, y1]]
                .map(|p| self.frame.coordinates(d.frame.at(p[0], p[1])));
            if corners.iter().any(|p| p[2].abs() > 0.012) {
                continue;
            }
            let overlap = (0..2).all(|a| {
                corners.iter().map(|p| p[a]).fold(f64::INFINITY, f64::min) < bounds[a + 2]
                    && corners
                        .iter()
                        .map(|p| p[a])
                        .fold(f64::NEG_INFINITY, f64::max)
                        > bounds[a]
            });
            if !overlap {
                continue;
            }
            *bytes += std::mem::size_of::<Mapped>();
            if *bytes > 4 * 1024 * 1024 || mapped.len() >= 256 {
                return Err("Fixture stain mappings exceed their bounded allowance.".into());
            }
            mapped.push(Mapped {
                deposit: d.clone(),
                receiver: self.frame.clone(),
            });
        }
        if mapped.is_empty() {
            return Ok(job);
        }
        let base = match job.recipe {
            RasterRecipe::Facade(_) => {
                let r = self
                    .fallback
                    .ok_or("Shared facade lacks finite stain receiver.")?;
                job.density = r.finish.density.min(128.);
                Base::Facade(r)
            }
            RasterRecipe::FacadeDeposits(r) => Base::Facade(r),
            RasterRecipe::Tile(r) => Base::Tile(r),
            RasterRecipe::TiledSkin(r) => Base::TiledSkin(r),
            RasterRecipe::Sign(r) => Base::Sign(r),
            _ => return Err("UV raster cannot receive spatial deposits.".into()),
        };
        job.recipe = RasterRecipe::Stained(Box::new(Recipe {
            base,
            deposits: mapped,
        }));
        Ok(job)
    }
}

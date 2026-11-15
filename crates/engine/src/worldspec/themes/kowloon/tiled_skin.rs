//! Finite ceramic-clad wall finish with repair campaigns, source UVs and local weathering.
use super::{
    facade_material::{Facade, Settings as FacadeSettings},
    metal::Binding,
    render_repair::{self, Repairs, StudyUv},
    skin::Wash,
    surface::{Coat, Layer, Sample},
    tile,
    wall::{self, Condition},
};
use serde::{Deserialize, Serialize};
/// Applies one finite wall field to the exposed triangles, preserving authored holes and slopes.
pub fn faces(
    drawing: &mut super::fixture::Fixture<'_, '_>,
    faces: &[[superspec_geometry::V3; 3]],
    recipe: &Recipe,
    group: &str,
) -> Result<(), String> {
    Field::new(recipe)?;
    for points in faces {
        super::bake_jobs::face(
            drawing,
            &recipe.finish.coat.material,
            points,
            super::bake_jobs::Job {
                density: recipe.finish.density,
                recipe: super::bake_jobs::Recipe::TiledSkin(Box::new(recipe.clone())),
            },
            group.into(),
        )?;
    }
    Ok(())
}
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Settings {
    pub seed: u32,
    pub field: tile::Settings,
    pub materials: tile::Materials,
    pub repair: Binding,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Recipe {
    pub bounds: [f64; 4],
    pub finish: FacadeSettings,
    pub condition: wall::Settings,
    pub tiles: Settings,
    pub washes: Vec<Wash>,
    pub openings: Vec<[f64; 4]>,
    pub mapping_seed: u32,
}
impl Recipe {
    pub(super) fn owned_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + self.finish.coat.material.capacity()
            + self.finish.backing.material.capacity()
            + self.tiles.repair.material.capacity()
            + self.tiles.materials.ceramic.capacity()
            + self.tiles.materials.bed.capacity()
            + self.tiles.materials.grime.capacity()
            + self.condition.ledges.capacity() * std::mem::size_of::<wall::Ledge>()
            + self.condition.openings.capacity() * std::mem::size_of::<wall::Opening>()
            + self.washes.capacity() * std::mem::size_of::<Wash>()
            + self.openings.capacity() * std::mem::size_of::<[f64; 4]>()
    }
}
pub struct Field<'a> {
    recipe: &'a Recipe,
    finish: Facade<'a>,
    condition: Condition,
    repairs: Repairs,
    tiles: tile::Tile,
    uvs: [StudyUv; 2],
    repair_uv: StudyUv,
}
impl<'a> Field<'a> {
    pub fn new(recipe: &'a Recipe) -> Result<Self, String> {
        let r = recipe;
        let b = r.bounds;
        if b.iter().any(|v| !v.is_finite() || v.abs() > 1000.)
            || b[0] >= b[2]
            || b[1] >= b[3]
            || r.finish.density < 0.1
            || r.condition
                .ledges
                .iter()
                .any(|l| !l.u0.is_finite() || !l.u1.is_finite())
            || r.tiles.repair.material.is_empty()
            || r.tiles.repair.material.len() > 4096
            || r.openings.len() > 256
            || r.openings.iter().any(|b| {
                b.iter().any(|v| !v.is_finite() || v.abs() > 10000.) || b[0] >= b[2] || b[1] >= b[3]
            })
            || r.washes.len() > 256
            || r.washes.iter().any(|w| {
                w.bounds.iter().any(|v| !v.is_finite() || v.abs() > 10000.)
                    || w.bounds[0] >= w.bounds[2]
                    || w.bounds[1] >= w.bounds[3]
                    || w.tint
                        .iter()
                        .any(|v| !v.is_finite() || !(0. ..=4.).contains(v))
            })
        {
            return Err(
                "Tiled wall finish requires bounded field, finite exposure and wash program."
                    .into(),
            );
        }
        r.tiles.materials.validate()?;
        let patches = if r.finish.concrete {
            vec![]
        } else {
            render_repair::plan(b, r.tiles.seed, &r.openings)?
        };
        Ok(Self {
            recipe,
            finish: Facade::new(&r.finish)?,
            condition: Condition::new(&r.condition)?,
            repairs: Repairs::new(&patches)?,
            tiles: tile::Tile::new(&r.tiles.field)?,
            uvs: [
                StudyUv::new(r.finish.coat.repeat, f64::from(r.mapping_seed))?,
                StudyUv::new(r.finish.backing.repeat, f64::from(r.mapping_seed) + 41.)?,
            ],
            repair_uv: StudyUv::new(r.tiles.repair.repeat, f64::from(r.tiles.seed) + 71.)?,
        })
    }
    pub fn structure(&self) -> Sample {
        let r = self.recipe;
        let layer = |material: &str| Layer {
            material: material.into(),
            weight: 1.,
            uv: [0.; 2],
            tint: Some([1.; 3]),
            roughness: None,
            coatings: vec![],
        };
        let mut coat = layer(&r.finish.coat.material);
        coat.coatings.push(Coat {
            material: r.tiles.repair.material.clone(),
            uv: [0.; 2],
            opacity: 0.,
            tint: None,
            roughness: None,
        });
        let mut ceramic = layer(&r.tiles.materials.ceramic);
        ceramic.coatings.push(Coat {
            material: r.tiles.materials.grime.clone(),
            uv: [0.; 2],
            opacity: 0.,
            tint: None,
            roughness: None,
        });
        Sample {
            height: 0.,
            roughness: Some(0.9),
            bump: Some(0.),
            height_weight: None,
            layer_elevation_weight: None,
            layers: vec![
                coat,
                layer(&r.finish.backing.material),
                layer(&r.tiles.materials.bed),
                ceramic,
            ],
        }
    }
    pub fn sample(&self, u: f64, v: f64) -> Result<Sample, String> {
        self.sample_with_retention(u, v).map(|v| v.0)
    }
    pub(super) fn sample_with_retention(&self, u: f64, v: f64) -> Result<(Sample, f64), String> {
        let r = self.recipe;
        let wash = r
            .washes
            .iter()
            .rev()
            .find(|w| w.contains(u, v))
            .map_or(r.finish.coat.tint, |w| w.tint);
        let wall = self.condition.sample(u, v)?;
        let mut sample = self.condition.plaster_sample_from(
            [u, v],
            f64::from(r.mapping_seed),
            &wall::Coating {
                material: r.finish.coat.material.clone(),
                uv: [u * r.finish.coat.repeat, v * r.finish.coat.repeat],
                tint: wash,
            },
            &wall::Backing {
                material: r.finish.backing.material.clone(),
                uv: [u * r.finish.backing.repeat, v * r.finish.backing.repeat],
            },
            &wall,
        )?;
        let repair = self.repairs.sample(u, v)?;
        let plaster = self.finish.finish(u, v)?;
        let tint = render_repair::patina(
            &self.condition,
            &repair,
            plaster.tone,
            &wall,
            u,
            v,
            r.tiles.seed,
        );
        let exposed = plaster.exposed * (1. - repair.coverage);
        sample.bump = Some(plaster.bump + repair.height);
        for (index, layer) in sample.layers.iter_mut().enumerate() {
            layer.weight = if index == 0 { 1. - exposed } else { exposed };
            layer.uv = self.uvs[index].sample(u, v);
            let original = layer.tint.unwrap();
            layer.tint = Some(std::array::from_fn(|axis| {
                original[axis] * tint[axis] / wall.tint[axis]
                    * if index == 1 {
                        plaster.backing_tone / plaster.tone
                            * if r.finish.concrete { wash[axis] } else { 1. }
                    } else {
                        1.
                    }
            }));
            if index == 0 {
                layer.coatings = vec![Coat {
                    material: r.tiles.repair.material.clone(),
                    uv: self.repair_uv.sample(u, v),
                    opacity: repair.coverage,
                    tint: None,
                    roughness: None,
                }];
            }
        }
        let ceramic =
            self.tiles
                .surface(u, v, 1. / r.finish.density, &r.tiles.materials, [1.; 3])?;
        for layer in &mut sample.layers {
            layer.weight *= 1. - ceramic.installed;
        }
        for mut layer in ceramic.layers {
            layer.tint = Some(std::array::from_fn(|axis| {
                layer.tint.unwrap()[axis]
                    * tint[axis]
                    * if layer.material == r.tiles.materials.ceramic {
                        wash[axis]
                    } else {
                        1.
                    }
            }));
            sample.layers.push(layer);
        }
        sample.bump = Some(sample.bump.unwrap_or(0.) * (1. - ceramic.installed) + ceramic.bump);
        sample.roughness =
            Some(sample.roughness.unwrap_or(0.9) * repair.age + 0.82 * (1. - repair.age));
        Ok((sample, repair.age))
    }
}

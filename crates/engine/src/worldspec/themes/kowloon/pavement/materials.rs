//! Shared ground finishes preserve authored overrides and source PBR channels.
use super::{
    render::Assets,
    surface::{Finish, Source},
};
use std::collections::{BTreeMap, BTreeSet};
use superspec_catalog as material_catalog;
use superspec_types::material::Material;
pub struct Settings<'a> {
    pub scope: &'a str,
    pub base: &'a str,
    pub declared: &'a BTreeMap<String, String>,
    pub colors: &'a BTreeMap<String, String>,
    pub authored: &'a BTreeSet<String>,
}
fn look(name: &str) -> Option<(&'static str, f64, f64, bool)> {
    Some(match name {
        "concrete" => ("#c4c1b5", 0.94, 0., true),
        "repair" => ("#bbb9af", 0.96, 0., true),
        "grout" => ("#58584d", 1., 0., true),
        "substrate" => ("#96917e", 0.98, 0., true),
        "joint" => ("#626153", 1., 0., true),
        "edge" => ("#9c998c", 0.96, 0., true),
        "tile" => ("#a4b2a6", 0.78, 0., true),
        "timber" => ("#897055", 0.92, 0., true),
        "water-film" => ("#2f3936", 0.1, 0., false),
        "crack" => ("#36392e", 1., 0., false),
        "paper" => ("#c9baa0", 0.96, 0., false),
        "brick" => ("#796451", 0.98, 0., true),
        "can" => ("#a5a394", 0.4, 0.7, false),
        "asphalt" => ("#535550", 0.98, 0., true),
        "spall" => ("#777465", 1., 0., true),
        "chip" => ("#b5b0a0", 0.98, 0., true),
        "manhole-rim" => ("#494940", 0.75, 0.65, false),
        "manhole-cover" => ("#68665b", 0.72, 0.7, true),
        "manhole-slot" => ("#292b26", 0.95, 0.2, false),
        _ => return None,
    })
}
pub fn resolve(
    name: &str,
    settings: &Settings<'_>,
    materials: &mut BTreeMap<String, Material>,
) -> Result<String, String> {
    if name.len() > 256
        || settings.scope.len() > 2048
        || settings.base.len() > 4096
        || materials.len() > 16384
    {
        return Err("Ground material identity or count exceeds allowance.".into());
    }
    if let Some(id) = settings.declared.get(name) {
        materials
            .get(id)
            .ok_or_else(|| format!("Unknown declared ground material {id}."))?
            .validate()?;
        return Ok(id.clone());
    }
    let shared = match name {
        "tile" => Some("$kowloon/wall-ceramic"),
        "tile-bed" => Some("$kowloon/wall-tile-bed"),
        "tile-grime" => Some("$kowloon/wall-tile-grime"),
        "asphalt" => Some("builtin:asphalt"),
        "timber" => Some("builtin:timber"),
        _ => None,
    };
    if let Some(id) = shared.filter(|id| id.starts_with("builtin:"))
        && !materials.contains_key(id)
    {
        materials.insert(
            id.into(),
            material_catalog::material(id).ok_or("Missing shipped ground material.")?,
        );
    }
    let shared = shared.and_then(|id| materials.get(id)).cloned();
    let color = settings
        .colors
        .get(name)
        .map(String::as_str)
        .or((name == "tile").then_some("#58635e"));
    if let Some(shared) = &shared
        && color.is_none()
    {
        shared.validate()?;
        return Ok(shared.id.clone());
    }
    let id = format!("$ground/{}/{name}", settings.scope);
    if settings.authored.contains(&id) {
        return Err(format!(
            "Reserved ground material {id} is already declared."
        ));
    }
    if let Some(existing) = materials.get(&id) {
        existing.validate()?;
        return Ok(id);
    }
    let material = if let Some(mut shared) = shared {
        shared.id = id.clone();
        shared.color = color.unwrap().into();
        shared
    } else {
        let base = materials
            .get(settings.base)
            .ok_or("Unknown base ground material.")?;
        let (default_color, roughness, metalness, textured) =
            look(name).ok_or_else(|| format!("Missing shared ground material {name}."))?;
        let normal = if textured && name != "edge" {
            base.normal_texture.clone()
        } else {
            None
        };
        Material {
            id: id.clone(),
            color: color.unwrap_or(default_color).into(),
            repeat: base.repeat,
            roughness,
            metalness,
            opacity: (name == "water-film").then_some(0.16),
            texture: if textured { base.texture.clone() } else { None },
            metallic_roughness_texture: if textured {
                base.metallic_roughness_texture.clone()
            } else {
                None
            },
            height_texture: if textured {
                base.height_texture.clone()
            } else {
                None
            },
            height_scale: if textured && base.height_texture.is_some() {
                base.height_scale
            } else {
                None
            },
            height_offset: if textured && base.height_texture.is_some() {
                base.height_offset
            } else {
                None
            },
            normal_scale: normal.as_ref().map(|_| {
                base.normal_scale.unwrap_or(1.)
                    * if matches!(name, "tile" | "timber") {
                        0.25
                    } else if matches!(name, "asphalt" | "spall") {
                        1.8
                    } else {
                        1.
                    }
            }),
            normal_texture: normal,
            ..Material::default()
        }
    };
    material.validate()?;
    materials.insert(id.clone(), material);
    Ok(id)
}
pub fn assets(
    settings: &Settings<'_>,
    materials: &mut BTreeMap<String, Material>,
) -> Result<Assets, String> {
    let mut source = |name| -> Result<Source, String> {
        let id = resolve(name, settings, materials)?;
        let m = &materials[&id];
        Ok(Source {
            id,
            repeat: m.repeat,
            roughness: Some(m.roughness),
        })
    };
    let mut sources = BTreeMap::new();
    for (finish, name) in [
        (Finish::Concrete, "concrete"),
        (Finish::Repair, "repair"),
        (Finish::Tile, "tile"),
        (Finish::Timber, "timber"),
        (Finish::Grout, "grout"),
        (Finish::Substrate, "substrate"),
        (Finish::Asphalt, "asphalt"),
        (Finish::ManholeRim, "manhole-rim"),
        (Finish::ManholeCover, "manhole-cover"),
        (Finish::ManholeSlot, "manhole-slot"),
        (Finish::Crack, "crack"),
        (Finish::Joint, "joint"),
        (Finish::Spall, "spall"),
        (Finish::Chip, "chip"),
        (Finish::TileGrime, "tile-grime"),
    ] {
        sources.insert(finish, source(name)?);
    }
    Ok(Assets {
        sources,
        tile_bed: source("tile-bed")?,
        edge: source("edge")?,
        water: source("water-film")?,
        paper: source("paper")?,
        brick: source("brick")?,
        can: source("can")?,
    })
}

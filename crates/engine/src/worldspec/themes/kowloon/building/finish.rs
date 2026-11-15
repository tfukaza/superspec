//! One seeded construction choice per lot, independent of facade order and drawing detail.
use crate::worldspec::themes::kowloon::{
    facade_envelope::Finish, kit::Kit, massing::Lot, metal::Binding, tile, tiled_skin,
};
pub struct Materials {
    pub wall: Binding,
    pub concrete: Binding,
    pub substrates: Vec<Binding>,
    pub tiles: tile::Materials,
    pub repair: Binding,
    pub density: f64,
}
pub fn choose(kit: &Kit, lot: &Lot, materials: &Materials) -> Result<Finish, String> {
    if materials.substrates.len() > 256
        || !materials.density.is_finite()
        || !(0.1..=4096.).contains(&materials.density)
    {
        return Err("Building finishes require bounded substrate and texel budgets.".into());
    }
    let mut rng = kit.rng(&format!("{}/cladding", lot.key))?;
    let bare = if materials.substrates.is_empty() {
        materials.concrete.clone()
    } else {
        materials.substrates[rng.integer(0, materials.substrates.len() - 1)].clone()
    };
    let concrete = rng.chance(0.3);
    let ground_tiles = rng.chance(0.45);
    let seed = rng.integer(1, 2147483647) as u32;
    let weather = &kit.settings().weathering;
    Ok(Finish {
        coat: if concrete {
            bare.clone()
        } else {
            materials.wall.clone()
        },
        bare,
        concrete,
        seed,
        density: materials.density,
        tiles: ground_tiles.then(|| tiled_skin::Settings {
            // Facade envelope replaces the repair stream with that face's seed.
            seed,
            field: tile::Settings {
                seed,
                share: 1.,
                wear: weather.wear * 0.5,
                grime: weather.grime,
                size: Some(0.18),
                aspect: Some(1.),
            },
            materials: tile::Materials {
                repeat: 1.,
                roughness: 0.65,
                ..materials.tiles.clone()
            },
            repair: materials.repair.clone(),
        }),
    })
}

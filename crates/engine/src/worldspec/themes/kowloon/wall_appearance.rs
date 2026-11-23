//! Wall finish slots remain distinct from authored material identities.
use super::{kit::Kit, kit_materials::Library, metal::Binding, signs::panel, timber, weathered};
use std::collections::BTreeMap;
use superspec_types::material::Material;

pub struct Appearance {
    pub inputs: BTreeMap<String, Material>,
    pub bindings: BTreeMap<String, String>,
}

pub(crate) struct Settings<'a> {
    pub seed: &'a str,
    pub grime: f64,
    pub wear: f64,
    pub density: f64,
}

impl Appearance {
    /// Registers shared finishes and builds a slot namespace for the receiving wall.
    pub fn new(library: &mut Library, wall: &str, substrates: &[String]) -> Result<Self, String> {
        let mut bindings = library.bind(wall, substrates)?.materials;
        for slot in ["wall", "concrete"]
            .into_iter()
            .map(str::to_owned)
            .chain((0..substrates.len()).map(|i| format!("substrate-{i}")))
        {
            if library.materials[&bindings[&slot]].height_texture.is_some() {
                return Err("Wall finish inputs do not support displacement.".into());
            }
        }
        for id in [
            "$kowloon/wall-ceramic",
            "$kowloon/wall-tile-bed",
            "$kowloon/wall-tile-grime",
        ] {
            bindings.insert(id.into(), id.into());
        }
        let mut inputs = BTreeMap::new();
        for (slot, id) in &bindings {
            let mut definition = library
                .materials
                .get(id)
                .ok_or("Missing kit material definition.")?
                .clone();
            definition.id = slot.clone();
            inputs.insert(slot.clone(), definition);
        }
        Ok(Self { inputs, bindings })
    }

    pub(crate) fn repeat(&self, slot: &str) -> f64 {
        self.inputs.get(slot).map_or(1., |m| m.repeat)
    }

    fn binding(&self, slot: &str) -> Binding {
        Binding {
            material: slot.into(),
            repeat: self.repeat(slot),
        }
    }

    pub(crate) fn kit(&self, settings: &Settings<'_>) -> Result<Kit, String> {
        Kit::new(
            panel::Settings {
                weathering: weathered::Settings {
                    seed: settings.seed.into(),
                    grime: settings.grime,
                    wear: settings.wear,
                    rust: settings.wear,
                    texture_density: settings.density,
                    metal: self.binding("metal"),
                    sheet: self.binding("sheet"),
                    paint: self.binding("paint"),
                },
                lettering: true,
                timber: timber::Settings {
                    textured: self.inputs["shop-timber"].texture.is_some(),
                    timber_repeat: self.repeat("timber"),
                    atlas_repeat: self.repeat("shop-timber"),
                    ..timber::Settings::default()
                },
            },
            1,
        )
    }
}

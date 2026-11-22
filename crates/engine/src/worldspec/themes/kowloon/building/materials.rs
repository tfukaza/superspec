//! Host-local kit slots are distinct from authored material identities, even when their names coincide.
use super::{Plan, draw, finish};
use crate::worldspec::themes::kowloon::{
    deck,
    kit::Kit,
    kit_materials::Library,
    mesh::Mesh,
    metal::Binding,
    pavement, shop,
    signs::{book::Book, panel},
    tile, timber, weathered,
};
use std::collections::{BTreeMap, BTreeSet};
use superspec_aml::material::Material;
use superspec_geometry::obstructions::Budget;

pub struct Appearance {
    /// Sampling definitions use local slot identities. They never enter the scene's authored namespace.
    pub inputs: BTreeMap<String, Material>,
    /// Scene bindings retain the original material identity and its authored PBR definition.
    pub bindings: BTreeMap<String, String>,
    pavement: pavement::render::Assets,
    substrates: usize,
}

pub struct Settings<'a> {
    pub seed: &'a str,
    pub grime: f64,
    pub wear: f64,
    pub rust: f64,
    pub density: f64,
    pub detail: u8,
}

impl Appearance {
    /// Registers shared delivery materials while creating an isolated sampling namespace for this host.
    pub fn new(library: &mut Library, wall: &str, substrates: &[String]) -> Result<Self, String> {
        let resolved = library.bind(wall, substrates)?;
        let mut bindings = resolved.materials;
        bindings.insert("roofing".into(), resolved.roofing);
        for slot in ["wall", "concrete", "roofing"]
            .into_iter()
            .map(str::to_owned)
            .chain((0..substrates.len()).map(|i| format!("substrate-{i}")))
        {
            if library.materials[&bindings[&slot]].height_texture.is_some() {
                return Err(
                    "Building wall, substrate and deck displacement is not connected yet.".into(),
                );
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
        // Generate deck definitions in the delivery namespace, then bind only their returned identities.
        let scope = format!("$deck/{}", bindings["concrete"]);
        let authored: BTreeSet<_> = library.authored_ids().cloned().collect();
        let pavement = pavement::materials::assets(
            &pavement::materials::Settings {
                scope: &scope,
                base: &bindings["concrete"],
                declared: &BTreeMap::from([("asphalt".into(), bindings["roofing"].clone())]),
                colors: &BTreeMap::from([("tile".into(), "#a8674c".into())]),
                authored: &authored,
            },
            &mut library.materials,
        )?;
        // Roof felt may have an authored name identical to a kit slot. Copy it under its own local role.
        let mut local_pavement = pavement;
        for source in local_pavement.sources.values_mut().chain([
            &mut local_pavement.tile_bed,
            &mut local_pavement.edge,
            &mut local_pavement.water,
            &mut local_pavement.paper,
            &mut local_pavement.brick,
            &mut local_pavement.can,
        ]) {
            let id = source.id.clone();
            let slot = format!("$deck-source/{id}");
            let mut definition = library
                .materials
                .get(&id)
                .ok_or("Missing deck material definition.")?
                .clone();
            if definition.height_texture.is_some() {
                return Err("Building deck displacement is not connected yet.".into());
            }
            definition.id = slot.clone();
            inputs.insert(slot.clone(), definition);
            bindings.insert(slot.clone(), id);
            source.id = slot;
        }
        Ok(Self {
            inputs,
            bindings,
            pavement: local_pavement,
            substrates: substrates.len(),
        })
    }

    pub fn repeat(&self, slot: &str) -> f64 {
        self.inputs.get(slot).map_or(1., |m| m.repeat)
    }
    fn binding(&self, slot: &str) -> Binding {
        Binding {
            material: slot.into(),
            repeat: self.repeat(slot),
        }
    }
    fn textured(&self, slot: &str) -> bool {
        self.inputs.get(slot).is_some_and(|m| m.texture.is_some())
    }
    pub fn kit(&self, settings: &Settings<'_>) -> Result<Kit, String> {
        Kit::new(
            panel::Settings {
                weathering: weathered::Settings {
                    seed: settings.seed.into(),
                    grime: settings.grime,
                    wear: settings.wear,
                    rust: settings.rust,
                    texture_density: settings.density,
                    metal: self.binding("metal"),
                    sheet: self.binding("sheet"),
                    paint: self.binding("paint"),
                },
                lettering: true,
                timber: timber::Settings {
                    textured: self.textured("shop-timber"),
                    timber_repeat: self.repeat("timber"),
                    atlas_repeat: self.repeat("shop-timber"),
                    ..timber::Settings::default()
                },
            },
            settings.detail,
        )
    }

    /// Draws one planned host with real source repeats and independently bound optional atlases.
    pub fn draw(
        &self,
        plan: &Plan,
        index: usize,
        mesh: &mut Mesh<'_>,
        settings: &Settings<'_>,
        budget: &mut Budget<'_>,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Book, String> {
        let site = plan.sites().get(index).ok_or("Unknown building site.")?;
        mesh.bind_source_materials(&self.bindings)?;
        let kit = self.kit(settings)?;
        let ceramic = &self.inputs["$kowloon/wall-ceramic"];
        let bed = &self.inputs["$kowloon/wall-tile-bed"];
        let materials = finish::Materials {
            wall: self.binding("wall"),
            concrete: self.binding("concrete"),
            substrates: (0..self.substrates)
                .map(|i| self.binding(&format!("substrate-{i}")))
                .collect(),
            tiles: tile::Materials {
                ceramic: ceramic.id.clone(),
                bed: bed.id.clone(),
                grime: "$kowloon/wall-tile-grime".into(),
                repeat: ceramic.repeat,
                bed_repeat: bed.repeat,
                bed_roughness: bed.roughness,
                roughness: ceramic.roughness,
            },
            repair: self.binding("render-repair"),
            density: settings.density,
        };
        let finishes = site
            .lots
            .iter()
            .map(|lot| finish::choose(&kit, lot, &materials))
            .collect::<Result<Vec<_>, _>>()?;
        let deck = deck::Settings {
            host: &site.host,
            seed: settings.seed,
            grime: settings.grime,
            wear: settings.wear,
            fine_marks: settings.detail > 0,
        };
        draw::site(
            plan,
            index,
            mesh,
            &draw::Settings {
                kit: &kit,
                finishes: &finishes,
                concrete: self.binding("concrete"),
                wall: self.binding("wall"),
                brushed: self.binding("brushed-metal"),
                plants: self.textured("balcony-plants"),
                cloth: self.textured("balcony-cloth"),
                posters: self.textured("posters"),
                shops: Some(self.shop_assets()),
                deck: &deck,
                pavement: &self.pavement,
            },
            &|slot| self.repeat(slot),
            budget,
            cancelled,
        )
    }

    fn shop_assets(&self) -> shop::shell::Assets {
        shop::shell::Assets {
            room: shop::room::Assets {
                posters: self.textured("posters"),
                grocer_props: self.textured("grocer-props"),
                snack_packets: self.textured("snack-packet"),
                market_apple: self.textured("market-apple"),
                shop_register: self.textured("shop-register"),
                market_trays: self.textured("market-trays"),
            },
            restaurant: shop::roast::Assets {
                hero_props: self.textured("market-hero-props"),
                food: self.textured("restaurant-food"),
                metal: self.textured("kitchen-metal"),
                surfaces: self.textured("restaurant-surfaces"),
                tiles: self.textured("restaurant-tiles"),
            },
        }
    }

    pub(crate) fn draw_reference_corner(
        &self,
        site: &super::Site,
        mesh: &mut Mesh<'_>,
        settings: &Settings<'_>,
    ) -> Result<Book, String> {
        mesh.bind_source_materials(&self.bindings)?;
        let kit = self.kit(settings)?;
        super::reference_corner::draw(
            &mut crate::worldspec::themes::kowloon::fixture::Fixture {
                pen: crate::worldspec::themes::kowloon::pen::Pen::new(
                    site.matrix,
                    "reference-corner",
                )?,
                mesh,
                repeats: &|slot| self.repeat(slot),
            },
            site,
            &kit,
            settings,
            self.binding("wall"),
            self.binding("concrete"),
            self.shop_assets(),
        )
    }
}

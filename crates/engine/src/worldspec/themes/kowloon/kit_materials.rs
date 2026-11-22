//! Shared procedural material identities; registration performs no image or host I/O.
use std::collections::{BTreeMap, BTreeSet};
use superspec_catalog as material_catalog;
use superspec_types::material::Material;
pub struct Library {
    pub materials: BTreeMap<String, Material>,
    authored: BTreeSet<String>,
    prefix: Option<String>,
}
#[derive(serde::Serialize)]
pub struct Bindings {
    pub materials: BTreeMap<String, String>,
    pub lettering: bool,
    pub roofing: String,
}
impl Library {
    /// Original declarations remain distinct from generated kit/deck definitions.
    pub fn authored_ids(&self) -> impl Iterator<Item = &String> {
        self.authored.iter()
    }
    /// Plain finishes and wall sources are registered once for a scene, preserving whole-material overrides.
    pub fn new(authored: &[Material], prefix: Option<&str>, night: bool) -> Result<Self, String> {
        if authored.len() > 16_000 || prefix.is_some_and(|p| p.is_empty() || p.len() > 512) {
            return Err("Kit material declarations exceed identity or count bounds.".into());
        }
        let mut materials = BTreeMap::new();
        for m in authored {
            m.validate()?;
            if m.id.is_empty()
                || m.id.len() > 4096
                || materials.insert(m.id.clone(), m.clone()).is_some()
            {
                return Err("Kit material declarations require unique bounded identities.".into());
            }
        }
        let mut library = Self {
            authored: materials.keys().cloned().collect(),
            materials,
            prefix: prefix.map(str::to_owned),
        };
        for (name, roughness, opacity, emissive) in [
            ("paint", 0.85, None, false),
            ("glass", 0.12, Some(0.18), false),
            ("display-glass", 0.08, Some(0.055), false),
            ("pane", 0.95, None, false),
            ("interior-lit", 0.95, None, true),
        ] {
            library.generated(Material {
                id: format!("$kowloon/{name}"),
                color: "#ffffff".into(),
                roughness,
                opacity,
                emissive: emissive.then(|| "#ffd6a3".into()),
                emissive_strength: emissive.then_some(if night { 1.6 } else { 0. }),
                ..Material::default()
            })?;
        }
        for (name, role, declared) in [
            ("paper", "wall-posters", "paper"),
            ("posters", "wall-posters", "posters"),
            ("edge", "concrete", "edge"),
            ("rust", "wall-rust", "rust"),
            ("pipe", "wall-metal", "metal"),
            ("cable", "wall-cable", "cable"),
            ("shutter", "wall-metal", "metal"),
            ("metal", "wall-metal", "metal"),
            ("ceramic", "ceramic", "ceramic"),
            ("ceramicWarm", "ceramic", "ceramic"),
            ("ceramicGray", "ceramic", "ceramic"),
        ] {
            let mut material = library
                .declared(&format!("wall-{declared}"))
                .cloned()
                .unwrap_or_else(|| {
                    material_catalog::material(&format!("builtin:{role}"))
                        .expect("shipped wall role")
                });
            material.id = format!("$kowloon/wall-{name}");
            library.generated(material)?;
        }
        let mut bed = if let Some(m) = library.declared("wall-tile-plaster") {
            Material {
                color: m.color.clone(),
                repeat: m.repeat,
                roughness: m.roughness,
                metalness: m.metalness,
                texture: m.texture.clone(),
                normal_texture: m.normal_texture.clone(),
                normal_scale: m.normal_scale,
                metallic_roughness_texture: m.metallic_roughness_texture.clone(),
                ..Material::default()
            }
        } else {
            material_catalog::material("builtin:tile-bed").expect("shipped tile bedding")
        };
        bed.id = "$kowloon/wall-tile-bed".into();
        library.generated(bed)?;
        let mut grime = library
            .declared("wall-tile-grime")
            .cloned()
            .unwrap_or_else(|| {
                material_catalog::material("builtin:tile-grime").expect("shipped tile grime")
            });
        grime.id = "$kowloon/wall-tile-grime".into();
        library.generated(grime)?;
        Ok(library)
    }
    fn generated(&mut self, material: Material) -> Result<(), String> {
        if self.authored.contains(&material.id) {
            return Err(format!(
                "Reserved kit material {} is already declared.",
                material.id
            ));
        }
        material.validate()?;
        self.materials.insert(material.id.clone(), material);
        Ok(())
    }
    fn declared(&self, role: &str) -> Option<&Material> {
        self.prefix
            .as_ref()
            .and_then(|prefix| self.materials.get(&format!("{prefix}-{role}")))
    }
    fn finish(&mut self, role: &str) -> Result<String, String> {
        if let Some(material) = self.declared(role) {
            return Ok(material.id.clone());
        }
        let id = format!("builtin:{role}");
        if !self.materials.contains_key(&id) {
            let material =
                material_catalog::material(&id).ok_or("Unknown shipped kit material role.")?;
            self.materials.insert(id.clone(), material);
        }
        Ok(id)
    }
    /// Host walls remain authored; structural concrete and every atlas keep their independent role.
    pub fn bind(&mut self, wall: &str, substrates: &[String]) -> Result<Bindings, String> {
        if substrates.len() > 256
            || !self.materials.contains_key(wall)
            || substrates.iter().any(|id| !self.materials.contains_key(id))
        {
            return Err("Kit walls and substrates require existing material definitions.".into());
        }
        let mut materials = BTreeMap::from([("wall".into(), wall.into())]);
        for (slot, role) in [
            ("posters", "wall-posters"),
            ("concrete", "concrete"),
            ("render-weathered", "render-weathered"),
            ("render-repair", "render-repair"),
            ("metal", "metal"),
            ("brushed-metal", "brushed-metal"),
            ("kitchen-metal", "kitchen-metal"),
            ("kitchen-worktop", "kitchen-worktop"),
            ("vent-metal", "vent-metal"),
            ("vent-edge", "vent-edge"),
            ("shop-register", "shop-register"),
            ("sheet", "sheet"),
            ("timber", "timber"),
            ("restaurant-food", "restaurant-food"),
            ("restaurant-surfaces", "restaurant-surfaces"),
            ("restaurant-tiles", "restaurant-tiles"),
            ("shop-timber", "shop-timber"),
            ("lettering", "lettering"),
            ("board", "signboard"),
            ("rust", "rust"),
            ("cloth", "cloth"),
            ("balcony-plants", "balcony-plants"),
            ("balcony-cloth", "balcony-cloth"),
            ("tarp", "tarp"),
            ("aircon", "aircon"),
            ("market-apple", "market-apple"),
            ("market-trays", "market-trays"),
            ("snack-packet", "snack-packet"),
            ("grocer-props", "grocer-props"),
            ("market-hero-props", "market-hero-props"),
        ] {
            materials.insert(slot.into(), self.finish(role)?);
        }
        for slot in ["paint", "pane", "glass", "display-glass", "interior-lit"] {
            materials.insert(slot.into(), format!("$kowloon/{slot}"));
        }
        for (i, id) in substrates.iter().enumerate() {
            materials.insert(format!("substrate-{i}"), id.clone());
        }
        Ok(Bindings {
            materials,
            lettering: true,
            roofing: self.finish("roofing")?,
        })
    }
}

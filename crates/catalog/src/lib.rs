//! Shipped material definitions and image identities. Hosts supply the image bytes.
use std::{collections::BTreeMap, sync::OnceLock};
use superspec_types::material::Material;

fn catalog() -> &'static BTreeMap<String, Material> {
    static CATALOG: OnceLock<BTreeMap<String, Material>> = OnceLock::new();
    CATALOG.get_or_init(|| {
        let mut materials: BTreeMap<String, Material> =
            serde_json::from_str(include_str!("material_catalog.json"))
                .expect("shipped material definitions");
        for (role, material) in &mut materials {
            material.id = format!("builtin:{role}");
        }
        materials
    })
}

/// Resolves a reserved material identity to an independent definition.
pub fn material(id: &str) -> Option<Material> {
    catalog().get(id.strip_prefix("builtin:")?).cloned()
}

/// Only catalog-listed images enter the shipped library namespace; arbitrary paths never do.
pub fn image_name(id: &str) -> Option<&str> {
    let name = id.strip_prefix("superspec-material:")?;
    catalog()
        .values()
        .any(|m| m.companions().contains(&id))
        .then_some(name)
}

/// Shipped companion identities for coarse browser-side preloading of procedural recipes.
pub fn images() -> std::collections::BTreeSet<String> {
    catalog()
        .values()
        .flat_map(|m| m.companions().into_iter().map(str::to_owned))
        .collect()
}

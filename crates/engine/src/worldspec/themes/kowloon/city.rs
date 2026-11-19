//! Maps AML roles to individual, local ItemSpec sources in the shipped theme.
use std::sync::Arc;

const STOCK_SLOTS: &[&str] = &[
    "grocer-props",
    "shop-register",
    "market-apple",
    "market-trays",
    "snack-packet",
];

pub(crate) fn stock_images() -> impl Iterator<Item = String> {
    STOCK_SLOTS.iter().flat_map(|slot| {
        superspec_catalog::material(&format!("builtin:{slot}"))
            .expect("shipped stock finish")
            .companions()
            .into_iter()
            .map(str::to_owned)
            .collect::<Vec<_>>()
    })
}

macro_rules! items {
    ($($id:literal),*) => {
        pub(super) fn definition(entry:&str)->Option<&'static str> {
            match entry {
                $(concat!($id,".item.json") => Some(include_str!(concat!("items/",$id,".item.json"))),)*
                _ => None,
            }
        }
    }
}
items!(
    "kowloon-aircon",
    "kowloon-avenue",
    "kowloon-building",
    "kowloon-corner",
    "kowloon-duct-bracket",
    "kowloon-duct-riser",
    "kowloon-duct-run",
    "kowloon-facade-side",
    "kowloon-floor",
    "kowloon-ground-floor",
    "kowloon-lamp",
    "kowloon-parapet",
    "kowloon-road",
    "kowloon-roof",
    "kowloon-shopfront",
    "kowloon-sidewalk",
    "kowloon-sign",
    "kowloon-tree",
    "kowloon-window"
);

pub(crate) fn request(
    node: &superspec_types::architecture::Node,
) -> Result<crate::authoring::Request, String> {
    let entry = format!("kowloon-{}.item.json", node.kind);
    let source = definition(&entry).ok_or_else(|| format!("Unknown city role {}.", node.kind))?;
    let mut parameters = if node.text("model") == Some("reference-corner") {
        super::appearance::reference_building().clone()
    } else if node.kind == "building" {
        static FAMILIES: std::sync::OnceLock<Vec<crate::itemspec::definition::Values>> =
            std::sync::OnceLock::new();
        let families = FAMILIES.get_or_init(|| {
            serde_json::from_str(include_str!("appearance/building-families.json"))
                .expect("Kowloon building families")
        });
        let seed = node.number("seed", node.number("floors", 4.)) as usize;
        let mut values = families[seed % families.len()].clone();
        let width = node.number("width", 9.8);
        let depth = node.number("depth", 7.);
        values.insert(
            "footprint".into(),
            serde_json::json!([[0, 0], [width, 0], [width, depth], [0, depth]]),
        );
        values.insert("seed".into(), seed.into());
        values
    } else {
        std::collections::BTreeMap::new()
    };
    for key in ["length", "width"] {
        if node.kind != "building"
            && let Some(value) = node.optional_number(key)
        {
            parameters.insert(key.into(), value.into());
        }
    }
    if node.kind == "lamp" {
        parameters.insert("banner".into(), node.boolean("banner", false).into());
    }
    if let Some(floors) = node.optional_number("floors") {
        parameters.insert("floors".into(), (floors as u32).into());
    }
    Ok(crate::authoring::Request {
        kind: "item".into(),
        entry,
        source: source.into(),
        parameters,
        ..Default::default()
    })
}

pub(crate) fn asset(entry: &str) -> Result<Arc<[u8]>, String> {
    definition(entry)
        .map(|s| Arc::from(s.as_bytes()))
        .ok_or_else(|| format!("Unknown city item dependency {entry}."))
}

pub(crate) fn finish<'a>(
    mut scene: crate::Scene,
    nodes: impl Iterator<Item = &'a superspec_types::architecture::Node>,
    cancelled: &dyn Fn() -> bool,
) -> Result<crate::Scene, superspec_types::diagnostic::Diagnostic> {
    let mut buildings = std::collections::BTreeSet::new();
    for node in nodes {
        if node.kind == "building" {
            let count = node.number("count", 1.) as usize;
            buildings.extend(
                (1..=count).map(|i| superspec_profiles::city::placement_id(&node.id, count, i)),
            );
        }
        if scene.manifest.lighting.is_none() {
            scene.manifest.lighting = Some(
                serde_json::from_str(include_str!("city-lighting.json"))
                    .expect("Kowloon city lighting"),
            );
        }
        if matches!(node.kind.as_str(), "tree" | "avenue" | "shrub") {
            super::planting::append(&mut scene, node, cancelled)?;
        }
    }
    // Stock props carry the catalog's UV layout. Keep their image, normal and
    // roughness bindings when the local ItemSpec is placed in the world.
    let mut used = std::collections::BTreeSet::new();
    for mesh in &mut scene.manifest.meshes {
        if !buildings.contains(&mesh.object_id) {
            continue;
        }
        let slot = mesh.id.rsplit('/').next().unwrap_or("");
        if STOCK_SLOTS.contains(&slot) {
            let material = format!("builtin:{slot}");
            mesh.material = Some(material.clone());
            used.insert(material);
        }
    }
    for id in used {
        if !scene.manifest.materials.iter().any(|m| m.id == id) {
            scene
                .manifest
                .materials
                .push(superspec_catalog::material(&id).expect("shipped stock finish"));
        }
    }
    Ok(scene)
}

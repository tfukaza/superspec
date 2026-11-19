//! Maps AML roles to individual, local ItemSpec sources in the shipped theme.
use std::sync::Arc;

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
    let mut parameters = if node.kind == "building" && node.number("floors", 4.) == 4. {
        serde_json::from_str(include_str!("appearance/reference-building.json"))
            .expect("reference shop house preset")
    } else {
        std::collections::BTreeMap::new()
    };
    for key in ["length", "width"] {
        if let Some(value) = node.optional_number(key) {
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
    for node in nodes {
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
    Ok(scene)
}

//! Maps AML roles to individual, local ItemSpec sources in the shipped theme.
use std::sync::Arc;

const STOCK_SLOTS: &[&str] = &[
    "grocer-props",
    "market-hero-props",
    "shop-register",
    "market-apple",
    "market-trays",
    "snack-packet",
];
const RESTAURANT_SLOTS: &[&str] = &[
    "restaurant-food",
    "kitchen-metal",
    "kitchen-worktop",
    "restaurant-surfaces",
    "restaurant-tiles",
];

pub(super) fn uses_stock_material(slot: &str) -> bool {
    STOCK_SLOTS.contains(&slot) || RESTAURANT_SLOTS.contains(&slot) || slot == "display-glass"
}

pub(crate) fn stock_images(restaurant: bool) -> impl Iterator<Item = String> {
    STOCK_SLOTS
        .iter()
        .chain(RESTAURANT_SLOTS.iter().filter(move |_| restaurant))
        .flat_map(|slot| {
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
    "kowloon-ceiling-bulkhead",
    "kowloon-wall-extraction-grille",
    "kowloon-passage-sign-lamp",
    "kowloon-utility-cabinet",
    "kowloon-ceiling-girder",
    "kowloon-ceiling-twin-strip",
    "kowloon-corner",
    "kowloon-duct-bracket",
    "kowloon-duct-riser",
    "kowloon-duct-run",
    "kowloon-facade-side",
    "kowloon-floor",
    "kowloon-ground-floor",
    "kowloon-hosted-window",
    "kowloon-hosted-window-sash",
    "kowloon-lamp",
    "kowloon-parapet",
    "kowloon-pillar",
    "kowloon-procedural-wall",
    "kowloon-service-pipe",
    "kowloon-service-duct",
    "kowloon-road",
    "kowloon-roof",
    "kowloon-shopfront",
    "kowloon-sidewalk",
    "kowloon-sign",
    "kowloon-tree",
    "kowloon-wall",
    "kowloon-weed",
    "kowloon-weed-paint",
    "kowloon-weed-seam",
    "kowloon-window"
);

fn service_entry(kind: &str) -> Option<&'static str> {
    match kind {
        "wall-extraction-grille" => Some("kowloon-wall-extraction-grille.item.json"),
        "passage-sign-lamp" => Some("kowloon-passage-sign-lamp.item.json"),
        "ceiling-bulkhead" => Some("kowloon-ceiling-bulkhead.item.json"),
        "utility-cabinet" => Some("kowloon-utility-cabinet.item.json"),
        "ceiling-twin-strip" => Some("kowloon-ceiling-twin-strip.item.json"),
        "ceiling-girder" => Some("kowloon-ceiling-girder.item.json"),
        "hosted-window" => Some("kowloon-hosted-window.item.json"),
        "pipe-run" => Some("kowloon-service-pipe.item.json"),
        "duct-run" => Some("kowloon-service-duct.item.json"),
        "procedural-wall" => Some("kowloon-procedural-wall.item.json"),
        _ => None,
    }
}

pub(crate) fn prop_images(kind: &str) -> Vec<String> {
    let Some(entry) = service_entry(kind) else {
        return vec![];
    };
    let item = crate::itemspec::definition::parse(entry, definition(entry).expect("shipped prop"))
        .expect("valid shipped prop");
    item.slots
        .into_values()
        .flat_map(|slot| {
            superspec_catalog::material(&slot.entry)
                .into_iter()
                .flat_map(|material| {
                    material
                        .companions()
                        .into_iter()
                        .map(str::to_owned)
                        .collect::<Vec<_>>()
                })
        })
        .collect()
}

pub(crate) fn hosted_window(width: f64, height: f64, open: bool) -> crate::authoring::Request {
    let entry = "kowloon-hosted-window.item.json";
    crate::authoring::Request {
        kind: "item".into(),
        entry: entry.into(),
        source: definition(entry).expect("shipped casement").into(),
        parameters: [
            ("width".into(), width.into()),
            ("height".into(), height.into()),
            ("open".into(), open.into()),
        ]
        .into(),
        ..Default::default()
    }
}

pub(crate) fn request(
    node: &superspec_types::architecture::Node,
) -> Result<crate::authoring::Request, String> {
    let entry = service_entry(&node.kind)
        .map(str::to_owned)
        .unwrap_or_else(|| format!("kowloon-{}.item.json", node.kind));
    if node.kind == "grass-field" {
        let path = node.text("src").ok_or("Missing grass-field companion.")?;
        return Ok(crate::authoring::Request {
            kind: "item".into(),
            entry,
            source: serde_json::json!({
                "format": "itemspec", "version": 1, "id": "grass-field",
                "generator": {"kind": "glb", "asset": path}, "collision": "none"
            })
            .to_string(),
            ..Default::default()
        });
    }
    let source = definition(&entry).ok_or_else(|| format!("Unknown city role {}.", node.kind))?;
    let mut parameters = if node.text("model") == Some("reference-corner") {
        super::appearance::reference_building().clone()
    } else if node.kind == "building" {
        static FAMILIES: std::sync::OnceLock<Vec<crate::itemspec::definition::Values>> =
            std::sync::OnceLock::new();
        let families = FAMILIES.get_or_init(|| {
            serde_json::from_str(include_str!("materials/building-families.json"))
                .expect("Kowloon building families")
        });
        let seed = node.number("seed", node.number("floors", 4.)) as usize;
        let mut values = families[seed % families.len()].clone();
        if let Some(trade) = node.text("front-trade") {
            for (plan, value) in [("shopPlans", "open"), ("tradePlans", trade)] {
                let rows = values
                    .entry(plan.into())
                    .or_insert_with(|| serde_json::json!([[]]));
                let row = rows[0].as_array_mut().ok_or("Invalid front shop plan")?;
                if row.is_empty() {
                    row.push(value.into());
                } else {
                    row[0] = value.into();
                }
            }
        }
        let width = node.number("width", 9.8);
        let depth = node.number("depth", 7.);
        let footprint = match node.attributes.get("footprint") {
            Some(superspec_types::architecture::Value::PlanPoints(points)) => {
                serde_json::json!(points)
            }
            _ => serde_json::json!([[0, 0], [width, 0], [width, depth], [0, depth]]),
        };
        values.insert("footprint".into(), footprint);
        if !node.children.is_empty() {
            let courtyards = node
                .children
                .iter()
                .map(|hole| match hole.attributes.get("points") {
                    Some(superspec_types::architecture::Value::PlanPoints(points)) => Ok(points),
                    _ => Err("Missing courtyard points."),
                })
                .collect::<Result<Vec<_>, _>>()?;
            values.insert("courtyards".into(), serde_json::json!(courtyards));
        }
        for (attribute, parameter) in [("ground-shops", "groundShops"), ("pavement", "pavement")] {
            if node.attributes.contains_key(attribute) {
                values.insert(parameter.into(), node.boolean(attribute, true).into());
            }
        }
        values.insert("seed".into(), seed.into());
        values
    } else {
        std::collections::BTreeMap::new()
    };
    // A render-only choice: the same building drawn with less geometry for a lower preset.
    if node.kind == "building"
        && let Some(level) = node.optional_number("graphics-detail")
    {
        parameters.insert(
            super::graphics_detail::PARAMETER.into(),
            (level as u8).into(),
        );
    }
    // Also render-only: how many shapes each part type draws. Unstated, the level's own.
    if node.kind == "building"
        && let Some(budget) = node.optional_number("variant-budget")
    {
        parameters.insert(super::variants::PARAMETER.into(), (budget as u8).into());
    }
    for key in ["length", "width", "depth", "height"] {
        if node.kind != "building"
            && let Some(value) = node.optional_number(key)
        {
            parameters.insert(key.into(), value.into());
        }
    }
    if node.kind == "ceiling-twin-strip" && node.attributes.contains_key("enclosed") {
        parameters.insert("enclosed".into(), node.boolean("enclosed", false).into());
    }
    if node.kind == "lamp" {
        parameters.insert("banner".into(), node.boolean("banner", false).into());
    }
    if node.kind == "ceiling-girder" {
        if let Some(gap) = node.optional_number("soffitGap") {
            parameters.insert("soffitGap".into(), gap.into());
        }
        if node.attributes.contains_key("hangerStations") {
            parameters.insert(
                "hangerStations".into(),
                serde_json::json!(node.points("hangerStations")),
            );
        }
    }
    if matches!(node.kind.as_str(), "pipe-run" | "duct-run") {
        for key in ["points", "supports"] {
            if node.attributes.contains_key(key) {
                parameters.insert(key.into(), serde_json::json!(node.points(key)));
            }
        }
        if let Some(radius) = node.optional_number("radius") {
            parameters.insert("radius".into(), radius.into());
        }
    }
    if matches!(node.kind.as_str(), "procedural-wall" | "utility-cabinet") {
        if let Some(seed) = node.optional_number("seed") {
            parameters.insert("seed".into(), (seed as u32).into());
        }
        if node.kind == "procedural-wall" {
            parameters.insert("dressing".into(), node.boolean("dressing", true).into());
        }
    }
    if node.kind == "weed"
        && let Some(variant) = node.text("variant")
    {
        parameters.insert("variant".into(), variant.into());
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
    if entry == "kowloon-weed-paint.glb" {
        return Ok(Arc::from(
            include_bytes!("items/kowloon-weed-paint.glb").as_slice(),
        ));
    }
    definition(entry)
        .map(|s| Arc::from(s.as_bytes()))
        .ok_or_else(|| format!("Unknown city item dependency {entry}."))
}

pub(crate) fn finish<'a>(
    items: &mut crate::worldspec::build::ItemOutput<'a>,
    nodes: impl Iterator<Item = &'a superspec_types::architecture::Node>,
    cancelled: &dyn Fn() -> bool,
    cache: Option<&std::sync::Mutex<super::cache::Cache>>,
) -> Result<(), superspec_types::diagnostic::Diagnostic> {
    // Standalone builds also share a plant design across repeated placements in this scene.
    let local_cache = std::sync::OnceLock::new();
    let mut buildings = std::collections::BTreeSet::new();
    let mut first_building = None;
    for node in nodes {
        if node.kind == "building" {
            first_building.get_or_insert(node);
            let count = node.number("count", 1.) as usize;
            buildings.extend(
                (1..=count).map(|i| superspec_profiles::city::placement_id(&node.id, count, i)),
            );
        }
        items
            .default_lighting(|| {
                serde_json::from_str(include_str!("city-lighting.json"))
                    .expect("Kowloon city lighting")
            })
            .map_err(|e| node.fail("AML_CITY", e))?;
        if matches!(node.kind.as_str(), "tree" | "avenue" | "shrub") {
            super::planting::append(
                items,
                node,
                cancelled,
                cache.unwrap_or_else(|| local_cache.get_or_init(Default::default)),
            )?;
        }
    }
    // Stock props carry the catalog's UV layout. Keep their image, normal and
    // roughness bindings when the local ItemSpec is placed in the world.
    if let Some(source) = first_building {
        items
            .bind_slots(
                source,
                &buildings,
                |slot| uses_stock_material(slot).then(|| format!("builtin:{slot}")),
                superspec_catalog::material,
            )
            .map_err(|e| source.fail("AML_CITY", e))?;
    }
    Ok(())
}

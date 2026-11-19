//! Temporary diagnostic; removed before hand-off.
use std::{collections::BTreeMap, sync::Arc};
use superspec_engine::authoring::{Project, Session};

fn base64(text: &str) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(text.len() / 4 * 3);
    let (mut bits, mut held) = (0u32, 0u32);
    for symbol in text.bytes() {
        let value = match symbol {
            b'A'..=b'Z' => symbol - b'A',
            b'a'..=b'z' => symbol - b'a' + 26,
            b'0'..=b'9' => symbol - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            _ => continue,
        };
        held = held << 6 | u32::from(value);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            bytes.push((held >> bits) as u8);
        }
    }
    bytes
}

#[test]
#[ignore]
fn diag() {
    let path = std::env::var("ITEM_BUILDING_PROJECT").expect("ITEM_BUILDING_PROJECT");
    let text = std::fs::read_to_string(&path).unwrap();
    let value: serde_json::Value = serde_json::from_str(&text).unwrap();
    let assets: BTreeMap<String, Arc<[u8]>> = value["assets"]
        .as_object()
        .into_iter()
        .flatten()
        .map(|(name, bytes)| (name.clone(), Arc::from(base64(bytes.as_str().unwrap()))))
        .collect();
    let project: Project = serde_json::from_value(value).unwrap();
    let asset = |name: &str| {
        assets
            .get(name)
            .cloned()
            .ok_or_else(|| format!("missing asset {name}"))
    };
    let mut session = Session::default();
    let shaded = session
        .shade_project(&project, 1, &asset, &|| false)
        .unwrap_or_else(|errors| panic!("{errors:?}"));
    let materials = &shaded.metadata["materials"];
    let mut seen = std::collections::BTreeSet::new();
    for (mesh, described) in shaded.metadata["shading"].as_object().unwrap() {
        if described["boundsMin"][2].as_f64().unwrap() > 12.3
            && described["boundsMax"][2].as_f64().unwrap() < 13.9
        {
            println!(
                "HIGH {mesh} [{}] {} {}",
                described["material"], described["boundsMin"], described["boundsMax"]
            );
        }
        if !mesh.contains("/parapet/parapet/") {
            continue;
        }
        let material = described["material"].as_str().unwrap();
        if seen.insert(material.to_owned()) {
            for layer in materials[material]["layers"].as_array().into_iter().flatten() {
                let shader = layer.get("plan").map_or(&layer["shader"], |p| &p["shader"]);
                println!(
                    "  material {material} layer {} {} follow {} named {} PARAMS {}",
                    layer["id"], layer["entry"], layer["follow"], shader["namedFeatures"], layer["parameters"]
                );
            }
        }
        let counts: Vec<String> = described["layerFeatures"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|features| {
                let numbers = features.as_array().unwrap();
                let mut kinds: BTreeMap<i64, usize> = BTreeMap::new();
                let mut near: BTreeMap<i64, f64> = BTreeMap::new();
                let zmax = described["boundsMax"][2].as_f64().unwrap();
                for row in numbers.chunks(12) {
                    let kind = row[3].as_f64().unwrap() as i64;
                    *kinds.entry(kind).or_default() += 1;
                    if kind >= 5 {
                        let dz = (row[2].as_f64().unwrap() - zmax).abs();
                        let e = near.entry(kind).or_insert(f64::INFINITY);
                        *e = e.min(dz);
                    }
                }
                format!("{}:{kinds:?} near-top-dz {near:?}", numbers.len() / 12)
            })
            .collect();
        if mesh.ends_with("side-0/side-0/part/parapet/parapet/part/parapet/parapet/front") {
            println!("bounds {} {}", described["boundsMin"], described["boundsMax"]);
            println!("o2w {}", described["objectToWorld"]);
            for row in described["layerFeatures"][2].as_array().unwrap().chunks(12) {
                println!("   row {:?}", row.iter().map(|v| v.as_f64().unwrap()).collect::<Vec<_>>());
            }
        }
        println!(
            "{mesh} [{material}] z {}..{} features {} layers {counts:#?}",
            described["boundsMin"][2],
            described["boundsMax"][2],
            described["features"].as_array().map_or(0, Vec::len) / 12
        );
    }
}

//! The Kowloon buildings as composed ItemSpec definitions (worldspec/themes/kowloon/items):
//! small items (window, air conditioner, floor, ground floor, shopfront, parapet, duct run and
//! riser, roof) that sides, buildings and the corner shop place as sub-items. Counts and
//! heights are derived from the controls here.
use serde_json::json;
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};
use superspec_engine::{
    itemspec::definition::{self as item, Environment, GeneratedItem, Values},
    worldspec::themes::kowloon::items,
};

/// Every definition in the directory, by file name, as sub-item entries name them.
fn library() -> BTreeMap<String, String> {
    let directory =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/worldspec/themes/kowloon/items");
    std::fs::read_dir(directory)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.to_string_lossy().ends_with(".item.json"))
        .map(|path| {
            (
                path.file_name().unwrap().to_string_lossy().into_owned(),
                std::fs::read_to_string(&path).unwrap(),
            )
        })
        .collect()
}

fn generate(name: &str, overrides: &[(&str, serde_json::Value)]) -> GeneratedItem {
    let files = library();
    let reader = |name: &str| -> Result<Arc<[u8]>, String> {
        files
            .get(name)
            .map(|text| Arc::from(text.as_bytes()))
            .ok_or(format!("Missing {name}"))
    };
    let definition = item::parse(name, &files[name]).unwrap();
    let values: Values = overrides
        .iter()
        .map(|(name, value)| ((*name).to_owned(), value.clone()))
        .collect();
    let mut environment = Environment::new(&reader).with_natives(items::natives());
    item::prepare_in(&definition, &values, &mut environment, &|| false)
        .unwrap_or_else(|error| panic!("{name}: {error}"))
        .build(&|| false)
        .unwrap_or_else(|error| panic!("{name}: {error}"))
}

fn parts<'a>(item: &'a GeneratedItem, prefix: &str) -> Vec<&'a str> {
    item.geometry
        .parts
        .iter()
        .map(|part| part.id.as_str())
        .filter(|id| id.starts_with(prefix))
        .collect()
}

/// Openings cut in the given parts' walls: distinct opening-edge sources per part.
fn openings(item: &GeneratedItem, prefix: &str) -> usize {
    item.geometry
        .features
        .iter()
        .filter(|f| f.name == "opening-edge" && f.part.starts_with(prefix))
        .map(|f| (f.part.clone(), f.source.clone()))
        .collect::<BTreeSet<_>>()
        .len()
}

/// The highest point of the meshes under one part.
fn top_of(item: &GeneratedItem, part: &str) -> f64 {
    let transforms = item.geometry.world_transforms().unwrap();
    let mut top = f64::NEG_INFINITY;
    for (node, matrix) in item.geometry.nodes.iter().zip(transforms) {
        if node.name.as_deref() != Some(part) {
            continue;
        }
        for index in &node.meshes {
            for p in &item.geometry.meshes[*index].positions {
                top = top.max(matrix.point(*p)[2]);
            }
        }
    }
    top
}

#[test]
fn every_small_item_builds_alone() {
    for (name, overrides) in [
        ("kowloon-window.item.json", vec![]),
        (
            "kowloon-window.item.json",
            vec![("bars", json!(true)), ("kind", json!("slider"))],
        ),
        ("kowloon-aircon.item.json", vec![]),
        ("kowloon-aircon.item.json", vec![("split", json!(true))]),
        ("kowloon-floor.item.json", vec![]),
        ("kowloon-floor.item.json", vec![("windows", json!(false))]),
        ("kowloon-ground-floor.item.json", vec![]),
        (
            "kowloon-shopfront.item.json",
            vec![("kind", json!("shop")), ("awning", json!(true))],
        ),
        ("kowloon-parapet.item.json", vec![("outlet", json!(true))]),
        ("kowloon-duct-run.item.json", vec![]),
        ("kowloon-duct-riser.item.json", vec![]),
        ("kowloon-roof.item.json", vec![]),
    ] {
        let built = generate(name, &overrides);
        assert!(!built.geometry.meshes.is_empty(), "{name}");
    }
    // A window reports its sill and an air conditioner where it drips, for grime to follow.
    let window = generate("kowloon-window.item.json", &[]);
    assert!(
        window
            .geometry
            .features
            .iter()
            .any(|f| f.name == "sill" && f.part == "window")
    );
    for split in [false, true] {
        let unit = generate("kowloon-aircon.item.json", &[("split", json!(split))]);
        let drips: Vec<&str> = unit
            .geometry
            .features
            .iter()
            .filter(|f| f.name == "drip")
            .map(|f| f.part.as_str())
            .collect();
        assert_eq!(drips, [if split { "condenser" } else { "unit" }]);
    }
}

#[test]
fn the_riser_spans_its_height_with_a_branch_per_floor() {
    for (floors, height) in [(2, 6.), (5, 15.)] {
        let riser = generate(
            "kowloon-duct-riser.item.json",
            &[("floors", json!(floors)), ("height", json!(height))],
        );
        // The elbow turns into the wall at the roof, 0.2 m above it with its section.
        let top = riser.geometry.bounds[1][2];
        assert!(
            (top - (height + 0.2)).abs() < 0.05,
            "{floors} floors: top {top}"
        );
        assert_eq!(parts(&riser, "branch-").len(), floors);
        assert!((riser.geometry.bounds[0][2] - 0.54).abs() < 1e-6);
    }
}

#[test]
fn a_floor_is_a_wall_with_its_windows_fixtures_and_weathering() {
    let floor = generate("kowloon-floor.item.json", &[]);
    let bays = ((9. - 1.8) / 2.1f64).floor() as usize;
    assert_eq!(openings(&floor, "wall"), bays);
    assert_eq!(parts(&floor, "window-").len(), bays);
    // The floor's wall is painted, then weathered under its top, its sills and its air
    // conditioners' drips.
    assert_eq!(
        floor.layers["wall"][0].entry,
        "kowloon-paint-coat.material.ts"
    );
    let follows: Vec<String> = floor.layers["wall"][1..]
        .iter()
        .map(|layer| {
            layer
                .follow
                .get("top")
                .cloned()
                .unwrap_or_else(|| "top".into())
        })
        .collect();
    assert_eq!(follows, ["top", "sill", "drip"]);
    // Sub-item features land on the floor's own parts, in floor coordinates: each window's
    // sill where it stands (at the sill height, 0.95 m), beside the wall's own opening sills.
    let sills: Vec<_> = floor
        .geometry
        .features
        .iter()
        .filter(|f| f.name == "sill" && f.part.starts_with("window-"))
        .collect();
    assert_eq!(sills.len(), bays);
    assert!(
        sills
            .iter()
            .all(|f| f.part.starts_with("window-") && (f.points[0][2] - 0.95).abs() < 1e-9)
    );
}

#[test]
fn a_side_stacks_a_ground_floor_upper_floors_and_a_parapet() {
    let side = generate("kowloon-facade-side.item.json", &[]);
    assert_eq!(parts(&side, "ground"), ["ground"]);
    assert_eq!(parts(&side, "floor-"), ["floor-0", "floor-1", "floor-2"]);
    assert_eq!(openings(&side, "floor-"), 3 * 3);
    // Sub-item layers reach the side: the floors' grime and the ground floor's stain and moss.
    let entries: BTreeSet<&str> = side.layers["wall"]
        .iter()
        .map(|l| l.entry.as_str())
        .collect();
    for entry in [
        "kowloon-top-grime.material.ts",
        "kowloon-ground-stain.material.ts",
        "kowloon-moss.material.ts",
    ] {
        assert!(entries.contains(entry), "{entry}");
    }
    // The drips of the floors' air conditioners are features of the floors.
    assert!(
        side.geometry
            .features
            .iter()
            .any(|f| f.name == "drip" && f.part.starts_with("floor-"))
    );
    // 3.35 m ground floor, three of 3 m, the parapet's 0.95 m and an 8 cm coping.
    assert!((top_of(&side, "parapet") - 13.38).abs() < 1e-6);
    // Without shops the ground storey is a floor of windows of the same height.
    let plain = generate(
        "kowloon-facade-side.item.json",
        &[("groundShops", json!(false))],
    );
    assert_eq!(parts(&plain, "groundFloor"), ["groundFloor"]);
    assert!((top_of(&plain, "parapet") - 13.38).abs() < 1e-6);
    // The same seed repeats a side; another rearranges it.
    let again = generate("kowloon-facade-side.item.json", &[]);
    assert_eq!(again.geometry_key, side.geometry_key);
    let other = generate("kowloon-facade-side.item.json", &[("seed", json!(7))]);
    assert_ne!(other.geometry_key, side.geometry_key);
}

/// The edges' walls of a building, by part: copies of `side` and `corner`.
fn sides(item: &GeneratedItem, prefix: &str) -> Vec<String> {
    parts(item, prefix).into_iter().map(str::to_owned).collect()
}

#[test]
fn a_building_follows_its_footprint_cutting_corners_between_streets() {
    // The reference shop house: 9.8 by 7 m, streets on the front and right, their corner cut
    // back 0.24 m along each.
    let building = generate("kowloon-building.item.json", &[]);
    assert_eq!(
        sides(&building, "side-"),
        ["side-0", "side-1", "side-2", "side-3"]
    );
    assert_eq!(sides(&building, "corner-"), ["corner-0"]);
    assert_eq!(sides(&building, "cornerSign-"), ["cornerSign-0"]);
    // Every wall reaches the same coping: 3.35 + 3 × 3 m, a 0.75 m parapet and 8 cm.
    for side in ["side-0", "side-1", "side-2", "side-3", "corner-0"] {
        assert!(
            (top_of(&building, side) - 13.18).abs() < 0.03,
            "{side}: {}",
            top_of(&building, side)
        );
    }
    // Street edges have shops; the others back doors, pipes and a riser instead.
    let under = |part: &str, piece: &str| {
        building
            .geometry
            .meshes
            .iter()
            .any(|m| m.id.starts_with(&format!("{part}/")) && m.id.contains(piece))
    };
    for street in ["side-0", "side-1"] {
        assert!(under(street, "/shop-"), "{street} has shops");
        assert!(!under(street, "/backDoor/"), "{street} has no back door");
    }
    for back in ["side-2", "side-3"] {
        assert!(!under(back, "/shop-"), "{back} has no shops");
        assert!(under(back, "/backDoor/"), "{back} has a back door");
    }
    let risers: BTreeSet<&str> = building
        .geometry
        .meshes
        .iter()
        .filter(|m| m.id.contains("/riser/"))
        .map(|m| m.id.split('/').next().unwrap())
        .collect();
    assert_eq!(risers, ["side-2", "side-3"].into());
    // The front's wall starts after the cut: the side spans 9.56 m and the corner 0.34 m.
    let front = &building
        .geometry
        .nodes
        .iter()
        .find(|n| n.name.as_deref() == Some("side-0"))
        .unwrap();
    assert_eq!(front.transform[3], [0., 0., 0.]);
    let corner = &building
        .geometry
        .nodes
        .iter()
        .find(|n| n.name.as_deref() == Some("corner-0"))
        .unwrap();
    assert!((corner.transform[3][0] - 9.56).abs() < 1e-9 && corner.transform[3][1].abs() < 1e-9);
    assert!(
        (corner.transform[0][0] - 0.5f64.sqrt()).abs() < 1e-9
            && (corner.transform[0][1] - 0.5f64.sqrt()).abs() < 1e-9
    );

    // An L-shaped block with three street edges: its two convex street corners are cut, its
    // concave one is not.
    let block = generate(
        "kowloon-building.item.json",
        &[
            (
                "footprint",
                json!([[0, 0], [12, 0], [12, 5], [6, 5], [6, 9], [0, 9]]),
            ),
            ("streets", json!([true, true, true, false, false, false])),
        ],
    );
    assert_eq!(sides(&block, "side-").len(), 6);
    assert_eq!(sides(&block, "corner-"), ["corner-0", "corner-1"]);
    // A clockwise footprint is refused at its part.
    let files = library();
    let reader = |name: &str| -> Result<Arc<[u8]>, String> {
        files
            .get(name)
            .map(|t| Arc::from(t.as_bytes()))
            .ok_or(format!("Missing {name}"))
    };
    let definition = item::parse("b", &files["kowloon-building.item.json"]).unwrap();
    let values: Values = [(
        "footprint".to_owned(),
        json!([[0, 0], [0, 7], [9, 7], [9, 0]]),
    )]
    .into();
    let error = item::prepare_in(
        &definition,
        &values,
        &mut Environment::new(&reader).with_natives(items::natives()),
        &|| false,
    )
    .err()
    .unwrap();
    assert_eq!(error.code, "ITEM_FOOTPRINT");
}

#[test]
fn a_plan_places_exactly_what_it_names_and_auto_keeps_the_seed() {
    // On the front (edge 0): storey 1 has no window in its middle bay; storey 2 holds two
    // cages and a split unit; the ground floor a gate and two shops.
    let planned = generate(
        "kowloon-building.item.json",
        &[
            ("windowsPlans", json!([[[], ["window", "none", "window"]]])),
            (
                "fixturesPlans",
                json!([[
                    [],
                    ["none", "none", "none"],
                    ["cage", "cage", "split"],
                    ["none", "aircon", "none"]
                ]]),
            ),
            ("shopPlans", json!([["gate", "shop", "shop"]])),
        ],
    );
    let front = |storey: &str, piece: &str| {
        planned.geometry.meshes.iter().any(|m| {
            m.id.starts_with("side-0/")
                && m.id.contains(&format!("{storey}/"))
                && m.id.contains(&format!("/{piece}/"))
        })
    };
    assert!(
        front("floor-0", "window-0")
            && !front("floor-0", "window-1")
            && front("floor-0", "window-2")
    );
    for bay in 0..3 {
        assert!(
            !front("floor-0", &format!("aircon-{bay}")),
            "storey 1 bay {bay} has no fixture"
        );
    }
    assert!(
        front("floor-1", "cage-0") && front("floor-1", "cage-1") && front("floor-1", "aircon-2")
    );
    assert!(!front("floor-1", "aircon-0") && !front("floor-1", "aircon-1"));
    assert!(
        front("floor-2", "aircon-1")
            && !front("floor-2", "aircon-0")
            && !front("floor-2", "aircon-2")
    );
    // A plan is an input like any other: the same plan and seed give the same building, and
    // the sides it does not name keep their seeded contents.
    let again = generate(
        "kowloon-building.item.json",
        &[
            ("windowsPlans", json!([[[], ["window", "none", "window"]]])),
            (
                "fixturesPlans",
                json!([[
                    [],
                    ["none", "none", "none"],
                    ["cage", "cage", "split"],
                    ["none", "aircon", "none"]
                ]]),
            ),
            ("shopPlans", json!([["gate", "shop", "shop"]])),
        ],
    );
    assert_eq!(again.geometry_key, planned.geometry_key);
    let seeded = generate("kowloon-building.item.json", &[]);
    let back = |item: &GeneratedItem| {
        item.geometry
            .parts
            .iter()
            .find(|p| p.id == "side-2")
            .unwrap()
            .geometry_key
            .clone()
    };
    assert_eq!(back(&planned), back(&seeded));
    // An entry outside the choices is refused.
    let files = library();
    let definition = item::parse("b", &files["kowloon-building.item.json"]).unwrap();
    let values: Values = [("fixturesPlans".to_owned(), json!([[["fridge"]]]))].into();
    assert_eq!(
        definition.resolve_parameters(&values).unwrap_err().code,
        "ITEM_PARAMETER"
    );
}

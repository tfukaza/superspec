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
    // Over the paint: the dirt every wall carries, then the layers that follow an edge.
    assert_eq!(
        floor.layers["wall"][1].entry,
        "kowloon-wall-dirt.material.ts"
    );
    let follows: Vec<String> = floor.layers["wall"][1..]
        .iter()
        .filter_map(|layer| layer.follow.get("source").cloned())
        .collect();
    // Runoff streaks from its top, its sills and its air conditioners' drips; on the tiled
    // spandrels from the sills and drips.
    assert_eq!(follows, ["eave", "sill", "drip"]);
    // The soot band follows the pavement line, which only a storey at street level has.
    let soot = floor.layers["wall"]
        .iter()
        .find(|layer| layer.entry == "kowloon-soot-band.material.ts")
        .expect("a soot band");
    assert_eq!(soot.follow["base"], "pavement");
    assert!(!floor.geometry.features.iter().any(|f| f.name == "pavement"));
    let street = generate("kowloon-floor.item.json", &[("level", json!(0))]);
    assert!(
        street
            .geometry
            .features
            .iter()
            .any(|f| f.name == "pavement")
    );
    let tiles: Vec<String> = floor.layers["tile"]
        .iter()
        .filter_map(|layer| layer.follow.get("source").cloned())
        .collect();
    assert_eq!(tiles, ["sill", "drip"]);
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
    // Sub-item layers reach the side: the floors' grime and the ground floor's soot band and
    // notices. Identical layers of the storeys merge, so the wall stays within the eight
    // layers a slot composes.
    let entries: BTreeSet<&str> = side.layers["wall"]
        .iter()
        .map(|l| l.entry.as_str())
        .collect();
    for entry in [
        "kowloon-grime-streaks.material.ts",
        "kowloon-wall-dirt.material.ts",
        "kowloon-soot-band.material.ts",
        "kowloon-notices.material.ts",
    ] {
        assert!(entries.contains(entry), "{entry}");
    }
    assert!(
        side.layers["wall"].len() <= 8,
        "{}",
        side.layers["wall"].len()
    );
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

fn translation(item: &GeneratedItem, part: &str) -> [f64; 3] {
    item.geometry
        .nodes
        .iter()
        .find(|n| n.name.as_deref() == Some(part))
        .unwrap()
        .transform[3]
}

#[test]
fn window_banks_and_signs_follow_their_plans_in_place() {
    // A floor with an explicit plan of two banks has two windows, centred on them.
    let floor = generate(
        "kowloon-floor.item.json",
        &[("spanPlan", json!([[0.55, 2.65], [3.2, 5.4]]))],
    );
    assert_eq!(parts(&floor, "window-"), ["window-0", "window-1"]);
    assert!((translation(&floor, "window-0")[0] - 1.6).abs() < 1e-9);
    assert!((translation(&floor, "window-1")[0] - 4.3).abs() < 1e-9);
    assert_eq!(openings(&floor, "wall"), 2);
    // Without a plan the evenly spaced bays are kept.
    assert_eq!(
        parts(&generate("kowloon-floor.item.json", &[]), "window-").len(),
        3
    );
    // Board signs stand where their rows put them on their edge, with their own text.
    let building = generate(
        "kowloon-building.item.json",
        &[
            ("boardTexts", json!(["永昌藥行", "興隆五金"])),
            (
                "boardPlans",
                json!([
                    [[-1.03, 4.72, 0.94, 2.82, 0, 0, 0, 0.85, 1.18, -0.08, 0]],
                    [[0.02, 6.05, 0.94, 3.33, 1, 0, 53.13, 0.78, 0, 0, 1]]
                ]),
            ),
            ("signs", json!(false)),
        ],
    );
    assert_eq!(sides(&building, "board-"), ["board-0-0", "board-1-0"]);
    assert!((translation(&building, "board-0-0")[0] + 1.03).abs() < 1e-9);
    assert!((translation(&building, "board-0-0")[2] - 4.72).abs() < 1e-9);
    // The second edge starts after the cut corner: (9.8, 0.24), running along +y.
    let second = translation(&building, "board-1-0");
    assert!(
        (second[0] - 9.8).abs() < 1e-9 && (second[1] - (0.24 + 0.02)).abs() < 1e-9,
        "{second:?}"
    );
    // A red board and an ivory one draw in their own tints, as vertex colors.
    let colors: BTreeSet<String> = building
        .geometry
        .meshes
        .iter()
        .filter(|m| m.id.starts_with("board-") && m.material_slot == "board")
        .flat_map(|m| {
            m.colors
                .iter()
                .map(|c| format!("{:.2}/{:.2}/{:.2}", c[0], c[1], c[2]))
        })
        .collect();
    assert!(colors.len() >= 2, "{colors:?}");
    // A sign list replaces the seeded signs.
    assert!(sides(&generate("kowloon-building.item.json", &[]), "cornerSign-").len() == 1);
    assert!(sides(&building, "cornerSign-").is_empty());
}

/// The reference shop house's plan (tests/studio/washroom-items/reference-shop-house.json).
fn plan(name: &str) -> Vec<(String, serde_json::Value)> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/studio/washroom-items")
        .join(name);
    let plan: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    plan.as_object()
        .unwrap()
        .iter()
        .filter(|(key, _)| !key.starts_with('/'))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect()
}

#[test]
fn the_reference_plan_builds_its_shops_balcony_roof_and_pavement() {
    let plan = plan("reference-shop-house.json");
    let overrides: Vec<(&str, serde_json::Value)> = plan
        .iter()
        .map(|(key, value)| (key.as_str(), value.clone()))
        .collect();
    let building = generate("kowloon-building.item.json", &overrides);
    // A piece is a path of part names; a mesh ID holds them in order among its own segments.
    let has = |piece: &str| {
        let wanted: Vec<&str> = piece.split('/').filter(|s| !s.is_empty()).collect();
        building.geometry.meshes.iter().any(|m| {
            let mut segments = m.id.split('/');
            wanted.iter().all(|name| segments.any(|s| s == *name))
        })
    };
    // The front: a gate bay and the open provisions shop with its stock on the pavement, under
    // a tarp on poles and a striped canvas; the balcony with its door, fan and plants.
    for piece in [
        "side-0/ground/shop-0/bay/",
        "side-0/ground/shop-1/room/",
        "side-0/ground/shop-1/stock/",
        "side-0/ground/shop-0/tarp/",
        "side-0/ground/shop-1/tarp/",
        "side-1/ground/shop-1/room/",
        "side-0/balcony/",
        "side-0/balconyFan/",
        "side-0/floor-0/door/",
        "side-0/drain-0/",
        "side-0/conduit-0/",
        "hut-0/",
        "tank-0/",
        "aerial-0-0/",
        "pavement-0/",
        "pavement-1/",
        "podium/",
        "board-1-1/board/",
    ] {
        assert!(has(piece), "{piece} is missing");
    }
    assert!(
        !has("rooftop-"),
        "the seeded roof deck is replaced by the kit"
    );
    // The pavement reaches the kerb height below the building's floor and no further.
    assert!((building.geometry.bounds[0][2] + 0.15).abs() < 1e-6);
    let triangles: usize = building
        .geometry
        .meshes
        .iter()
        .map(|m| m.indices.len() / 3)
        .sum();
    let slots: BTreeSet<&str> = building
        .geometry
        .meshes
        .iter()
        .map(|m| m.material_slot.as_str())
        .collect();
    let vertices: usize = building
        .geometry
        .meshes
        .iter()
        .map(|m| m.positions.len())
        .sum();
    println!(
        "reference plan: {} meshes, {triangles} triangles, {vertices} vertices, slots {slots:?}",
        building.geometry.meshes.len()
    );
    // Well within the million triangles a placed item may hold.
    assert!(triangles < 400_000);
}

#[test]
fn the_small_warehouse_plan_builds_from_the_same_items() {
    let plan = plan("small-warehouse.json");
    let overrides: Vec<(&str, serde_json::Value)> = plan
        .iter()
        .map(|(key, value)| (key.as_str(), value.clone()))
        .collect();
    let building = generate("kowloon-building.item.json", &overrides);
    let has = |piece: &str| {
        let wanted: Vec<&str> = piece.split('/').filter(|s| !s.is_empty()).collect();
        building.geometry.meshes.iter().any(|m| {
            let mut segments = m.id.split('/');
            wanted.iter().all(|name| segments.any(|s| s == *name))
        })
    };
    // A shutter and a door over a tiled dado, a number plate, two caged windows (one hooded),
    // three panelled boards, the corner drain, a tiled roof deck, and paving on all four sides.
    for piece in [
        "side-0/ground/shop-0/bay/",
        "side-0/ground/shop-1/bay/",
        "side-0/ground/dado-0/",
        "plaque-0-0/flat/",
        "side-0/floor-0/cage-0/",
        "side-0/floor-0/cage-1/",
        "side-0/floor-0/canopy-0/",
        "board-0-0/board/",
        "board-0-2/board/",
        "side-0/drain-0/",
        "roofTiles-0/",
        "pavement-0/",
        "pavement-3/",
    ] {
        assert!(has(piece), "{piece} is missing");
    }
    for piece in [
        "corner-0/",
        "hut-0/",
        "side-0/balcony/",
        "side-0/floor-0/canopy-1/",
    ] {
        assert!(!has(piece), "{piece} should not be there");
    }
    // Three storeys: 3.3 m and two of 3 m, a 0.45 m parapet and its 8 cm coping.
    assert!((top_of(&building, "side-0") - 9.83).abs() < 0.03);
    // The deck is tiled in its own slot, and the dado in the walls' tile.
    let slots: BTreeSet<&str> = building
        .geometry
        .meshes
        .iter()
        .map(|m| m.material_slot.as_str())
        .collect();
    assert!(slots.contains("roof-tile") && slots.contains("tile"));
}

#[test]
fn a_sign_is_an_item_of_its_own() {
    let sign =
        |overrides: &[(&str, serde_json::Value)]| generate("kowloon-sign.item.json", overrides);
    let extent = |item: &GeneratedItem, axis: usize| {
        item.geometry.bounds[1][axis] - item.geometry.bounds[0][axis]
    };
    // Every mounting builds alone, each as its own part.
    for mount in ["board", "blade", "hung", "flat"] {
        let built = sign(&[("mount", json!(mount))]);
        assert_eq!(parts(&built, ""), [mount], "{mount}");
        assert!(!built.geometry.meshes.is_empty());
    }
    // A board's size is its controls': a taller, wider board is a larger sign.
    let small = sign(&[("width", json!(0.6)), ("height", json!(1.5))]);
    let large = sign(&[("width", json!(1.2)), ("height", json!(3.6))]);
    assert!((extent(&large, 2) - extent(&small, 2)) > 1.8);
    assert!(extent(&large, 0) > extent(&small, 0) + 0.5);
    // Different words are different lettering; a horizontal board reads along its width.
    let words = sign(&[("text", json!("永昌藥行"))]);
    let other = sign(&[("text", json!("電器修理"))]);
    assert_ne!(words.geometry_key, other.geometry_key);
    let across = sign(&[
        ("text", json!("電器修理")),
        ("horizontal", json!(true)),
        ("width", json!(1.95)),
        ("height", json!(0.64)),
    ]);
    assert!(extent(&across, 0) > extent(&across, 2));
    // Schemes change the board's colours; a panelled board shows its squares instead of words.
    let tint = |item: &GeneratedItem| {
        item.geometry
            .meshes
            .iter()
            .filter(|m| m.material_slot == "board")
            .flat_map(|m| {
                m.colors
                    .iter()
                    .map(|c| format!("{:.2}/{:.2}/{:.2}", c[0], c[1], c[2]))
            })
            .collect::<BTreeSet<_>>()
    };
    assert_ne!(
        tint(&sign(&[("scheme", json!("red"))])),
        tint(&sign(&[("scheme", json!("green"))]))
    );
    let panelled = sign(&[
        ("scheme", json!("white")),
        ("panels", json!(6)),
        ("panel", json!("#2f7a4a")),
    ]);
    assert_ne!(panelled.geometry_key, words.geometry_key);
    // It reports where grime and rust start: its top edge and its wall fixings.
    let features: BTreeSet<&str> = words
        .geometry
        .features
        .iter()
        .map(|f| f.name.as_str())
        .collect();
    assert!(
        features.contains("top") && features.contains("fixing"),
        "{features:?}"
    );
    // An angled board stands off the wall along its angle.
    let angled = sign(&[("angle", json!(53.13))]);
    assert!(extent(&angled, 1) > extent(&words, 1) + 0.3);
}

#[test]
fn a_steel_balcony_has_rails_where_a_parapet_is_solid() {
    let rails = |rail: &str| {
        let item = generate(
            "kowloon-facade-side.item.json",
            &[("balcony", json!(true)), ("balconyRail", json!(rail))],
        );
        item.geometry
            .meshes
            .iter()
            .filter(|m| m.id.starts_with("balcony/"))
            .map(|m| m.material_slot.clone())
            .collect::<BTreeSet<_>>()
    };
    assert!(rails("steel").contains("metal"), "{:?}", rails("steel"));
    assert!(!rails("parapet").contains("metal"));
}

#[test]
fn wall_patterns_run_on_round_the_corners() {
    // Each side's walls are measured from its distance along the cut footprint, so the concrete
    // and paint continue round every corner: at a side's start its pattern reads that distance.
    let building = generate("kowloon-building.item.json", &[]);
    let anchor = |part: &str| {
        building
            .geometry
            .anchors
            .iter()
            .find(|a| a.part == part)
            .unwrap()
            .clone()
    };
    let along = |part: &str| {
        let a = anchor(part);
        let start = translation(&building, part);
        (0..3)
            .map(|i| (start[i] - a.origin[i]) * a.u[i])
            .sum::<f64>()
    };
    let cut = 0.24f64;
    let corner = cut * 2f64.sqrt();
    assert!(along("side-0").abs() < 1e-9);
    assert!(
        (along("corner-0") - (9.8 - cut)).abs() < 1e-9,
        "{}",
        along("corner-0")
    );
    assert!(
        (along("side-1") - (9.8 - cut + corner)).abs() < 1e-9,
        "{}",
        along("side-1")
    );
    assert!(
        (along("side-2") - (9.8 - cut + corner + 7. - cut)).abs() < 1e-9,
        "{}",
        along("side-2")
    );
    // Upright, from the ground: every side's v is +z and its origin on the ground.
    for part in ["side-0", "side-1", "side-2", "side-3", "corner-0"] {
        let a = anchor(part);
        assert!(
            (a.v[2] - 1.).abs() < 1e-9 && a.origin[2].abs() < 1e-9,
            "{part}: {a:?}"
        );
    }
}

//! The Kowloon buildings as composed ItemSpec definitions (worldspec/themes/kowloon/items):
//! small items (window, air conditioner, floor, ground floor, shopfront, parapet, duct run and
//! riser, roof) that sides, buildings and the corner shop place as sub-items. Counts and
//! heights are derived from the controls here.
use serde_json::json;
use std::{
    collections::{BTreeMap, BTreeSet},
    f64::consts::PI,
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
    // Over the paint: the repairs and peels placed by the floor's frames, the inked edges every
    // wall carries, then the layers that follow a feature.
    assert_eq!(floor.layers["wall"][1].entry, "kowloon-repairs.material.ts");
    assert_eq!(
        floor.layers["wall"][2].entry,
        "kowloon-edge-ink.material.ts"
    );
    let follows: Vec<_> = floor.layers["wall"][2..]
        .iter()
        .filter_map(|layer| layer.follow.get("zone"))
        .map(|zone| serde_json::to_value(zone).expect("followed zones"))
        .collect();
    // Runoff streaks hang in every runoff zone the items state (under the storey's top, its
    // sills, a parapet's coping, a balcony's ledge, the fixtures' drips), a finer fringe under
    // the tops, sills and copings, and rust runs from the fixings.
    assert_eq!(
        follows,
        [
            json!(["drip", "ledge", "runoff-coping", "runoff-sill", "runoff-top"]),
            json!(["runoff-coping", "runoff-sill", "runoff-top"]),
            json!(["fixing-drip", "rust"]),
        ]
    );
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
    // The tiled spandrels carry their own runs down the joints (kowloon-wall-tile); over them
    // only the rust runs from the fixtures.
    let tiles: Vec<_> = floor.layers["tile"]
        .iter()
        .map(|layer| layer.entry.as_str())
        .collect();
    assert_eq!(tiles, ["kowloon-rust-run.material.ts"]);
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
        "kowloon-edge-ink.material.ts",
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
    // With its seeded open shops it stays within what a placed item may hold.
    let vertices: usize = block
        .geometry
        .meshes
        .iter()
        .map(|m| m.positions.len())
        .sum();
    assert!(vertices < 1_000_000, "the block has {vertices} vertices");
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
        // Planned cables on both streets (the front's third rounds the corner), hoods of
        // loose sheet, a rail on the rear and left parapets, and the roof's clutter.
        "side-0/cable-0/",
        "side-0/cable-2/cable",
        "side-0/cable-4/",
        "side-1/cable-1/",
        "side-0/floor-0/canopy-0/sheet",
        // Slices of the neighbours against the back and the left, before their facades.
        "party-2/",
        "party-3/",
        "side-2/floor-0/",
        "side-3/floor-0/",
        "roofRail-2/",
        "roofRail-3/",
        "roofClutter-0/metal",
        "roofClutter-0/clutter",
        "roofClutter-0/cable",
    ] {
        assert!(has(piece), "{piece} is missing");
    }
    for piece in [
        "roofRail-0/",
        "roofRail-1/",
        "side-0/cableAuto-0/",
        "side-2/cableAuto-0/",
        "party-0/",
        "party-1/",
    ] {
        assert!(!has(piece), "{piece} should not be there");
    }
    // The cable round the corner ends on the side street's wall: beyond the front's end in
    // x, and behind the front's wall plane.
    let transforms = building.geometry.world_transforms().unwrap();
    let mut reach = [f64::NEG_INFINITY; 2];
    for (node, matrix) in building.geometry.nodes.iter().zip(&transforms) {
        for index in &node.meshes {
            let mesh = &building.geometry.meshes[*index];
            let mut segments = mesh.id.split('/');
            if mesh.material_slot == "cable"
                && ["side-0", "cable-2"]
                    .iter()
                    .all(|n| segments.any(|s| s == *n))
            {
                for p in &mesh.positions {
                    let p = matrix.point(*p);
                    reach = [reach[0].max(p[0]), reach[1].max(p[1])];
                }
            }
        }
    }
    assert!(
        (reach[0] - 9.83).abs() < 0.03,
        "stands off the side wall at x = {}",
        reach[0]
    );
    assert!(
        (reach[1] - 0.55 - 2.586).abs() < 0.03,
        "runs to y = {}",
        reach[1]
    );
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

/// The distance from `p` to the segment `a`-`b`.
fn to_segment(p: [f64; 3], a: [f64; 3], b: [f64; 3]) -> f64 {
    let along: [f64; 3] = std::array::from_fn(|i| b[i] - a[i]);
    let length: f64 = along.iter().map(|v| v * v).sum();
    let t = if length > 0. {
        ((0..3).map(|i| (p[i] - a[i]) * along[i]).sum::<f64>() / length).clamp(0., 1.)
    } else {
        0.
    };
    (0..3)
        .map(|i| (p[i] - a[i] - along[i] * t).powi(2))
        .sum::<f64>()
        .sqrt()
}

/// A parapet's wall is weathered from its `top` (the streaks under its coping), so every face
/// of every parapet's wall must find a `top` feature of its own part within its own height:
/// at each of its corners, on the long sides as on the corner's facets.
#[test]
fn every_parapet_wall_of_the_reference_plan_has_a_top_within_its_height() {
    let plan = plan("reference-shop-house.json");
    let overrides: Vec<(&str, serde_json::Value)> = plan
        .iter()
        .map(|(key, value)| (key.as_str(), value.clone()))
        .collect();
    let building = generate("kowloon-building.item.json", &overrides);
    let transforms = building.geometry.world_transforms().unwrap();
    let mut walls = 0;
    for (node, matrix) in building.geometry.nodes.iter().zip(&transforms) {
        let part = node.name.as_deref().unwrap_or_default();
        for index in &node.meshes {
            let mesh = &building.geometry.meshes[*index];
            if mesh.material_slot != "wall" || !mesh.id.contains("/parapet/parapet/") {
                continue;
            }
            walls += 1;
            let points: Vec<[f64; 3]> = mesh.positions.iter().map(|p| matrix.point(*p)).collect();
            let low = points.iter().map(|p| p[2]).fold(f64::INFINITY, f64::min);
            let high = points
                .iter()
                .map(|p| p[2])
                .fold(f64::NEG_INFINITY, f64::max);
            let height = high - low;
            assert!((0.3..=2.).contains(&height), "{}: {height}", mesh.id);
            let tops: Vec<_> = building
                .geometry
                .features
                .iter()
                .filter(|f| f.part == part && f.name == "top")
                .collect();
            for p in &points {
                let nearest = tops
                    .iter()
                    .flat_map(|f| f.points.windows(2))
                    .map(|ends| to_segment(*p, ends[0], ends[1]))
                    .fold(f64::INFINITY, f64::min);
                assert!(
                    nearest <= height + 1e-6,
                    "{} at {p:?}: the nearest of {} top features of {part} is {nearest} m away, beyond its height {height}",
                    mesh.id,
                    tops.len()
                );
            }
        }
    }
    // Four sides, each with a front, a back and edges (the round corner's parapet is part of
    // its one wall, which carries its own `coping` line).
    assert!(walls >= 12, "{walls} parapet wall faces");
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
    // A shutter and a door over a tiled dado, a number plate, two caged windows (one roofed),
    // three panelled boards, the corner drain, a tiled roof deck, and paving on all four sides;
    // planned vents, a second conduit to its junction box, and a string course on every side.
    for piece in [
        "side-0/ground/shop-0/bay/",
        "side-0/ground/shop-1/bay/",
        "side-0/ground/dado-0/",
        "plaque-0-0/flat/",
        "side-0/floor-0/cage-0/",
        "side-0/floor-0/cage-1/",
        "board-0-0/board/",
        "board-0-2/board/",
        "side-0/drain-0/",
        "side-0/conduit-1/",
        "side-0/ventAt-0/",
        "side-0/ventAt-2/",
        "side-0/floor-0/band/",
        "side-1/floor-0/band/",
        "side-2/floor-1/band/",
        "side-3/floor-0/band/",
        "side-1/floor-0/window-0/bars/",
        "roofTiles-0/",
        "pavement-0/",
        "pavement-3/",
        // The planned meter by the door and the one planned cable.
        "side-0/ground/meterAt-0/",
        "side-0/cable-0/",
    ] {
        assert!(has(piece), "{piece} is missing");
    }
    // What the plan leaves out: the shop fascia the string course replaces, the window hoods
    // a full cage's own roof replaces, meters, and the back sides' seeded units, pipes and vents.
    for piece in [
        "corner-0/",
        "hut-0/",
        "side-0/balcony/",
        "side-0/floor-0/canopy-0/",
        "side-0/floor-0/canopy-1/",
        "side-0/ground/fascia/",
        "side-0/ground/meters-1/",
        "side-1/cableAuto-0/",
        "roofClutter-0/",
        "side-0/downpipe/",
        "side-1/downpipe/",
        "side-1/pipe-0/",
        "side-2/pipe-0/",
        "side-1/floor-0/aircon-0/",
        "side-1/floor-1/aircon-1/",
        "side-1/floor-0/vent-0/",
        "side-2/floor-0/window-0/",
        "side-3/riser/",
    ] {
        assert!(!has(piece), "{piece} should not be there");
    }
    // The fitted bays fill their wall openings: the shutter's head box ends at the opening's
    // head (2.82 m) and the door's frame at its own (2.52 m), each as wide as its span.
    let transforms = building.geometry.world_transforms().unwrap();
    let extent = |piece: &str, slot: &str| {
        let wanted: Vec<&str> = piece.split('/').collect();
        let mut bounds = [[f64::INFINITY; 3], [f64::NEG_INFINITY; 3]];
        for (node, matrix) in building.geometry.nodes.iter().zip(&transforms) {
            for index in &node.meshes {
                let mesh = &building.geometry.meshes[*index];
                let mut segments = mesh.id.split('/');
                if mesh.material_slot != slot
                    || !wanted.iter().all(|name| segments.any(|s| s == *name))
                {
                    continue;
                }
                for p in &mesh.positions {
                    let p = matrix.point(*p);
                    for axis in 0..3 {
                        bounds[0][axis] = bounds[0][axis].min(p[axis]);
                        bounds[1][axis] = bounds[1][axis].max(p[axis]);
                    }
                }
            }
        }
        bounds
    };
    let shutter = extent("side-0/ground/shop-0/bay", "metal");
    assert!(
        (shutter[0][0] - 0.45).abs() < 1e-6
            && (shutter[1][0] - 2.75).abs() < 1e-6
            && (shutter[1][2] - 2.82).abs() < 1e-6,
        "{shutter:?}"
    );
    let wall = extent("side-0/ground/wall", "wall");
    assert!((wall[1][0] - 6.).abs() < 1e-6, "{wall:?}");
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
    // The top storey's windows show their cartons, in the goods' plain slot; the others none.
    assert!(has("side-0/floor-1/window-0/window/stock"));
    assert!(!has("side-0/floor-0/window-0/window/stock"));
    // A storeroom's window is left unglazed, so the stacks show: the glass is a painted surface.
    assert!(has("side-0/floor-0/window-0/window/glass"));
    assert!(!has("side-0/floor-1/window-0/window/glass"));
    // The planned door's leaves are its paint, not the white of an unbaked finish: blue leads.
    let leaves = extent("side-0/ground/shop-1/bay", "paint");
    assert!((leaves[1][0] - 5.1).abs() < 1e-6, "{leaves:?}");
    let blue = building
        .geometry
        .meshes
        .iter()
        .filter(|m| {
            let mut segments = m.id.split('/');
            m.material_slot == "paint"
                && ["shop-1", "bay"].iter().all(|n| segments.any(|s| s == *n))
        })
        .flat_map(|m| &m.colors)
        .filter(|c| c[2] > c[0] * 1.5 && c[2] > 0.05)
        .count();
    assert!(blue > 100, "{blue} blue vertices");
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

/// The item-space positions of the meshes under `part` whose ID contains `piece`.
fn points_of(item: &GeneratedItem, part: &str, piece: &str) -> Vec<[f64; 3]> {
    let transforms = item.geometry.world_transforms().unwrap();
    let mut points = vec![];
    for (node, matrix) in item.geometry.nodes.iter().zip(transforms) {
        if node.name.as_deref() != Some(part) {
            continue;
        }
        for index in &node.meshes {
            let mesh = &item.geometry.meshes[*index];
            if mesh.id.contains(piece) {
                points.extend(mesh.positions.iter().map(|p| matrix.point(*p)));
            }
        }
    }
    points
}

#[test]
fn a_round_corner_is_one_smooth_surface_that_mouldings_patterns_and_pavement_follow() {
    // The reference corner: a radius of 0.55 m. On a square corner the arc's radius is the cut
    // and its centre (9.25, 0.55); the corner is one item that turns through it in eight chords.
    let cut = 0.55f64;
    let building = generate(
        "kowloon-building.item.json",
        &[
            ("cornerStyle", json!("round")),
            ("cut", json!(cut)),
            ("cornerTiles", json!(true)),
        ],
    );
    assert!(sides(&building, "corner-").is_empty() && sides(&building, "cornerArc-").is_empty());
    assert_eq!(sides(&building, "cornerRound-"), ["cornerRound-0"]);
    let (n, round) = (8usize, "cornerRound-0");
    let center = [9.8 - cut, cut];
    let chord = 2. * cut * (90f64 / (2 * n) as f64).to_radians().sin();
    // It stands where the front's wall ends, along the front, and reaches the coping.
    let start = translation(&building, round);
    assert!((start[0] - (9.8 - cut)).abs() < 1e-9 && start[1].abs() < 1e-9);
    let node = building
        .geometry
        .nodes
        .iter()
        .find(|node| node.name.as_deref() == Some(round))
        .unwrap();
    assert_eq!(node.transform[0], [1., 0., 0.]);
    assert!((top_of(&building, round) - 13.18).abs() < 1e-9);
    // One receiver a material: the wall from the pavement to the coping (with the parapet's top
    // and inner face), the plinth, a tile band and a string course a storey, the parapet's
    // ledge and the coping. Sixteen flat sides stood here before.
    let corner: Vec<_> = building
        .geometry
        .meshes
        .iter()
        .filter(|mesh| mesh.id.starts_with("cornerRound-0/"))
        .collect();
    let count = |slot: &str| corner.iter().filter(|m| m.material_slot == slot).count();
    assert_eq!(
        [
            count("wall-round"),
            count("plinth"),
            count("tile"),
            count("trim")
        ],
        [1, 1, 3, 5]
    );
    assert_eq!(corner.len(), 10);
    let sub = |a: [f64; 3], b: [f64; 3]| [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
    let cross = |a: [f64; 3], b: [f64; 3]| {
        [
            a[1] * b[2] - a[2] * b[1],
            a[2] * b[0] - a[0] * b[2],
            a[0] * b[1] - a[1] * b[0],
        ]
    };
    let dot = |a: [f64; 3], b: [f64; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    for mesh in &corner {
        // No triangle is degenerate, and every normal is a unit vector on its triangle's side.
        for triangle in mesh.indices.chunks_exact(3) {
            let [a, b, c] = [0, 1, 2].map(|k| mesh.positions[triangle[k] as usize]);
            let facing = cross(sub(b, a), sub(c, a));
            assert!(dot(facing, facing).sqrt() > 1e-9, "{}: degenerate", mesh.id);
            for index in triangle {
                let normal = mesh.normals[*index as usize];
                assert!((dot(normal, normal) - 1.).abs() < 1e-9 && dot(facing, normal) > 0.);
            }
        }
        // Everything stays between the two walls: inside the quarter the round turns through.
        for p in &mesh.positions {
            let from = [p[0] + start[0] - center[0], p[1] + start[1] - center[1]];
            assert!(
                from[0] > -1e-9 && from[1] < 1e-9 && from[0].hypot(from[1]) < cut + 0.111,
                "{}: {p:?}",
                mesh.id
            );
        }
    }
    // The wall is smooth: its outer face is n chords of the round, each vertex with the normal of
    // its own radius, and reads the pattern by the distance round the outline and the height.
    let wall = corner
        .iter()
        .find(|m| m.material_slot == "wall-round")
        .unwrap();
    assert_eq!(wall.id, "cornerRound-0/part/wall/wall/side");
    let mut outer = BTreeSet::new();
    for ((p, normal), uv) in wall.positions.iter().zip(&wall.normals).zip(&wall.uvs) {
        let spoke = [p[0] + start[0] - center[0], p[1] + start[1] - center[1]];
        let reach = spoke[0].hypot(spoke[1]);
        if normal[2] != 0. || (reach - cut).abs() > 1e-9 {
            continue;
        }
        assert!(
            (normal[0] - spoke[0] / reach).abs() < 1e-9
                && (normal[1] - spoke[1] / reach).abs() < 1e-9
        );
        let k = (spoke[0] / cut).clamp(-1., 1.).asin() / (PI / 2. / n as f64);
        assert!(
            (k - k.round()).abs() < 1e-6,
            "{p:?} is off the round's chords"
        );
        assert!((uv[0] - (9.8 - cut + k.round() * chord)).abs() < 1e-9);
        assert!((uv[1] - p[2]).abs() < 1e-9);
        outer.insert(k.round() as usize);
    }
    assert_eq!(outer.len(), n + 1);
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
    // The pattern is continuous from the front, round the corner, into the second street: the
    // corner starts at the front's end, and the second wall where the corner's last chord ends.
    assert!((along(round) - (9.8 - cut)).abs() < 1e-9);
    let second = translation(&building, "side-1");
    assert!((second[0] - 9.8).abs() < 1e-9 && (second[1] - cut).abs() < 1e-9);
    assert!((along("side-1") - (9.8 - cut + n as f64 * chord)).abs() < 1e-9);
    // The tile band runs through the corner on every upper storey, and the string courses of
    // both streets end square where the corner's begin and end, section on section to within a
    // millimeter (the walls' are native mouldings, whose rounded arrises bulge a little less).
    assert!(!points_of(&building, round, "/tiles-2/").is_empty());
    let ring = points_of(&building, round, "/band-1/");
    for (side, at) in [("side-0", [9.8 - cut, 0.]), ("side-1", [9.8, cut])] {
        let end: Vec<[f64; 3]> = points_of(&building, side, "/floor-1/part/band/")
            .into_iter()
            .filter(|p| (p[0] - at[0]).hypot(p[1] - at[1]) < 0.15)
            .collect();
        assert!(end.len() >= 5, "{side} has a string course");
        for p in &end {
            assert!(
                ring.iter()
                    .any(|q| (0..3).all(|i| (p[i] - q[i]).abs() < 1e-3)),
                "{side} and the corner do not meet at {p:?}"
            );
        }
    }
    // The roof slab follows the same outline, 0.2 m inside it: one corner becomes n + 1.
    let roof = points_of(&building, "roof", "roof/top");
    assert_eq!(roof.len(), 3 + n + 1);
    let nearest = roof
        .iter()
        .map(|p| ((p[0] - center[0]).hypot(p[1] - center[1]) - (cut - 0.2)).abs())
        .fold(f64::MAX, f64::min);
    assert!(nearest < 0.01, "the roof's corner is {nearest} off its arc");
    // The pavement turns the corner too: the two streets' pavements stop where the round
    // starts and ends, and between them rings of stones and a kerb whose road side is an arc
    // a pavement's depth outside the wall's.
    let (front, side) = (
        points_of(&building, "pavement-0", ""),
        points_of(&building, "pavement-1", ""),
    );
    assert!(front.iter().all(|p| p[0] < center[0] + 1e-4));
    assert!(side.iter().all(|p| p[1] > center[1] - 1e-4));
    let turned = points_of(&building, "pavementCorner-0", "");
    let reach = |p: &[f64; 3]| (p[0] - center[0]).hypot(p[1] - center[1]);
    assert!(
        turned
            .iter()
            .all(|p| p[0] > center[0] - 1e-4 && p[1] < center[1] + 1e-4)
    );
    let outermost = turned.iter().map(reach).fold(0., f64::max);
    assert!((outermost - (cut + 1.45)).abs() < 1e-4, "{outermost}");
    let kerb = points_of(&building, "pavementCorner-0", "kerb");
    let road: Vec<_> = kerb
        .iter()
        .filter(|p| (reach(p) - (cut + 1.45)).abs() < 1e-4)
        .collect();
    // (the kerb's outer arc is at least twelve chords, each with a foot and a head.)
    assert!(
        road.len() >= 2 * 13,
        "{} kerb points on the arc",
        road.len()
    );
    assert!(kerb.iter().all(|p| p[2] > -0.15 - 1e-6 && p[2] < 0.));

    // Another turn: a corner of about 63 degrees is rounded the same way, and its wall ends
    // where the second street's begins.
    let skew = generate(
        "kowloon-building.item.json",
        &[
            ("cornerStyle", json!("round")),
            ("cut", json!(0.8)),
            ("footprint", json!([[0, 0], [10, 0], [13, 6], [0, 6]])),
        ],
    );
    let begins = translation(&skew, "side-1");
    let wall = points_of(&skew, round, "/wall/");
    let gap = wall
        .iter()
        .map(|p| (p[0] - begins[0]).hypot(p[1] - begins[1]))
        .fold(f64::MAX, f64::min);
    assert!(gap < 1e-9, "the corner ends {gap} m from the second wall");
    assert!(
        wall.iter()
            .all(|p| p[1] > -1e-9 && p[1] < begins[1] + 0.3 && p[0] < begins[0] + 1e-9)
    );
}

/// The least distance in plan from `at` to a vertex of the `slot` meshes under the parts.
fn nearest(item: &GeneratedItem, parts: &[&str], slot: &str, at: [f64; 2]) -> f64 {
    let transforms = item.geometry.world_transforms().unwrap();
    let mut least = f64::INFINITY;
    for (node, matrix) in item.geometry.nodes.iter().zip(transforms) {
        if !parts.contains(&node.name.as_deref().unwrap_or_default()) {
            continue;
        }
        for index in &node.meshes {
            let mesh = &item.geometry.meshes[*index];
            if mesh.material_slot == slot {
                for p in &mesh.positions {
                    let p = matrix.point(*p);
                    least = least.min((p[0] - at[0]).hypot(p[1] - at[1]));
                }
            }
        }
    }
    least
}

#[test]
fn upright_corners_and_shop_piers_are_rounded() {
    // Where two walls meet at a square corner each gives up its sharp end for half a round of
    // the building's `arris`: the nearest either comes to the corner is the round's middle,
    // r (sqrt 2 - 1) from it. The rounds are part of the walls' fronts.
    let r = 0.025f64;
    let middle = r * (2f64.sqrt() - 1.);
    let building = generate("kowloon-building.item.json", &[]);
    for (corner, parts) in [
        ([9.8, 7.], ["side-1", "side-2"]),
        ([0., 7.], ["side-2", "side-3"]),
        ([0., 0.], ["side-3", "side-0"]),
    ] {
        let least = nearest(&building, &parts, "wall", corner);
        assert!((least - middle).abs() < 1e-9, "{corner:?}: {least}");
    }
    // The chamfered street corner keeps its two arrises.
    assert!(nearest(&building, &["side-0", "corner-0"], "wall", [9.56, 0.]) < 1e-9);
    // Without an arris every corner is sharp, as it was.
    let sharp = generate("kowloon-building.item.json", &[("arris", json!(0))]);
    assert!(nearest(&sharp, &["side-1", "side-2"], "wall", [9.8, 7.]) < 1e-9);
    // The small warehouse's four square corners are rounded from the ground to the parapet.
    let plan = plan("small-warehouse.json");
    let overrides: Vec<(&str, serde_json::Value)> = plan
        .iter()
        .map(|(key, value)| (key.as_str(), value.clone()))
        .collect();
    let warehouse = generate("kowloon-building.item.json", &overrides);
    for (corner, parts) in [
        ([6., 0.], ["side-0", "side-1"]),
        ([6., 6.], ["side-1", "side-2"]),
        ([0., 6.], ["side-2", "side-3"]),
        ([0., 0.], ["side-3", "side-0"]),
    ] {
        let least = nearest(&warehouse, &parts, "wall", corner);
        assert!((least - middle).abs() < 1e-9, "{corner:?}: {least}");
    }
    let rounded = warehouse
        .geometry
        .meshes
        .iter()
        .filter(|mesh| mesh.id.starts_with("side-0/") && mesh.id.ends_with("/front"))
        .filter(|mesh| {
            let half = 0.5f64.sqrt();
            mesh.normals
                .iter()
                .any(|n| (n[0] - half).abs() < 1e-9 && (n[1] + half).abs() < 1e-9 && n[2] == 0.)
        })
        .count();
    // (the ground storey, two upper storeys and the parapet.)
    assert_eq!(rounded, 4);
    // A shop opening's jambs are rounded too: the piers' fronts turn into the reveals.
    let ground = building
        .geometry
        .meshes
        .iter()
        .find(|mesh| {
            mesh.id.starts_with("side-0/")
                && mesh.id.contains("/ground/")
                && mesh.id.ends_with("/wall/front")
        })
        .unwrap();
    for jamb in [[1., 0., 0.], [-1., 0., 0.]] {
        assert!(ground.normals.contains(&jamb));
    }
}

/// A character without a cell in the shipped lettering atlas is left off its sign without an
/// error, so every board text of the washroom building plans must be covered by the board face.
#[test]
fn washroom_plan_board_texts_have_every_glyph_in_the_shipped_atlas() {
    use superspec_engine::worldspec::themes::kowloon::signs::lettering::{
        Direction, Face, layout_line,
    };
    let directory =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/studio/washroom-items");
    let mut texts = 0;
    let mut missing = BTreeSet::new();
    for path in std::fs::read_dir(directory)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|e| e == "json"))
    {
        let plan: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        for text in plan["boardTexts"].as_array().into_iter().flatten() {
            let text = text.as_str().unwrap();
            texts += 1;
            // Boards are lettered in the Kai face (items.rs), along their width or down it.
            for direction in [Direction::Ltr, Direction::Ttb] {
                let layout = layout_line(text, Face::Kai, 1., 1., direction).unwrap();
                for character in layout.missing {
                    missing.insert(format!("{character} in {text:?} of {}", path.display()));
                }
            }
        }
    }
    assert!(texts > 0, "no washroom plan lists board texts");
    assert!(
        missing.is_empty(),
        "glyphs missing from the atlas: {missing:#?}"
    );
}

/// Whether any mesh lies under the given path of part names.
fn under(item: &GeneratedItem, piece: &str) -> bool {
    let wanted: Vec<&str> = piece.split('/').filter(|s| !s.is_empty()).collect();
    item.geometry.meshes.iter().any(|m| {
        let mut segments = m.id.split('/');
        wanted.iter().all(|name| segments.any(|s| s == *name))
    })
}

#[test]
fn a_clutter_plan_stands_crates_and_cartons_by_the_bays_it_names() {
    // A heap stands by the jamb its plan names, reaching past it by the room there is.
    let left = generate(
        "kowloon-shopfront.item.json",
        &[
            ("width", json!(3)),
            ("clutter", json!("crates")),
            ("clutterSide", json!("left")),
            ("clutterLeft", json!(0.4)),
        ],
    );
    // (1.8 m wide: 0.4 m of it beyond the left jamb at -1.5.)
    assert!((translation(&left, "clutter")[0] + 1.).abs() < 1e-9);
    let right = generate(
        "kowloon-shopfront.item.json",
        &[
            ("width", json!(3)),
            ("clutter", json!("mixed")),
            ("clutterSide", json!("right")),
            ("clutterRight", json!(5)),
        ],
    );
    // (at most six tenths of it stands beyond the jamb.)
    assert!((translation(&right, "clutter")[0] - 1.68).abs() < 1e-9);
    assert!(parts(&generate("kowloon-shopfront.item.json", &[]), "clutter").is_empty());
    // A ground floor's plan names each bay's heap; an open shop may have one beside its stock.
    let ground = generate(
        "kowloon-ground-floor.item.json",
        &[
            ("width", json!(10)),
            ("shopPlan", json!(["gate", "shutter", "open"])),
            (
                "clutterPlan",
                json!(["cartons-left", "none", "crates-right"]),
            ),
        ],
    );
    assert!(under(&ground, "shop-0/clutter"));
    assert!(!under(&ground, "shop-1/clutter"));
    assert!(under(&ground, "shop-2/clutter") && under(&ground, "shop-2/stock"));
    // A building hands each street side its row; sides it does not name keep the seed.
    let building = generate(
        "kowloon-building.item.json",
        &[
            ("shopPlans", json!([["gate", "shutter", "shutter"]])),
            ("clutterPlans", json!([["mixed-left", "none", "none"]])),
        ],
    );
    assert!(under(&building, "side-0/ground/shop-0/clutter"));
    assert!(!under(&building, "side-0/ground/shop-1/clutter"));
    assert!(!under(&building, "side-0/ground/shop-2/clutter"));
    // Without a plan about one seeded bay in four is an open shop.
    let (mut open, mut bays) = (0, 0);
    for seed in 0..16 {
        let ground = generate(
            "kowloon-ground-floor.item.json",
            &[("seed", json!(seed)), ("width", json!(12.7))],
        );
        for bay in 0..4 {
            bays += 1;
            open += usize::from(under(&ground, &format!("shop-{bay}/room")));
        }
    }
    assert!(
        (10..=24).contains(&open),
        "{open} of {bays} seeded bays are open"
    );
}

#[test]
fn a_gate_takes_its_lattice_paint_from_the_building() {
    let colors = |item: &GeneratedItem| -> Vec<u64> {
        item.geometry
            .meshes
            .iter()
            .filter(|m| m.id.contains("bay"))
            .flat_map(|m| m.colors.iter().flatten().map(|v| v.to_bits()))
            .collect()
    };
    let gate = |overrides: &[(&str, serde_json::Value)]| {
        let mut all = vec![("kind", json!("gate"))];
        all.extend(overrides.iter().cloned());
        generate("kowloon-shopfront.item.json", &all)
    };
    let seeded = gate(&[]);
    assert_eq!(
        colors(&seeded),
        colors(&gate(&[("gateColor", json!("#647a6b"))]))
    );
    assert_ne!(
        colors(&seeded),
        colors(&gate(&[("gateColor", json!("#7d7b74"))]))
    );
    let plan = [("shopPlans", json!([["gate", "shutter", "shutter"]]))];
    let mut grey = plan.to_vec();
    grey.push(("gateColor", json!("#7d7b74")));
    let part = |item: &GeneratedItem| -> Vec<u64> {
        item.geometry
            .meshes
            .iter()
            .filter(|m| m.id.starts_with("side-0/") && m.id.contains("/shop-0/"))
            .flat_map(|m| m.colors.iter().flatten().map(|v| v.to_bits()))
            .collect()
    };
    assert_ne!(
        part(&generate("kowloon-building.item.json", &plan)),
        part(&generate("kowloon-building.item.json", &grey))
    );
}

#[test]
fn shop_rooms_of_two_streets_do_not_meet_behind_their_corner() {
    // The deepest point behind the wall of the rooms under a part.
    let depth = |item: &GeneratedItem, piece: &str| {
        let wanted: Vec<&str> = piece.split('/').collect();
        let transforms = item.geometry.world_transforms().unwrap();
        let mut deepest = f64::NEG_INFINITY;
        for (node, _) in item.geometry.nodes.iter().zip(&transforms) {
            for index in &node.meshes {
                let mesh = &item.geometry.meshes[*index];
                let mut segments = mesh.id.split('/');
                if wanted.iter().all(|name| segments.any(|s| s == *name)) {
                    for p in &mesh.positions {
                        deepest = deepest.max(p[1]);
                    }
                }
            }
        }
        deepest
    };
    // The front's last shop reaches the corner, 3.1 m deep. The side that leaves the corner
    // yields: its first bay starts within those 3.1 m, where only the half metre the front
    // keeps clear of the corner is free, too shallow for a room, so it stays shut; its second
    // bay starts beyond and is an open shop.
    let plans = json!([["shutter", "shutter", "open"], ["open", "open"]]);
    let building = generate(
        "kowloon-building.item.json",
        &[("shopPlans", plans.clone())],
    );
    assert!(under(&building, "side-0/ground/shop-2/room"));
    assert!(!under(&building, "side-1/ground/shop-0/room"));
    assert!(under(&building, "side-1/ground/shop-0/bay"));
    assert!(under(&building, "side-1/ground/shop-1/room"));
    // Where the plan says the first side keeps 3 m clear of the corner, the yielding side's
    // rooms within the first side's depth are that deep and no deeper.
    let clear = generate(
        "kowloon-building.item.json",
        &[
            (
                "shopPlans",
                json!([["open", "shutter", "shutter"], ["open", "open"]]),
            ),
            ("shopDepth", json!(4)),
            ("cornerClear", json!(3)),
        ],
    );
    assert!((depth(&clear, "side-0/ground/shop-0/room") - 4.).abs() < 0.02);
    for bay in ["shop-0", "shop-1"] {
        let reach = depth(&clear, &format!("side-1/ground/{bay}/room"));
        assert!((reach - 3.14).abs() < 0.02, "{bay} is {reach} m deep");
    }
    // A side with no street before it keeps its depth.
    let alone = generate(
        "kowloon-building.item.json",
        &[
            ("streets", json!([false, true, false, false])),
            ("shopPlans", json!([[], ["open", "open"]])),
        ],
    );
    assert!((depth(&alone, "side-1/ground/shop-0/room") - 3.1).abs() < 0.02);
}

/// The bounds of one part's meshes whose ids end with `piece`.
fn extent(item: &GeneratedItem, part: &str, piece: &str) -> [[f64; 3]; 2] {
    let transforms = item.geometry.world_transforms().unwrap();
    let mut bounds = [[f64::INFINITY; 3], [f64::NEG_INFINITY; 3]];
    for (node, matrix) in item.geometry.nodes.iter().zip(transforms) {
        if node.name.as_deref() != Some(part) {
            continue;
        }
        for index in &node.meshes {
            let mesh = &item.geometry.meshes[*index];
            if !mesh.id.ends_with(piece) {
                continue;
            }
            for p in &mesh.positions {
                let p = matrix.point(*p);
                for k in 0..3 {
                    bounds[0][k] = bounds[0][k].min(p[k]);
                    bounds[1][k] = bounds[1][k].max(p[k]);
                }
            }
        }
    }
    bounds
}

fn corners(item: &GeneratedItem, id: &str) -> usize {
    item.geometry
        .meshes
        .iter()
        .filter(|m| m.id == id)
        .map(|m| m.indices.len())
        .sum()
}

#[test]
fn sashes_grilles_and_lit_rooms_follow_a_windows_controls() {
    let window = |overrides: &[(&str, serde_json::Value)]| {
        let mut values = vec![("width", json!(2.1)), ("height", json!(1.7))];
        values.extend(overrides.iter().cloned());
        generate("kowloon-window.item.json", &values)
    };
    // No light stands open at a chance of zero; two do at 2, swung or pushed out in front of
    // the wall (-y).
    let shut = window(&[("openChance", json!(0))]);
    let open = window(&[("openChance", json!(2))]);
    let front = |item: &GeneratedItem| extent(item, "window", "/glass")[0][1];
    assert!(front(&shut) > -0.05, "{}", front(&shut));
    assert!(front(&open) < front(&shut) - 0.1, "{}", front(&open));
    // Four lights across 2.1 m by default; three at 0.7 m a light, with a mid rail in the frame.
    let narrow = window(&[
        ("openChance", json!(0)),
        ("lightWidth", json!(0.7)),
        ("midRail", json!(true)),
    ]);
    assert_eq!(
        corners(&narrow, "window/glass") * 4,
        corners(&shut, "window/glass") * 3
    );
    assert!(corners(&narrow, "window/paint") < corners(&shut, "window/paint"));
    // A louvred top light is slats instead of one pane.
    let louvred = window(&[("openChance", json!(0)), ("louvre", json!("right"))]);
    assert!(corners(&louvred, "window/glass") > corners(&shut, "window/glass"));
    // A lit room is drawn in depth, its walls in `room-lit` and its lamp's tube in the lit
    // lamps' slot, and its panes are left out to show it; at `room` 0 it is a flat back panel
    // in the lamps' slot. An unlit window keeps its dark room.
    assert_eq!(corners(&shut, "window/tube-light"), 0);
    assert_eq!(corners(&shut, "window/room-lit"), 0);
    let flat = window(&[("lit", json!(true)), ("room", json!(0))]);
    assert_eq!(corners(&flat, "window/tube-light"), 6);
    assert_eq!(corners(&flat, "window/room-lit"), 0);
    let lit = window(&[("lit", json!(true))]);
    assert!(corners(&lit, "window/tube-light") > 6);
    assert!(corners(&lit, "window/room-lit") > 6);
    for item in [&flat, &lit] {
        assert_eq!(corners(item, "window/glass"), 0);
        assert_eq!(corners(item, "window/pane"), 0);
    }
    // A grille over the upper half starts halfway up the opening.
    let whole = window(&[("bars", json!(true)), ("barsPattern", json!("diamond"))]);
    let upper = window(&[
        ("bars", json!(true)),
        ("barsPattern", json!("diamond")),
        ("barsShare", json!(0.5)),
    ]);
    assert!(extent(&whole, "bars", "")[0][2] < 0.);
    let foot = extent(&upper, "bars", "")[0][2];
    assert!((foot - (1.8 * 0.5 - 0.04)).abs() < 0.03, "{foot}");
    assert!((extent(&upper, "bars", "")[1][2] - extent(&whole, "bars", "")[1][2]).abs() < 1e-6);
}

#[test]
fn a_fixture_plan_cages_a_window_unit_and_moves_a_condenser_along_its_bank() {
    // Three bays of 2.1 m from 1.35 m; the sill is at 0.95 m.
    let floor = generate(
        "kowloon-floor.item.json",
        &[
            ("fixturePlan", json!(["caged-aircon", "split", "none"])),
            ("fixtureAt", json!([[0.3, 0.05], [0.6, 0.1]])),
        ],
    );
    assert_eq!(parts(&floor, "cage-"), ["cage-0"]);
    assert_eq!(parts(&floor, "aircon-"), ["aircon-1"]);
    // The cage holds the unit: its casing is among the cage's meshes, inside its bars.
    let cage = extent(&floor, "cage-0", "/paint");
    let unit = extent(&floor, "cage-0", "/aircon");
    assert!(unit[0][0] > cage[0][0] && unit[1][0] < cage[1][0]);
    assert!(unit[0][1] > cage[0][1] && unit[0][2] > cage[0][2]);
    let at = translation(&floor, "cage-0");
    assert!((at[0] - 2.7).abs() < 1e-9 && (at[2] - 1.).abs() < 1e-9);
    // The condenser stands 0.6 m right of its bank's centre, its foot 0.1 m above the sill.
    let at = translation(&floor, "aircon-1");
    assert!((at[0] - 5.1).abs() < 1e-9 && (at[2] - 1.05).abs() < 1e-9);
    // Unplanned, a fixture hangs centred under its sill as before.
    let plain = generate(
        "kowloon-floor.item.json",
        &[("fixturePlan", json!(["none", "split", "none"]))],
    );
    let at = translation(&plain, "aircon-1");
    assert!((at[0] - 4.5).abs() < 1e-9 && (at[2] - 0.23).abs() < 1e-9);
    // The seeded share: with every bay a window unit, `cagedChance` 1 cages them all.
    let seeded = generate(
        "kowloon-floor.item.json",
        &[
            ("airconChance", json!(1)),
            ("condenserChance", json!(0)),
            ("cagedChance", json!(1)),
        ],
    );
    assert_eq!(parts(&seeded, "cage-").len(), 3);
    assert!(parts(&seeded, "aircon-").is_empty());
}

#[test]
fn a_back_door_stands_in_an_opening_of_the_ground_storeys_wall() {
    let side = |overrides: &[(&str, serde_json::Value)]| {
        let mut values = vec![("groundShops", json!(false))];
        values.extend(overrides.iter().cloned());
        generate("kowloon-facade-side.item.json", &values)
    };
    let plain = side(&[]);
    let door = side(&[("backDoor", json!(true))]);
    assert!(parts(&plain, "backDoor").is_empty());
    assert_eq!(parts(&door, "backDoor"), ["backDoor"]);
    assert_eq!(
        openings(&door, "groundFloor"),
        openings(&plain, "groundFloor") + 1
    );
    // Seeded, it stands between two bays (three of 2.1 m from 1.35 m), clear of their windows.
    let x = translation(&door, "backDoor")[0];
    assert!(
        (x - 3.45).abs() < 1e-9 || (x - 5.55).abs() < 1e-9,
        "the door is at {x}"
    );
    // The leaf is behind the wall plane (y > 0 is inside), within the opening's 2.05 m.
    let leaf = extent(&door, "backDoor", "/paint");
    assert!(
        leaf[0][1] > 0. && leaf[1][1] < 0.25 && leaf[1][2] < 2.1,
        "{leaf:?}"
    );
    // A plan places it.
    for at in [3.45, 5.55] {
        let placed = side(&[("backDoor", json!(true)), ("backDoorAt", json!(at))]);
        assert!((translation(&placed, "backDoor")[0] - at).abs() < 1e-9);
    }
}

/// The world positions and tints of the meshes whose ids hold every name of `piece` in order,
/// in one slot.
fn piece_vertices(item: &GeneratedItem, piece: &str, slot: &str) -> Vec<([f64; 3], [f64; 4])> {
    let wanted: Vec<&str> = piece.split('/').filter(|s| !s.is_empty()).collect();
    let transforms = item.geometry.world_transforms().unwrap();
    let mut found = vec![];
    for (node, matrix) in item.geometry.nodes.iter().zip(&transforms) {
        for index in &node.meshes {
            let mesh = &item.geometry.meshes[*index];
            let mut segments = mesh.id.split('/');
            if mesh.material_slot != slot || !wanted.iter().all(|n| segments.any(|s| s == *n)) {
                continue;
            }
            for (p, c) in mesh.positions.iter().zip(&mesh.colors) {
                found.push((matrix.point(*p), *c));
            }
        }
    }
    found
}

/// Whether a mesh's id holds every name of `piece` in order.
fn has_piece(item: &GeneratedItem, piece: &str) -> bool {
    let wanted: Vec<&str> = piece.split('/').filter(|s| !s.is_empty()).collect();
    item.geometry.meshes.iter().any(|m| {
        let mut segments = m.id.split('/');
        wanted.iter().all(|name| segments.any(|s| s == *name))
    })
}

#[test]
fn party_edges_stand_a_neighbour_against_the_building() {
    let building = |overrides: &[(&str, serde_json::Value)]| {
        let mut all = vec![
            ("signs", json!(false)),
            ("roofRailPlan", json!([0, 0, 1, 1])),
        ];
        all.extend(overrides.iter().cloned());
        generate("kowloon-building.item.json", &all)
    };
    let has = |item: &GeneratedItem, prefix: &str| {
        item.geometry
            .meshes
            .iter()
            .any(|m| m.id.split('/').any(|s| s == prefix))
    };
    // None by default: every edge is its own facade.
    let bare = building(&[]);
    assert!(!(0..4).any(|e| has(&bare, &format!("party-{e}"))));
    // A wall replaces the back's facade (and its roof rail) with a plain wall a little proud
    // of it, past both its ends; a street edge keeps its facade whatever its plan says.
    let walled = building(&[("partyPlans", json!(["wall", "none", "wall", "none"]))]);
    assert!(has(&walled, "party-2") && !has(&walled, "side-2") && !has(&walled, "roofRail-2"));
    assert!(has(&walled, "side-0") && !has(&walled, "party-0") && has(&walled, "roofRail-3"));
    let wall = piece_vertices(&walled, "party-2", "wall");
    let (low_y, high_y) = wall.iter().fold((f64::MAX, f64::MIN), |(a, b), (p, _)| {
        (a.min(p[1]), b.max(p[1]))
    });
    // The back edge (y = 7) faces +y: the wall's face stands 8 cm out, its back in the wall.
    assert!(
        (high_y - 7.08).abs() < 1e-6 && low_y > 6.7,
        "{low_y}..{high_y}"
    );
    let top = wall.iter().map(|(p, _)| p[2]).fold(f64::MIN, f64::max);
    let parapet = top_of(&bare, "side-2");
    assert!(
        (top - parapet).abs() < 1.0,
        "the wall's top {top} by the parapet's {parapet}"
    );
    // It is stained: its tints vary from its foot to its top.
    let shades: BTreeSet<i64> = wall
        .iter()
        .map(|(_, c)| ((c[0] + c[1] + c[2]) * 50.) as i64)
        .collect();
    assert!(shades.len() > 8, "{} shades", shades.len());
    // A stub keeps the facade behind a slice of the neighbour 1.1 to 1.9 m deep, with
    // windows, at the storeys its plan gives; the seed varies the rest.
    let stubbed = |storeys: u32, seed: u32| {
        building(&[
            ("partyPlans", json!(["none", "none", "stub", "stub"])),
            ("partyStoreyPlans", json!([0, 0, storeys, 0])),
            ("seed", json!(seed)),
        ])
    };
    let stub = stubbed(2, 3);
    assert!(has(&stub, "side-2") && has(&stub, "party-2") && has(&stub, "party-3"));
    let slice = piece_vertices(&stub, "party-2", "wall");
    let deepest = slice.iter().map(|(p, _)| p[1]).fold(f64::MIN, f64::max);
    assert!((8.1..=8.9).contains(&deepest), "reaches y = {deepest}");
    let height = slice.iter().map(|(p, _)| p[2]).fold(f64::MIN, f64::max);
    // Two storeys (3.35 m and 3 m) and a parapet of 0.4 to 1 m.
    assert!((6.7..=7.4).contains(&height), "{height}");
    assert!(!piece_vertices(&stub, "party-2", "pane").is_empty());
    assert!(!piece_vertices(&stub, "party-2", "frame").is_empty());
    // The left edge (x = 0) faces -x: its slice stands out beyond it.
    let left = piece_vertices(&stub, "party-3", "wall");
    assert!(left.iter().all(|(p, _)| p[0] < 1e-6));
    // A party edge has no seeded back door of its own.
    assert!(!has(&stub, "backDoor"));
    let heights: BTreeSet<i64> = (0..6)
        .map(|seed| {
            piece_vertices(&stubbed(0, seed), "party-3", "wall")
                .iter()
                .map(|(p, _)| p[2])
                .fold(f64::MIN, f64::max)
        })
        .map(|h| (h * 10.) as i64)
        .collect();
    assert!(heights.len() > 1, "seeds give one height: {heights:?}");
}

#[test]
fn awning_plans_conduit_colour_and_a_rusted_head_box_reach_their_natives() {
    let plan = plan("small-warehouse.json");
    let overrides: Vec<(&str, serde_json::Value)> = plan
        .iter()
        .map(|(key, value)| (key.as_str(), value.clone()))
        .collect();
    let small = generate("kowloon-building.item.json", &overrides);
    // The planned pale conduits.
    let conduit = piece_vertices(&small, "side-0/conduit-0", "metal");
    assert!(!conduit.is_empty());
    assert!(
        conduit.iter().all(|(_, c)| c[0] > 0.3 && c[2] > 0.3),
        "the conduit is pale"
    );
    // The shutter's head box is streaked: rust and grey both show on its face.
    let head: Vec<[f64; 4]> = piece_vertices(&small, "side-0/ground/shop-0/bay", "metal")
        .into_iter()
        .filter(|(p, _)| p[2] > 2.45 && p[1] < -0.12)
        .map(|(_, c)| c)
        .collect();
    let rusty = head.iter().filter(|c| c[0] > c[2] * 2.).count();
    let shades: BTreeSet<i64> = head.iter().map(|c| ((c[0] - c[2]) * 200.) as i64).collect();
    assert!(
        rusty > 20 && shades.len() > 8,
        "{rusty} rusty of {}, {} shades",
        head.len(),
        shades.len()
    );
    // The number plate's face is planned (a bold one reads from across the street).
    let plate = |face: &str| {
        generate(
            "kowloon-sign.item.json",
            &[
                ("mount", json!("flat")),
                ("text", json!("12")),
                ("width", json!(0.27)),
                ("height", json!(0.21)),
                ("face", json!(face)),
            ],
        )
        .geometry_key
    };
    assert_ne!(plate("kai"), plate("heavy"));
    assert!(has_piece(&small, "plaque-0-0/flat/"));
    // Awning plans per bay: stripes and sag reach the striped canvas and the tarp.
    let ground = |overrides: &[(&str, serde_json::Value)]| {
        let mut all = vec![
            ("width", json!(9.0)),
            ("shopSpans", json!([[0.4, 2.7], [3.05, 8.75]])),
            ("awningPlan", json!(["tarp", "striped"])),
            ("shopPlan", json!(["gate", "gate"])),
        ];
        all.extend(overrides.iter().cloned());
        generate("kowloon-ground-floor.item.json", &all)
    };
    let plain = ground(&[]);
    let planned = ground(&[
        ("awningStripePlan", json!([0, 25])),
        ("awningSagPlan", json!([0.6, 0])),
    ]);
    let count = |item: &GeneratedItem, piece: &str| piece_vertices(item, piece, "tarp").len();
    assert!(count(&planned, "shop-1/tarp") > count(&plain, "shop-1/tarp"));
    let lowest = |item: &GeneratedItem| {
        piece_vertices(item, "shop-0/tarp", "tarp")
            .iter()
            .map(|(p, _)| p[2])
            .fold(f64::MAX, f64::min)
    };
    assert!(lowest(&planned) < lowest(&plain) - 0.1);
}

/// The length of a polyline.
fn polyline_length(points: &[[f64; 3]]) -> f64 {
    points
        .windows(2)
        .map(|pair| {
            (0..3)
                .map(|i| (pair[1][i] - pair[0][i]).powi(2))
                .sum::<f64>()
                .sqrt()
        })
        .sum()
}

/// The reference building states where its runoff hangs: a zone below every storey's top and
/// every sill, the coping and the round corner's string courses, whose `u` runs on from side to
/// corner to side, and no zone ever reaches above its edge.
#[test]
fn the_reference_buildings_runoff_zones_run_on_round_the_corner() {
    use superspec_engine::{authoring::item_features, materialspec::program::NamedFeatures};
    let plan = plan("reference-shop-house.json");
    let overrides: Vec<(&str, serde_json::Value)> = plan
        .iter()
        .map(|(key, value)| (key.as_str(), value.clone()))
        .collect();
    let building = generate("kowloon-building.item.json", &overrides);
    let zones = |name: &str| -> Vec<&item::Feature> {
        building
            .geometry
            .features
            .iter()
            .filter(|f| f.name == name && f.zone.is_some())
            .collect()
    };
    for name in ["runoff-top", "runoff-sill", "runoff-coping", "drip"] {
        assert!(!zones(name).is_empty(), "no {name} zone");
    }
    // Every storey top of the front side and of the corner's courses, at the same heights.
    let tops = zones("runoff-top");
    let corner: Vec<_> = tops
        .iter()
        .filter(|f| f.part.starts_with("cornerRound"))
        .collect();
    assert!(!corner.is_empty());
    for course in &corner {
        let start = course.points[0];
        let end = *course.points.last().unwrap();
        let zone = course.zone.as_ref().unwrap();
        // The side that ends where the course starts, and the one that starts where it ends.
        let meets = |at: [f64; 3], first: bool| {
            tops.iter()
                .filter(|f| f.part.starts_with("side-"))
                .find(|f| {
                    let p = if first {
                        f.points[0]
                    } else {
                        *f.points.last().unwrap()
                    };
                    (0..3).all(|i| (p[i] - at[i]).abs() < 0.02)
                })
                .unwrap_or_else(|| panic!("no side's top meets the course at {at:?}"))
        };
        let before = meets(start, false);
        let after = meets(end, true);
        let ending = before.zone.as_ref().unwrap().u0 + polyline_length(&before.points);
        assert!((ending - zone.u0).abs() < 0.03, "{ending} then {}", zone.u0);
        let leaving = zone.u0 + polyline_length(&course.points);
        let next = after.zone.as_ref().unwrap().u0;
        assert!((leaving - next).abs() < 0.03, "{leaving} then {next}");
    }
    // On a front storey's wall: just below its top it is in the storey's zone, just above it
    // (the storey above or the parapet) in no storey's zone but the next.
    let identity = [
        1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.,
    ];
    let side: Vec<_> = building
        .geometry
        .features
        .iter()
        .filter(|f| f.part == "side-0")
        .cloned()
        .collect();
    let named = NamedFeatures::new(&item_features(&side, &identity)).unwrap();
    let top = tops
        .iter()
        .filter(|f| f.part == "side-0")
        .max_by(|a, b| a.points[0][2].total_cmp(&b.points[0][2]))
        .unwrap();
    let [a, b] = [top.points[0], *top.points.last().unwrap()];
    let middle: [f64; 3] = std::array::from_fn(|i| (a[i] + b[i]) / 2.);
    // Facing out of the front: the side runs along +x, so its front faces -y.
    let out = [0., -1., 0.];
    let set = named.zones("runoff-top").unwrap();
    let below = set.at([middle[0], middle[1], middle[2] - 0.05], out).0;
    assert!(
        below.is_some_and(|hit| (hit.v - 0.05).abs() < 1e-6),
        "{below:?}"
    );
    assert!(
        set.at([middle[0], middle[1], middle[2] + 0.05], out)
            .0
            .is_none()
    );
}

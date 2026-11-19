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
    generate_with(name, overrides, |_| {})
}

fn generate_with(
    name: &str,
    overrides: &[(&str, serde_json::Value)],
    edit: impl FnOnce(&mut item::ItemDefinition),
) -> GeneratedItem {
    let files = library();
    let reader = |name: &str| -> Result<Arc<[u8]>, String> {
        files
            .get(name)
            .map(|text| Arc::from(text.as_bytes()))
            .ok_or(format!("Missing {name}"))
    };
    let mut definition = item::parse(name, &files[name]).unwrap();
    edit(&mut definition);
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

/// The bounds, in item coordinates, of the meshes whose ids `wanted` accepts.
fn bounds_of(item: &GeneratedItem, wanted: impl Fn(&str) -> bool) -> [[f64; 3]; 2] {
    let transforms = item.geometry.world_transforms().unwrap();
    let mut bounds = [[f64::INFINITY; 3], [f64::NEG_INFINITY; 3]];
    for (node, matrix) in item.geometry.nodes.iter().zip(transforms) {
        for index in &node.meshes {
            let mesh = &item.geometry.meshes[*index];
            if !wanted(&mesh.id) {
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
    assert!(
        bounds[0][0].is_finite(),
        "no meshes matched among {:?}",
        item.geometry
            .meshes
            .iter()
            .map(|m| &m.id)
            .collect::<Vec<_>>()
    );
    bounds
}

/// Whether a mesh is part `name` of the `k`th bracket sub-item.
fn piece(id: &str, k: usize, name: &str) -> bool {
    id.starts_with(&format!("bracket-{k}/")) && id.contains(&format!("/{name}/"))
}

/// Authored features of one name whose part starts with `prefix`.
fn features_of<'a>(item: &'a GeneratedItem, name: &str, prefix: &str) -> Vec<&'a item::Feature> {
    item.geometry
        .features
        .iter()
        .filter(|f| f.name == name && f.part.starts_with(prefix))
        .collect()
}

#[test]
fn the_riser_stands_close_on_strapped_brackets_between_its_seams() {
    let (height, base, story): (f64, f64, f64) = (15., 0.9, 3.);
    let riser = generate(
        "kowloon-duct-riser.item.json",
        &[("floors", json!(5)), ("height", json!(height))],
    );
    // The body's back stands 6 cm in front of the wall (y 0; the item's front is toward -y).
    let body = bounds_of(&riser, |id| id.starts_with("riser/body"));
    assert!((body[1][1] + 0.06).abs() < 1e-6, "{body:?}");
    // One bracket midway between each pair of seams, which are at most 1.3 m apart from 0.6 m
    // above the foot to 0.6 m below the top (15 - 0.35 m).
    let span = height - 0.35 - base - 1.2;
    let count = (span / 1.3 - 1e-9).ceil() as usize;
    assert_eq!(parts(&riser, "bracket-").len(), count);
    let mut levels = vec![];
    for k in 0..count {
        let plate = bounds_of(&riser, |id| piece(id, k, "plate"));
        let strap = bounds_of(&riser, |id| piece(id, k, "strap"));
        // A plate flat on the wall, wider than the duct, and a strap round the duct's sides and
        // front whose feet are bolted to it.
        assert!(
            plate[1][1].abs() < 1e-9 && plate[0][1] > -0.02,
            "{k}: {plate:?}"
        );
        assert!(plate[0][0] < body[0][0] - 0.1 && plate[1][0] > body[1][0] + 0.1);
        assert!(
            strap[0][1] < body[0][1] && strap[0][0] < body[0][0] && strap[1][0] > body[1][0],
            "{k}: {strap:?} round {body:?}"
        );
        assert!(strap[1][1] > -0.02, "{k}: the strap's feet reach the plate");
        levels.push((plate[0][2] + plate[1][2]) / 2.);
    }
    // Spaced about 1.3 m, moved clear of each storey's branch collar and string course.
    for pair in levels.windows(2) {
        assert!((0.85..=1.55).contains(&(pair[1] - pair[0])), "{levels:?}");
    }
    for j in 0..5 {
        let branch = (j + 1) as f64 * story - 0.45;
        assert!(
            levels.iter().all(|z| (z - branch).abs() > 0.19),
            "{levels:?}"
        );
        let collar = bounds_of(&riser, |id| id.starts_with(&format!("branch-{j}/fitting")));
        assert!(
            collar[1][1] > -0.001 && collar[0][1] > -0.03,
            "{j}: {collar:?}"
        );
    }
    // The wall's materials find a fixing across each bracket plate and branch collar, with rust
    // below the bracket feet and a drip below each collar, all on the wall plane.
    let fixings = features_of(&riser, "fixing", "bracket-");
    assert_eq!(fixings.len(), count);
    for (fixing, z) in fixings.iter().zip(&levels) {
        assert!(
            fixing
                .points
                .iter()
                .all(|p| p[1] == 0. && (p[2] - z).abs() < 1e-9),
            "{:?} at {z}",
            fixing.points
        );
    }
    assert_eq!(features_of(&riser, "rust", "bracket-").len(), 2 * count);
    assert!(
        features_of(&riser, "rust", "bracket-")
            .iter()
            .all(|f| f.zone.is_some() && f.points[0][2] < levels.iter().cloned().fold(0., f64::max))
    );
    for name in ["fixing", "rust", "drip"] {
        assert_eq!(features_of(&riser, name, "riser").len(), 5, "{name}");
    }
    assert!(
        riser
            .geometry
            .features
            .iter()
            .filter(|f| f.within > 0.)
            .all(|f| f.points.iter().all(|p| p[1] == 0.) && f.within == 1.)
    );
}

#[test]
fn the_run_hangs_close_under_the_parapet_with_collared_drops() {
    // 5 m to the left (direction -1): seams from 0.5 m in, at most 1.4 m apart.
    let run = generate("kowloon-duct-run.item.json", &[]);
    let body = bounds_of(&run, |id| id.starts_with("run/body"));
    assert!((body[1][1] + 0.06).abs() < 1e-6, "{body:?}");
    let pitch = 4. / 3.;
    assert_eq!(parts(&run, "bracket-").len(), 3);
    for k in 0..3 {
        let x = -(0.5 + pitch * (k as f64 + 0.5));
        let plate = bounds_of(&run, |id| piece(id, k, "plate"));
        let hanger = bounds_of(&run, |id| piece(id, k, "hanger"));
        // Its plate is on the wall below the run, and the hanger cradles it from below without
        // rising above it (the parapet's ledge is just over it).
        assert!(
            plate[1][1].abs() < 1e-9 && plate[0][2] < body[0][2] - 0.1,
            "{plate:?}"
        );
        assert!(((plate[0][0] + plate[1][0]) / 2. - x).abs() < 1e-9);
        assert!(
            hanger[0][2] < body[0][2]
                && hanger[1][2] < body[1][2] + 0.01
                && hanger[0][1] < body[0][1],
            "{k}: {hanger:?} under {body:?}"
        );
    }
    // Drops into the wall every second bracket interval (about the 2.1 m bay), each through a
    // collar bolted to the wall, a quarter interval from the seams and brackets.
    assert_eq!(parts(&run, "branch-"), ["branch-0", "branch-2"]);
    for j in [0, 2] {
        let collar = bounds_of(&run, |id| id.starts_with(&format!("branch-{j}/fitting")));
        assert!(
            collar[1][1] > -0.001 && collar[0][1] > -0.03,
            "{j}: {collar:?}"
        );
        let x = -(0.5 + pitch * (j as f64 + 0.25));
        assert!(((collar[0][0] + collar[1][0]) / 2. - x).abs() < 1e-9);
    }
    // Rust runs down from below each hanger's foot, and down the wall from each collar.
    let rust = features_of(&run, "rust", "bracket-");
    assert_eq!(rust.len(), 3);
    for f in &rust {
        let zone = f.zone.as_ref().expect("a rust zone");
        assert!(
            f.points.iter().all(|p| p[1] == 0. && p[2] < body[0][2]),
            "{:?}",
            f.points
        );
        assert!((zone.down[2] + 1.).abs() < 1e-9, "{:?}", zone.down);
    }
    assert_eq!(features_of(&run, "fixing", "bracket-").len(), 3);
    assert_eq!(features_of(&run, "fixing", "run").len(), 2);
    assert_eq!(features_of(&run, "rust", "run").len(), 2);
    assert_eq!(features_of(&run, "drip", "run").len(), 1);
}

#[test]
fn a_sides_ducts_stand_close_and_report_their_wall_fixings() {
    for (bands, gap) in [(false, 0.06), (true, 0.13)] {
        let side = generate(
            "kowloon-facade-side.item.json",
            &[
                ("bands", json!(bands)),
                ("ductRun", json!(true)),
                ("ducts", json!(6)),
            ],
        );
        // The riser and run stand in one plane, clear of the string courses when there are any.
        for part in ["riser", "run"] {
            let body = bounds_of(&side, |id| {
                id.starts_with(&format!("{part}/")) && id.ends_with(&format!("{part}/body"))
            });
            assert!((body[1][1] + gap).abs() < 1e-6, "{part}: {body:?}");
        }
        // The run is under the roof's edge (the side's height, 3.35 + 3 × 3 m), clear of it.
        let run = bounds_of(&side, |id| id.starts_with("run/"));
        assert!(
            run[1][2] < 12.35 - 0.05 && run[0][2] > 12.35 - 0.6,
            "{run:?}"
        );
        // The industrial ducts stand on brackets too, and give the wall their fixings, rust and
        // drips on its plane.
        assert!(!parts(&side, "duct-").is_empty());
        for name in ["fixture", "fixing", "rust", "drip"] {
            let found = features_of(&side, name, "duct-");
            assert!(!found.is_empty(), "{name}");
            assert!(
                found
                    .iter()
                    .all(|f| f.points.iter().all(|p| p[1].abs() < 1e-9))
            );
        }
        let duct = bounds_of(&side, |id| id.starts_with("duct-0/"));
        assert!(duct[0][1] > -(gap + 0.62 + 0.1), "{duct:?}");
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
            json!([
                "drip",
                "ledge",
                "runoff-coping",
                "runoff-sill",
                "runoff-top"
            ]),
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
    // four lettered blade signs, the corner drain, a tiled roof deck, and paving on all four sides;
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
    // Each sign face stays over its pier in X and extends from its stand-off out into
    // the street (-Y). A wall-parallel board or missing lettering fails these bounds.
    for (index, ([pier, reach, width, depth], slot)) in [
        ([0.18, 0.32, 0.76, 0.055], "board"),
        ([2.90, 0.32, 0.76, 0.036], "paint"),
        ([5.29, 0.30, 0.60, 0.105], "paint"),
        ([5.65, 0.36, 0.64, 0.055], "board"),
    ]
    .into_iter()
    .enumerate()
    {
        let id = format!("board-0-{index}");
        let board = extent(&id, slot);
        assert!(
            board[0][0] > pier - 0.08 && board[1][0] < pier + depth + 0.08,
            "{id}: {board:?}"
        );
        assert!(
            (board[0][1] + reach + width).abs() < 0.05,
            "{id}: {board:?}"
        );
        assert!((board[1][1] + reach).abs() < 0.05, "{id}: {board:?}");
        let letters = extent(&id, "lettering");
        assert!(letters[0][2] < letters[1][2], "{id} is missing lettering");
    }
    let shutter = extent("side-0/ground/shop-0/bay", "metal");
    assert!(
        (shutter[0][0] - 0.45).abs() < 1e-6
            && (shutter[1][0] - 2.75).abs() < 1e-6
            && (shutter[1][2] - 2.82).abs() < 1e-6,
        "{shutter:?}"
    );
    let wall = extent("side-0/ground/wall", "wall");
    let miter_tip = 6. - 0.025 * (1. - 0.5f64.sqrt());
    assert!((wall[1][0] - miter_tip).abs() < 1e-6, "{wall:?}");
    // Three storeys: 3.3 m and two of 3 m, a 0.45 m parapet and its 8 cm coping.
    assert!((top_of(&building, "side-0") - 9.83).abs() < 0.03);
    // The deck is tiled in its own slot, and the dado in one of its own (the spandrels' tiles are `tile`).
    let slots: BTreeSet<&str> = building
        .geometry
        .meshes
        .iter()
        .map(|m| m.material_slot.as_str())
        .collect();
    assert!(slots.contains("roof-tile") && slots.contains("dado"));
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

    let styled = |design: &str, subtitle: &str| {
        sign(&[
            ("design", json!(design)),
            ("scheme", json!("green")),
            ("text", json!("興隆五金")),
            ("subtitle", json!(subtitle)),
        ])
    };
    for design in ["hardware", "grocery", "electrical", "wholesale", "pharmacy"] {
        let with_caption = styled(design, "HARDWARE");
        let painted = |item: &GeneratedItem| {
            item.geometry
                .meshes
                .iter()
                .any(|mesh| mesh.material_slot == "paint")
        };
        assert!(painted(&with_caption), "{design} has artwork");
        let text_vertices = |item: &GeneratedItem| {
            item.geometry
                .meshes
                .iter()
                .find(|mesh| mesh.material_slot == "lettering")
                .unwrap()
                .uvs
                .len()
        };
        assert!(text_vertices(&with_caption) > text_vertices(&styled(design, "")));
    }
    let board_corners = |item: &GeneratedItem| {
        item.geometry
            .meshes
            .iter()
            .filter(|mesh| mesh.material_slot == "board")
            .any(|mesh| {
                mesh.positions
                    .iter()
                    .any(|p| p[0].abs() < 1e-9 && p[2].abs() < 1e-9)
            })
    };
    assert!(board_corners(&words));
    assert!(!board_corners(&styled("hardware", "")));
    assert!(!board_corners(&styled("wholesale", "")));
}

#[test]
fn a_steel_balcony_has_rails_where_a_parapet_is_solid() {
    let name = "kowloon-facade-side.item.json";
    for rail in ["steel", "parapet"] {
        let overrides = [("balcony", json!(true)), ("balconyRail", json!(rail))];
        let facade = generate(name, &overrides);
        let original = generate_with(name, &overrides, |definition| {
            definition
                .parts
                .iter_mut()
                .find(|part| part.id == "balcony")
                .unwrap()
                .slots
                .clear();
        });
        let balcony = |item: GeneratedItem| -> BTreeMap<_, _> {
            item.geometry
                .meshes
                .iter()
                .filter(|m| m.id.starts_with("balcony/"))
                .map(|m| (m.id.clone(), m.clone()))
                .collect()
        };
        let bound = balcony(facade);
        let unbound = balcony(original);
        assert_eq!(
            bound.keys().collect::<Vec<_>>(),
            unbound.keys().collect::<Vec<_>>()
        );
        assert_eq!(bound.contains_key("balcony/metal"), rail == "steel");
        if rail == "steel" {
            assert!(bound.contains_key("balcony/paint"));
        }
        for (id, before) in unbound {
            let after = &bound[&id];
            assert_eq!(after.positions, before.positions);
            assert_eq!(after.indices, before.indices);
            let slot = if before.material_slot == "concrete" {
                "suspended-concrete"
            } else {
                &before.material_slot
            };
            assert_eq!(after.material_slot, slot);
        }
    }
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

/// World positions and normals of the vertices of the meshes under parts named `part` (or
/// all parts for `""`) in one slot.
fn slot_points(item: &GeneratedItem, part: &str, slot: &str) -> Vec<([f64; 3], [f64; 3])> {
    let transforms = item.geometry.world_transforms().unwrap();
    let mut found = vec![];
    for (node, matrix) in item.geometry.nodes.iter().zip(&transforms) {
        for index in &node.meshes {
            let mesh = &item.geometry.meshes[*index];
            if mesh.material_slot != slot || !(part.is_empty() || mesh.id.starts_with(part)) {
                continue;
            }
            for (p, n) in mesh.positions.iter().zip(&mesh.normals) {
                found.push((matrix.point(*p), matrix.direction(*n)));
            }
        }
    }
    found
}

/// The zones of `names` over the features of the parts whose IDs start with `part`.
fn zone_sets(
    item: &GeneratedItem,
    part: &str,
) -> superspec_engine::materialspec::program::NamedFeatures {
    use superspec_engine::{authoring::item_features, materialspec::program::NamedFeatures};
    let features: Vec<_> = item
        .geometry
        .features
        .iter()
        .filter(|f| f.part.starts_with(part))
        .cloned()
        .collect();
    let identity = [
        1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.,
    ];
    NamedFeatures::new(&item_features(&features, &identity)).unwrap()
}

/// A window states where its frame's face and its glass stand: `head` and `jamb` on the plane of
/// the frame's face, `glass` round the glazed opening on the glass's plane, and zones hanging
/// from the head down the frame (`runoff-head`) and down the glass (`runoff-glass`), each on its
/// own plane, so the frame's face is in the one and the glass behind it only in the other.
#[test]
fn a_windows_head_hangs_runoff_down_its_frame_and_its_glass() {
    for kind in ["casement", "slider", "timber", "louver"] {
        let window = generate(
            "kowloon-window.item.json",
            &[("kind", json!(kind)), ("openChance", json!(0))],
        );
        let (width, height) = (1.2, 1.4);
        let feature = |name: &str| -> &item::Feature {
            window
                .geometry
                .features
                .iter()
                .find(|f| f.name == name)
                .unwrap_or_else(|| panic!("{kind}: no {name}"))
        };
        for name in ["head", "jamb", "glass", "runoff-head", "runoff-glass"] {
            feature(name);
        }
        assert!(feature("head").zone.is_none() && feature("runoff-head").zone.is_some());
        let head = feature("runoff-head");
        assert!(head.points.iter().all(|p| (p[2] - height).abs() < 1e-9));
        // The frame's face, measured from its meshes: the outer frame's front at its outer edges
        // (beads and hinges stand a little further out, sashes and mullions a little back; what
        // is painted in the room behind is far behind).
        // A timber frame is drawn in the timber slots, its painted faces in `frame`.
        let frame: Vec<_> = ["frame", "timber", "shop-timber"]
            .into_iter()
            .flat_map(|slot| slot_points(&window, "window/", slot))
            .collect();
        let plane = head.points[0][1];
        let edges: Vec<_> = frame
            .iter()
            // Its arrises are chamfered by about a millimetre.
            .filter(|(p, n)| {
                n[1] < -0.9 && (p[0].abs() - width / 2.).abs() < 0.002 && p[2] >= -1e-6
            })
            .collect();
        assert!(
            edges.len() >= 4,
            "{kind}: {} outer front corners {:?}",
            edges.len(),
            frame
                .iter()
                .filter(|(p, n)| n[1] < -0.3 && p[0].abs() > width / 2. - 0.01)
                .collect::<Vec<_>>()
        );
        assert!(
            edges.iter().all(|(p, _)| (p[1] - plane).abs() < 1e-6),
            "{kind}: {edges:?} off y = {plane}"
        );
        let faces: Vec<_> = frame
            .iter()
            .filter(|(p, n)| {
                n[1] < -0.9
                    && p[1] < 0.
                    && p[0].abs() <= width / 2. + 1e-6
                    && p[2] >= -1e-6
                    && p[2] <= height + 1e-6
            })
            .collect();
        assert!(!faces.is_empty(), "{kind}: no frame faces");
        let zones = zone_sets(&window, "window");
        let (on_head, on_glass) = (
            zones.zones("runoff-head").unwrap(),
            zones.zones("runoff-glass").unwrap(),
        );
        // Every face of the frame's members below the head is in the head's zone.
        let held = faces
            .iter()
            .filter(|(p, n)| on_head.at(*p, *n).0.is_some())
            .count();
        let missed: BTreeSet<i64> = faces
            .iter()
            .filter(|(p, n)| on_head.at(*p, *n).0.is_none())
            .map(|(p, _)| (p[1] * 1000.).round() as i64)
            .collect();
        assert!(
            held * 10 >= faces.len() * 9,
            "{kind}: {held} of {}, missed at {missed:?}",
            faces.len()
        );
        // Nothing above the head is.
        assert!(
            on_head
                .at([0., plane, height + 0.05], [0., -1., 0.])
                .0
                .is_none()
        );
        // The glass is in the glass's zone and not in the frame's, which stands in front of it
        // (a louvre's slats tilt from the frame's face back to behind it).
        let glass = slot_points(&window, "window/", "glass");
        assert!(!glass.is_empty(), "{kind}: no glass");
        let upright: Vec<_> = glass.iter().filter(|(_, n)| n[1].abs() > 0.5).collect();
        assert!(!upright.is_empty());
        for (p, n) in &upright {
            let hit = on_glass.at(*p, *n).0;
            assert!(
                hit.is_some_and(|hit| hit.v >= 0.),
                "{kind}: glass at {p:?} is in no zone"
            );
            assert!(
                kind == "louver" || on_head.at(*p, *n).0.is_none(),
                "{kind}: glass at {p:?} is in the frame's"
            );
        }
        // The glass's outline lies on the glass's plane, behind the frame's face.
        let outline = feature("glass");
        assert!(outline.closed && outline.points.len() == 4);
        assert!(outline.points.iter().all(|p| p[1] > plane));
    }
    // A window without glass states no glass zone.
    let bare = generate("kowloon-window.item.json", &[("glazing", json!(false))]);
    assert!(
        !bare
            .geometry
            .features
            .iter()
            .any(|f| f.name == "runoff-glass")
    );
    // The window's slot map lists every slot its native draws, so none is dropped (a timber
    // window's frame is in `timber`).
    let definition: serde_json::Value =
        serde_json::from_str(&library()["kowloon-window.item.json"]).unwrap();
    let mapped = definition["parts"][0]["slots"].as_object().unwrap();
    let natives = items::natives();
    for kind in ["casement", "slider", "timber", "louver"] {
        for (lit, storage, textured) in [(false, 0., true), (true, 0., false), (false, 0.5, true)] {
            let values: Values = [
                ("kind", json!(kind)),
                ("lit", json!(lit)),
                ("room", json!(if lit { 2.6 } else { 0. })),
                ("storage", json!(storage)),
                ("textured", json!(textured)),
                ("handles", json!(true)),
            ]
            .into_iter()
            .map(|(k, v)| (k.to_owned(), v))
            .collect();
            for mesh in natives
                .generate("kowloon/window", &values, &|| false)
                .unwrap()
                .meshes
            {
                assert!(
                    mapped.contains_key(&mesh.material_slot),
                    "{kind}: the window's slots drop {}",
                    mesh.material_slot
                );
            }
        }
    }
}

/// A neighbour's stub reports its coping, string courses, sills, units' drips and foot, and the
/// building's party part hangs the main walls' zones below them: under each sill its wall is in
/// `runoff-sill`, under the coping in `runoff-coping`, and nothing above an edge is in its zone.
#[test]
fn party_walls_and_stubs_weather_from_their_coping_courses_and_sills() {
    let building = |plans: serde_json::Value, storeys: serde_json::Value| {
        generate(
            "kowloon-building.item.json",
            &[
                ("signs", json!(false)),
                ("partyPlans", plans),
                ("partyStoreyPlans", storeys),
            ],
        )
    };
    let stub = building(json!(["none", "none", "stub", "wall"]), json!([0, 0, 3, 0]));
    let zoned = |name: &str, part: &str| -> Vec<item::Feature> {
        stub.geometry
            .features
            .iter()
            .filter(|f| f.name == name && f.part == part && f.zone.is_some())
            .cloned()
            .collect()
    };
    for name in ["runoff-coping", "runoff-top", "runoff-sill"] {
        assert!(!zoned(name, "party-2").is_empty(), "stub: no {name}");
    }
    // Two string courses (three storeys) on each of the three faces.
    assert_eq!(zoned("runoff-top", "party-2").len(), 6);
    // The coping zone hangs from the top of the stub's wall (meshes keep positions to 0.1 mm).
    let near = 1e-4;
    let wall = slot_points(&stub, "party-2/", "wall");
    let high = wall.iter().map(|(p, _)| p[2]).fold(f64::MIN, f64::max);
    for coping in zoned("runoff-coping", "party-2") {
        assert!(
            coping.points.iter().all(|p| (p[2] - high).abs() < near),
            "{:?} under {high}",
            coping.points
        );
    }
    let zones = zone_sets(&stub, "party-2");
    let sills = zones.zones("runoff-sill").unwrap();
    let trim = slot_points(&stub, "party-2/", "trim");
    for sill in zoned("runoff-sill", "party-2") {
        let [a, b] = [sill.points[0], sill.points[1]];
        let middle: [f64; 3] = std::array::from_fn(|i| (a[i] + b[i]) / 2.);
        // A sill's underside stands there: trim faces down at the edge's height over its middle.
        assert!(
            trim.iter().any(|(p, n)| n[2] < -0.9
                && (p[2] - middle[2]).abs() < near
                && (0..2).all(|i| (p[i] - middle[i]).abs() < 0.2 + (b[i] - a[i]).abs() / 2.)),
            "no sill at {middle:?}"
        );
        let below = [middle[0], middle[1], middle[2] - 0.1];
        let hit = sills.at(below, sill.normal).0;
        assert!(hit.is_some_and(|hit| (hit.v - 0.1).abs() < 1e-6), "{hit:?}");
        let above = [middle[0], middle[1], middle[2] + 0.02];
        assert!(sills.at(above, sill.normal).0.is_none());
        // Its wall faces out along the feature's normal where the zone is.
        assert!(wall.iter().any(|(p, n)| {
            (0..3).map(|i| n[i] * sill.normal[i]).sum::<f64>() > 0.9
                && (0..3)
                    .map(|i| (p[i] - middle[i]) * sill.normal[i])
                    .sum::<f64>()
                    .abs()
                    < near
        }));
    }
    // A plain wall reports its coping and foot, with the coping's zone down its face.
    let plain = zoned("runoff-coping", "party-3");
    assert!(!plain.is_empty());
    let plain_wall = slot_points(&stub, "party-3/", "wall");
    let plain_high = plain_wall
        .iter()
        .map(|(p, _)| p[2])
        .fold(f64::MIN, f64::max);
    assert!(
        plain
            .iter()
            .all(|f| (f.points[0][2] - plain_high).abs() < near)
    );
    assert!(
        stub.geometry
            .features
            .iter()
            .any(|f| f.part == "party-3" && f.name == "base")
    );
    assert!(zoned("runoff-sill", "party-3").is_empty());
    // A stub's front is a back lane's: by default stacks rise past its coping and a back door
    // stands under a hood (whose top is 2.25 m up); a lane plan leaves them out.
    let hood = |item: &GeneratedItem| {
        slot_points(item, "party-2/", "trim")
            .iter()
            .any(|(p, n)| n[2] > 0.9 && (p[2] - 2.25).abs() < 1e-3)
    };
    let stacks = |item: &GeneratedItem| {
        let metal = slot_points(item, "party-2/", "metal");
        let wall = slot_points(item, "party-2/", "wall");
        let high = wall.iter().map(|(p, _)| p[2]).fold(f64::MIN, f64::max);
        metal.iter().any(|(p, _)| p[2] > high + 0.5)
    };
    assert!(hood(&stub) && stacks(&stub));
    let quiet = generate(
        "kowloon-building.item.json",
        &[
            ("signs", json!(false)),
            ("partyPlans", json!(["none", "none", "stub", "wall"])),
            ("partyStoreyPlans", json!([0, 0, 3, 0])),
            ("partyLanePlans", json!([[], [], [0, 0, 0, 0], []])),
        ],
    );
    assert!(!hood(&quiet) && !stacks(&quiet));
    // Its features follow its windows: with the door gone, its bay may have a ground window
    // and so a sill more (unless the seed leaves that bay blank).
    assert!(
        zoned("runoff-sill", "party-2").len()
            <= quiet
                .geometry
                .features
                .iter()
                .filter(|f| f.name == "runoff-sill" && f.part == "party-2")
                .count()
    );
}

/// The highest point of the plinth meshes whose ID starts with `prefix`.
fn plinth_top(item: &GeneratedItem, prefix: &str) -> f64 {
    let transforms = item.geometry.world_transforms().unwrap();
    let mut top = f64::NEG_INFINITY;
    for (node, matrix) in item.geometry.nodes.iter().zip(transforms) {
        for index in &node.meshes {
            let mesh = &item.geometry.meshes[*index];
            if mesh.id.starts_with(prefix) && mesh.material_slot == "plinth" {
                for p in &mesh.positions {
                    top = top.max(matrix.point(*p)[2]);
                }
            }
        }
    }
    top
}

#[test]
fn a_plinth_is_planned_per_edge_and_on_the_corner() {
    // By default every street edge and the corner share `plinth`; a plan sets each edge, and
    // `cornerPlinth` the corner, as the reference shop house does (1 m fronts, a 1.5 m drum).
    let planned = generate(
        "kowloon-building.item.json",
        &[
            ("cornerStyle", json!("round")),
            ("cut", json!(0.55)),
            ("plinthPlans", json!([1.0, 0.6, 0, 0])),
            ("cornerPlinth", json!(1.5)),
        ],
    );
    assert!((plinth_top(&planned, "side-0/") - 1.0).abs() < 0.02);
    assert!((plinth_top(&planned, "side-1/") - 0.6).abs() < 0.02);
    assert!((plinth_top(&planned, "cornerRound-0/") - 1.5).abs() < 0.02);
    assert_eq!(plinth_top(&planned, "side-2/"), f64::NEG_INFINITY);
    let seeded = generate(
        "kowloon-building.item.json",
        &[("cornerStyle", json!("round")), ("cut", json!(0.55))],
    );
    assert!((plinth_top(&seeded, "side-1/") - 1.02).abs() < 0.02);
    assert!((plinth_top(&seeded, "cornerRound-0/") - 1.02).abs() < 0.02);
}

#[test]
fn the_plinth_runs_on_round_a_neighbour_beside_a_street() {
    // The reference shop house's side street (edge 1, along x = 9.8) is mostly shop openings:
    // its plinth stands on three piers of 0.4 to 0.55 m between and beside them. The back
    // neighbour's stub ends on that street, so its foot is painted to the same height and the
    // plinth runs on past the building's back.
    let plan = plan("reference-shop-house.json");
    let overrides: Vec<(&str, serde_json::Value)> = plan
        .iter()
        .map(|(key, value)| (key.as_str(), value.clone()))
        .collect();
    let item = generate("kowloon-building.item.json", &overrides);
    // The world positions of the meshes `chosen` keeps (by ID and slot).
    let points = |item: &GeneratedItem, chosen: &dyn Fn(&str, &str) -> bool| {
        let transforms = item.geometry.world_transforms().unwrap();
        let mut points = vec![];
        for (node, matrix) in item.geometry.nodes.iter().zip(transforms) {
            for index in &node.meshes {
                let mesh = &item.geometry.meshes[*index];
                if chosen(&mesh.id, &mesh.material_slot) {
                    points.extend(mesh.positions.iter().map(|p| matrix.point(*p)));
                }
            }
        }
        points
    };
    let piers = points(&item, &|id, slot| {
        id.starts_with("side-1/") && slot == "plinth"
    });
    for (from, to) in [(0.55, 0.95), (3.1, 3.6), (6.45, 7.0)] {
        let on = |y: f64| piers.iter().any(|p| (p[1] - y).abs() < 1e-6 && p[2] > 0.99);
        assert!(on(from) && on(to), "a pier's plinth from {from} to {to}");
    }
    assert!(piers.iter().all(|p| {
        [(0.55, 0.95), (3.1, 3.6), (6.45, 7.0)]
            .iter()
            .any(|(from, to)| p[1] > from - 1e-6 && p[1] < to + 1e-6)
    }));
    // The stub's end stands out of the side's wall plane as the plinth does, up to 1 m.
    let band = |item: &GeneratedItem| -> Vec<[f64; 3]> {
        points(item, &|id, slot| {
            id.starts_with("party-2/") && slot == "wall"
        })
        .into_iter()
        .filter(|p| p[0] > 9.8 + 0.017)
        .collect()
    };
    let foot = band(&item);
    assert!(!foot.is_empty(), "the back neighbour's end has a plinth");
    let high = foot.iter().map(|p| p[2]).fold(f64::MIN, f64::max);
    assert!((high - 1.).abs() < 1e-9, "its plinth is {high} m high");
    assert!(foot.iter().all(|p| p[1] > 7. - 1e-6 && p[1] < 9.));
    // Without a plinth on the street beside it, the neighbour has none.
    let mut bare = overrides.clone();
    bare.retain(|(key, _)| *key != "plinthPlans");
    bare.push(("plinthPlans", json!([1.0, 0, 0, 0])));
    assert!(band(&generate("kowloon-building.item.json", &bare)).is_empty());
}

/// The area two convex polygons share, in one plane's (u, v), and its perimeter.
fn shared_area(a: &[[f64; 2]], b: &[[f64; 2]]) -> (f64, f64) {
    let cross = |o: [f64; 2], p: [f64; 2], q: [f64; 2]| {
        (p[0] - o[0]) * (q[1] - o[1]) - (p[1] - o[1]) * (q[0] - o[0])
    };
    let area = |p: &[[f64; 2]]| {
        (0..p.len())
            .map(|i| cross([0.; 2], p[i], p[(i + 1) % p.len()]))
            .sum::<f64>()
            / 2.
    };
    // Both counterclockwise; clip `a` by every edge of `b` (Sutherland-Hodgman).
    let ccw = |p: &[[f64; 2]]| {
        let mut p = p.to_vec();
        if area(&p) < 0. {
            p.reverse();
        }
        p
    };
    let (mut clipped, b) = (ccw(a), ccw(b));
    for i in 0..b.len() {
        let (e0, e1) = (b[i], b[(i + 1) % b.len()]);
        let input = std::mem::take(&mut clipped);
        for j in 0..input.len() {
            let (p, q) = (input[j], input[(j + 1) % input.len()]);
            let (sp, sq) = (cross(e0, e1, p), cross(e0, e1, q));
            if sp >= 0. {
                clipped.push(p);
            }
            if (sp >= 0.) != (sq >= 0.) {
                let t = sp / (sp - sq);
                clipped.push([p[0] + (q[0] - p[0]) * t, p[1] + (q[1] - p[1]) * t]);
            }
        }
        if clipped.len() < 3 {
            return (0., 0.);
        }
    }
    let perimeter = (0..clipped.len())
        .map(|i| {
            let (p, q) = (clipped[i], clipped[(i + 1) % clipped.len()]);
            (q[0] - p[0]).hypot(q[1] - p[1])
        })
        .sum();
    (area(&clipped).abs(), perimeter)
}

/// One receiver's face laid over another's: the front one's mesh id and slot, the one behind
/// it, the area they share in square meters, and how wide the widest piece of it is (twice
/// its area over its perimeter: a strip's width).
struct Stacked {
    front: (String, String),
    back: (String, String),
    area: f64,
    width: f64,
}

/// Every pair of receivers (meshes as placed) with faces facing the same way, the second's
/// within `behind` meters behind the first's plane (or `ahead` in front of it), that share more
/// than `least` square meters: a surface laid on another closer than a depth buffer separates
/// them from afar. Largest share first.
fn stacked(item: &GeneratedItem, behind: f64, ahead: f64, least: f64) -> Vec<Stacked> {
    struct Face {
        receiver: usize,
        normal: [f64; 3],
        offset: f64,
        points: [[f64; 3]; 3],
        low: [f64; 3],
        high: [f64; 3],
    }
    let cell = behind.max(ahead).max(1e-4);
    let dot = |a: [f64; 3], b: [f64; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let transforms = item.geometry.world_transforms().unwrap();
    let (mut names, mut buckets) = (Vec::new(), BTreeMap::<[i64; 4], Vec<Face>>::new());
    for (node, matrix) in item.geometry.nodes.iter().zip(&transforms) {
        for index in &node.meshes {
            let mesh = &item.geometry.meshes[*index];
            let receiver = names.len();
            names.push((mesh.id.clone(), mesh.material_slot.clone()));
            for triangle in mesh.indices.chunks(3) {
                let points = [0, 1, 2].map(|k| matrix.point(mesh.positions[triangle[k] as usize]));
                let (u, v) = (
                    [0, 1, 2].map(|k| points[1][k] - points[0][k]),
                    [0, 1, 2].map(|k| points[2][k] - points[0][k]),
                );
                let n = [
                    u[1] * v[2] - u[2] * v[1],
                    u[2] * v[0] - u[0] * v[2],
                    u[0] * v[1] - u[1] * v[0],
                ];
                let length = dot(n, n).sqrt();
                if length < 2e-8 {
                    continue;
                }
                let normal = n.map(|c| c / length);
                let offset = dot(normal, points[0]);
                let key = [
                    (normal[0] * 200.).round() as i64,
                    (normal[1] * 200.).round() as i64,
                    (normal[2] * 200.).round() as i64,
                    (offset / cell).floor() as i64,
                ];
                let low = [0, 1, 2].map(|k| points.iter().map(|p| p[k]).fold(f64::MAX, f64::min));
                let high = [0, 1, 2].map(|k| points.iter().map(|p| p[k]).fold(f64::MIN, f64::max));
                buckets.entry(key).or_default().push(Face {
                    receiver,
                    normal,
                    offset,
                    points,
                    low,
                    high,
                });
            }
        }
    }
    let mut shared = BTreeMap::<(usize, usize), [f64; 2]>::new();
    for (key, faces) in &buckets {
        let near: Vec<&Face> = (-1..=1)
            .filter_map(|step| buckets.get(&[key[0], key[1], key[2], key[3] + step]))
            .flatten()
            .collect();
        for a in faces {
            for b in near.iter().copied() {
                // How far `b` lies behind `a`'s plane, along `a`'s outward normal.
                let gap = a.offset - b.offset;
                if a.receiver == b.receiver
                    || gap > behind
                    || -gap > ahead
                    || dot(a.normal, b.normal) < 0.99999
                    || (0..3).any(|k| a.low[k] > b.high[k] + cell || b.low[k] > a.high[k] + cell)
                {
                    continue;
                }
                // The plane's own axes, from the first face's normal.
                let n = a.normal;
                let helper = if n[0].abs() < 0.9 {
                    [1., 0., 0.]
                } else {
                    [0., 1., 0.]
                };
                let mut e1 = [
                    helper[1] * n[2] - helper[2] * n[1],
                    helper[2] * n[0] - helper[0] * n[2],
                    helper[0] * n[1] - helper[1] * n[0],
                ];
                let l = dot(e1, e1).sqrt();
                e1 = e1.map(|c| c / l);
                let e2 = [
                    n[1] * e1[2] - n[2] * e1[1],
                    n[2] * e1[0] - n[0] * e1[2],
                    n[0] * e1[1] - n[1] * e1[0],
                ];
                let flat = |f: &Face| f.points.map(|p| [dot(p, e1), dot(p, e2)]);
                let (area, perimeter) = shared_area(&flat(a), &flat(b));
                if area > 0. {
                    let entry: &mut [f64; 2] = shared.entry((a.receiver, b.receiver)).or_default();
                    entry[0] += area;
                    entry[1] = entry[1].max(2. * area / perimeter);
                }
            }
        }
    }
    let mut found: Vec<Stacked> = shared
        .into_iter()
        .filter(|(_, [area, _])| *area > least)
        .map(|((a, b), [area, width])| Stacked {
            front: names[a].clone(),
            back: names[b].clone(),
            area,
            width,
        })
        .collect();
    found.sort_by(|a, b| b.area.total_cmp(&a.area));
    found
}

fn listed(found: &[&Stacked]) -> String {
    found
        .iter()
        .map(|s| {
            format!(
                "{:.4} m2 ({:.4} m wide)  {}  |  {}",
                s.area, s.width, s.front.0, s.back.0
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn no_two_receivers_of_the_planned_buildings_lie_on_one_another() {
    // A band laid on a wall (tiles, a dado, a plinth) stands only millimeters proud of it, and
    // a depth buffer seen from tens of meters away does not tell such faces apart: the wall
    // shows through as the camera moves. Such bands sit in a recess cut for them instead.
    //
    // Pairs drawn by fixtures, frames and signs whose own makers have yet to part them, by the
    // names in their two mesh ids: they are listed rather than hidden, so none is added.
    const PENDING: &[(&str, &str)] = &[
        ("/wall/wall/edge", "roof/top"),
        ("/wall/wall/reveal", "/room/room/"),
        ("/fascia/fascia/", "/balcony/balcony/"),
        // A balcony's slab and the doors' steps end in the planes of the wall and its bands.
        ("/door/door/concrete", "/wall/wall/edge"),
        ("/backDoor/backDoor/concrete", "/wall/wall/edge"),
        ("/backDoor/backDoor/concrete", "/dado"),
        ("/balcony/balcony/", "/tileEnd/tileEnd/edge"),
        // A shopfront's frame stands in its jambs' plane, where the plinth ends.
        ("/room/room/metal", "/plinth-"),
        // The next side's wall end under a plinth or a dado at a square corner.
        ("/plinth-", "/wall/wall/edge"),
        ("/dado", "/wall/wall/edge"),
        // Pipes, conduits and neighbours' slices, drawn by their own generators.
        ("/conduit-", "/plinth-"),
        ("/downpipe/", "/plinth-"),
        ("/drain-", "/plinth-"),
        ("/drain-", "/dado"),
        ("party-", "party-"),
        ("party-", "/band/band/concrete"),
    ];
    let pending = |s: &Stacked| {
        PENDING.iter().any(|(a, b)| {
            (s.front.0.contains(a) && s.back.0.contains(b))
                || (s.front.0.contains(b) && s.back.0.contains(a))
        })
    };
    // Only the building's own fabric: the receivers its walls, parapets and bands are made of.
    let fabric = ["wall", "wall-round", "tile", "dado", "plinth", "trim"];
    for name in ["reference-shop-house.json", "small-warehouse.json"] {
        let plan = plan(name);
        let overrides: Vec<(&str, serde_json::Value)> = plan
            .iter()
            .map(|(key, value)| (key.as_str(), value.clone()))
            .collect();
        let building = generate("kowloon-building.item.json", &overrides);
        // Two faces in one plane, within 2 mm of it.
        let all = stacked(&building, 0.002, 0.002, 1e-4);
        let coplanar: Vec<&Stacked> = all
            .iter()
            .filter(|s| s.front.0 < s.back.0)
            .filter(|s| fabric.contains(&s.front.1.as_str()) || fabric.contains(&s.back.1.as_str()))
            .filter(|s| !pending(s))
            .collect();
        assert!(
            coplanar.is_empty(),
            "{name}: {} coplanar pairs:\n{}",
            coplanar.len(),
            listed(&coplanar)
        );
        // Nothing within 3 cm behind the front of a tiled, dado or plinth band, but the
        // hairlines of wall a seat's margin leaves at the panel's edge and beside its openings
        // (and the centimeter of wall under a shop opening's foot where a plinth meets its jamb).
        let near = stacked(&building, 0.03, 0.002, 1e-4);
        let bands: Vec<&Stacked> = near
            .iter()
            .filter(|s| ["tile", "dado", "plinth"].contains(&s.front.1.as_str()))
            .filter(|s| s.width > 0.012 && !pending(s))
            .collect();
        assert!(
            bands.is_empty(),
            "{name}: {} faces lie just behind a band:\n{}",
            bands.len(),
            listed(&bands)
        );
    }
}

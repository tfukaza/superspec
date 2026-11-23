//! Shops and hung cloth at the three graphics detail levels: level 0 is the authored
//! fixture, levels 1 and 2 keep its shell, lights, outline and colours with less geometry.
use serde_json::{Value, json};
use std::collections::BTreeMap;
use superspec_engine::{
    itemspec::definition::{NativeOutput, Values},
    worldspec::themes::kowloon::items,
};

fn generate(name: &str, parameters: &Value, level: Option<u8>) -> NativeOutput {
    let mut values: Values = serde_json::from_value(parameters.clone()).unwrap();
    if let Some(level) = level {
        values.insert("graphicsDetail".into(), json!(level));
    }
    items::natives()
        .generate(name, &values, &|| false)
        .unwrap_or_else(|error| panic!("{name} {parameters} at {level:?}: {error}"))
}

fn levels(name: &str, parameters: &Value) -> [NativeOutput; 3] {
    [None, Some(1), Some(2)].map(|level| generate(name, parameters, level))
}

fn triangles(output: &NativeOutput) -> usize {
    output.meshes.iter().map(|m| m.indices.len() / 3).sum()
}

fn by_slot(output: &NativeOutput) -> BTreeMap<String, usize> {
    let mut slots = BTreeMap::new();
    for mesh in &output.meshes {
        *slots.entry(mesh.material_slot.clone()).or_default() += mesh.indices.len() / 3;
    }
    slots
}

fn bounds(output: &NativeOutput) -> [[f64; 3]; 2] {
    let mut low = [f64::INFINITY; 3];
    let mut high = [f64::NEG_INFINITY; 3];
    for p in output.meshes.iter().flat_map(|m| &m.positions) {
        for axis in 0..3 {
            low[axis] = low[axis].min(p[axis]);
            high[axis] = high[axis].max(p[axis]);
        }
    }
    [low, high]
}

fn room(trade: &str) -> Value {
    json!({"catalogStock": true, "trade": trade, "width": 3.0, "height": 2.85, "depth": 3.1,
           "storey": 3.4, "seed": 4017})
}

const TRADES: [&str; 6] = [
    "provisions",
    "grocer",
    "greengrocer",
    "hardware",
    "herbalist",
    "roast",
];

fn cloth() -> Vec<(&'static str, Value)> {
    vec![
        (
            "kowloon/sagging-tarp",
            json!({"width": 2.9, "depth": 1.35, "drop": 0.46, "sag": 0.28, "tilt": 0.12,
                   "valance": 0.16, "poles": 2, "stand": 2.7, "seed": 7}),
        ),
        (
            "kowloon/sagging-tarp",
            json!({"width": 2.7, "depth": 1.5, "drop": 0.42, "sag": 0.07, "tilt": 0,
                   "valance": 0.24, "scallops": 8, "stripes": 16, "frame": true, "poles": 0,
                   "seed": 7}),
        ),
        (
            "kowloon/awning",
            json!({"width": 3.2, "depth": 0.85, "drop": 0.3, "seed": 7}),
        ),
        (
            "kowloon/tarp",
            json!({"width": 3.2, "stripes": true, "seed": 7}),
        ),
        ("kowloon/sheet-canopy", json!({"width": 1.6, "seed": 7})),
    ]
}

fn slot(output: &NativeOutput, name: &str) -> usize {
    by_slot(output).get(name).copied().unwrap_or(0)
}

/// Every stocked room is the authored one at level 0 and within its budget below it: a
/// room of goods coloured per vertex at level 1, its massing at level 2.
#[test]
fn rooms_keep_their_shell_and_lights_and_shed_their_stock() {
    for trade in TRADES {
        let parameters = room(trade);
        let built = levels("kowloon/shop-room", &parameters);
        let counts = built.each_ref().map(triangles);
        println!("shop-room {trade}: {counts:?}");
        assert_eq!(
            built[0].meshes,
            generate("kowloon/shop-room", &parameters, Some(0)).meshes,
            "{trade}"
        );
        assert!(
            counts[0] > 14_000 && counts[1] <= 12_000 && counts[2] <= 2_200,
            "{trade}: {counts:?}"
        );
        // Hardware is turned wares at every level, so its level 1 keeps the most.
        assert!(
            counts[1] * 10 < counts[0] * 6 && counts[2] < counts[1] / 2,
            "{trade}: {counts:?}"
        );
        for (level, output) in built.iter().enumerate() {
            // Walls, ceiling and floor are the same faces at every level.
            for shell in ["interior", "concrete", "shop-floor"] {
                assert_eq!(
                    slot(output, shell),
                    slot(&built[0], shell),
                    "{trade} level {level}: {shell}"
                );
                assert!(slot(output, shell) > 0, "{trade} level {level}: {shell}");
            }
            // A reduced room delivers none of the baked stock atlases.
            if level > 0 {
                for baked in [
                    "snack-packet",
                    "market-hero-props",
                    "market-trays",
                    "market-apple",
                    "shop-register",
                    "restaurant-food",
                ] {
                    assert_eq!(slot(output, baked), 0, "{trade} level {level}: {baked}");
                }
            }
            if level == 2 {
                assert_eq!(slot(output, "grocer-props"), 0, "{trade}");
            }
            // The goods keep their colours: a massed room is not one grey block.
            let colours: std::collections::BTreeSet<String> = output
                .meshes
                .iter()
                .filter(|mesh| mesh.material_slot == "stock")
                .flat_map(|mesh| mesh.colors.iter().map(|color| format!("{color:?}")))
                .collect();
            assert!(
                colours.len() >= 40,
                "{trade} level {level}: {} colours",
                colours.len()
            );
        }
        // The room stays inside its walls at every level.
        let authored = bounds(&built[0]);
        for (level, output) in built.iter().enumerate().skip(1) {
            let reduced = bounds(output);
            for corner in 0..2 {
                for axis in 0..3 {
                    assert!(
                        (authored[corner][axis] - reduced[corner][axis]).abs() < 0.08,
                        "{trade} level {level}: {authored:?} became {reduced:?}"
                    );
                }
            }
        }
    }
}

/// Shop light positions are read from the fitted shop's `tube-light` mesh, so it is drawn,
/// under the same name and in the same place, at every level.
#[test]
fn the_fitted_shop_keeps_its_tube_lights() {
    let built = levels("kowloon/shop-room", &room("provisions"));
    let tubes = |output: &NativeOutput| -> Vec<(String, [[f64; 3]; 2])> {
        output
            .meshes
            .iter()
            .filter(|mesh| mesh.material_slot == "tube-light")
            .map(|mesh| {
                let mut low = [f64::INFINITY; 3];
                let mut high = [f64::NEG_INFINITY; 3];
                for p in &mesh.positions {
                    for axis in 0..3 {
                        low[axis] = low[axis].min(p[axis]);
                        high[axis] = high[axis].max(p[axis]);
                    }
                }
                (mesh.id.clone(), [low, high])
            })
            .collect()
    };
    let authored = tubes(&built[0]);
    assert!(!authored.is_empty());
    for (level, output) in built.iter().enumerate().skip(1) {
        let reduced = tubes(output);
        assert_eq!(reduced.len(), authored.len(), "level {level}");
        for ((id, span), (authored_id, authored_span)) in reduced.iter().zip(&authored) {
            assert_eq!(id, authored_id, "level {level}");
            for corner in 0..2 {
                // Fewer sides round the tube move its extremes by less than its radius.
                assert!(
                    (0..3).all(
                        |axis| (span[corner][axis] - authored_span[corner][axis]).abs() < 0.02
                    ),
                    "level {level}: {span:?} for {authored_span:?}"
                );
            }
        }
    }
}

/// Street stock stands where it stood, in the low-detail goods a reduced level stocks.
#[test]
fn street_stock_keeps_its_place_and_sheds_its_catalog_goods() {
    for (kind, width, low, very_low) in [
        ("shop", 3.0, 1_800, 600),
        ("crates", 1.8, 400, 400),
        ("cartons", 1.8, 200, 200),
        ("mixed", 1.8, 700, 600),
    ] {
        let parameters = json!({"catalogStock": true, "width": width, "kind": kind, "seed": 4017});
        let built = levels("kowloon/street-stock", &parameters);
        let counts = built.each_ref().map(triangles);
        println!("street-stock {kind}: {counts:?}");
        assert_eq!(
            built[0].meshes,
            generate("kowloon/street-stock", &parameters, Some(0)).meshes,
            "{kind}"
        );
        assert!(
            counts[0] > counts[1] && counts[1] >= counts[2] && counts[2] > 0,
            "{kind}: {counts:?}"
        );
        assert!(
            counts[1] <= low && counts[2] <= very_low,
            "{kind}: {counts:?}"
        );
        let authored = bounds(&built[0]);
        for (level, output) in built.iter().enumerate().skip(1) {
            assert_eq!(
                by_slot(output).keys().collect::<Vec<_>>(),
                ["stock"],
                "{kind}"
            );
            let reduced = bounds(output);
            for corner in 0..2 {
                for axis in 0..3 {
                    assert!(
                        (authored[corner][axis] - reduced[corner][axis]).abs() < 0.12,
                        "{kind} level {level}: {authored:?} became {reduced:?}"
                    );
                }
            }
        }
    }
}

/// Awnings and tarpaulins shade the street: a reduced level sweeps the same cloth, with
/// the same edges, fall and posts, over a coarser grid, without its hems and lashings.
#[test]
fn hung_cloth_keeps_its_outline_at_a_fraction_of_its_triangles() {
    for (index, (name, parameters)) in cloth().into_iter().enumerate() {
        let built = levels(name, &parameters);
        let counts = built.each_ref().map(triangles);
        println!("{name} {index}: {counts:?}");
        assert_eq!(
            built[0].meshes,
            generate(name, &parameters, Some(0)).meshes,
            "{name}"
        );
        // A striped canvas is still cut along its stripes, so its level 2 keeps more.
        assert!(
            counts[1] * 5 <= counts[0] && counts[2] * 3 <= counts[1] * 2 && counts[2] > 0,
            "{name} {index}: {counts:?}"
        );
        let cloth_bounds = |output: &NativeOutput| {
            let mut low = [f64::INFINITY; 3];
            let mut high = [f64::NEG_INFINITY; 3];
            for mesh in &output.meshes {
                if !matches!(mesh.material_slot.as_str(), "tarp" | "sheet") {
                    continue;
                }
                for p in &mesh.positions {
                    for axis in 0..3 {
                        low[axis] = low[axis].min(p[axis]);
                        high[axis] = high[axis].max(p[axis]);
                    }
                }
            }
            [low, high]
        };
        let authored = cloth_bounds(&built[0]);
        for (level, output) in built.iter().enumerate().skip(1) {
            let reduced = cloth_bounds(output);
            for corner in 0..2 {
                for axis in 0..3 {
                    // Rolled hems, ropes and the folds between coarser rows: centimetres.
                    assert!(
                        (authored[corner][axis] - reduced[corner][axis]).abs() < 0.12,
                        "{name} {index} level {level} axis {axis}: {authored:?} became {reduced:?}"
                    );
                }
            }
            // Posts, frames and brackets stand where they stood.
            let (a, b) = (bounds(&built[0]), bounds(output));
            for corner in 0..2 {
                for axis in 0..3 {
                    assert!(
                        (a[corner][axis] - b[corner][axis]).abs() < 0.12,
                        "{name} {index} level {level}: {a:?} became {b:?}"
                    );
                }
            }
        }
    }
}

/// Washing is cloth in its dye at every level: cut garments on coarser grids and meshed
/// ones as their outlines, without pegs and hangers.
#[test]
fn washing_keeps_its_garments_as_outlines() {
    for (name, low, very_low) in [
        ("kowloon/washing", 120, 90),
        ("kowloon/hangers", 420, 320),
        ("kowloon/drying-rack", 200, 160),
        ("kowloon/drying-pole", 160, 120),
        ("kowloon/drying-lines", 600, 320),
    ] {
        let built = levels(name, &json!({}));
        let counts = built.each_ref().map(triangles);
        println!("{name}: {counts:?}");
        assert_eq!(
            built[0].meshes,
            generate(name, &json!({}), Some(0)).meshes,
            "{name}"
        );
        assert!(
            counts[1] <= low && counts[2] <= very_low && counts[2] < counts[1],
            "{name}: {counts:?}"
        );
        for (level, output) in built.iter().enumerate() {
            assert!(slot(output, "balcony-cloth") > 0, "{name} level {level}");
        }
    }
}

/// A closed shutter bay keeps its curtain, guides and head box; the lowest level hangs the
/// curtain as one sheet.
#[test]
fn shutters_keep_their_curtain() {
    let parameters = json!({"kind": "shutter", "width": 3.0, "height": 3.35, "seed": 11});
    let built = levels("kowloon/ground-bay", &parameters);
    let counts = built.each_ref().map(triangles);
    println!("ground-bay shutter: {counts:?}");
    assert!(
        counts[0] > counts[1] && counts[1] > counts[2] && counts[2] >= 40,
        "{counts:?}"
    );
    let (a, b) = (bounds(&built[0]), bounds(&built[2]));
    for corner in 0..2 {
        for axis in 0..3 {
            assert!(
                (a[corner][axis] - b[corner][axis]).abs() < 0.08,
                "{a:?} became {b:?}"
            );
        }
    }
}

/// Where each level's triangles are, per shop native and slot: run with
/// `--ignored --nocapture`.
#[test]
#[ignore = "a report, not a check"]
fn report_shop_and_cloth_triangles() {
    let mut cases: Vec<(&str, Value)> = TRADES
        .iter()
        .map(|trade| ("kowloon/shop-room", room(trade)))
        .collect();
    for kind in ["shop", "crates", "cartons", "mixed"] {
        cases.push((
            "kowloon/street-stock",
            json!({"catalogStock": true, "width": if kind == "shop" { 3.0 } else { 1.8 },
                   "kind": kind, "seed": 4017}),
        ));
    }
    for kind in ["shutter", "gate", "door", "shop", "service"] {
        cases.push((
            "kowloon/ground-bay",
            json!({"kind": kind, "width": 3.0, "height": 3.35, "seed": 11}),
        ));
    }
    cases.extend(cloth());
    for name in [
        "kowloon/roller-shutter",
        "kowloon/entrance",
        "kowloon/washing",
        "kowloon/hangers",
        "kowloon/drying-rack",
        "kowloon/drying-pole",
        "kowloon/drying-lines",
    ] {
        cases.push((name, json!({})));
    }
    for (name, parameters) in cases {
        let built = levels(name, &parameters);
        println!(
            "{name} {}: {:?}",
            parameters
                .get("trade")
                .or(parameters.get("kind"))
                .map_or(String::new(), |v| v.to_string()),
            built.each_ref().map(triangles)
        );
        let slots = built.each_ref().map(by_slot);
        let mut names: Vec<_> = slots.iter().flat_map(|s| s.keys().cloned()).collect();
        names.sort();
        names.dedup();
        for slot in names {
            let counts = slots.each_ref().map(|s| s.get(&slot).copied().unwrap_or(0));
            println!(
                "  {:>7} {:>7} {:>7}  {slot}",
                counts[0], counts[1], counts[2]
            );
        }
    }
}

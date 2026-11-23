//! The Kowloon fixtures that describe themselves as shared prototypes (the condenser and the
//! conduit), and a survey of what there is to share in the Stoa city's buildings.
//!
//! `cargo test --release -p superspec-engine --test kowloon_prototypes -- --ignored --nocapture survey`
//! prints, over the seven Stoa prototypes at each graphics detail: per part kind, the
//! instances, distinct shapes and triangles; per prototype generator, the placements and
//! distinct prototypes; and per wall panel and material, the distinct content keys by size,
//! with openings and with features. It writes the same to
//! `generated/graphics-detail/placements/survey.txt` (or `PLACEMENT_SURVEY_OUTPUT`).
#[path = "support/placements.rs"]
mod placements;

use placements::{Tolerance, rebuilds_the_expanded_meshes};
use serde_json::json;
use std::{
    collections::{BTreeMap, BTreeSet},
    hash::{Hash, Hasher},
    sync::Arc,
};
use superspec_engine::{
    Session,
    authoring::{Mapping, MaterialAssignment, ModelSource, Project},
    itemspec::definition::{
        self as item, Environment, GeneratedItem, ItemGeometry, NativeGenerators, NativeOutput,
        PreparedItem, Values,
    },
    worldspec::themes::kowloon::items,
};

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

fn prepare_with(name: &str, values: &Values, natives: Arc<dyn NativeGenerators>) -> PreparedItem {
    let files = library();
    let reader = |name: &str| -> Result<Arc<[u8]>, String> {
        files
            .get(name)
            .map(|text| Arc::from(text.as_bytes()))
            .ok_or(format!("Missing {name}"))
    };
    let definition = item::parse(name, &files[name]).unwrap();
    let mut environment = Environment::new(&reader).with_natives(natives);
    item::prepare_in(&definition, values, &mut environment, &|| false)
        .unwrap_or_else(|error| panic!("{name}: {error}"))
}

fn generate(name: &str, values: &Values) -> GeneratedItem {
    prepare_with(name, values, items::natives())
        .build(&|| false)
        .unwrap_or_else(|error| panic!("{name}: {error}"))
}

/// The Kowloon generators with no prototype described: what every item was before.
struct Unshared(Arc<dyn NativeGenerators>);
impl NativeGenerators for Unshared {
    fn preserve_signed_zero(&self, material_slot: &str) -> bool {
        self.0.preserve_signed_zero(material_slot)
    }
    fn identity(&self, name: &str) -> Option<String> {
        self.0.identity(name)
    }
    fn names(&self) -> Vec<String> {
        self.0.names()
    }
    fn generate(
        &self,
        name: &str,
        parameters: &Values,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<NativeOutput, String> {
        self.0.generate(name, parameters, cancelled)
    }
    fn features(&self, name: &str, parameters: &Values) -> Vec<item::Feature> {
        self.0.features(name, parameters)
    }
}

fn values(value: serde_json::Value) -> Values {
    serde_json::from_value(value).unwrap()
}

/// A split air conditioner as a building gives it: a condenser, its line set and its hose.
fn split(seed: u32, level: u8, budget: i64, size: [f64; 2]) -> GeneratedItem {
    generate(
        "kowloon-aircon.item.json",
        &values(json!({"split": true, "seed": seed, "graphicsDetail": level,
            "variantBudget": budget, "width": size[0], "height": size[1]})),
    )
}

const SEEDS: u32 = 40;
/// The room a building gives a condenser, and a room for the wider cases and a second fan.
const ROOMS: [[f64; 2]; 2] = [[0.95, 0.6], [1.6, 1.2]];
/// A render mesh keeps vertex colours to a thousandth, both the drawn colour and the shade a
/// prototype reads back, so a shade times a colour is the drawn colour to half an 8-bit step.
const CLOSE: Tolerance = Tolerance {
    position: 1e-9,
    colour: 0.5 / 255.,
};
/// For a fixture that takes no colour from its instance: the prototype is the fixture.
const EXACT: Tolerance = Tolerance {
    position: 0.,
    colour: 0.,
};

fn placement<'a>(built: &'a GeneratedItem, path: &str) -> Option<&'a item::PartPlacement> {
    built
        .geometry
        .placements
        .iter()
        .find(|placement| placement.path == path)
}

#[test]
fn condensers_of_one_entry_in_one_room_are_one_prototype() {
    for room in ROOMS {
        for (level, budget) in [(0u8, 4i64), (0, 8), (1, 2), (2, 1)] {
            // The prototype of each catalogue entry, and how many seeds drew it.
            let mut entries: BTreeMap<u64, (String, usize)> = BTreeMap::new();
            let mut colourings = BTreeSet::new();
            for seed in 0..SEEDS {
                let built = split(seed, level, budget, room);
                let condenser = placement(&built, "condenser")
                    .unwrap_or_else(|| panic!("seed {seed}: the condenser is not a placement"));
                let prototype = &built.geometry.prototypes[&condenser.prototype];
                assert_eq!(prototype.name, "kowloon/condenser");
                assert_eq!(prototype.content["graphicsDetail"], json!(level));
                // A prototype names no seed and no budget.
                assert!(!prototype.content.contains_key("seed"));
                assert!(!prototype.content.contains_key("variantBudget"));
                let entry = prototype.content["entry"].as_u64().unwrap();
                assert!(entry < budget as u64, "entry {entry} of budget {budget}");
                let known = entries
                    .entry(entry)
                    .or_insert_with(|| (condenser.prototype.clone(), 0));
                assert_eq!(known.0, condenser.prototype, "entry {entry} seed {seed}");
                known.1 += 1;
                colourings.insert(format!("{:?}", condenser.inputs));
            }
            let keys: BTreeSet<_> = entries.values().map(|(key, _)| key).collect();
            assert_eq!(keys.len(), entries.len(), "entries are distinct prototypes");
            assert!(entries.len() <= budget as usize);
            println!(
                "room {room:?} level {level} budget {budget}: {} prototypes, {} colourings",
                entries.len(),
                colourings.len()
            );
            // Colour is the instance's: far more colourings than shapes.
            assert!(colourings.len() > 3 * entries.len());
        }
    }
    // Another room, another detail or another weight of drawing is another prototype.
    let key = |level: u8, room: [f64; 2]| {
        let built = split(3, level, 1, room);
        placement(&built, "condenser").unwrap().prototype.clone()
    };
    let first = key(0, ROOMS[0]);
    assert_eq!(first, key(0, ROOMS[0]));
    assert_ne!(first, key(1, ROOMS[0]));
    assert_ne!(first, key(2, ROOMS[0]));
    assert_ne!(first, key(0, ROOMS[1]));
    assert_ne!(first, key(0, [0.96, 0.6]));
}

#[test]
fn a_budget_keeps_the_entry_a_larger_budget_folds_onto() {
    for seed in 0..SEEDS {
        let entry = |budget: i64| {
            let built = split(seed, 0, budget, ROOMS[0]);
            let condenser = placement(&built, "condenser").unwrap();
            built.geometry.prototypes[&condenser.prototype].content["entry"]
                .as_u64()
                .unwrap()
        };
        let largest = entry(64);
        for budget in [1, 2, 4, 8, 16, 32] {
            assert_eq!(entry(budget), largest % budget as u64, "seed {seed}");
        }
    }
}

#[test]
fn a_condenser_without_a_catalogue_is_not_shared() {
    let built = split(5, 0, 0, ROOMS[0]);
    assert!(placement(&built, "condenser").is_none());
    // Its conduits state every value that shapes them, so they still are.
    assert!(placement(&built, "lines").is_some());
}

#[test]
fn placing_the_prototypes_rebuilds_every_split_unit() {
    for room in ROOMS {
        for (level, budget) in [(0u8, -1i64), (0, 64), (1, -1), (1, 16), (2, -1), (2, 8)] {
            for seed in 0..SEEDS {
                let built = split(seed, level, budget, room);
                let covered = rebuilds_the_expanded_meshes(&built.geometry, &CLOSE);
                let drawn: usize = built
                    .geometry
                    .meshes
                    .iter()
                    .map(|mesh| mesh.indices.len() / 3)
                    .sum();
                // Every part of a split unit is a placement.
                assert_eq!(covered, drawn, "room {room:?} level {level} seed {seed}");
            }
        }
    }
}

#[test]
fn a_prototype_carries_shades_and_a_placement_the_colours() {
    let built = split(11, 0, -1, ROOMS[0]);
    let condenser = placement(&built, "condenser").unwrap();
    let prototype = &built.geometry.prototypes[&condenser.prototype];
    // The roles are the instance's inputs, in name order.
    assert!(prototype.roles.windows(2).all(|pair| pair[0] < pair[1]));
    assert!(prototype.roles.iter().any(|role| role == "body"));
    let body = prototype
        .roles
        .iter()
        .position(|role| role == "body")
        .unwrap() as u8
        + 1;
    let (mut tinted, mut own) = (0, 0);
    for mesh in &prototype.meshes {
        assert_eq!(mesh.roles.len(), mesh.positions.len());
        for (role, colour) in mesh.roles.iter().zip(&mesh.colors) {
            assert!(usize::from(*role) <= prototype.roles.len());
            if *role == body {
                tinted += 1;
                // A shade of the case paint is a grey: the paint's hue is the placement's.
                assert!(
                    (colour[0] - colour[1]).abs() < 1e-9 && (colour[1] - colour[2]).abs() < 1e-9
                );
            } else if *role == 0 {
                own += 1;
            }
        }
    }
    assert!(tinted > 0 && own > 0, "{tinted} tinted, {own} own");
    // The conduits take no colour from their instance.
    let lines = placement(&built, "lines").unwrap();
    assert!(lines.inputs.is_empty());
    assert!(built.geometry.prototypes[&lines.prototype].roles.is_empty());
    // Two units of one entry share the prototype allocation and differ by their inputs.
    let (mut first, mut other) = (None, None);
    for seed in 0..SEEDS {
        let built = split(seed, 0, 1, ROOMS[0]);
        let condenser = placement(&built, "condenser").unwrap().clone();
        match &first {
            None => first = Some(condenser),
            Some(known) if known.inputs != condenser.inputs => {
                other = Some(condenser);
                break;
            }
            _ => {}
        }
    }
    let (first, other) = (first.unwrap(), other.expect("two colourings in 40 seeds"));
    assert_eq!(first.prototype, other.prototype);
    assert_eq!(first.transform, other.transform);
}

/// Decision 2 of the placements plan: a condenser takes one colour from its instance, its
/// case paint. Its bracket steel, its line set's duct and its cover are its entry's, so
/// instances of one entry are one shape in one set of secondary colours.
#[test]
fn a_condenser_has_one_colour_of_its_own_and_its_entry_has_the_rest() {
    for room in ROOMS {
        for (level, budget) in [(0u8, 8i64), (1, 2), (2, 1)] {
            // Per entry: the vertices the first instance painted in the entry's own colours.
            let mut entries: BTreeMap<u64, (Vec<Vec<[f64; 4]>>, BTreeSet<String>)> =
                BTreeMap::new();
            for seed in 0..SEEDS {
                let built = split(seed, level, budget, room);
                let condenser = placement(&built, "condenser").unwrap();
                let prototype = &built.geometry.prototypes[&condenser.prototype];
                assert_eq!(prototype.roles, ["body"], "seed {seed}");
                assert_eq!(condenser.inputs.keys().collect::<Vec<_>>(), ["body"]);
                let body = condenser.inputs["body"];
                let mut own = Vec::new();
                for (shared, &index) in prototype.meshes.iter().zip(&condenser.meshes) {
                    let drawn = &built.geometry.meshes[index];
                    // The instance's shape is its entry's, bit for bit; only colours differ.
                    assert_eq!(shared.positions, drawn.positions, "seed {seed}");
                    assert_eq!(shared.normals, drawn.normals, "seed {seed}");
                    assert_eq!(shared.uvs, drawn.uvs, "seed {seed}");
                    assert_eq!(shared.indices, drawn.indices, "seed {seed}");
                    own.push(
                        drawn
                            .colors
                            .iter()
                            .zip(&shared.roles)
                            .map(|(colour, role)| if *role == 0 { *colour } else { [0.; 4] })
                            .collect::<Vec<_>>(),
                    );
                    // And a vertex in the case paint is a shade of this instance's.
                    for ((colour, shade), role) in
                        drawn.colors.iter().zip(&shared.colors).zip(&shared.roles)
                    {
                        if *role == 1 {
                            for k in 0..3 {
                                assert!((shade[k] * body[k] - colour[k]).abs() <= CLOSE.colour);
                            }
                        }
                    }
                }
                let entry = prototype.content["entry"].as_u64().unwrap();
                let known = entries
                    .entry(entry)
                    .or_insert_with(|| (own.clone(), BTreeSet::new()));
                assert!(
                    known.0 == own,
                    "entry {entry}, seed {seed}: its own colours differ"
                );
                known.1.insert(format!("{body:?}"));
            }
            // Variety stays in the case paint: several paints to an entry.
            let paints: usize = entries.values().map(|(_, paints)| paints.len()).sum();
            assert!(
                paints > 3 * entries.len(),
                "{paints} paints of {} entries",
                entries.len()
            );
        }
    }
}

/// A native fixture alone in an item, as a building's items hand it its parameters.
fn alone(
    name: &str,
    parameters: &serde_json::Value,
    seed: Option<u32>,
    level: u8,
    budget: i64,
) -> GeneratedItem {
    let mut parameters = parameters.clone();
    if let Some(seed) = seed {
        parameters["seed"] = json!(seed);
    }
    parameters["graphicsDetail"] = json!(level);
    parameters["variantBudget"] = json!(budget);
    // A peg beside it, so the item has geometry at a level where the fixture draws nothing.
    generate_source(
        &json!({"format":"itemspec","version":2,"id":"alone",
            "parts":[{"id":"p","generator":{"kind":"native","name":name,"parameters":parameters}},
                     {"id":"peg","generator":{"kind":"box","size":[0.1,0.1,0.1]},
                      "placement":{"position":[0,5,0]}}]})
        .to_string(),
    )
}

/// The triangles the fixture of [`alone`] drew.
fn drawn_alone(built: &GeneratedItem) -> usize {
    let part = built
        .geometry
        .parts
        .iter()
        .find(|part| part.id == "p")
        .unwrap();
    part.meshes
        .iter()
        .map(|&mesh| built.geometry.meshes[mesh].indices.len() / 3)
        .sum()
}

/// The fixtures whose seed picks a catalogue entry and whose instances share its shape, with
/// parameters a building gives them, and the colour each takes from its instance.
fn catalogued() -> Vec<(&'static str, serde_json::Value, Option<&'static str>)> {
    vec![
        ("kowloon/aircon", json!({"plates": true}), Some("body")),
        (
            "kowloon/aircon",
            json!({"drop": 0.4, "louvred": true}),
            Some("body"),
        ),
        ("kowloon/window", json!({}), None),
        (
            "kowloon/window",
            json!({"kind": "slider", "openChance": 1.3, "frame": "#6a3b2a"}),
            None,
        ),
        ("kowloon/window", json!({"lit": true, "room": 2.6}), None),
        ("kowloon/window", json!({"storage": 0.7}), None),
        (
            "kowloon/plants",
            json!({"width": 1.1, "offset": 0.14}),
            None,
        ),
        (
            "kowloon/sheet-canopy",
            json!({"width": 1.3, "depth": 0.5, "drop": 0.12, "color": "#7f7b6e"}),
            None,
        ),
        (
            "kowloon/sagging-tarp",
            json!({"stripes": 6, "stand": 2.4}),
            None,
        ),
    ]
}

#[test]
fn fixtures_of_one_entry_and_the_same_parameters_are_one_prototype() {
    for (name, parameters, role) in catalogued() {
        for (level, budget) in [(0u8, -1i64), (0, 8), (1, -1), (2, -1)] {
            let most = if budget > 0 {
                budget as usize
            } else {
                [4, 2, 1][usize::from(level)]
            };
            let mut entries: BTreeMap<u64, String> = BTreeMap::new();
            let mut colourings = BTreeSet::new();
            for seed in 0..SEEDS {
                let built = alone(name, &parameters, Some(seed), level, budget);
                let drawn = drawn_alone(&built);
                let Some(placed) = placement(&built, "p") else {
                    panic!("{name} {parameters} seed {seed} level {level}: not a placement");
                };
                let prototype = &built.geometry.prototypes[&placed.prototype];
                // A prototype names its entry, and no seed and no budget.
                assert!(!prototype.content.contains_key("seed"));
                assert!(!prototype.content.contains_key("variantBudget"));
                assert_eq!(prototype.content["graphicsDetail"], json!(level));
                let entry = prototype.content["entry"].as_u64().unwrap();
                assert!((entry as usize) < most, "{name}: entry {entry} of {most}");
                assert_eq!(
                    entries
                        .entry(entry)
                        .or_insert_with(|| placed.prototype.clone()),
                    &placed.prototype,
                    "{name} {parameters}: entry {entry}, seed {seed}"
                );
                // One colour of its own, or none.
                assert_eq!(
                    prototype.roles,
                    role.into_iter().collect::<Vec<_>>(),
                    "{name}"
                );
                colourings.insert(format!("{:?}", placed.inputs));
                // Placing the prototype is the fixture, whole.
                let covered = rebuilds_the_expanded_meshes(&built.geometry, &CLOSE);
                assert_eq!(
                    covered, drawn,
                    "{name} {parameters} seed {seed} level {level}"
                );
            }
            let keys: BTreeSet<_> = entries.values().collect();
            assert_eq!(
                keys.len(),
                entries.len(),
                "{name}: entries are distinct prototypes"
            );
            println!(
                "{name} {parameters} level {level} budget {budget}: {} prototypes, {} colourings",
                entries.len(),
                colourings.len()
            );
            match role {
                // Its colour is the instance's: far more colourings than shapes.
                Some(_) => assert!(colourings.len() > 3 * entries.len(), "{name}"),
                None => assert_eq!(colourings.len(), 1, "{name}"),
            }
        }
        // Other parameters are another prototype, and without a catalogue nothing is shared.
        let key = |parameters: &serde_json::Value, budget: i64| {
            let built = alone(name, parameters, Some(3), 0, budget);
            placement(&built, "p").map(|placed| placed.prototype.clone())
        };
        let mut wider = parameters.clone();
        wider["width"] = json!(1.45);
        if name != "kowloon/aircon" {
            assert_ne!(key(&parameters, 1), key(&wider, 1), "{name}");
        }
        assert_eq!(key(&parameters, 1), key(&parameters, 1), "{name}");
        assert!(
            key(&parameters, 1).is_some() && key(&parameters, 0).is_none(),
            "{name}"
        );
    }
}

/// The fixtures that read no seed: equal parameters are one prototype, drawn as the fixture.
#[test]
fn a_fixture_without_a_seed_is_a_prototype_of_its_parameters() {
    for (name, parameters) in [
        (
            "kowloon/security-bars",
            json!({"pattern": "grid", "width": 1.32, "height": 1.5}),
        ),
        ("kowloon/wall-vent", json!({})),
        ("kowloon/downpipe", json!({})),
        ("kowloon/cornice", json!({})),
        ("kowloon/roof-outlet", json!({})),
        ("kowloon/wall-lamp", json!({})),
        ("kowloon/balcony", json!({})),
        ("kowloon/rail", json!({})),
        ("kowloon/ladder", json!({})),
    ] {
        for level in 0..=2u8 {
            let built = alone(name, &parameters, None, level, -1);
            let drawn = drawn_alone(&built);
            if drawn == 0 {
                // A part that draws nothing at this level places nothing.
                assert!(built.geometry.placements.is_empty(), "{name} level {level}");
                continue;
            }
            let placed = placement(&built, "p")
                .unwrap_or_else(|| panic!("{name} level {level}: not a placement"));
            let prototype = &built.geometry.prototypes[&placed.prototype];
            assert!(
                prototype.roles.is_empty() && placed.inputs.is_empty(),
                "{name}"
            );
            assert_eq!(
                rebuilds_the_expanded_meshes(&built.geometry, &EXACT),
                drawn,
                "{name}"
            );
            // The budget shapes nothing in it.
            let other = alone(name, &parameters, None, level, 16);
            assert_eq!(
                placement(&other, "p").unwrap().prototype,
                placed.prototype,
                "{name}"
            );
        }
    }
}

/// A cage and a drying rack take several colours from their own streams (rust, a roof sheet,
/// a dye for each garment, a caged unit's paint), so they are not one shape and one tint.
#[test]
fn fixtures_whose_colours_do_not_factor_are_not_shared() {
    for (name, parameters) in [
        ("kowloon/cage", json!({})),
        ("kowloon/cage", json!({"contents": "washing"})),
        (
            "kowloon/cage",
            json!({"contents": "aircon", "width": 0.98, "height": 0.68, "depth": 0.52}),
        ),
        ("kowloon/drying-rack", json!({})),
    ] {
        for seed in 0..8 {
            let built = alone(name, &parameters, Some(seed), 0, -1);
            assert!(built.geometry.placements.is_empty(), "{name} {parameters}");
            assert!(built.geometry.prototypes.is_empty(), "{name} {parameters}");
        }
    }
}

/// One prototype of the Stoa city (`levelspec/projects/klali-stoa/scene.aml`), as `city.rs`
/// hands it to the building item, and how many buildings stand from it. The same seven as
/// `kowloon_variants.rs` surveys.
struct Stoa {
    #[allow(dead_code)]
    name: &'static str,
    floors: usize,
    seed: usize,
    trade: Option<&'static str>,
    courtyard: bool,
    placed: usize,
}

const STOA: [Stoa; 7] = [
    Stoa {
        name: "5 floors, seed 8",
        floors: 5,
        seed: 8,
        trade: None,
        courtyard: false,
        placed: 6,
    },
    Stoa {
        name: "4 floors, seed 4",
        floors: 4,
        seed: 4,
        trade: None,
        courtyard: false,
        placed: 8,
    },
    Stoa {
        name: "4 floors, seed 4, roast shop",
        floors: 4,
        seed: 4,
        trade: Some("roast"),
        courtyard: false,
        placed: 1,
    },
    Stoa {
        name: "6 floors, seed 6",
        floors: 6,
        seed: 6,
        trade: None,
        courtyard: false,
        placed: 5,
    },
    Stoa {
        name: "3 floors, seed 9",
        floors: 3,
        seed: 9,
        trade: None,
        courtyard: false,
        placed: 4,
    },
    Stoa {
        name: "7 floors, seed 7",
        floors: 7,
        seed: 7,
        trade: None,
        courtyard: false,
        placed: 4,
    },
    Stoa {
        name: "courtyard, 2 floors, seed 6",
        floors: 2,
        seed: 6,
        trade: None,
        courtyard: true,
        placed: 1,
    },
];

impl Stoa {
    fn values(&self, level: u8) -> Values {
        let families: Vec<Values> = serde_json::from_str(include_str!(
            "../src/worldspec/themes/kowloon/materials/building-families.json"
        ))
        .unwrap();
        let mut values = families[self.seed % families.len()].clone();
        if let Some(trade) = self.trade {
            for (plan, value) in [("shopPlans", "open"), ("tradePlans", trade)] {
                let rows = values.entry(plan.into()).or_insert_with(|| json!([[]]));
                let row = rows[0].as_array_mut().unwrap();
                if row.is_empty() {
                    row.push(value.into());
                } else {
                    row[0] = value.into();
                }
            }
        }
        let footprint = if self.courtyard {
            values.insert("groundShops".into(), false.into());
            values.insert("pavement".into(), false.into());
            json!([[-6.9, -8.9], [6.9, -8.9], [6.9, 8.9], [-6.9, 8.9]])
        } else {
            json!([[0, 0], [9.8, 0], [9.8, 7.], [0, 7.]])
        };
        values.insert("footprint".into(), footprint);
        values.insert("seed".into(), self.seed.into());
        values.insert("floors".into(), self.floors.into());
        values.insert("graphicsDetail".into(), level.into());
        values
    }
}

const BUILDING: &str = "kowloon-building.item.json";

/// Placements are an annotation: the building is what it was without them, and placing its
/// prototypes rebuilds the meshes they cover.
#[test]
fn a_building_is_the_same_with_and_without_its_placements() {
    let stoa = &STOA[1];
    for level in [0u8, 1] {
        let values = stoa.values(level);
        let with = generate(BUILDING, &values);
        let without = prepare_with(BUILDING, &values, Arc::new(Unshared(items::natives())))
            .build(&|| false)
            .unwrap();
        assert!(without.geometry.placements.is_empty() && without.geometry.prototypes.is_empty());
        assert!(!with.geometry.placements.is_empty());
        assert_eq!(with.geometry_key, without.geometry_key, "level {level}");
        assert_eq!(with.geometry.parts, without.geometry.parts, "level {level}");
        assert_eq!(with.geometry.nodes, without.geometry.nodes, "level {level}");
        assert_eq!(with.geometry.features, without.geometry.features);
        assert_eq!(with.geometry.anchors, without.geometry.anchors);
        assert_eq!(with.geometry.bounds, without.geometry.bounds);
        assert!(
            with.geometry.meshes == without.geometry.meshes,
            "level {level}: meshes"
        );
        assert_eq!(
            serde_json::to_string(&with.collision).unwrap(),
            serde_json::to_string(&without.collision).unwrap()
        );
        assert_eq!(
            serde_json::to_string(&with.attachments).unwrap(),
            serde_json::to_string(&without.attachments).unwrap()
        );
        assert_eq!(
            serde_json::to_string(&with.materials).unwrap(),
            serde_json::to_string(&without.materials).unwrap()
        );
        let covered = rebuilds_the_expanded_meshes(&with.geometry, &CLOSE);
        assert!(covered > 0);
        // Every placement is a turn about Z of an upright prototype, and none is mirrored.
        for placed in with.geometry.placed().unwrap() {
            assert_eq!(placed.down().unwrap(), [0., 0., -1.], "{}", placed.path);
            assert!(!with.geometry.prototypes[&placed.prototype].mirrored);
        }
    }
}

/// The graphics detail is render-only: every level places the same condensers with the same
/// colours, and draws them from fewer shapes.
#[test]
fn every_graphics_detail_places_the_same_condensers_in_the_same_colours() {
    let stoa = &STOA[1];
    // Each condenser's case paint (its steel, duct and cover are its entry's, and the entry
    // is the level's budget's), and its prototype.
    let condensers = |level: u8| -> BTreeMap<String, (String, String)> {
        let built = generate(BUILDING, &stoa.values(level));
        built
            .geometry
            .placements
            .iter()
            .filter(|placed| {
                built.geometry.prototypes[&placed.prototype].name == "kowloon/condenser"
            })
            .map(|placed| {
                (
                    placed.path.clone(),
                    (format!("{:?}", placed.inputs), placed.prototype.clone()),
                )
            })
            .collect()
    };
    let full = condensers(0);
    assert!(!full.is_empty(), "the building has split units");
    let shapes = |placed: &BTreeMap<String, (String, String)>| {
        placed
            .values()
            .map(|(_, key)| key.clone())
            .collect::<BTreeSet<_>>()
    };
    let mut fewest = shapes(&full).len();
    for level in [1u8, 2] {
        let reduced = condensers(level);
        assert_eq!(
            full.keys().collect::<Vec<_>>(),
            reduced.keys().collect::<Vec<_>>(),
            "level {level}"
        );
        for (path, (inputs, _)) in &reduced {
            assert_eq!(inputs, &full[path].0, "{path} at level {level}");
        }
        assert!(shapes(&reduced).len() <= fewest, "level {level}");
        assert!(shapes(&reduced).is_disjoint(&shapes(&full)));
        fewest = shapes(&reduced).len();
    }
}

/// An item of one part that draws a prototype from what a placement record states of it.
fn library_item(stated: &serde_json::Value) -> String {
    json!({"format":"itemspec","version":2,"id":"library",
        "parts":[{"id":"p","generator":{"kind":"prototype","name":stated["name"],
                  "content":stated["content"],"mirrored":stated["mirrored"]}}]})
    .to_string()
}

fn generate_source(source: &str) -> GeneratedItem {
    let reader = |name: &str| -> Result<Arc<[u8]>, String> { Err(format!("Missing {name}")) };
    let definition = item::parse("library.item.json", source).unwrap();
    let mut environment = Environment::new(&reader).with_natives(items::natives());
    item::prepare_in(&definition, &Values::new(), &mut environment, &|| false)
        .unwrap_or_else(|error| panic!("{source}: {error}"))
        .build(&|| false)
        .unwrap_or_else(|error| panic!("{source}: {error}"))
}

/// A bake tool draws each prototype through an item from its name, content and reflection
/// alone, and gets the engine's prototype, value for value.
#[test]
fn a_prototype_part_draws_every_prototype_of_a_building_as_the_building_holds_it() {
    for level in 0..=2u8 {
        let building = generate(BUILDING, &STOA[1].values(level));
        assert!(!building.geometry.prototypes.is_empty());
        for (key, prototype) in &building.geometry.prototypes {
            let stated = json!({"name": prototype.name, "content": prototype.content,
                "mirrored": prototype.mirrored});
            let drawn = generate_source(&library_item(&stated));
            assert_eq!(drawn.geometry.meshes.len(), prototype.meshes.len());
            for (mesh, shared) in drawn.geometry.meshes.iter().zip(&prototype.meshes) {
                let mut renamed = shared.clone();
                renamed.id = format!("p/{}", shared.id);
                assert!(
                    mesh == &renamed,
                    "{} {key} at level {level}: {} differs",
                    prototype.name,
                    shared.id
                );
            }
            assert!(drawn.geometry.placements.is_empty());
        }
    }
}

#[test]
fn an_unknown_prototype_fails_where_the_definition_names_it() {
    let reader = |name: &str| -> Result<Arc<[u8]>, String> { Err(format!("Missing {name}")) };
    let prepare = |source: &str| {
        let definition = item::parse("library.item.json", source).unwrap();
        let mut environment = Environment::new(&reader).with_natives(items::natives());
        item::prepare_in(&definition, &Values::new(), &mut environment, &|| false)
    };
    let source =
        library_item(&json!({"name": "kowloon/condensor", "content": {}, "mirrored": false}));
    let mut error = prepare(&source).err().unwrap();
    assert_eq!(error.code, "ITEM_NATIVE");
    assert_eq!(error.pointer, "/parts/0/generator/name");
    item::locate_diagnostic(&source, &mut error);
    assert!(source[error.offset.unwrap()..].starts_with("\"kowloon/condensor\""));
    // Content that is no prototype of the generator: an entry beyond the catalogue, and a
    // value it does not read.
    for content in [
        json!({"entry": 4096}),
        json!({"entry": 1, "colour": "#ffffff"}),
    ] {
        let source = library_item(
            &json!({"name": "kowloon/condenser", "content": content, "mirrored": false}),
        );
        let error = prepare(&source).unwrap().build(&|| false).err().unwrap();
        assert_eq!(error.pointer, "/parts/0/generator", "{content}");
        assert!(
            error.message.contains("kowloon/condenser"),
            "{}",
            error.message
        );
    }
}

fn stock_asset(id: &str) -> Result<Arc<[u8]>, String> {
    let name = superspec_catalog::image_name(id).ok_or(format!("Unknown stock texture {id}"))?;
    std::fs::read(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../assets/materials")
            .join(name),
    )
    .map(Arc::from)
    .map_err(|e| e.to_string())
}

/// The project the publication shades one building in (`editor/city-project.ts`): the building
/// as model `p0` with the street catalogue's slot bindings, densities and mappings.
fn building_project(stoa: &Stoa, level: u8, transform: [[f64; 3]; 4]) -> Project {
    let catalogue: serde_json::Value = serde_json::from_str(include_str!(
        "../src/worldspec/themes/kowloon/materials/street-materials.json"
    ))
    .unwrap();
    let role = &catalogue["roles"]["building"];
    let slots: BTreeMap<String, String> = serde_json::from_value(role["slots"].clone()).unwrap();
    let used: BTreeSet<&String> = slots.values().collect();
    let materials: Vec<MaterialAssignment> =
        serde_json::from_value(catalogue["materials"].clone()).unwrap();
    Project {
        schema_version: 2,
        kind: "material".into(),
        modules: library(),
        models: vec![ModelSource {
            id: "p0".into(),
            kind: "item".into(),
            entry: Some(BUILDING.into()),
            parameters: stoa.values(level),
            transform: Some(transform),
            slot_densities: serde_json::from_value(role["slotDensities"].clone()).unwrap(),
            slots: slots.clone(),
            ..ModelSource::default()
        }],
        materials: materials
            .into_iter()
            .filter(|material| used.contains(&material.id))
            .collect(),
        resolution: 2048,
        texel_density: Some(160.),
        mappings: serde_json::from_value(catalogue["mappings"].clone()).unwrap(),
        mapping: Mapping::World,
        ..Project::default()
    }
}

/// One mesh of a scene: its positions and colours per corner, in triangle order.
fn corners(scene: &superspec_engine::Scene, id: &str) -> Vec<([f64; 3], [f64; 3])> {
    let mesh = scene
        .manifest
        .meshes
        .iter()
        .find(|mesh| mesh.id == id)
        .unwrap_or_else(|| panic!("the scene has no mesh {id}"));
    assert!(mesh.visible, "{id}");
    scene.indices[mesh.index_start..mesh.index_start + mesh.index_count]
        .iter()
        .map(|&index| {
            let at = (mesh.vertex_start + index as usize) * 3;
            (
                [0, 1, 2].map(|k| f64::from(scene.positions[at + k])),
                [0, 1, 2].map(|k| f64::from(scene.colors[at + k])),
            )
        })
        .collect()
}

/// What the part bake reads: a building's item metadata, as the studio binary emits it in a
/// shading report's `metadata.items`, names every placement's meshes as the bake's scene does
/// and, with the prototypes drawn from that record alone, rebuilds the meshes it covers.
#[test]
fn a_buildings_metadata_places_its_prototypes_onto_the_scenes_meshes() {
    let stoa = &STOA[1];
    // The building stands turned and away from the origin: its record must not show it.
    let (sin, cos) = 0.6f64.sin_cos();
    let stand = [
        [cos, sin, 0.],
        [-sin, cos, 0.],
        [0., 0., 1.],
        [31., -17., 2.5],
    ];
    let home = [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.], [0.; 3]];
    let mut session = Session::default();
    let mut record = |transform| {
        let project = building_project(stoa, 0, transform);
        let prepared = session
            .prepare_project(&project, 1, &stock_asset, &|| false)
            .unwrap_or_else(|errors| panic!("{}", errors[0].message));
        // Through text, as a host receives it.
        let text = serde_json::to_string(&prepared.items().unwrap()["p0"]).unwrap();
        let item: serde_json::Value = serde_json::from_str(&text).unwrap();
        (item, prepared.scene)
    };
    let (item, scene) = record(stand);
    let (at_home, _) = record(home);
    assert_eq!(item["prototypes"], at_home["prototypes"]);
    assert_eq!(item["placements"], at_home["placements"]);
    if let Ok(output) = std::env::var("PLACEMENT_METADATA_OUTPUT") {
        let sample = json!({"prototypes": item["prototypes"],
            "placements": item["placements"].as_array().unwrap().iter().take(3).collect::<Vec<_>>()});
        std::fs::write(output, serde_json::to_string_pretty(&sample).unwrap()).unwrap();
    }
    let prototypes = item["prototypes"].as_object().unwrap();
    let placements = item["placements"].as_array().unwrap();
    assert!(!placements.is_empty());
    // Each prototype drawn once, from its record.
    let drawn: BTreeMap<&String, GeneratedItem> = prototypes
        .iter()
        .map(|(key, stated)| (key, generate_source(&library_item(stated))))
        .collect();
    for (key, stated) in prototypes {
        let meshes = &drawn[key].geometry.meshes;
        let slots: Vec<&str> = meshes.iter().map(|m| m.material_slot.as_str()).collect();
        assert_eq!(json!(slots), stated["slots"], "{key}");
        let triangles: usize = meshes.iter().map(|m| m.indices.len() / 3).sum();
        assert_eq!(json!(triangles), stated["triangles"], "{key}");
        assert!(stated["mirrored"].is_boolean() && stated["lodGroup"].is_string());
        assert!(["wall", "free"].contains(&stated["mount"].as_str().unwrap()));
    }
    let model = superspec_geometry::Matrix(stand);
    let mut covered = BTreeSet::new();
    let mut triangles = 0;
    for placement in placements {
        let path = placement["path"].as_str().unwrap();
        let key = placement["prototype"].as_str().unwrap().to_owned();
        let stated = &prototypes[&key];
        // 16 numbers, column-major, affine and a proper motion.
        let t: Vec<f64> = serde_json::from_value(placement["transform"].clone()).unwrap();
        assert_eq!(t.len(), 16, "{path}");
        assert_eq!([t[3], t[7], t[11], t[15]], [0., 0., 0., 1.], "{path}");
        let local = superspec_geometry::Matrix([
            [t[0], t[1], t[2]],
            [t[4], t[5], t[6]],
            [t[8], t[9], t[10]],
            [t[12], t[13], t[14]],
        ]);
        assert!(local.determinant() > 0., "{path}");
        assert_eq!(placement["down"], json!([0., 0., -1.]), "{path}");
        // One input per role, and nothing else.
        let inputs: BTreeMap<String, [f64; 3]> =
            serde_json::from_value(placement["inputs"].clone()).unwrap();
        assert_eq!(
            json!(inputs.keys().collect::<Vec<_>>()),
            stated["roles"],
            "{path}"
        );
        let slots: BTreeMap<String, String> =
            serde_json::from_value(placement["slots"].clone()).unwrap();
        let names: Vec<String> = serde_json::from_value(placement["meshes"].clone()).unwrap();
        let sources: Vec<_> = drawn[&key]
            .geometry
            .meshes
            .iter()
            .filter(|mesh| slots.contains_key(&mesh.material_slot))
            .collect();
        assert_eq!(sources.len(), names.len(), "{path}");
        let to_world = model.then(local);
        for (source, name) in sources.iter().zip(&names) {
            assert!(covered.insert(name.clone()), "{name} is covered twice");
            assert!(name.starts_with("p0/part/"), "{name}");
            // The scene's mesh is in the slot the placement says, bound as it says.
            let slot = &slots[&source.material_slot];
            assert_eq!(item["meshSlots"][name], json!(slot), "{name}");
            match placement["assignments"].get(&source.material_slot) {
                Some(assignment) => {
                    let binding = &placement["binding"][&source.material_slot];
                    assert!(binding["entry"].is_string() && binding["seed"].is_number());
                    assert!(binding["parameters"].is_object() && binding["layers"].is_array());
                    assert!(binding["density"].as_f64().unwrap() > 0., "{name}");
                    assert!(binding["mapping"].is_string(), "{name}");
                    assert!(assignment.is_string());
                }
                None => assert!(placement["binding"].get(&source.material_slot).is_none()),
            }
            // And is the prototype's mesh placed: the model's placement, then the record's.
            let scene_corners = corners(&scene, name);
            assert_eq!(scene_corners.len(), source.indices.len(), "{name}");
            triangles += source.indices.len() / 3;
            for (&index, (position, colour)) in source.indices.iter().zip(&scene_corners) {
                let index = index as usize;
                let placed = to_world.point(source.positions[index]);
                let tint = match source.roles.get(index).copied().unwrap_or(0) {
                    0 => [1.; 3],
                    role => inputs[stated["roles"][usize::from(role) - 1].as_str().unwrap()],
                };
                for k in 0..3 {
                    assert!(
                        (placed[k] - position[k]).abs() < 2e-4,
                        "{name}: {placed:?} is not {position:?}"
                    );
                    assert!(
                        (source.colors[index][k] * tint[k] - colour[k]).abs()
                            <= CLOSE.colour + 1e-6,
                        "{name}: colour"
                    );
                }
            }
        }
    }
    assert!(triangles > 0);
    println!(
        "{} placements of {} prototypes cover {triangles} triangles",
        placements.len(),
        prototypes.len()
    );
    // Placements of one prototype with equal bindings are what one raster serves.
    let rasters: BTreeSet<String> = placements
        .iter()
        .map(|p| format!("{} {} {}", p["prototype"], p["down"], p["binding"]))
        .collect();
    assert!(
        rasters.len() < placements.len(),
        "{} rasters",
        rasters.len()
    );
    // The city's own compilation names the same meshes under the building's owner, which is
    // how the bake finds a shaded mesh in the delivered scene.
    let aml = r#"<architecture version="1" id="building-0"><city><building id="prototype" position="0 0 0" floors="4" seed="4" graphics-detail="0" /></city></architecture>"#;
    let compiled = superspec_engine::compile_with_assets(aml, 1, &|id| {
        stock_asset(id).map(|bytes| bytes.to_vec())
    })
    .unwrap();
    let delivered: BTreeSet<&str> = compiled
        .manifest
        .meshes
        .iter()
        .map(|mesh| mesh.id.as_str())
        .collect();
    for name in &covered {
        let owned = format!("city/prototype{}", &name["p0".len()..]);
        assert!(
            delivered.contains(owned.as_str()),
            "the city scene has no {owned}"
        );
    }
}

/// A library project of prototypes, one unplaced model each from a record's `prototypes`
/// alone, shades to scene meshes whose vertices are the prototype's in order, so the roles the
/// metadata lists per vertex belong to the vertices a tool reads from the packet.
#[test]
fn a_library_project_delivers_each_prototype_with_its_roles_per_vertex() {
    let building = generate(BUILDING, &STOA[1].values(0));
    let mut project = building_project(
        &STOA[1],
        0,
        [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.], [0.; 3]],
    );
    let bound = project.models[0].slots.clone();
    let models: Vec<ModelSource> = building
        .geometry
        .prototypes
        .iter()
        .map(|(key, prototype)| ModelSource {
            // A model's identity derives from the content key, never from a path.
            id: format!("proto-{}", &key[..16]),
            kind: "item".into(),
            source: Some(library_item(&json!({"name": prototype.name,
                "content": prototype.content, "mirrored": prototype.mirrored}))),
            slots: prototype
                .meshes
                .iter()
                .filter_map(|mesh| {
                    Some((
                        mesh.material_slot.clone(),
                        bound.get(&mesh.material_slot)?.clone(),
                    ))
                })
                .collect(),
            ..ModelSource::default()
        })
        .collect();
    let mut session = Session::default();
    let mut tinted = 0;
    // A project composes at most 16 models: a library is several projects.
    for (models, prototypes) in models.chunks(16).zip(
        building
            .geometry
            .prototypes
            .iter()
            .collect::<Vec<_>>()
            .chunks(16),
    ) {
        project.models = models.to_vec();
        let prepared = session
            .prepare_project(&project, 1, &stock_asset, &|| false)
            .unwrap_or_else(|errors| panic!("{}", errors[0].message));
        let items = prepared.items().unwrap();
        tinted += delivered_roles(&prepared, items, prototypes);
    }
    assert!(tinted > 0);
}

/// Checks one library project of [`a_library_project_delivers_each_prototype_with_its_roles_per_vertex`]
/// and returns the vertices it delivers with a role.
fn delivered_roles(
    prepared: &superspec_engine::authoring::PreparedProject,
    items: &serde_json::Map<String, serde_json::Value>,
    prototypes: &[(&String, &Arc<item::Prototype>)],
) -> usize {
    let mut tinted = 0;
    for (key, prototype) in prototypes {
        let id = format!("proto-{}", &key[..16]);
        let item = &items[&id];
        assert!(item["placements"].as_array().unwrap().is_empty(), "{id}");
        for mesh in &prototype.meshes {
            let name = format!("{id}/part/p/p/{}", mesh.id);
            let row = prepared
                .scene
                .manifest
                .meshes
                .iter()
                .find(|row| row.id == name)
                .unwrap_or_else(|| panic!("the scene has no mesh {name}"));
            assert_eq!(row.vertex_count, mesh.positions.len(), "{name}");
            for (at, position) in mesh.positions.iter().enumerate() {
                for k in 0..3 {
                    let delivered = prepared.scene.positions[(row.vertex_start + at) * 3 + k];
                    assert!((f64::from(delivered) - position[k]).abs() < 1e-5, "{name}");
                }
            }
            if prototype.roles.is_empty() {
                assert!(item.get("meshRoles").is_none(), "{name}");
            } else {
                assert_eq!(item["meshRoles"][&name], json!(mesh.roles), "{name}");
                tinted += mesh.roles.iter().filter(|role| **role > 0).count();
            }
        }
    }
    tinted
}

fn hashed(value: &impl Hash) -> u64 {
    #[allow(deprecated)]
    let mut hasher = std::hash::SipHasher::new();
    value.hash(&mut hasher);
    hasher.finish()
}

/// The kind of part a leaf part is: the last two named steps of its path, without indices
/// (`aircon/condenser`, `floor/cage`).
fn kind(path: &str) -> String {
    let mut steps: Vec<&str> = Vec::new();
    for step in path.split('/') {
        let step = step.trim_end_matches(|c: char| c.is_ascii_digit() || c == '-');
        if !step.is_empty() && step != "part" && steps.last() != Some(&step) {
            steps.push(step);
        }
    }
    steps[steps.len().saturating_sub(2)..].join("/")
}

/// A part's shape up to a rigid motion or a reflection, as `kowloon_variants.rs` hashes it:
/// each triangle's slot, sorted edge lengths and distance from the part's area centroid, in
/// hundredths of a millimetre. Returns the hash and the triangle count.
fn shape(geometry: &ItemGeometry, meshes: &[usize]) -> (u64, usize) {
    let q = |v: f64| (v * 1e5).round() as i64;
    let sub = |a: [f64; 3], b: [f64; 3]| [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
    let len = |a: [f64; 3]| (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt();
    let mut centre = [0.; 3];
    let mut total = 0.;
    let mut triangles = Vec::new();
    for &mesh in meshes {
        let mesh = &geometry.meshes[mesh];
        for corner in mesh.indices.chunks_exact(3) {
            let p = [0, 1, 2].map(|k| mesh.positions[corner[k] as usize]);
            let (u, v) = (sub(p[1], p[0]), sub(p[2], p[0]));
            let area = len([
                u[1] * v[2] - u[2] * v[1],
                u[2] * v[0] - u[0] * v[2],
                u[0] * v[1] - u[1] * v[0],
            ]) / 2.;
            let middle = [0, 1, 2].map(|k| (p[0][k] + p[1][k] + p[2][k]) / 3.);
            for k in 0..3 {
                centre[k] += middle[k] * area;
            }
            total += area;
            triangles.push((hashed(&mesh.material_slot), p, middle));
        }
    }
    if total > 0. {
        centre = centre.map(|v| v / total);
    }
    let mut plain: Vec<_> = triangles
        .iter()
        .map(|(slot, p, middle)| {
            let mut edges = [
                q(len(sub(p[1], p[0]))),
                q(len(sub(p[2], p[1]))),
                q(len(sub(p[0], p[2]))),
            ];
            edges.sort_unstable();
            (*slot, edges, q(len(sub(*middle, centre))))
        })
        .collect();
    plain.sort_unstable();
    (hashed(&plain), triangles.len())
}

#[derive(Default)]
struct Kind {
    instances: usize,
    triangles: usize,
    shapes: BTreeSet<u64>,
    distinct_triangles: usize,
}

#[derive(Default)]
struct Placed {
    placements: usize,
    /// Placements in the seven prototypes, each counted once.
    authored: usize,
    triangles: usize,
    prototypes: BTreeMap<String, usize>,
    colourings: BTreeSet<String>,
    gravities: BTreeSet<String>,
    mirrored: usize,
}

#[derive(Default)]
struct Panels {
    instances: usize,
    /// Panels in the seven prototypes, each counted once.
    authored: usize,
    opened: usize,
    featured_instances: usize,
    sizes: BTreeSet<[i64; 3]>,
    bare: BTreeSet<String>,
    shapes: BTreeSet<String>,
    featured: BTreeSet<String>,
    /// The same three, when no key is shared between two buildings.
    per_building: [usize; 3],
}

/// Run with `--ignored --nocapture`. See the module comment.
#[test]
#[ignore = "a report, not a check"]
fn survey() {
    let most = std::env::var("PLACEMENT_SURVEY_ROWS")
        .ok()
        .and_then(|rows| rows.parse().ok())
        .unwrap_or(60);
    let mut report = String::from(
        "Parts-as-placements survey: the seven Stoa prototypes (29 buildings).\n\
         Instances and triangles count every building placed; shapes, prototypes and keys are\n\
         distinct across the whole city.\n\n",
    );
    for level in 0..=2u8 {
        let mut kinds: BTreeMap<String, Kind> = BTreeMap::new();
        let mut placed: BTreeMap<String, Placed> = BTreeMap::new();
        let mut panels: BTreeMap<(String, String), Panels> = BTreeMap::new();
        let mut materials: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        let (mut all_triangles, mut placed_triangles) = (0usize, 0usize);
        for stoa in &STOA {
            let prepared = prepare_with(BUILDING, &stoa.values(level), items::natives());
            let building = prepared.build(&|| false).unwrap();
            let geometry = &building.geometry;
            // 1. Leaf parts by kind: a mesh id is its part's path and a slot or face group.
            let mut leaves: BTreeMap<String, Vec<usize>> = BTreeMap::new();
            for (index, mesh) in geometry.meshes.iter().enumerate() {
                let path = mesh.id.rsplit_once('/').map_or("", |(path, _)| path);
                leaves.entry(path.to_owned()).or_default().push(index);
            }
            for (path, meshes) in &leaves {
                let (shape, triangles) = shape(geometry, meshes);
                if triangles == 0 {
                    continue;
                }
                all_triangles += triangles * stoa.placed;
                let row = kinds.entry(kind(path)).or_default();
                row.instances += stoa.placed;
                row.triangles += triangles * stoa.placed;
                if row.shapes.insert(shape) {
                    row.distinct_triangles += triangles;
                }
            }
            // 2. Placements by generator.
            for placement in geometry.placed().unwrap() {
                let prototype = &geometry.prototypes[&placement.prototype];
                let triangles: usize = placement
                    .meshes
                    .iter()
                    .map(|&mesh| geometry.meshes[mesh].indices.len() / 3)
                    .sum();
                placed_triangles += triangles * stoa.placed;
                let row = placed.entry(prototype.name.clone()).or_default();
                row.placements += stoa.placed;
                row.authored += 1;
                row.triangles += triangles * stoa.placed;
                row.prototypes
                    .entry(placement.prototype.clone())
                    .or_insert_with(|| {
                        prototype
                            .meshes
                            .iter()
                            .map(|mesh| mesh.indices.len() / 3)
                            .sum()
                    });
                row.colourings
                    .insert(format!("{} {:?}", placement.prototype, placement.inputs));
                row.gravities
                    .insert(format!("{:?}", placement.down().unwrap()));
                row.mirrored += usize::from(prototype.mirrored) * stoa.placed;
            }
            // 3. Panels by part name and the material their slots are given.
            let mut own: BTreeMap<(String, String), [BTreeSet<String>; 3]> = BTreeMap::new();
            for content in prepared.part_contents().unwrap() {
                if content.kind != "panel" {
                    continue;
                }
                let material = content
                    .slots
                    .iter()
                    .map(|slot| {
                        let request = building.materials.get(slot);
                        if let Some(request) = request {
                            materials
                                .entry(format!("{slot} = {}", request.entry))
                                .or_default()
                                .insert(serde_json::to_string(request).unwrap());
                        }
                        // A slot the building states no material for is bound by its host.
                        match request {
                            Some(request) => format!("{slot} = {}", request.entry),
                            None => format!("{slot} (bound by the host)"),
                        }
                    })
                    .collect::<BTreeSet<_>>()
                    .into_iter()
                    .collect::<Vec<_>>()
                    .join(", ");
                let name = content.path.rsplit('/').next().unwrap();
                let name = name.trim_end_matches(|c: char| c.is_ascii_digit() || c == '-');
                let key = (name.to_owned(), material);
                let row = panels.entry(key.clone()).or_default();
                row.instances += stoa.placed;
                row.authored += 1;
                row.opened += usize::from(content.openings > 0) * stoa.placed;
                row.featured_instances += usize::from(content.features > 0) * stoa.placed;
                row.sizes
                    .insert(content.size.unwrap().map(|v| (v * 1000.).round() as i64));
                row.bare.insert(content.bare.clone());
                row.shapes.insert(content.shape.clone());
                row.featured.insert(content.featured.clone());
                let own = own.entry(key).or_default();
                own[0].insert(content.bare);
                own[1].insert(content.shape);
                own[2].insert(content.featured);
            }
            for (key, sets) in own {
                let row = panels.get_mut(&key).unwrap();
                for (total, set) in row.per_building.iter_mut().zip(&sets) {
                    *total += set.len();
                }
            }
        }
        report += &format!("== Graphics detail {level} ==\n\n");
        report += &format!(
            "Part kinds ({} triangles in the city)\n  {:>9} {:>8} {:>10} {:>10}  part\n",
            all_triangles, "instances", "shapes", "triangles", "distinct"
        );
        let mut sorted: Vec<_> = kinds.iter().collect();
        sorted.sort_by_key(|(_, row)| std::cmp::Reverse(row.triangles));
        let mut totals = [0usize; 4];
        for (index, (name, row)) in sorted.iter().enumerate() {
            totals[0] += row.instances;
            totals[1] += row.shapes.len();
            totals[2] += row.triangles;
            totals[3] += row.distinct_triangles;
            if index < most {
                report += &format!(
                    "  {:>9} {:>8} {:>10} {:>10}  {name}\n",
                    row.instances,
                    row.shapes.len(),
                    row.triangles,
                    row.distinct_triangles
                );
            }
        }
        report += &format!(
            "  {:>9} {:>8} {:>10} {:>10}  all {} kinds\n\n",
            totals[0],
            totals[1],
            totals[2],
            totals[3],
            sorted.len()
        );
        report += &format!(
            "Placements ({placed_triangles} of {all_triangles} triangles are placed prototypes)\n  \
             {:>10} {:>8} {:>10} {:>10} {:>10} {:>10} {:>9} {:>8}  generator\n",
            "placements",
            "authored",
            "prototypes",
            "colourings",
            "triangles",
            "distinct",
            "gravities",
            "mirrored"
        );
        for (name, row) in &placed {
            report += &format!(
                "  {:>10} {:>8} {:>10} {:>10} {:>10} {:>10} {:>9} {:>8}  {name}\n",
                row.placements,
                row.authored,
                row.prototypes.len(),
                row.colourings.len(),
                row.triangles,
                row.prototypes.values().sum::<usize>(),
                row.gravities.len(),
                row.mirrored
            );
        }
        report += &format!(
            "\nPanels: distinct content keys in the city, and in brackets summed per prototype\n\
             (of `authored`, the panels of the seven prototypes counted once each)\n  \
             {:>9} {:>8} {:>7} {:>8} {:>6} {:>12} {:>12} {:>12}  part: slot\n",
            "instances",
            "authored",
            "opened",
            "featured",
            "sizes",
            "bare",
            "+openings",
            "+features"
        );
        let mut sorted: Vec<_> = panels.iter().collect();
        sorted.sort_by_key(|(_, row)| std::cmp::Reverse(row.instances));
        for ((name, material), row) in sorted {
            let pair = |city: usize, own: usize| format!("{city} ({own})");
            report += &format!(
                "  {:>9} {:>8} {:>7} {:>8} {:>6} {:>12} {:>12} {:>12}  {name}: {material}\n",
                row.instances,
                row.authored,
                row.opened,
                row.featured_instances,
                row.sizes.len(),
                pair(row.bare.len(), row.per_building[0]),
                pair(row.shapes.len(), row.per_building[1]),
                pair(row.featured.len(), row.per_building[2]),
            );
        }
        if !materials.is_empty() {
            report += "\nPanel materials the building states: distinct requests over the seven prototypes\n";
            for (material, requests) in &materials {
                report += &format!("  {:>3}  {material}\n", requests.len());
            }
        }
        report += "\n";
    }
    println!("{report}");
    let output = std::env::var("PLACEMENT_SURVEY_OUTPUT").unwrap_or_else(|_| {
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../generated/graphics-detail/placements/survey.txt"
        )
        .into()
    });
    let output = std::path::Path::new(&output);
    std::fs::create_dir_all(output.parent().unwrap()).unwrap();
    std::fs::write(output, report).unwrap();
}

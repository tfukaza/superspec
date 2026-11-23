//! Parts as placements: a native generator that describes its output as a shared prototype
//! is recorded as a placement of that prototype, through nested items, slot maps, reflections
//! and partial rebuilds, while the expanded meshes stay what they were.
#[path = "support/placements.rs"]
mod placements;

use placements::{Tolerance, rebuilds_the_expanded_meshes};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{Arc, Mutex},
};
use superspec_engine::itemspec::definition::{
    self as item, Environment, GeneratedItem, ItemGeometry, ItemMesh, Mount, NativeGenerators,
    NativeOutput, NativePrototype, PreparedItem, Values,
};
use superspec_geometry::Matrix;

/// A native host with one generator, `test/tile`: a lopsided wall tile `size` wide whose
/// colour is its seed's. `shared` says whether it describes the tile as a prototype.
struct Tiles {
    version: &'static str,
    shared: bool,
    /// How often a prototype's shape was asked for.
    drawn: Mutex<usize>,
}

impl Tiles {
    fn host(version: &'static str, shared: bool) -> Arc<Self> {
        Arc::new(Self {
            version,
            shared,
            drawn: Mutex::new(0),
        })
    }
    fn size(parameters: &Values) -> f64 {
        parameters.get("size").and_then(Value::as_f64).unwrap_or(1.)
    }
    fn body(parameters: &Values) -> [f64; 3] {
        let seed = parameters.get("seed").and_then(Value::as_u64).unwrap_or(0);
        [
            0.2 + 0.1 * (seed % 7) as f64,
            0.9 - 0.05 * (seed % 11) as f64,
            0.3 + 0.02 * (seed % 13) as f64,
        ]
    }
    /// The tile drawn in `body`: a face of four shades of it and a trim that is grey but for
    /// one corner. `roles` marks the corners that are shades of the body.
    fn meshes(size: f64, body: [f64; 3], roles: bool) -> Vec<ItemMesh> {
        let shade = |by: f64| [body[0] * by, body[1] * by, body[2] * by, 1.];
        let mesh = |slot: &str,
                    positions: Vec<[f64; 3]>,
                    normal: [f64; 3],
                    colors: Vec<[f64; 4]>,
                    indices: Vec<u32>,
                    tinted: Vec<u8>| ItemMesh {
            id: slot.into(),
            normals: vec![normal; positions.len()],
            uvs: positions.iter().map(|p| [p[0], p[2]]).collect(),
            positions,
            additional_uvs: BTreeMap::new(),
            colors,
            tangents: vec![],
            indices,
            material_slot: slot.into(),
            roles: if roles { tinted } else { vec![] },
        };
        vec![
            mesh(
                "face",
                vec![
                    [0., 0., 0.],
                    [size, 0., 0.],
                    [size, 0., size * 2.],
                    [0., 0., size * 2.],
                ],
                [0., -1., 0.],
                vec![shade(1.), shade(0.8), shade(0.6), shade(0.5)],
                vec![0, 1, 2, 0, 2, 3],
                vec![1; 4],
            ),
            mesh(
                "trim",
                vec![[0., 0., 0.], [size, -0.2, 0.], [0., 0., 0.3]],
                [0.6, 0., 0.8],
                vec![[0.3, 0.3, 0.3, 1.], [0.25, 0.3, 0.35, 1.], shade(0.5)],
                vec![0, 1, 2],
                vec![0, 0, 1],
            ),
        ]
    }
}

impl NativeGenerators for Tiles {
    fn identity(&self, name: &str) -> Option<String> {
        (name == "test/tile").then(|| format!("test/tile@{}", self.version))
    }
    fn names(&self) -> Vec<String> {
        vec!["test/tile".into()]
    }
    fn generate(
        &self,
        _: &str,
        parameters: &Values,
        _: &dyn Fn() -> bool,
    ) -> Result<NativeOutput, String> {
        Ok(NativeOutput {
            meshes: Self::meshes(Self::size(parameters), Self::body(parameters), false),
            attachments: vec![],
        })
    }
    fn prototype(&self, _: &str, parameters: &Values) -> Option<NativePrototype> {
        self.shared.then(|| NativePrototype {
            content: Values::from([("size".into(), json!(Self::size(parameters)))]),
            inputs: BTreeMap::from([("body".into(), Self::body(parameters))]),
            mount: Mount::Wall,
        })
    }
    fn prototype_meshes(
        &self,
        _: &str,
        content: &Values,
        _: &dyn Fn() -> bool,
    ) -> Result<NativeOutput, String> {
        *self.drawn.lock().unwrap() += 1;
        if let Some(unknown) = content.keys().find(|key| *key != "size") {
            return Err(format!("unknown content {unknown}"));
        }
        Ok(NativeOutput {
            meshes: Self::meshes(Self::size(content), [1.; 3], true),
            attachments: vec![],
        })
    }
}

fn tile(transform: Value) -> Value {
    json!({"format":"itemspec","version":2,"id":"tile","transform":transform,
        "parameters":{"size":{"type":"number","default":1,"min":0.1,"max":10},
                      "seed":{"type":"integer","default":0,"min":0,"max":1000}},
        "parts":[{"id":"tile","generator":{"kind":"native","name":"test/tile",
                  "parameters":{"size":{"parameter":"size"},"seed":{"parameter":"seed"}}},
                  "slots":{"face":"paint","trim":"metal"}}]})
}
const IDENTITY: [[f64; 3]; 4] = [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.], [0., 0., 0.]];
const MIRROR: [[f64; 3]; 4] = [[-1., 0., 0.], [0., 1., 0.], [0., 0., 1.], [0., 0., 0.]];

/// A row of tiles (turned about Z, one tilted, one with its trim mapped nowhere) and a
/// mirrored tile, as sub-items.
fn wall(transform: Value) -> Value {
    json!({"format":"itemspec","version":2,"id":"wall","transform":transform,
        "parameters":{"size":{"type":"number","default":1,"min":0.1,"max":10},
                      "odd":{"type":"number","default":1.5,"min":0.1,"max":10}},
        "parts":[
            {"id":"row","repeat":[{"name":"i","count":4}],
             "generator":{"kind":"item","entry":"tile.item.json",
                          "parameters":{"size":{"parameter":"size"},"seed":{"expr":"i * 3 + 1"}}},
             "placement":{"position":["i * 2.5",0.25,"i * 0.5"],
                          "rotation":[0,0,"i * 90"]},
             "slots":{"paint":"wall-paint","metal":"wall-metal"}},
            {"id":"tilted","generator":{"kind":"item","entry":"tile.item.json",
                          "parameters":{"size":{"parameter":"size"},"seed":5}},
             "placement":{"position":[0,4,1],"rotation":[35,0,20]},
             "slots":{"paint":"wall-paint","metal":"wall-metal"}},
            {"id":"bare","generator":{"kind":"item","entry":"tile.item.json",
                          "parameters":{"size":{"parameter":"odd"},"seed":6}},
             "placement":{"position":[3,4,0]},
             "slots":{"paint":"wall-paint"}},
            {"id":"mirrored","generator":{"kind":"item","entry":"mirror.item.json",
                          "parameters":{"size":{"parameter":"size"},"seed":7}},
             "placement":{"position":[6,4,0],"rotation":[0,0,45]},
             "slots":{"paint":"wall-paint","metal":"wall-metal"}}]})
}

fn files() -> BTreeMap<String, Arc<[u8]>> {
    [
        ("tile.item.json", tile(json!(IDENTITY))),
        ("mirror.item.json", tile(json!(MIRROR))),
        ("wall.item.json", wall(json!(IDENTITY))),
        ("mirrored-wall.item.json", wall(json!(MIRROR))),
    ]
    .into_iter()
    .map(|(name, value)| (name.to_owned(), Arc::from(value.to_string().into_bytes())))
    .collect()
}

fn prepare(entry: &str, overrides: Value, host: &Arc<Tiles>) -> PreparedItem {
    let files = files();
    let reader = |name: &str| -> Result<Arc<[u8]>, String> {
        files.get(name).cloned().ok_or(format!("Missing {name}"))
    };
    let definition = item::parse(entry, std::str::from_utf8(&files[entry]).unwrap()).unwrap();
    let mut environment = Environment::new(&reader).with_natives(host.clone());
    item::prepare_in(
        &definition,
        &serde_json::from_value(overrides).unwrap(),
        &mut environment,
        &|| false,
    )
    .unwrap()
}

fn build(entry: &str, overrides: Value, host: &Arc<Tiles>) -> GeneratedItem {
    prepare(entry, overrides, host).build(&|| false).unwrap()
}

const EXACT: Tolerance = Tolerance {
    position: 1e-9,
    colour: 1e-12,
};

fn keys(geometry: &ItemGeometry) -> BTreeMap<String, String> {
    geometry
        .placements
        .iter()
        .map(|placement| (placement.path.clone(), placement.prototype.clone()))
        .collect()
}

#[test]
fn equal_content_is_one_prototype_whatever_the_seed_and_other_content_another() {
    let host = Tiles::host("1", true);
    let key = |overrides: Value, host: &Arc<Tiles>| {
        let built = build("tile.item.json", overrides, host);
        assert_eq!(built.geometry.placements.len(), 1);
        assert_eq!(built.geometry.prototypes.len(), 1);
        built.geometry.placements[0].prototype.clone()
    };
    let first = key(json!({"seed": 1}), &host);
    for seed in 2..40 {
        assert_eq!(key(json!({"seed": seed}), &host), first, "seed {seed}");
    }
    assert_ne!(key(json!({"seed": 1, "size": 2}), &host), first);
    // Another implementation of the generator is another prototype.
    assert_ne!(key(json!({"seed": 1}), &Tiles::host("2", true)), first);
    // The seed is the placement's own: its input, never its prototype.
    let inputs = |seed: u64| {
        build("tile.item.json", json!({"seed": seed}), &host)
            .geometry
            .placements[0]
            .inputs
            .clone()
    };
    assert_ne!(inputs(1), inputs(2));
    assert_eq!(
        inputs(1)["body"],
        Tiles::body(&Values::from([("seed".into(), json!(1))]))
    );
}

#[test]
fn a_prototype_is_drawn_once_for_all_the_parts_that_place_it() {
    let host = Tiles::host("1", true);
    let built = build("wall.item.json", json!({}), &host);
    // Six tiles of one size and one of another: the mirrored tile reflects the first shape.
    assert_eq!(built.geometry.placements.len(), 7);
    assert_eq!(*host.drawn.lock().unwrap(), 2);
    let distinct: BTreeSet<_> = keys(&built.geometry).into_values().collect();
    assert_eq!(distinct.len(), 3);
    assert_eq!(built.geometry.prototypes.len(), 3);
}

#[test]
fn placing_the_prototypes_rebuilds_the_expanded_meshes() {
    let host = Tiles::host("1", true);
    for entry in [
        "tile.item.json",
        "mirror.item.json",
        "wall.item.json",
        "mirrored-wall.item.json",
    ] {
        for size in [1., 0.7] {
            let built = build(entry, json!({"size": size}), &host);
            let covered = rebuilds_the_expanded_meshes(&built.geometry, &EXACT);
            let drawn: usize = built
                .geometry
                .meshes
                .iter()
                .map(|mesh| mesh.indices.len() / 3)
                .sum();
            // Every mesh of these items is a tile's.
            assert_eq!(covered, drawn, "{entry} size {size}");
        }
    }
}

#[test]
fn a_placement_is_lifted_with_its_path_slots_and_surviving_meshes() {
    let host = Tiles::host("1", true);
    let built = build("wall.item.json", json!({}), &host);
    let geometry = &built.geometry;
    let placement = |path: &str| {
        geometry
            .placements
            .iter()
            .find(|placement| placement.path == path)
            .unwrap_or_else(|| panic!("no placement at {path}"))
    };
    let row = placement("row-2/tile");
    assert_eq!(
        row.slots,
        BTreeMap::from([
            ("face".to_owned(), "wall-paint".to_owned()),
            ("trim".to_owned(), "wall-metal".to_owned())
        ])
    );
    assert_eq!(row.meshes.len(), 2);
    assert_eq!(geometry.parts[row.node].id, "row-2");
    // The part that maps the trim nowhere keeps the placement, without the trim.
    let bare = placement("bare/tile");
    assert_eq!(
        bare.slots,
        BTreeMap::from([("face".to_owned(), "wall-paint".to_owned())])
    );
    assert_eq!(bare.meshes.len(), 1);
    assert_eq!(geometry.meshes[bare.meshes[0]].material_slot, "wall-paint");
    // Its prototype is still the whole tile.
    assert_eq!(geometry.prototypes[&bare.prototype].meshes.len(), 2);
    // Every mesh belongs to at most one placement, and every placed mesh to its node.
    let mut seen = BTreeSet::new();
    for placement in &geometry.placements {
        for mesh in &placement.meshes {
            assert!(seen.insert(*mesh));
            assert!(geometry.nodes[placement.node].meshes.contains(mesh));
        }
    }
}

#[test]
fn a_turn_keeps_the_prototype_and_a_reflection_names_the_reflected_one() {
    let host = Tiles::host("1", true);
    let built = build("wall.item.json", json!({}), &host);
    let placed = built.geometry.placed().unwrap();
    let at = |path: &str| placed.iter().find(|p| p.path == path).unwrap();
    let upright = at("row-0/tile");
    // Turns about Z are one prototype with one direction of down.
    for turned in ["row-1/tile", "row-2/tile", "row-3/tile"] {
        assert_eq!(at(turned).prototype, upright.prototype, "{turned}");
        assert_eq!(at(turned).down().unwrap(), [0., 0., -1.], "{turned}");
        assert_ne!(at(turned).transform, upright.transform, "{turned}");
    }
    // A tilt is the same shape seen by another gravity.
    let tilted = at("tilted/tile");
    assert_eq!(tilted.prototype, upright.prototype);
    let down = tilted.down().unwrap();
    assert_ne!(down, [0., 0., -1.]);
    assert!((down.iter().map(|v| v * v).sum::<f64>() - 1.).abs() < 1e-5);
    // A reflection is another prototype: the first one's mirror image, placed by a proper
    // motion.
    let mirrored = at("mirrored/tile");
    assert_ne!(mirrored.prototype, upright.prototype);
    assert!(Matrix(mirrored.transform).determinant() > 0.);
    assert_eq!(mirrored.down().unwrap(), [0., 0., -1.]);
    let (plain, reflected) = (
        &built.geometry.prototypes[&upright.prototype],
        &built.geometry.prototypes[&mirrored.prototype],
    );
    assert!(!plain.mirrored && reflected.mirrored);
    assert_eq!(**reflected, plain.reflected());
    assert_eq!(reflected.reflected(), **plain);
    // Reflecting the whole wall swaps the two: what was mirrored is the plain tile again.
    let flipped = build("mirrored-wall.item.json", json!({}), &host);
    let (before, after) = (keys(&built.geometry), keys(&flipped.geometry));
    assert_eq!(after["mirrored/tile"], before["row-0/tile"]);
    assert_eq!(after["row-0/tile"], before["mirrored/tile"]);
    assert_eq!(after["tilted/tile"], before["mirrored/tile"]);
}

/// What an item is apart from its placements.
fn substance(item: &GeneratedItem) -> Value {
    let mut value = serde_json::to_value(item).unwrap();
    let geometry = value["geometry"].as_object_mut().unwrap();
    geometry.remove("placements");
    geometry.remove("prototypes");
    value
}

#[test]
fn placements_change_nothing_else_about_an_item() {
    let (shared, plain) = (Tiles::host("1", true), Tiles::host("1", false));
    for entry in [
        "tile.item.json",
        "wall.item.json",
        "mirrored-wall.item.json",
    ] {
        let with = build(entry, json!({"size": 1.5}), &shared);
        let without = build(entry, json!({"size": 1.5}), &plain);
        assert!(!with.geometry.placements.is_empty());
        assert!(without.geometry.placements.is_empty() && without.geometry.prototypes.is_empty());
        assert_eq!(with.geometry_key, without.geometry_key, "{entry}");
        assert_eq!(with.geometry.meshes, without.geometry.meshes, "{entry}");
        assert_eq!(with.geometry.nodes, without.geometry.nodes, "{entry}");
        assert_eq!(with.geometry.parts, without.geometry.parts, "{entry}");
        assert_eq!(with.geometry.features, without.geometry.features, "{entry}");
        assert_eq!(with.geometry.anchors, without.geometry.anchors, "{entry}");
        assert_eq!(with.geometry.bounds, without.geometry.bounds, "{entry}");
        assert_eq!(substance(&with), substance(&without), "{entry}");
        // An item without placements serializes without a trace of them.
        let text = serde_json::to_string(&without).unwrap();
        for field in ["placements", "prototypes", "roles"] {
            assert!(!text.contains(&format!("\"{field}\"")), "{entry}: {field}");
        }
        // With them, only prototype meshes carry roles.
        let value = serde_json::to_value(&with).unwrap();
        assert!(value["geometry"]["placements"].is_array());
        for mesh in value["geometry"]["meshes"].as_array().unwrap() {
            assert!(mesh.get("roles").is_none());
        }
    }
}

#[test]
fn a_kept_part_keeps_its_placements_on_a_rebuild() {
    let host = Tiles::host("1", true);
    let first = build("wall.item.json", json!({}), &host);
    let drawn = *host.drawn.lock().unwrap();
    let changed = prepare("wall.item.json", json!({"odd": 2.5}), &host);
    let (rebuilt, built) = changed.rebuild(&first, &|| false).unwrap();
    assert_eq!(built, ["bare"]);
    // Only the changed tile's new shape was drawn.
    assert_eq!(*host.drawn.lock().unwrap(), drawn + 1);
    // The rebuilt item is the item built afresh, placements and prototypes included.
    let fresh = build("wall.item.json", json!({"odd": 2.5}), &host);
    assert_eq!(rebuilt.geometry.placements, fresh.geometry.placements);
    assert_eq!(rebuilt.geometry.prototypes, fresh.geometry.prototypes);
    assert_eq!(rebuilt.geometry.meshes, fresh.geometry.meshes);
    rebuilds_the_expanded_meshes(&rebuilt.geometry, &EXACT);
    // The kept parts' placements are the ones they had; the changed part's is new.
    let (before, after) = (keys(&first.geometry), keys(&rebuilt.geometry));
    for (path, key) in &before {
        if path == "bare/tile" {
            assert_ne!(&after[path], key);
        } else {
            assert_eq!(&after[path], key, "{path}");
        }
    }
    // The prototype no part places any more is gone.
    assert!(
        !rebuilt
            .geometry
            .prototypes
            .contains_key(&before["bare/tile"])
    );
    // Unchanged, the item shares the earlier geometry outright.
    let same = prepare("wall.item.json", json!({}), &host);
    let (shared, built) = same.rebuild(&first, &|| false).unwrap();
    assert!(built.is_empty() && Arc::ptr_eq(&shared.geometry, &first.geometry));
}

/// An item of one part that draws a prototype from what a placement's record states of it.
fn library(name: &str, content: &Values, mirrored: bool) -> String {
    json!({"format":"itemspec","version":2,"id":"library",
        "parts":[{"id":"p","generator":{"kind":"prototype","name":name,
                  "content":content,"mirrored":mirrored}}]})
    .to_string()
}

fn prepare_source(source: &str, host: &Arc<Tiles>) -> Result<PreparedItem, item::ItemDiagnostic> {
    let reader = |name: &str| -> Result<Arc<[u8]>, String> { Err(format!("Missing {name}")) };
    let definition = item::parse("library.item.json", source)?;
    let mut environment = Environment::new(&reader).with_natives(host.clone());
    item::prepare_in(&definition, &Values::new(), &mut environment, &|| false)
}

#[test]
fn a_prototype_part_draws_the_prototype_its_placements_share() {
    let host = Tiles::host("1", true);
    let built = build("wall.item.json", json!({}), &host);
    assert_eq!(built.geometry.prototypes.len(), 3);
    for (key, prototype) in &built.geometry.prototypes {
        // Only what a sidecar states of the prototype goes in, through JSON.
        let stated: Value = serde_json::from_str(
            &serde_json::to_string(
                &json!({"name": prototype.name, "content": prototype.content,
                "mirrored": prototype.mirrored}),
            )
            .unwrap(),
        )
        .unwrap();
        let source = library(
            stated["name"].as_str().unwrap(),
            &serde_json::from_value(stated["content"].clone()).unwrap(),
            stated["mirrored"].as_bool().unwrap(),
        );
        let drawn = prepare_source(&source, &host)
            .unwrap()
            .build(&|| false)
            .unwrap();
        // The part's meshes are the prototype's, value for value, under the part's name.
        assert_eq!(drawn.geometry.meshes.len(), prototype.meshes.len());
        for (mesh, shared) in drawn.geometry.meshes.iter().zip(&prototype.meshes) {
            let mut renamed = shared.clone();
            renamed.id = format!("p/{}", shared.id);
            assert_eq!(mesh, &renamed, "prototype {key}");
            assert_eq!(mesh.roles.len(), mesh.positions.len());
        }
        // In the item's own space too: the part is unplaced.
        assert_eq!(
            drawn
                .flattened()
                .unwrap()
                .iter()
                .map(|mesh| &mesh.positions)
                .collect::<Vec<_>>(),
            prototype
                .meshes
                .iter()
                .map(|mesh| &mesh.positions)
                .collect::<Vec<_>>()
        );
        // It is the shape itself, not a placement of it.
        assert!(drawn.geometry.placements.is_empty() && drawn.geometry.prototypes.is_empty());
    }
    // The reflected prototype is another part key, and neither is the instance's.
    let key = |source: &str| prepare_source(source, &host).unwrap().geometry_key.clone();
    let content = Values::from([("size".into(), json!(1.))]);
    let plain = key(&library("test/tile", &content, false));
    assert_eq!(plain, key(&library("test/tile", &content, false)));
    assert_ne!(plain, key(&library("test/tile", &content, true)));
    let instance = json!({"format":"itemspec","version":2,"id":"library",
        "parts":[{"id":"p","generator":{"kind":"native","name":"test/tile",
                  "parameters":content}}]})
    .to_string();
    assert_ne!(plain, key(&instance));
}

#[test]
fn an_unknown_prototype_fails_where_the_definition_names_it() {
    let host = Tiles::host("1", true);
    // A generator the host does not offer: refused when the item is prepared.
    let source = library("test/slab", &Values::new(), false);
    let mut error = prepare_source(&source, &host).err().unwrap();
    assert_eq!(error.code, "ITEM_NATIVE");
    assert_eq!(error.pointer, "/parts/0/generator/name");
    assert!(error.message.contains("test/slab") && error.message.contains("test/tile"));
    item::locate_diagnostic(&source, &mut error);
    let located = &source[error.offset.unwrap()..];
    assert!(located.starts_with("\"test/slab\""), "located at {located}");
    // Content the generator does not draw: refused by the generator when the part is built.
    let content = Values::from([("entry".into(), json!(7))]);
    let source = library("test/tile", &content, false);
    let error = prepare_source(&source, &host)
        .unwrap()
        .build(&|| false)
        .err()
        .unwrap();
    assert_eq!(error.pointer, "/parts/0/generator");
    assert!(
        error.message.contains("test/tile") && error.message.contains("unknown content entry"),
        "{}",
        error.message
    );
    // The generator belongs to a part, and its fields are closed.
    let rooted = json!({"format":"itemspec","version":2,"id":"library",
        "generator":{"kind":"prototype","name":"test/tile"}})
    .to_string();
    assert!(
        item::parse("library.item.json", &rooted)
            .and_then(|definition| {
                let reader =
                    |name: &str| -> Result<Arc<[u8]>, String> { Err(format!("Missing {name}")) };
                let mut environment = Environment::new(&reader).with_natives(host.clone());
                item::prepare_in(&definition, &Values::new(), &mut environment, &|| false)
            })
            .is_err()
    );
    let misspelt = json!({"format":"itemspec","version":2,"id":"library",
        "parts":[{"id":"p","generator":{"kind":"prototype","name":"test/tile","contents":{}}}]})
    .to_string();
    assert!(item::parse("library.item.json", &misspelt).is_err());
}

#[test]
fn a_host_without_prototypes_is_served_by_the_defaults() {
    /// The defaults: no prototype, and a prototype's shape is what `generate` draws.
    struct Plain;
    impl NativeGenerators for Plain {
        fn identity(&self, _: &str) -> Option<String> {
            Some("plain@1".into())
        }
        fn names(&self) -> Vec<String> {
            vec![]
        }
        fn generate(
            &self,
            _: &str,
            parameters: &Values,
            _: &dyn Fn() -> bool,
        ) -> Result<NativeOutput, String> {
            Ok(NativeOutput {
                meshes: Tiles::meshes(Tiles::size(parameters), [0.5; 3], false),
                attachments: vec![],
            })
        }
    }
    let values = Values::from([("size".into(), json!(2.))]);
    assert_eq!(Plain.prototype("test/tile", &values), None);
    assert_eq!(
        Plain
            .prototype_meshes("test/tile", &values, &|| false)
            .unwrap()
            .meshes,
        Plain
            .generate("test/tile", &values, &|| false)
            .unwrap()
            .meshes
    );
}

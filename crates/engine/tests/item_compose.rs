//! ItemSpec composition: sub-items, repeated and conditional parts, inline expressions and
//! native generators. Expectations are derived from the definitions' own numbers.
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};
use superspec_engine::itemspec::definition::{
    self as item, Environment, GeneratedItem, ItemDiagnostic, ItemMesh, NativeGenerators,
    NativeOutput, Values, expression::Expression,
};

fn library(files: &[(&str, Value)]) -> BTreeMap<String, Arc<[u8]>> {
    files
        .iter()
        .map(|(name, value)| {
            (
                (*name).to_owned(),
                Arc::from(value.to_string().into_bytes()),
            )
        })
        .collect()
}

fn build_with(
    top: &Value,
    files: &BTreeMap<String, Arc<[u8]>>,
    overrides: Values,
    natives: Option<Arc<dyn NativeGenerators>>,
) -> Result<GeneratedItem, ItemDiagnostic> {
    let definition = item::parse("top.item.json", &top.to_string())?;
    let reader = |name: &str| -> Result<Arc<[u8]>, String> {
        files.get(name).cloned().ok_or(format!("Missing {name}"))
    };
    let mut environment = Environment::new(&reader);
    if let Some(natives) = natives {
        environment = environment.with_natives(natives);
    }
    item::prepare_in(&definition, &overrides, &mut environment, &|| false)?.build(&|| false)
}

fn build(
    top: &Value,
    files: &BTreeMap<String, Arc<[u8]>>,
) -> Result<GeneratedItem, ItemDiagnostic> {
    build_with(top, files, Values::new(), None)
}

fn cube(id: &str, size: f64) -> Value {
    json!({"format":"itemspec","version":2,"id":id,
        "parameters":{"size":{"type":"number","default":size,"min":0.01,"max":10}},
        "parts":[{"id":"body","generator":{"kind":"box","size":[{"parameter":"size"},{"parameter":"size"},{"parameter":"size"}]},
                  "slots":{"side":"paint","top":"paint","bottom":"paint"}}],
        "slots":{"paint":{"entry":"materials/paint.ms","parameters":{},"seed":0}},
        "attachments":[{"name":"top","position":[0,0,{"parameter":"size"}],"direction":[0,0,1]}]})
}

#[test]
fn expressions_compare_choose_and_hash() {
    let value = |text: &str| {
        Expression::parse(text)
            .unwrap()
            .evaluate(&mut |_, _| Ok(0.))
            .unwrap()
    };
    assert_eq!(value("2 < 3"), 1.);
    assert_eq!(value("2 >= 3"), 0.);
    assert_eq!(value("1 + 1 == 2"), 1.);
    assert_eq!(value("3 != 3"), 0.);
    assert_eq!(value("7 % 3"), 1.);
    assert_eq!(value("-1 % 3"), 2.);
    assert_eq!(value("clamp(5, 0, 2)"), 2.);
    assert_eq!(value("select(2 > 1, 10, 20)"), 10.);
    let r = value("random(4, 2)");
    assert!((0. ..1.).contains(&r));
    assert_eq!(r, value("random(4, 2)"));
    assert_ne!(r, value("random(2, 4)"));
    assert_eq!(value("random(0)"), value("random(-0)"));
    // A comparison binds looser than arithmetic.
    assert_eq!(value("1 + 2 > 2 * 1"), 1.);
}

#[test]
fn a_choice_control_reads_as_its_index_in_expressions() {
    // `shape` is box (0), slab (1) or post (2): its index keeps one part and sizes another.
    let top = json!({"format":"itemspec","version":2,"id":"choice",
        "parameters":{"shape":{"type":"choice","default":"slab","choices":["box","slab","post"]}},
        "derived":{"tall":"select(shape == 2, 3, 1)"},
        "parts":[{"id":"body","generator":{"kind":"box","size":[1,1,{"derived":"tall"}]}},
                 {"id":"cap","when":"shape == 1","generator":{"kind":"box","size":[1,1,0.1]},
                  "placement":{"position":[0,0,"1 + shape"]}}]});
    let files = BTreeMap::new();
    let ids = |item: &GeneratedItem| -> Vec<String> {
        item.geometry.parts.iter().map(|p| p.id.clone()).collect()
    };
    let slab = build(&top, &files).unwrap();
    assert_eq!(ids(&slab), ["body", "cap"]);
    // The cap stands at 1 + 1 and is 0.1 thick.
    assert_eq!(slab.geometry.bounds[1][2], 2.1);
    let post = build_with(
        &top,
        &files,
        [("shape".to_owned(), json!("post"))].into(),
        None,
    )
    .unwrap();
    assert_eq!(ids(&post), ["body"]);
    assert_eq!(post.geometry.bounds[1][2], 3.);
}

#[test]
fn a_sub_item_is_placed_with_its_arguments_slots_and_attachments() {
    let files = library(&[("cube.item.json", cube("cube", 1.))]);
    let top = json!({"format":"itemspec","version":2,"id":"stack",
        "parameters":{"edge":{"type":"number","default":0.5,"min":0.1,"max":2}},
        "parts":[{"id":"lower","generator":{"kind":"item","entry":"cube.item.json","parameters":{"size":{"parameter":"edge"}}}},
                 {"id":"upper","generator":{"kind":"item","entry":"cube.item.json","parameters":{"size":{"expr":"edge / 2"}}},
                  "placement":{"position":[0,0,{"parameter":"edge"}]},"slots":{"paint":"trim"}}]});
    let item = build(&top, &files).unwrap();
    // The lower cube spans 0.5; the upper one 0.25 standing on it.
    assert_eq!(item.geometry.bounds, [[0., 0., 0.], [0.5, 0.5, 0.75]]);
    let slots: Vec<&str> = item
        .geometry
        .meshes
        .iter()
        .map(|m| m.material_slot.as_str())
        .collect();
    assert!(slots.contains(&"paint") && slots.contains(&"trim"));
    // Sub-item materials fill slots the parent does not assign; a renamed slot carries its request.
    assert_eq!(item.materials["paint"].entry, "materials/paint.ms");
    assert_eq!(item.materials["trim"].entry, "materials/paint.ms");
    let top_of = |name: &str| {
        item.attachments
            .iter()
            .find(|a| a.name == name)
            .unwrap()
            .position
    };
    assert_eq!(top_of("lower/top"), [0., 0., 0.5]);
    assert_eq!(top_of("upper/top"), [0., 0., 0.75]);
    // Mesh IDs are prefixed by the part.
    assert!(
        item.geometry
            .meshes
            .iter()
            .all(|m| m.id.starts_with("lower/") || m.id.starts_with("upper/"))
    );
}

#[test]
fn slot_densities_reach_the_parent_through_the_slots_sub_items_land_in() {
    let dense = |id: &str, density: f64| {
        let mut definition = cube(id, 1.);
        definition["densities"] = json!({"paint": density});
        definition
    };
    let files = library(&[
        ("notice.item.json", dense("notice", 640.)),
        ("sign.item.json", dense("sign", 800.)),
        ("crate.item.json", dense("crate", 48.)),
        ("plain.item.json", cube("plain", 1.)),
    ]);
    let part = |id: &str, entry: &str, slot: Option<&str>| {
        let mut part = json!({"id":id,"generator":{"kind":"item","entry":entry}});
        if let Some(slot) = slot {
            part["slots"] = json!({"paint": slot});
        }
        part
    };
    let top = |parts: Value, densities: Value| json!({"format":"itemspec","version":2,"id":"shop","parts":parts,"densities":densities});
    // A sub-item's density follows its slot into the parent: kept under its own name, renamed
    // by the part's slot map, and absent for a sub-item that states none.
    let shop = build(
        &top(
            json!([
                part("notice", "notice.item.json", None),
                part("sign", "sign.item.json", Some("lettering")),
                part("goods", "crate.item.json", Some("goods")),
                part("shelf", "plain.item.json", Some("shelf")),
            ]),
            json!({}),
        ),
        &files,
    )
    .unwrap();
    assert_eq!(
        shop.densities,
        [
            ("goods".to_owned(), 48.),
            ("lettering".to_owned(), 800.),
            ("paint".to_owned(), 640.)
        ]
        .into()
    );
    // Two sub-items in one slot: the finest holds, in either order of the parts.
    for order in [["sign", "crate"], ["crate", "sign"]] {
        let parts: Vec<Value> = order
            .iter()
            .map(|name| part(name, &format!("{name}.item.json"), None))
            .collect();
        let shared = build(&top(json!(parts), json!({})), &files).unwrap();
        assert_eq!(shared.densities, [("paint".to_owned(), 800.)].into());
    }
    // The parent's own statement for a slot comes before its sub-items', even when coarser.
    let stated = build(
        &top(
            json!([part("sign", "sign.item.json", None)]),
            json!({"paint": 96., "trim": 320.}),
        ),
        &files,
    )
    .unwrap();
    assert_eq!(
        stated.densities,
        [("paint".to_owned(), 96.), ("trim".to_owned(), 320.)].into()
    );
    // Two levels down: a building holding the shop still hears the sign.
    let mut nested = files.clone();
    nested.extend(library(&[(
        "shop.item.json",
        top(
            json!([part("sign", "sign.item.json", Some("lettering"))]),
            json!({}),
        ),
    )]));
    let building = build(
        &top(json!([part("shop", "shop.item.json", None)]), json!({})),
        &nested,
    )
    .unwrap();
    assert_eq!(building.densities, [("lettering".to_owned(), 800.)].into());
    // A density is no part of the geometry: the same meshes with and without it.
    let bare = build(
        &top(json!([part("sign", "plain.item.json", None)]), json!({})),
        &files,
    );
    assert_eq!(
        bare.unwrap().geometry_key,
        build(
            &top(
                json!([part("sign", "plain.item.json", None)]),
                json!({"paint": 800.})
            ),
            &files
        )
        .unwrap()
        .geometry_key
    );
    assert!(serde_json::to_value(&shop).unwrap()["densities"]["lettering"] == 800.);
    // Out of range, or for a slot that is no identifier, is refused where it is written.
    for (densities, pointer) in [
        (json!({"paint": 0.5}), "/densities/paint"),
        (json!({"paint": 100001}), "/densities/paint"),
        (json!({"": 64}), "/densities/"),
    ] {
        let error = build(
            &top(json!([part("sign", "plain.item.json", None)]), densities),
            &files,
        )
        .unwrap_err();
        assert_eq!(
            (error.code.as_str(), error.pointer.as_str()),
            ("ITEM_DENSITY", pointer)
        );
    }
}

#[test]
fn repeated_parts_read_their_indices_and_conditions_drop_copies() {
    let files = library(&[("cube.item.json", cube("cube", 1.))]);
    let top = json!({"format":"itemspec","version":2,"id":"grid",
        "parameters":{"columns":{"type":"integer","default":3,"min":0,"max":10},
                      "rows":{"type":"integer","default":2,"min":0,"max":10}},
        "parts":[{"id":"cell","repeat":[{"name":"i","count":{"parameter":"columns"}},{"name":"j","count":"rows"}],
                  "when":"(i + j) % 2 == 0",
                  "generator":{"kind":"item","entry":"cube.item.json","parameters":{"size":0.5}},
                  "placement":{"position":["i * 1","0","j * 1"]}}]});
    let item = build(&top, &files).unwrap();
    let ids: Vec<&str> = item.geometry.parts.iter().map(|p| p.id.as_str()).collect();
    assert_eq!(ids, ["cell-0-0", "cell-1-1", "cell-2-0"]);
    // Columns 0..2 and rows 0..1 of half-meter cubes on a one-meter pitch.
    assert_eq!(item.geometry.bounds, [[0., 0., 0.], [2.5, 0.5, 1.5]]);
    // Counts follow the controls, and zero copies leave the other parts.
    let mut overrides = Values::new();
    overrides.insert("columns".into(), json!(1));
    overrides.insert("rows".into(), json!(1));
    let one = build_with(&top, &files, overrides, None).unwrap();
    assert_eq!(one.geometry.parts.len(), 1);
}

#[test]
fn choices_templates_and_attachment_routes_follow_the_copy() {
    let files = library(&[
        ("small.item.json", cube("small", 0.2)),
        ("large.item.json", cube("large", 0.4)),
        ("cube-1.item.json", cube("one", 0.3)),
    ]);
    let top = json!({"format":"itemspec","version":2,"id":"choice",
        "parts":[
            {"id":"pick","repeat":[{"name":"i","count":2}],
             "generator":{"kind":"item","entry":["small.item.json","large.item.json"],"pick":"i"},
             "placement":{"position":["i * 2","0","0"]}},
            {"id":"named","generator":{"kind":"item","entry":"cube-{n}.item.json"},
             "repeat":[{"name":"n","count":2}],"when":"n == 1","placement":{"position":[5,0,0]}},
            {"id":"stub","repeat":[{"name":"k","count":2}],
             "generator":{"kind":"route","section":{"kind":"round","radius":0.02},
                          "points":[[0,0,0],[0,0,0.3]],"from":{"attachment":"pick-{k}/top"}}}]});
    let item = build(&top, &files).unwrap();
    let part = |id: &str| item.geometry.parts.iter().any(|p| p.id == id);
    assert!(
        part("pick-0") && part("pick-1") && part("named-1") && part("stub-0") && part("stub-1")
    );
    assert!(!part("named-0"));
    // The second copy picked the large cube: its top attachment is at 0.4.
    let at = |name: &str| {
        item.attachments
            .iter()
            .find(|a| a.name == name)
            .unwrap()
            .position
    };
    assert_eq!(at("pick-0/top"), [0., 0., 0.2]);
    assert_eq!(at("pick-1/top"), [2., 0., 0.4]);
    // Each stub starts on its own cube's top and rises 0.3.
    assert_eq!(at("stub-1/end"), [2., 0., 0.7]);
    assert_eq!(item.geometry.bounds[1][2], 0.7);
}

#[test]
fn cycles_depth_and_missing_entries_are_refused() {
    let looped = json!({"format":"itemspec","version":2,"id":"loop",
        "parts":[{"id":"again","generator":{"kind":"item","entry":"loop.item.json"}}]});
    let files = library(&[("loop.item.json", looped.clone())]);
    let error = build(&looped, &files).unwrap_err();
    assert_eq!(error.code, "ITEM_SUB_ITEM");
    assert!(
        error.message.contains("contains itself"),
        "{}",
        error.message
    );

    let mut chain = vec![];
    for level in 0..12 {
        chain.push((
            format!("level-{level}.item.json"),
            json!({"format":"itemspec","version":2,"id":format!("level-{level}"),
                "parts":[{"id":"next","generator":{"kind":"item","entry":format!("level-{}.item.json", level + 1)}}]}),
        ));
    }
    let named: Vec<(&str, Value)> = chain.iter().map(|(n, v)| (n.as_str(), v.clone())).collect();
    let deep = library(&named);
    let error = build(&chain[0].1, &deep).unwrap_err();
    assert_eq!(error.code, "ITEM_SUB_ITEM");
    assert!(error.message.contains("nest at most"), "{}", error.message);

    let missing = json!({"format":"itemspec","version":2,"id":"missing",
        "parts":[{"id":"gone","generator":{"kind":"item","entry":"gone.item.json"}}]});
    assert_eq!(
        build(&missing, &BTreeMap::new()).unwrap_err().pointer,
        "/parts/0/generator"
    );
}

#[test]
fn repeat_limits_and_identifiers_are_checked() {
    let files = library(&[("cube.item.json", cube("cube", 1.))]);
    let part = |repeat: Value| {
        json!({"format":"itemspec","version":2,"id":"r",
            "parameters":{"n":{"type":"number","default":2}},
            "parts":[{"id":"c","repeat":repeat,"generator":{"kind":"item","entry":"cube.item.json"}}]})
    };
    for (repeat, code) in [
        (json!([{"name":"n","count":2}]), "ITEM_REPEAT"),
        (json!([{"name":"i","count":2.5}]), "ITEM_REPEAT"),
        (json!([{"name":"i","count":300}]), "ITEM_REPEAT"),
        (
            json!([{"name":"i","count":200},{"name":"j","count":200}]),
            "ITEM_LIMIT",
        ),
    ] {
        assert_eq!(
            build(&part(repeat.clone()), &files).unwrap_err().code,
            code,
            "{repeat}"
        );
    }
}

/// A native host with one generator: a unit quad at the origin, counting its calls.
struct Quads(Mutex<usize>);
impl NativeGenerators for Quads {
    fn identity(&self, name: &str) -> Option<String> {
        (name == "test/quad").then(|| "test/quad@1".into())
    }
    fn names(&self) -> Vec<String> {
        vec!["test/quad".into()]
    }
    fn generate(
        &self,
        _: &str,
        parameters: &Values,
        _: &dyn Fn() -> bool,
    ) -> Result<NativeOutput, String> {
        *self.0.lock().unwrap() += 1;
        let size = parameters.get("size").and_then(Value::as_f64).unwrap_or(1.);
        let tint = parameters
            .get("tint")
            .and_then(Value::as_str)
            .unwrap_or("none")
            .to_owned();
        Ok(NativeOutput {
            meshes: vec![ItemMesh {
                id: "quad".into(),
                positions: vec![
                    [0., 0., 0.],
                    [size, 0., 0.],
                    [size, 0., size],
                    [0., 0., size],
                ],
                normals: vec![[0., -1., 0.]; 4],
                uvs: vec![[0., 0.]; 4],
                additional_uvs: BTreeMap::new(),
                colors: vec![],
                tangents: vec![],
                indices: vec![0, 1, 2, 0, 2, 3],
                material_slot: tint,
                roles: vec![],
            }],
            attachments: vec![],
        })
    }
}

#[test]
fn native_parts_call_the_host_with_resolved_parameters() {
    let quads = Arc::new(Quads(Mutex::new(0)));
    let top = json!({"format":"itemspec","version":2,"id":"natives",
        "parameters":{"size":{"type":"number","default":0.5}},
        "parts":[{"id":"q","repeat":[{"name":"i","count":3}],
                  "generator":{"kind":"native","name":"test/quad",
                               "parameters":{"size":{"expr":"size * (i + 1)"},
                                             "tint":{"choose":["red","green"],"by":"i"}}},
                  "slots":{"red":"warm","green":"cool"}}]});
    let item = build_with(&top, &BTreeMap::new(), Values::new(), Some(quads.clone())).unwrap();
    assert_eq!(*quads.0.lock().unwrap(), 3);
    let slots: Vec<&str> = item
        .geometry
        .meshes
        .iter()
        .map(|m| m.material_slot.as_str())
        .collect();
    assert_eq!(slots, ["warm", "cool", "warm"]);
    assert_eq!(item.geometry.bounds[1], [1.5, 0., 1.5]);
    // Without a host, or with an unknown name, preparing fails at the part.
    let error = build(&top, &BTreeMap::new()).unwrap_err();
    assert_eq!(error.code, "ITEM_NATIVE");
    let mut unknown = top.clone();
    unknown["parts"][0]["generator"]["name"] = json!("test/none");
    let error = build_with(&unknown, &BTreeMap::new(), Values::new(), Some(quads)).unwrap_err();
    assert!(error.message.contains("test/quad"), "{}", error.message);
}

#[test]
fn a_changed_sub_item_rebuilds_only_its_parts() {
    let files = library(&[("cube.item.json", cube("cube", 1.))]);
    let top = json!({"format":"itemspec","version":2,"id":"pair",
        "parameters":{"a":{"type":"number","default":0.5},"b":{"type":"number","default":0.5}},
        "parts":[{"id":"first","generator":{"kind":"item","entry":"cube.item.json","parameters":{"size":{"parameter":"a"}}}},
                 {"id":"second","generator":{"kind":"item","entry":"cube.item.json","parameters":{"size":{"parameter":"b"}}},
                  "placement":{"position":[2,0,0]}}]});
    let definition = item::parse("top.item.json", &top.to_string()).unwrap();
    let reader = |name: &str| -> Result<Arc<[u8]>, String> {
        files.get(name).cloned().ok_or(format!("Missing {name}"))
    };
    let prepare = |overrides: Values| {
        item::prepare_in(
            &definition,
            &overrides,
            &mut Environment::new(&reader),
            &|| false,
        )
        .unwrap()
    };
    let first = prepare(Values::new()).build(&|| false).unwrap();
    let mut changed = Values::new();
    changed.insert("b".into(), json!(0.7));
    let again = prepare(changed);
    assert_ne!(again.geometry_key, first.geometry_key);
    let (rebuilt, built) = again.rebuild(&first, &|| false).unwrap();
    assert_eq!(built, ["second"]);
    assert_eq!(rebuilt.geometry.bounds[1], [2.7, 0.7, 0.7]);
}

#[test]
fn grids_are_read_by_cell_and_count_and_texts_by_element() {
    let top = json!({"format":"itemspec","version":2,"id":"grids",
        "parameters":{
            "plan":{"type":"grid","default":[[1,2,3],[4]]},
            "kinds":{"type":"grid","default":[["b","a"]],"choices":["x","a","b"]},
            "names":{"type":"texts","default":["first","second"]},
            "label":{"type":"text","default":"hi"}},
        "derived":{"rows":"count(plan)","first":"count(plan, 0)","second":"count(plan, 1)","none":"count(plan, 5)",
                   "three":"cell(plan, 0, 2)","four":"cell(plan, 1, 0)","absent":"cell(plan, 2, 0)","b":"cell(kinds, 0, 0)"},
        "parts":[{"id":"q","repeat":[{"name":"i","count":{"derived":"first"}}],
                  "generator":{"kind":"native","name":"test/quad","parameters":{"size":{"expr":"cell(plan, 0, i) / 10"},
                      "tint":{"element":"names","at":"i"}}}}]});
    let quads = Arc::new(Quads(Mutex::new(0)));
    let item = build_with(&top, &BTreeMap::new(), Values::new(), Some(quads)).unwrap();
    let derived = |name: &str| {
        let definition = item::parse("g", &top.to_string()).unwrap();
        let values = definition.resolve_parameters(&Values::new()).unwrap();
        definition.resolve_derived(&values).unwrap()[name]
    };
    assert_eq!(
        [
            derived("rows"),
            derived("first"),
            derived("second"),
            derived("none")
        ],
        [2., 3., 1., 0.]
    );
    assert_eq!(
        [
            derived("three"),
            derived("four"),
            derived("absent"),
            derived("b")
        ],
        [3., 4., 0., 2.]
    );
    // Three copies, sized 0.1, 0.2 and 0.3, the first two named by the texts and the third by none.
    assert_eq!(item.geometry.parts.len(), 3);
    assert!((item.geometry.bounds[1][0] - 0.3).abs() < 1e-9);
    let slots: Vec<&str> = item
        .geometry
        .meshes
        .iter()
        .map(|m| m.material_slot.as_str())
        .collect();
    assert_eq!(slots, ["first", "second", "none"]);
    // An entry that is not among a grid's choices, or a text that is too long, is refused.
    let definition = item::parse("g", &top.to_string()).unwrap();
    let bad: Values = [("kinds".to_owned(), json!([["fridge"]]))].into();
    assert!(definition.resolve_parameters(&bad).is_err());
    let long: Values = [("label".to_owned(), json!("x".repeat(300)))].into();
    assert!(definition.resolve_parameters(&long).is_err());
}

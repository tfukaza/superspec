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

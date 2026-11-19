//! ItemSpec version 2: parts and derived values, routes and fittings, panels and openings.
//! Expectations are derived here from the definitions' dimensions, not from the generators.
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, HashMap},
    f64::consts::PI,
    sync::Arc,
};
use superspec_engine::itemspec::definition::{
    self as item, GeneratedItem, ItemDiagnostic, ItemMesh, Values, expression::Expression,
};
use superspec_geometry::{
    Matrix, Operation, Solid,
    math::{add, cross, dot, length, scale, sub},
};

const WALL: &str = include_str!("../src/itemspec/definition/fixtures/washroom-wall.item.json");
const WALL_LOW: &str =
    include_str!("../src/itemspec/definition/fixtures/washroom-wall-low.item.json");
const PIPE: &str = include_str!("../src/itemspec/definition/fixtures/washroom-pipe.item.json");
const DUCT: &str = include_str!("../src/itemspec/definition/fixtures/washroom-duct.item.json");

fn absent(name: &str) -> Result<Arc<[u8]>, String> {
    Err(format!("Missing {name}"))
}
fn values(pairs: &[(&str, f64)]) -> Values {
    pairs
        .iter()
        .map(|(name, value)| ((*name).to_owned(), json!(value)))
        .collect()
}
fn generate(text: &str, overrides: &[(&str, f64)]) -> Result<GeneratedItem, ItemDiagnostic> {
    let definition = item::parse("test.item.json", text)?;
    item::generate(&definition, &values(overrides), &absent, &|| false)
}
fn close(a: f64, b: f64, tolerance: f64) {
    assert!((a - b).abs() <= tolerance, "{a} != {b} (±{tolerance})");
}
fn close3(a: [f64; 3], b: [f64; 3], tolerance: f64) {
    for axis in 0..3 {
        assert!(
            (a[axis] - b[axis]).abs() <= tolerance,
            "{a:?} != {b:?} (±{tolerance})"
        );
    }
}
fn meshes<'a>(generated: &'a GeneratedItem, ids: &[&str]) -> Vec<&'a ItemMesh> {
    ids.iter()
        .map(|id| {
            generated
                .geometry
                .meshes
                .iter()
                .find(|mesh| mesh.id == *id)
                .unwrap_or_else(|| panic!("no mesh {id}"))
        })
        .collect()
}
fn triangles(mesh: &ItemMesh) -> impl Iterator<Item = [usize; 3]> + '_ {
    mesh.indices
        .chunks_exact(3)
        .map(|t| [t[0] as usize, t[1] as usize, t[2] as usize])
}
fn area(mesh: &ItemMesh) -> f64 {
    triangles(mesh)
        .map(|[a, b, c]| {
            length(cross(
                sub(mesh.positions[b], mesh.positions[a]),
                sub(mesh.positions[c], mesh.positions[a]),
            )) / 2.
        })
        .sum()
}
/// Checks that the meshes together bound closed solids with outward winding and
/// outward vertex normals, and returns the enclosed volume. Corners are welded at 0.1 µm.
fn closed_volume(parts: &[&ItemMesh]) -> f64 {
    let key = |p: [f64; 3]| p.map(|v| (v * 1e7).round() as i64);
    let mut edges: HashMap<([i64; 3], [i64; 3]), i32> = HashMap::new();
    let mut volume = 0.;
    for mesh in parts {
        assert_eq!(mesh.positions.len(), mesh.normals.len(), "{}", mesh.id);
        assert_eq!(mesh.positions.len(), mesh.uvs.len(), "{}", mesh.id);
        for normal in &mesh.normals {
            close(length(*normal), 1., 1e-9);
        }
        for corners in triangles(mesh) {
            let [a, b, c] = corners.map(|i| mesh.positions[i]);
            let facing = cross(sub(b, a), sub(c, a));
            assert!(length(facing) > 1e-14, "{}: degenerate triangle", mesh.id);
            for corner in corners {
                assert!(
                    dot(facing, mesh.normals[corner]) > 0.,
                    "{}: a vertex normal opposes its triangle",
                    mesh.id
                );
            }
            volume += dot(a, cross(b, c)) / 6.;
            for (from, to) in [(a, b), (b, c), (c, a)] {
                *edges.entry((key(from), key(to))).or_default() += 1;
            }
        }
    }
    for ((from, to), count) in &edges {
        assert_eq!(*count, 1, "edge {from:?} to {to:?} is used {count} times");
        assert_eq!(
            edges.get(&(*to, *from)),
            Some(&1),
            "edge {from:?} to {to:?} has no opposite: the surface is open"
        );
    }
    assert!(volume > 0., "the surface winds inward");
    volume
}
fn attachment(generated: &GeneratedItem, name: &str) -> ([f64; 3], [f64; 3]) {
    let found = generated
        .attachments
        .iter()
        .find(|a| a.name == name)
        .unwrap_or_else(|| panic!("no attachment {name}"));
    (found.position, found.direction)
}
fn part_key<'a>(generated: &'a GeneratedItem, id: &str) -> &'a str {
    &generated
        .geometry
        .parts
        .iter()
        .find(|part| part.id == id)
        .unwrap()
        .geometry_key
}
fn item(parts: Value) -> String {
    json!({"format":"itemspec","version":2,"id":"test","parts":parts}).to_string()
}

// ---- Step 1: parts and derived values -------------------------------------------------

/// Keys and serialized geometry captured from the version 1 implementation before
/// version 2 existed (2026-11-16).
#[test]
fn version_1_definitions_keep_their_geometry_keys_and_meshes() {
    for (name, generator, key, geometry, whole) in [
        (
            "pipe",
            json!({"kind":"pipe","length":{"parameter":"length"},"radius":0.5,"wallThickness":0.1,"segments":16,"capped":true}),
            "84d736c6bb2e53adacfcb710b4e31a92902d13418f2993dc6bceaadb3b560843",
            "902f3c746dab7b152de406e37803f664d511cfeae18ad71a9c24395b808fec2e",
            "0f1580e1fbaa56215f4c0b1c34d3b3391394db1f8cf3cf3713cb794e61f0f757",
        ),
        (
            "duct",
            json!({"kind":"duct","length":{"parameter":"length"},"width":4.,"height":1.,"wallThickness":0.1,"capped":true}),
            "4d465e9bbdfe6c069ab903125fa2b02a393033a8d93f98bdf400a1ebfe8e1711",
            "5f2dda024dcfd95fc5217ad5f7f1095414cf87448fe1f9409ce3131a840924e0",
            "aef1a53f527125d7c90a406dfd2bffdf1bf4fdd66b232e66c999eb709f1c915d",
        ),
    ] {
        let value = json!({"format":"itemspec","version":1,"id":name,"parameters":{
            "length":{"type":"number","role":"geometry","default":2.,"min":0.01,"max":100.}},
            "generator":generator,
            "transform":[[1., 0., 0.], [0., 1., 0.], [0., 0., 1.], [3., 4., 5.]]});
        let definition = item::parse("golden.item.json", &value.to_string()).unwrap();
        let generated = item::generate(&definition, &Values::new(), &absent, &|| false).unwrap();
        assert_eq!(generated.geometry_key, key);
        assert_eq!(
            format!(
                "{:x}",
                Sha256::digest(serde_json::to_vec(&*generated.geometry).unwrap())
            ),
            geometry
        );
        assert_eq!(
            format!(
                "{:x}",
                Sha256::digest(serde_json::to_vec(&generated).unwrap())
            ),
            whole
        );
        assert_eq!(generated.geometry.meshes[0].material_slot, "surface");
        assert_eq!(generated.attachments[0].name, "start");
    }
    // Version 1 does not accept version 2 structure, and the reverse.
    let mixed = json!({"format":"itemspec","version":1,"id":"x","derived":{"a":"1"},
        "generator":{"kind":"pipe","length":1.,"radius":0.1}});
    assert_eq!(
        item::parse("x", &mixed.to_string()).unwrap_err().code,
        "ITEM_VERSION"
    );
    let rooted = json!({"format":"itemspec","version":2,"id":"x",
        "generator":{"kind":"pipe","length":1.,"radius":0.1}});
    assert_eq!(
        item::parse("x", &rooted.to_string()).unwrap_err().code,
        "ITEM_PART"
    );
    let boxed = json!({"format":"itemspec","version":1,"id":"x",
        "generator":{"kind":"box","size":[1.,1.,1.]}});
    assert_eq!(
        item::parse("x", &boxed.to_string()).unwrap_err().code,
        "ITEM_PART"
    );
}

#[test]
fn expressions_match_hand_computed_values() {
    let evaluate = |text: &str| {
        Expression::parse(text)
            .unwrap()
            .evaluate(&mut |name, _| match name {
                "height" => Ok(2.6),
                "width" => Ok(4.),
                _ => panic!("unexpected name {name}"),
            })
            .unwrap()
    };
    assert_eq!(evaluate("min(1.1, height - 1.0)"), 1.1);
    assert_eq!(evaluate("2 + 3 * 4 - 6 / 4"), 12.5);
    assert_eq!(evaluate("8 / 4 / 2"), 1.);
    assert_eq!(evaluate("2 - 3 - 4"), -5.);
    assert_eq!(evaluate("-(1.5 - 2) * 2e1"), 10.);
    assert_eq!(evaluate("--2"), 2.);
    assert_eq!(
        evaluate("round(2.5) + floor(-1.5) + ceil(0.2) + abs(-3)"),
        5.
    );
    assert_eq!(evaluate("max(1, 2, 7.5, 3)"), 7.5);
    assert_eq!(evaluate("floor(width / 1.5) * (width - 1)"), 6.);
    for (text, offset) in [
        ("2 +", 3),
        ("2 $ 3", 2),
        ("min(1)", 0),
        ("sqrt(4)", 0),
        ("(1 + 2", 6),
        ("1.2.3", 0),
    ] {
        assert_eq!(
            Expression::parse(text).unwrap_err().offset,
            offset,
            "{text}"
        );
    }
    assert!(
        Expression::parse("1 / (2 - 2)")
            .unwrap()
            .evaluate(&mut |_, _| Ok(0.))
            .unwrap_err()
            .message
            .contains("zero")
    );
}

fn boxes(derived: Value, size: Value) -> String {
    json!({"format":"itemspec","version":2,"id":"boxes",
        "parameters":{
            "width":{"type":"number","default":2.,"min":0.5,"max":8.},
            "depth":{"type":"number","default":0.5,"min":0.1,"max":2.},
            "gloss":{"type":"number","role":"finish","default":0.5,"min":0.,"max":1.}},
        "derived":derived,
        "parts":[
            {"id":"a","generator":{"kind":"box","size":size}},
            {"id":"b","generator":{"kind":"box","size":[1.,{"parameter":"depth"},0.25]},
             "placement":{"position":[0.,0.,{"derived":"top"}]},
             "slots":{"side":"paint","top":"paint"}}],
        "slots":{"paint":{"entry":"materials/paint.material.ts","parameters":{"gloss":{"parameter":"gloss"}}}}})
    .to_string()
}

#[test]
fn derived_values_drive_parts_and_errors_point_at_their_expression() {
    let text = boxes(
        json!({"half":"width / 2","top":"max(0.75, half - 0.5) * 2"}),
        json!([{"parameter":"width"}, 0.5, {"derived":"half"}]),
    );
    let definition = item::parse("boxes.item.json", &text).unwrap();
    let prepared = item::prepare(&definition, &Values::new(), &absent, &|| false).unwrap();
    assert_eq!(prepared.derived["half"], 1.);
    assert_eq!(prepared.derived["top"], 1.5);
    let wide = item::prepare(&definition, &values(&[("width", 6.)]), &absent, &|| false).unwrap();
    assert_eq!(wide.derived["half"], 3.);
    assert_eq!(wide.derived["top"], 5.);
    // The washroom pipe spaces its couplings from its height: (2.3 - 0.1) / 4.
    let pipe = item::parse("pipe.item.json", PIPE).unwrap();
    let prepared = item::prepare(&pipe, &Values::new(), &absent, &|| false).unwrap();
    close(prepared.derived["spacing"], 0.55, 1e-12);
    close(prepared.derived["third"], 1.65, 1e-12);

    for (derived, size, code, pointer, fragment) in [
        (
            json!({"top":"1","a":"b + 1","b":"a * 2"}),
            json!([1., 1., 1.]),
            "ITEM_EXPRESSION",
            "/derived/b",
            "depends on itself",
        ),
        (
            json!({"top":"width + nope"}),
            json!([1., 1., 1.]),
            "ITEM_EXPRESSION",
            "/derived/top",
            "Unknown name nope. (character 9)",
        ),
        (
            json!({"top":"gloss * 2"}),
            json!([1., 1., 1.]),
            "ITEM_EXPRESSION",
            "/derived/top",
            "finish control gloss",
        ),
        (
            json!({"top":"1 / (width - width)"}),
            json!([1., 1., 1.]),
            "ITEM_EXPRESSION",
            "/derived/top",
            "Division by zero. (character 3)",
        ),
        (
            json!({"top":"2 * (width"}),
            json!([1., 1., 1.]),
            "ITEM_EXPRESSION",
            "/derived/top",
            "Expected ')'",
        ),
        (
            json!({"top":"1","width":"2"}),
            json!([1., 1., 1.]),
            "ITEM_EXPRESSION",
            "/derived/width",
            "differs from every parameter",
        ),
    ] {
        let text = boxes(derived, size);
        let error = item::parse("boxes.item.json", &text).unwrap_err();
        assert_eq!(error.code, code, "{error}");
        assert_eq!(error.pointer, pointer, "{error}");
        assert!(error.message.contains(fragment), "{error}");
        assert_eq!(error.source, "boxes.item.json");
        // The diagnostic's range is the expression's own JSON string.
        let name = pointer.rsplit('/').next().unwrap();
        let expression = serde_json::from_str::<Value>(&text).unwrap()["derived"][name].to_string();
        let units: Vec<u16> = text.encode_utf16().collect();
        let range = &units[error.offset.unwrap()..error.end.unwrap()];
        assert_eq!(String::from_utf16(range).unwrap(), expression);
    }
    // A reference to a derived value that does not exist points at the field using it.
    let text = boxes(json!({"top":"1"}), json!([{"derived":"missing"}, 1., 1.]));
    let error = generate(&text, &[]).unwrap_err();
    assert_eq!(error.code, "ITEM_EXPRESSION");
    assert_eq!(error.pointer, "/parts/0/generator/size/0");
    // Geometry cannot read a finish control through a scalar either.
    let text = boxes(json!({"top":"1"}), json!([{"parameter":"gloss"}, 1., 1.]));
    assert_eq!(generate(&text, &[]).unwrap_err().code, "ITEM_PARAMETER");
}

#[test]
fn parts_emit_one_mesh_per_slotted_face_group_with_their_own_keys() {
    let text = boxes(
        json!({"half":"width / 2","top":"1.5"}),
        json!([{"parameter":"width"}, 0.5, {"derived":"half"}]),
    );
    let definition = item::parse("boxes.item.json", &text).unwrap();
    let cold = item::generate(&definition, &Values::new(), &absent, &|| false).unwrap();
    // Part `a` has no slot map: every face group gets a slot of its own name.
    // Part `b` maps two groups; its unmapped `bottom` group is not emitted.
    let ids: Vec<_> = cold
        .geometry
        .meshes
        .iter()
        .map(|m| (m.id.as_str(), m.material_slot.as_str(), m.indices.len() / 3))
        .collect();
    assert_eq!(
        ids,
        [
            ("a/bottom", "bottom", 2),
            ("a/side", "side", 8),
            ("a/top", "top", 2),
            ("b/side", "paint", 8),
            ("b/top", "paint", 2)
        ]
    );
    close(
        closed_volume(&meshes(&cold, &["a/bottom", "a/side", "a/top"])),
        2. * 0.5 * 1.,
        1e-12,
    );
    // Part `b` is 1 x 0.5 x 0.25 and sits at z = 1.5.
    assert_eq!(cold.geometry.bounds, [[0., 0., 0.], [2., 0.5, 1.75]]);
    assert_eq!(cold.geometry.nodes.len(), 2);
    assert_eq!(cold.geometry.nodes[1].id, "part/b");
    assert_eq!(cold.geometry.nodes[1].transform[3], [0., 0., 1.5]);
    assert_eq!(cold.geometry.anchors.len(), 2);
    assert_eq!(cold.geometry.anchors[1].origin, [0., 0., 1.5]);
    // Side texture coordinates are metric: perimeter by height.
    let side = meshes(&cold, &["a/side"])[0];
    let reach = side
        .uvs
        .iter()
        .fold([0f64; 2], |m, uv| [m[0].max(uv[0]), m[1].max(uv[1])]);
    assert_eq!(reach, [5., 1.]);

    // A finish edit changes no key and shares the geometry.
    let finish =
        item::prepare(&definition, &values(&[("gloss", 0.9)]), &absent, &|| false).unwrap();
    assert_eq!(finish.geometry_key, cold.geometry_key);
    let (warm, built) = finish.rebuild(&cold, &|| false).unwrap();
    assert!(built.is_empty());
    assert!(Arc::ptr_eq(&warm.geometry, &cold.geometry));
    assert_eq!(warm.materials["paint"].parameters["gloss"], 0.9);

    // `width` is read by part `a` only; `depth` by part `b` only.
    let wide = item::prepare(&definition, &values(&[("width", 3.)]), &absent, &|| false).unwrap();
    assert_ne!(wide.geometry_key, cold.geometry_key);
    assert_eq!(
        wide.part_keys[1],
        ("b".to_owned(), part_key(&cold, "b").to_owned())
    );
    assert_ne!(wide.part_keys[0].1, part_key(&cold, "a"));
    assert_eq!(wide.reuse(&cold).unwrap_err().code, "ITEM_IDENTITY");
    let (resized, built) = wide.rebuild(&cold, &|| false).unwrap();
    assert_eq!(built, ["a"]);
    assert_eq!(meshes(&resized, &["b/side"]), meshes(&cold, &["b/side"]));
    assert_eq!(resized.geometry.bounds[1], [3., 0.5, 1.75]);
    // A rebuild equals a cold build of the same inputs.
    assert_eq!(
        serde_json::to_value(&resized).unwrap(),
        serde_json::to_value(wide.build(&|| false).unwrap()).unwrap()
    );
    let deep = item::prepare(&definition, &values(&[("depth", 1.)]), &absent, &|| false).unwrap();
    assert_eq!(deep.rebuild(&cold, &|| false).unwrap().1, ["b"]);

    // Structure errors carry a pointer to the part.
    for (parts, code, pointer) in [
        (
            json!([{"id":"a","generator":{"kind":"box","size":[1.,1.,1.]},"slots":{"front":"x"}}]),
            "ITEM_PART",
            "/parts/0/slots/front",
        ),
        (
            json!([{"id":"a","generator":{"kind":"box","size":[1.,1.,1.]}},
                   {"id":"a","generator":{"kind":"box","size":[1.,1.,1.]}}]),
            "ITEM_PART",
            "/parts/1/id",
        ),
        (
            json!([{"id":"a","generator":{"kind":"glb","asset":"a.glb"}}]),
            "ITEM_PART",
            "/parts/0/generator",
        ),
        (
            json!([{"id":"a","generator":{"kind":"box","size":[1.,-1.,1.]}}]),
            "ITEM_GEOMETRY",
            "/parts/0/generator/size/1",
        ),
        (json!([]), "ITEM_LIMIT", "/parts"),
    ] {
        let error = generate(&item(parts), &[]).unwrap_err();
        assert_eq!(
            (error.code.as_str(), error.pointer.as_str()),
            (code, pointer)
        );
    }
    // Straight version 1 generators work as parts and name their ends after the part.
    let straight = generate(
        &item(json!([
            {"id":"stub","generator":{"kind":"pipe","length":1.5,"radius":0.1,"segments":8},
             "placement":{"position":[1.,0.,0.],"rotation":[0.,90.,0.]}}])),
        &[],
    )
    .unwrap();
    assert_eq!(straight.geometry.meshes[0].id, "stub/surface");
    close3(attachment(&straight, "stub/end").0, [2.5, 0., 0.], 1e-12);
    close3(attachment(&straight, "stub/end").1, [1., 0., 0.], 1e-12);
    close3(straight.geometry.bounds[1], [2.5, 0.1, 0.1], 1e-9);
}

// ---- Step 2: routes and fittings ------------------------------------------------------

fn route(generator: Value) -> String {
    let mut generator = generator;
    generator["kind"] = json!("route");
    item(json!([{"id":"run","generator":generator}]))
}
fn bent(radius: f64, wall: f64) -> String {
    route(
        json!({"section":{"kind":"round","radius":0.05},"wallThickness":wall,
        "points":[[0.,0.,0.],[2.,0.,0.],[2.,0.,1.5]],
        "bendRadius":radius,"segments":12,"bendSegments":6}),
    )
}

#[test]
fn route_centreline_length_is_its_straights_plus_its_arcs() {
    let generated = generate(&bent(0.25, 0.), &[]).unwrap();
    // A right-angle fillet of radius R replaces 2R of straight run with a quarter circle.
    let centreline = 2. + 1.5 - 2. * 0.25 + 0.25 * PI / 2.;
    let body = meshes(&generated, &["run/body"])[0];
    let stations: Vec<f64> = body.uvs.iter().map(|uv| uv[1]).collect();
    close(
        stations.iter().cloned().fold(0., f64::max),
        centreline,
        1e-9,
    );
    close(stations.iter().cloned().fold(1., f64::min), 0., 0.);
    // Around the section the coordinate is the perimeter: 2 pi r.
    close(
        body.uvs.iter().map(|uv| uv[0]).fold(0., f64::max),
        2. * PI * 0.05,
        1e-12,
    );
    // Rings: start, end of the first run, six bend steps, end. Twelve quads between each pair.
    assert_eq!(body.indices.len() / 3, 8 * 12 * 2);
    assert_eq!(
        meshes(&generated, &["run/end"])[0].indices.len() / 3,
        2 * 10
    );
    close3(attachment(&generated, "run/start").0, [0., 0., 0.], 0.);
    close3(attachment(&generated, "run/start").1, [-1., 0., 0.], 0.);
    close3(attachment(&generated, "run/end").0, [2., 0., 1.5], 1e-12);
    close3(attachment(&generated, "run/end").1, [0., 0., 1.], 1e-12);
    // On the first straight run every body vertex is one radius from the X axis, and
    // every vertex of the bend is one radius from its circle of radius 0.25 about (1.75, 0, 0.25).
    for (position, normal) in body.positions.iter().zip(&body.normals) {
        if position[0] <= 1.75 {
            close(position[1].hypot(position[2]), 0.05, 1e-12);
            close(normal[0], 0., 1e-12);
        } else if position[2] < 0.25 {
            let ring = (position[0] - 1.75).hypot(position[2] - 0.25) - 0.25;
            close(ring.hypot(position[1]), 0.05, 1e-12);
        }
    }
    // A closed solid whose volume is the 12-gon section swept along the centreline.
    // The six-step bend cuts the corner slightly, so allow 2 %.
    let section = 12. / 2. * 0.05 * 0.05 * (2. * PI / 12.).sin();
    let volume = closed_volume(&meshes(&generated, &["run/body", "run/end"]));
    close(volume, section * centreline, 0.02 * section * centreline);
    assert!(volume < section * centreline);
    // A hollow route is closed too: outer wall, inner wall and two annular ends.
    let hollow = generate(&bent(0.25, 0.01), &[]).unwrap();
    let bore = 12. / 2. * 0.04 * 0.04 * (2. * PI / 12.).sin();
    close(
        closed_volume(&meshes(&hollow, &["run/body", "run/end"])),
        (section - bore) * centreline,
        0.02 * (section - bore) * centreline,
    );
    assert_eq!(
        meshes(&hollow, &["run/body"])[0].indices.len() / 3,
        2 * 8 * 12 * 2
    );
    // Features: both ends, and the anchor at the start.
    let ends: Vec<_> = generated
        .geometry
        .features
        .iter()
        .filter(|f| f.name == "end")
        .collect();
    assert_eq!(ends.len(), 2);
    assert_eq!(ends[1].points.len(), 12);
    for point in &ends[1].points {
        close(point[2], 1.5, 1e-12);
        close((point[0] - 2.).hypot(point[1]), 0.05, 1e-12);
    }
    let anchor = &generated.geometry.anchors[0];
    assert_eq!((anchor.origin, anchor.v), ([0.; 3], [1., 0., 0.]));
}

#[test]
fn a_bend_that_does_not_fit_fails_with_item_route_and_recovers() {
    // Two right-angle bends of radius 0.4 need 0.8 m of the 0.6 m middle run.
    let tight = |radius: f64| {
        route(json!({"section":{"kind":"round","radius":0.05},
            "points":[[0.,0.,0.],[1.,0.,0.],[1.,0.6,0.],[2.,0.6,0.]],"bendRadius":radius}))
    };
    let error = generate(&tight(0.4), &[]).unwrap_err();
    assert_eq!(error.code, "ITEM_ROUTE");
    assert_eq!(error.pointer, "/parts/0/generator/points/2");
    assert!(
        error.message.contains("0.8000 m of a 0.6000 m run"),
        "{error}"
    );
    // 0.3 fits exactly: the two arcs meet in the middle of the run.
    let fitted = generate(&tight(0.3), &[]).unwrap();
    let length = 1. + 0.6 + 1. - 4. * 0.3 + 2. * 0.3 * PI / 2.;
    let body = meshes(&fitted, &["run/body"])[0];
    close(
        body.uvs.iter().map(|uv| uv[1]).fold(0., f64::max),
        length,
        1e-9,
    );
    closed_volume(&meshes(&fitted, &["run/body", "run/end"]));
    for (text, pointer) in [
        // The radius must exceed the section's half extent, or the inside of the bend folds.
        (bent(0.05, 0.), "/parts/0/generator/bendRadius"),
        (
            route(
                json!({"section":{"kind":"round","radius":0.05},"bendRadius":0.2,
                "points":[[0.,0.,0.],[1.,0.,0.],[0.5,0.,0.]]}),
            ),
            "/parts/0/generator/points/1",
        ),
        (
            route(
                json!({"section":{"kind":"round","radius":0.05},"bendRadius":0.2,
                "points":[[0.,0.,0.],[0.,0.,0.]]}),
            ),
            "/parts/0/generator/points/1",
        ),
        (
            route(
                json!({"section":{"kind":"rect","width":0.2,"height":0.1},"wallThickness":0.05,
                "points":[[0.,0.,0.],[1.,0.,0.]]}),
            ),
            "/parts/0/generator/wallThickness",
        ),
        (
            route(
                json!({"section":{"kind":"rect","width":0.2,"height":0.1},"up":[1.,0.,0.],
                "points":[[0.,0.,0.],[1.,0.,0.]]}),
            ),
            "/parts/0/generator/up",
        ),
    ] {
        let error = generate(&text, &[]).unwrap_err();
        assert_eq!(
            (error.code.as_str(), error.pointer.as_str()),
            ("ITEM_ROUTE", pointer)
        );
    }
}

fn hangers(length: f64) -> GeneratedItem {
    generate(
        &route(json!({"section":{"kind":"rect","width":0.3,"height":0.2},"wallThickness":0.005,
            "points":[[0.,0.,2.],[length,0.,2.]],
            "fittings":[
                {"kind":"bracket","id":"hanger","at":{"every":1.,"inset":0.2},"reach":0.5,"direction":[0.,0.,1.]},
                {"kind":"flange","id":"seam","at":{"fraction":0.4},"overhang":0.02,"thickness":0.02,"bolts":8}]})),
        &[],
    )
    .unwrap()
}

#[test]
fn repeated_brackets_follow_the_every_formula_and_respace_on_resize() {
    for length in [4.07, 5.3, 1.3] {
        let generated = hangers(length);
        // Even spacing no larger than `every`, inside the insets.
        let available = length - 2. * 0.2;
        let intervals = (available / 1.).ceil();
        let spacing = available / intervals;
        assert!(spacing <= 1.);
        let contacts: Vec<_> = generated
            .geometry
            .features
            .iter()
            .filter(|f| f.name == "bracket-contact")
            .collect();
        assert_eq!(contacts.len(), intervals as usize + 1, "length {length}");
        for (index, contact) in contacts.iter().enumerate() {
            let x = 0.2 + index as f64 * spacing;
            let name = format!("run/hanger-{}", index + 1);
            close3(attachment(&generated, &name).0, [x, 0., 2.], 1e-12);
            assert_eq!(contact.source.as_deref(), Some(&name[4..]));
            // The contact is the base plate, 0.5 m above the centreline, facing back down.
            let centre = contact
                .points
                .iter()
                .fold([0.; 3], |sum, p| add(sum, scale(*p, 0.25)));
            close3(centre, [x, 0., 2.5], 1e-12);
            close3(contact.normal, [0., 0., -1.], 1e-12);
        }
        // The seam keeps its station at four tenths of the length.
        close3(
            attachment(&generated, "run/seam").0,
            [0.4 * length, 0., 2.],
            1e-12,
        );
        let joints = generated
            .geometry
            .features
            .iter()
            .filter(|f| f.name == "joint")
            .count();
        assert_eq!(joints, contacts.len() + 1);
        closed_volume(&meshes(&generated, &["run/bracket"]));
        closed_volume(&meshes(&generated, &["run/body", "run/end"]));
        // Eight hexagonal bolts: two four-triangle caps and six side quads each.
        assert_eq!(
            meshes(&generated, &["run/bolt"])[0].indices.len() / 3,
            8 * 20
        );
        closed_volume(&meshes(&generated, &["run/bolt"]));
        // The flange is a 0.34 x 0.24 collar around the 0.3 x 0.2 section, 0.02 long.
        close(
            closed_volume(&meshes(&generated, &["run/fitting"])),
            (0.34 * 0.24 - 0.3 * 0.2) * 0.02,
            1e-12,
        );
    }
}

fn branched(height: f64) -> GeneratedItem {
    let text = json!({"format":"itemspec","version":2,"id":"branched",
        "parameters":{"height":{"type":"number","default":2.,"min":1.,"max":6.}},
        "parts":[
            {"id":"riser","generator":{"kind":"route","section":{"kind":"round","radius":0.05},
                "wallThickness":0.005,"segments":12,
                "points":[[0.,0.,0.],[0.,0.,{"parameter":"height"}]],
                "fittings":[
                    {"kind":"tee","id":"tee-1","at":{"fraction":0.5},"direction":[1.,0.,0.2],"length":0.3},
                    {"kind":"valve","id":"shutoff","at":{"from":"start","offset":0.3},"length":0.16,"handle":"lever"},
                    {"kind":"coupling","id":"top","at":{"from":"end","offset":0.2},"length":0.08,"clearance":0.01}]}},
            {"id":"branch","generator":{"kind":"route","section":{"kind":"round","radius":0.05},
                "segments":12,"from":{"attachment":"riser/tee-1/branch"},
                "points":[[0.,0.,0.],[0.5,0.,0.],[0.8,0.,0.]],
                "fittings":[{"kind":"valve","id":"tap","at":{"point":1},"length":0.1},
                            {"kind":"coupling","id":"mid","at":{"from":"center","offset":0.},"length":0.1,"clearance":0.01}]}},
            {"id":"plinth","generator":{"kind":"box","size":[0.4,0.4,0.05]},
             "placement":{"position":[-0.2,-0.2,-0.05]}}]})
    .to_string();
    generate(&text, &[("height", height)]).unwrap()
}

#[test]
fn a_branch_follows_its_tee_when_the_riser_is_resized() {
    let (short, tall) = (branched(2.), branched(3.));
    for (generated, height) in [(&short, 2.), (&tall, 3.)] {
        // The tee sits at half the riser; its direction is flattened against the route.
        let (position, direction) = attachment(generated, "riser/tee-1/branch");
        close3(position, [0.3, 0., height / 2.], 1e-12);
        close3(direction, [1., 0., 0.], 1e-12);
        close3(attachment(generated, "branch/start").0, position, 0.);
        close3(
            attachment(generated, "branch/end").0,
            [1.1, 0., height / 2.],
            1e-12,
        );
        let body = meshes(generated, &["branch/body"])[0];
        let low = body.positions.iter().map(|p| p[2]).fold(f64::MAX, f64::min);
        close(low, height / 2. - 0.05, 1e-12);
        // Fittings keep their stations: 0.3 m from the start, 0.2 m from the end.
        close3(
            attachment(generated, "riser/shutoff").0,
            [0., 0., 0.3],
            1e-12,
        );
        close3(
            attachment(generated, "riser/top").0,
            [0., 0., height - 0.2],
            1e-12,
        );
        close3(
            attachment(generated, "branch/mid").0,
            [0.7, 0., height / 2.],
            1e-12,
        );
        // `point` places a fitting at a route point: here the second, 0.5 m along.
        close3(
            attachment(generated, "branch/tap").0,
            [0.8, 0., height / 2.],
            1e-12,
        );
        closed_volume(&meshes(generated, &["riser/body", "riser/end"]));
        closed_volume(&meshes(generated, &["riser/fitting"]));
    }
    // The branch reads the tee's position, so it and the riser change; the plinth does not.
    assert_ne!(part_key(&short, "riser"), part_key(&tall, "riser"));
    assert_ne!(part_key(&short, "branch"), part_key(&tall, "branch"));
    assert_eq!(part_key(&short, "plinth"), part_key(&tall, "plinth"));
    // A route cannot start from an attachment that no earlier part provides.
    let text = serde_json::to_string(&json!({"format":"itemspec","version":2,"id":"x","parts":[
        {"id":"late","generator":{"kind":"route","section":{"kind":"round","radius":0.05},
            "from":{"attachment":"riser/end"},"points":[[0.,0.,0.],[1.,0.,0.]]}}]}))
    .unwrap();
    let error = generate(&text, &[]).unwrap_err();
    assert_eq!(
        (error.code.as_str(), error.pointer.as_str()),
        ("ITEM_ROUTE", "/parts/0/generator/from")
    );
}

#[test]
fn fittings_must_sit_on_a_straight_run_and_be_well_formed() {
    let with = |fitting: Value| {
        route(
            json!({"section":{"kind":"round","radius":0.05},"bendRadius":0.25,
            "points":[[0.,0.,0.],[2.,0.,0.],[2.,0.,1.5]],"fittings":[fitting]}),
        )
    };
    // The bend runs from station 1.75 to 1.75 + 0.25 pi / 2 = 2.1427.
    assert!(generate(&with(json!({"kind":"coupling","id":"c","at":{"from":"start","offset":1.7},"length":0.1,"clearance":0.01})), &[]).is_ok());
    for (fitting, pointer) in [
        (
            json!({"kind":"coupling","id":"c","at":{"from":"start","offset":1.72},"length":0.1,"clearance":0.01}),
            "/parts/0/generator/fittings/0",
        ),
        (
            json!({"kind":"coupling","id":"c","at":{"point":1},"length":0.1,"clearance":0.01}),
            "/parts/0/generator/fittings/0",
        ),
        (
            json!({"kind":"coupling","id":"c","at":{"from":"start","offset":0.},"length":0.1,"clearance":0.01}),
            "/parts/0/generator/fittings/0",
        ),
        (
            json!({"kind":"flange","id":"f","at":{"fraction":0.2},"radius":0.04,"thickness":0.02}),
            "/parts/0/generator/fittings/0/radius",
        ),
        (
            json!({"kind":"flange","id":"f","at":{"fraction":0.2},"radius":0.1,"overhang":0.05,"thickness":0.02}),
            "/parts/0/generator/fittings/0",
        ),
        (
            json!({"kind":"flange","id":"f","at":{"fraction":0.2},"radius":0.1,"thickness":0.02,"bolts":2.5}),
            "/parts/0/generator/fittings/0/bolts",
        ),
        (
            json!({"kind":"bracket","id":"b","at":{"fraction":0.2},"reach":0.3,"direction":[1.,0.,0.]}),
            "/parts/0/generator/fittings/0/direction",
        ),
        (
            json!({"kind":"bracket","id":"b","at":{"fraction":0.2},"reach":0.05,"direction":[0.,0.,1.]}),
            "/parts/0/generator/fittings/0/reach",
        ),
        (
            json!({"kind":"tee","id":"t","at":{"fraction":0.2},"direction":[0.,1.,0.],"length":0.04}),
            "/parts/0/generator/fittings/0/length",
        ),
        (
            json!({"kind":"coupling","id":"start","at":{"fraction":0.2},"length":0.1,"clearance":0.01}),
            "/parts/0/generator/fittings/0/id",
        ),
        (
            json!({"kind":"coupling","id":"c","at":{"every":0.,"inset":0.1},"length":0.1,"clearance":0.01}),
            "/parts/0/generator/fittings/0/at",
        ),
    ] {
        let error = generate(&with(fitting.clone()), &[]).unwrap_err();
        assert_eq!(
            (error.code.as_str(), error.pointer.as_str()),
            ("ITEM_FITTING", pointer),
            "{fitting}"
        );
    }
}

// ---- Step 3: panels and openings ------------------------------------------------------

#[test]
fn wall_volume_is_the_panel_minus_its_opening_and_reveals_carry_their_group() {
    let wall = generate(WALL, &[]).unwrap();
    let slab = closed_volume(&meshes(
        &wall,
        &["wall/front", "wall/back", "wall/edge", "wall/reveal"],
    ));
    // 2.7 x 2.6 x 0.25 minus a 0.9 x 0.7 window through the thickness.
    close(slab, 2.7 * 2.6 * 0.25 - 0.9 * 0.7 * 0.25, 1e-12);
    // The same solid from the boolean kernel (which stores f32).
    let cut = Solid::cube([0.9, 0.7, 1.])
        .unwrap()
        .transform(Matrix::translation([1.2, 1.1, -0.5]))
        .unwrap();
    let kernel = Solid::cube([2.7, 2.6, 0.25])
        .unwrap()
        .boolean(&cut, Operation::Difference)
        .unwrap();
    close(slab, kernel.volume(), 1e-5);
    // The window is 0.6 m from the far end and its sill is min(1.1, height - 1) up.
    let reveal = meshes(&wall, &["wall/reveal"])[0];
    assert_eq!(reveal.material_slot, "concrete");
    close(area(reveal), 2. * (0.9 + 0.7) * 0.25, 1e-12);
    for (position, normal) in reveal.positions.iter().zip(&reveal.normals) {
        let [x, y, z] = *position;
        let on_side = (x - 1.2).abs() < 1e-12 || (x - 2.1).abs() < 1e-12;
        let on_level = (z - 1.1).abs() < 1e-12 || (z - 1.8).abs() < 1e-12;
        assert!(on_side || on_level, "{position:?} is not on the opening");
        assert!((0. ..=0.25).contains(&y));
        // Reveal normals face into the opening, whose centre is (1.65, 1.45).
        assert!(dot(*normal, [1.65 - x, 0., 1.45 - z]) > 0.);
        assert_eq!(normal[1], 0.);
    }
    for (id, facing) in [("wall/front", -1.), ("wall/back", 1.)] {
        let face = meshes(&wall, &[id])[0];
        close(area(face), 2.7 * 2.6 - 0.9 * 0.7, 1e-12);
        assert!(face.normals.iter().all(|n| *n == [0., facing, 0.]));
        // No triangle of a face covers the opening.
        for [a, b, c] in triangles(face) {
            let centre = scale(
                add(add(face.positions[a], face.positions[b]), face.positions[c]),
                1. / 3.,
            );
            assert!(!(centre[0] > 1.2 && centre[0] < 2.1 && centre[2] > 1.1 && centre[2] < 1.8));
        }
    }
    // The front is mapped in meters from its bottom-left corner; the back is mirrored
    // so it reads the same way round from behind.
    let front = meshes(&wall, &["wall/front"])[0];
    assert!(
        front
            .positions
            .iter()
            .zip(&front.uvs)
            .all(|(p, uv)| *uv == [p[0], p[2]])
    );
    let back = meshes(&wall, &["wall/back"])[0];
    assert!(
        back.positions
            .iter()
            .zip(&back.uvs)
            .all(|(p, uv)| (uv[0] - (2.7 - p[0])).abs() < 1e-12)
    );
    close(
        area(meshes(&wall, &["wall/edge"])[0]),
        2. * (2.7 + 2.6) * 0.25,
        1e-12,
    );

    // Frame: the 0.9 x 0.7 ring with a 0.05 border, 0.08 deep, centred in the wall.
    let frame = meshes(&wall, &["wall/frame"]);
    assert_eq!(frame[0].material_slot, "frame");
    close(closed_volume(&frame), (0.9 * 0.7 - 0.8 * 0.6) * 0.08, 1e-12);
    let depth: Vec<f64> = frame[0].positions.iter().map(|p| p[1]).collect();
    close(
        depth.iter().cloned().fold(f64::MAX, f64::min),
        0.125 - 0.04,
        1e-12,
    );
    close(
        depth.iter().cloned().fold(f64::MIN, f64::max),
        0.125 + 0.04,
        1e-12,
    );
    // Glazing fills the frame; the sill is a T: a 0.96 x 0.03 nose and 0.9 x 0.06 inside, 0.03 thick.
    let glazing = meshes(&wall, &["wall/glazing"]);
    assert_eq!(glazing[0].material_slot, "glass");
    close(closed_volume(&glazing), 0.8 * 0.6 * 0.006, 1e-12);
    close(
        closed_volume(&meshes(&wall, &["wall/sill"])),
        (0.96 * 0.03 + 0.9 * 0.06) * 0.03,
        1e-12,
    );
    let sill = meshes(&wall, &["wall/sill"])[0];
    close(
        sill.positions.iter().map(|p| p[1]).fold(f64::MAX, f64::min),
        -0.03,
        0.,
    );
    close(
        sill.positions.iter().map(|p| p[2]).fold(f64::MAX, f64::min),
        1.1,
        0.,
    );
    assert_eq!(wall.geometry.bounds, [[0., -0.03, 0.], [2.7, 0.25, 2.6]]);
}

#[test]
fn an_arched_recess_removes_its_depth_only() {
    let wall = generate(WALL_LOW, &[]).unwrap();
    let slab = closed_volume(&meshes(
        &wall,
        &["wall/front", "wall/back", "wall/edge", "wall/reveal"],
    ));
    // A 0.5 wide, 0.6 tall arch: a 0.5 x 0.35 rectangle under a half disc of radius 0.25
    // drawn with 16 chords, 0.12 deep.
    let arch = 0.5 * 0.35 + 16. / 2. * 0.25 * 0.25 * (PI / 16.).sin();
    close(slab, 2.7 * 1.2 * 0.25 - arch * 0.12, 1e-12);
    // The back is untouched and the recess floor belongs to the reveal.
    close(area(meshes(&wall, &["wall/back"])[0]), 2.7 * 1.2, 1e-12);
    close(
        area(meshes(&wall, &["wall/front"])[0]),
        2.7 * 1.2 - arch,
        1e-12,
    );
    let reveal = meshes(&wall, &["wall/reveal"])[0];
    let floor: f64 = triangles(reveal)
        .filter(|t| t.iter().all(|i| reveal.normals[*i] == [0., -1., 0.]))
        .map(|[a, b, c]| {
            assert!([a, b, c].iter().all(|i| reveal.positions[*i][1] == 0.12));
            length(cross(
                sub(reveal.positions[b], reveal.positions[a]),
                sub(reveal.positions[c], reveal.positions[a]),
            )) / 2.
        })
        .sum();
    close(floor, arch, 1e-12);
    // Centred: the arch spans u 1.1 to 1.6 and v 0.35 to 0.95.
    let edge = wall
        .geometry
        .features
        .iter()
        .find(|f| f.name == "opening-edge")
        .unwrap();
    assert!(edge.closed && edge.face == "front" && edge.source.as_deref() == Some("niche"));
    let (mut low, mut high) = ([f64::MAX; 3], [f64::MIN; 3]);
    for point in &edge.points {
        for axis in 0..3 {
            low[axis] = low[axis].min(point[axis]);
            high[axis] = high[axis].max(point[axis]);
        }
    }
    close3(low, [1.1, 0., 0.35], 1e-12);
    close3(high, [1.6, 0., 0.95], 1e-12);
    // A recess has no edge on the back face.
    assert_eq!(
        wall.geometry
            .features
            .iter()
            .filter(|f| f.name == "opening-edge")
            .count(),
        1
    );
}

#[test]
fn panel_features_and_openings_follow_their_rules_through_a_resize() {
    let feature = |wall: &GeneratedItem, name: &str, face: &str| {
        wall.geometry
            .features
            .iter()
            .find(|f| f.name == name && f.face == face)
            .unwrap_or_else(|| panic!("no {name} on {face}"))
            .clone()
    };
    for (width, height) in [(2.7, 2.6), (4.2, 2.6), (2.7, 2.0), (1.7, 5.)] {
        let wall = generate(WALL, &[("width", width), ("height", height)]).unwrap();
        // u: 0.6 m from the end. v: min(1.1, height - 1) from the start.
        let (left, sill) = (width - 0.6 - 0.9, f64::min(1.1, height - 1.));
        assert_eq!(
            feature(&wall, "base", "front").points,
            [[0., 0., 0.], [width, 0., 0.]]
        );
        assert_eq!(
            feature(&wall, "top", "front").points,
            [[0., 0., height], [width, 0., height]]
        );
        assert_eq!(
            feature(&wall, "top", "back").points,
            [[0., 0.25, height], [width, 0.25, height]]
        );
        assert_eq!(feature(&wall, "base", "back").normal, [0., 1., 0.]);
        let edge = feature(&wall, "opening-edge", "front");
        assert_eq!(edge.source.as_deref(), Some("window"));
        assert_eq!(edge.points.len(), 4);
        close3(edge.points[0], [left, 0., sill], 1e-12);
        close3(edge.points[2], [left + 0.9, 0., sill + 0.7], 1e-12);
        // The sill line is the nose of the sill on the front and the opening's edge behind.
        let front = feature(&wall, "sill", "front");
        close3(front.points[0], [left - 0.03, 0., sill], 1e-12);
        close3(front.points[1], [left + 0.93, 0., sill], 1e-12);
        close3(
            feature(&wall, "sill", "back").points[0],
            [left, 0.25, sill],
            1e-12,
        );
        assert_eq!(front.normal, [0., -1., 0.]);
        // The anchor stays on the bottom-left corner, so the existing wall keeps its mapping.
        let anchor = &wall.geometry.anchors[0];
        assert_eq!(
            (anchor.origin, anchor.u, anchor.v, anchor.normal),
            ([0.; 3], [1., 0., 0.], [0., 0., 1.], [0., -1., 0.])
        );
        close(
            closed_volume(&meshes(
                &wall,
                &["wall/front", "wall/back", "wall/edge", "wall/reveal"],
            )),
            (width * height - 0.9 * 0.7) * 0.25,
            1e-11,
        );
    }
    // The item transform carries features, anchors and attachments with the meshes.
    let mut moved: Value = serde_json::from_str(WALL).unwrap();
    moved["transform"] = json!([[0., 1., 0.], [-1., 0., 0.], [0., 0., 1.], [10., 20., 30.]]);
    let wall = generate(&moved.to_string(), &[]).unwrap();
    close3(
        feature(&wall, "base", "front").points[1],
        [10., 22.7, 30.],
        1e-12,
    );
    close3(feature(&wall, "base", "front").normal, [1., 0., 0.], 1e-12);
    close3(wall.geometry.anchors[0].origin, [10., 20., 30.], 0.);
    close3(wall.geometry.anchors[0].u, [0., 1., 0.], 0.);
    close3(wall.geometry.bounds[0], [9.75, 20., 30.], 1e-12);
}

fn panel(openings: Value) -> String {
    item(
        json!([{"id":"wall","generator":{"kind":"panel","width":3.,"height":2.,"thickness":0.2,"openings":openings}}]),
    )
}

#[test]
fn openings_that_overlap_or_leave_the_panel_fail_with_item_opening() {
    let window = |id: &str, u: f64, v: f64| json!({"id":id,"width":0.8,"height":0.6,"u":{"from":"start","offset":u},"v":{"from":"start","offset":v}});
    // Side by side with the minimum 0.02 m between them is accepted.
    let pair = generate(
        &panel(json!([window("a", 0.5, 0.5), window("b", 1.32, 0.5)])),
        &[],
    )
    .unwrap();
    close(
        closed_volume(&meshes(
            &pair,
            &["wall/front", "wall/back", "wall/edge", "wall/reveal"],
        )),
        (3. * 2. - 2. * 0.8 * 0.6) * 0.2,
        1e-12,
    );
    let mut arch = window("a", 0.5, 0.5);
    arch["shape"] = json!("arch");
    arch["height"] = json!(0.3);
    let mut deep = window("a", 0.5, 0.5);
    deep["mode"] = json!("recess");
    deep["depth"] = json!(0.2);
    let mut framed = window("a", 0.5, 0.5);
    framed["frame"] = json!({"border":0.3,"depth":0.05});
    let mut repeated = window("a", 0.5, 0.5);
    repeated["u"] = json!({"every":0.5});
    for (openings, pointer, fragment) in [
        (
            json!([window("a", 0.5, 0.5), window("b", 1.2, 0.9)]),
            "/parts/0/generator/openings/1",
            "overlaps opening a",
        ),
        (
            json!([window("a", 0.5, 0.5), window("b", 1.31, 0.5)]),
            "/parts/0/generator/openings/1",
            "less than 0.02 m",
        ),
        (
            json!([window("a", 2.3, 0.5)]),
            "/parts/0/generator/openings/0",
            "inside the 3 by 2 m panel",
        ),
        (
            json!([window("a", 0.5, 1.39)]),
            "/parts/0/generator/openings/0",
            "inside",
        ),
        (
            json!([window("a", 0.01, 0.5)]),
            "/parts/0/generator/openings/0",
            "inside",
        ),
        (
            json!([window("a", 0.5, 0.5), window("a", 1.5, 0.5)]),
            "/parts/0/generator/openings/1/id",
            "unique",
        ),
        (
            json!([arch]),
            "/parts/0/generator/openings/0/height",
            "taller than half",
        ),
        (
            json!([deep]),
            "/parts/0/generator/openings/0/depth",
            "shallower",
        ),
        (
            json!([framed]),
            "/parts/0/generator/openings/0/frame/border",
            "no opening",
        ),
        (
            json!([repeated]),
            "/parts/0/generator/openings/0/u",
            "for fittings",
        ),
    ] {
        let text = panel(openings);
        let mut error = generate(&text, &[]).unwrap_err();
        assert_eq!(
            (error.code.as_str(), error.pointer.as_str()),
            ("ITEM_OPENING", pointer)
        );
        assert!(error.message.contains(fragment), "{error}");
        item::locate_diagnostic(&text, &mut error);
        assert!(error.offset.is_some() && error.end > error.offset);
    }
    // A resize that pushes an opening out fails; the previous result is still usable.
    let definition = item::parse("wall.item.json", WALL).unwrap();
    let good = item::generate(&definition, &Values::new(), &absent, &|| false).unwrap();
    let mut narrow: Value = serde_json::from_str(WALL).unwrap();
    narrow["parameters"]["width"]["min"] = json!(1.);
    let error = generate(&narrow.to_string(), &[("width", 1.4)]).unwrap_err();
    assert_eq!(error.code, "ITEM_OPENING");
    assert!(error.message.contains("window"), "{error}");
    assert_eq!(good.geometry.parts[0].id, "wall");
}

// ---- The washroom's shapes as items ---------------------------------------------------

#[test]
fn the_washroom_pipe_keeps_fittings_at_their_stations_when_lengthened() {
    for height in [2.3, 3.5] {
        let pipe = generate(PIPE, &[("height", height)]).unwrap();
        // The riser stands 0.14 m off the wall and turns into it with a 0.1 m bend.
        let centreline = height + 0.14 - 2. * 0.1 + 0.1 * PI / 2.;
        let body = meshes(&pipe, &["riser/body"])[0];
        close(
            body.uvs.iter().map(|uv| uv[1]).fold(0., f64::max),
            centreline,
            1e-9,
        );
        closed_volume(&meshes(&pipe, &["riser/body", "riser/end"]));
        // Flanges of radius 0.13 at the floor and the wall; the pipe's own radius is 0.09.
        // Their bolts run through: heads are a quarter of the 0.04 overhang across and
        // stand 0.6 of that proud on both faces, so 0.006 into the floor and the wall.
        close3(pipe.geometry.bounds[0], [-0.006, 2.12 - 0.13, -0.006], 1e-9);
        close3(
            pipe.geometry.bounds[1],
            [0.14 + 0.13, 2.12 + 0.13, height + 0.13],
            1e-9,
        );
        close3(attachment(&pipe, "riser/end").0, [0., 2.12, height], 1e-12);
        close3(attachment(&pipe, "riser/end").1, [-1., 0., 0.], 1e-12);
        close3(
            attachment(&pipe, "riser/floor").0,
            [0.14, 2.12, 0.015],
            1e-12,
        );
        close3(
            attachment(&pipe, "riser/wall").0,
            [0.015, 2.12, height],
            1e-9,
        );
        for (index, name) in ["riser/coupling-1", "riser/coupling-2", "riser/coupling-3"]
            .iter()
            .enumerate()
        {
            let z = (index + 1) as f64 * (height - 0.1) / 4.;
            close3(attachment(&pipe, name).0, [0.14, 2.12, z], 1e-12);
        }
        // Five joints for rust to follow, each a 24-point ring of the pipe's radius.
        let joints: Vec<_> = pipe
            .geometry
            .features
            .iter()
            .filter(|f| f.name == "joint")
            .collect();
        assert_eq!(joints.len(), 5);
        let coupling = joints
            .iter()
            .find(|f| f.source.as_deref() == Some("coupling-2"))
            .unwrap();
        assert_eq!(
            (coupling.points.len(), coupling.closed, coupling.width),
            (24, true, 0.07)
        );
        for point in &coupling.points {
            close(point[2], (height - 0.1) / 2., 1e-12);
            close((point[0] - 0.14).hypot(point[1] - 2.12), 0.09, 1e-12);
        }
        // Two flanges and three couplings: annular caps of 48 triangles and two 48-triangle
        // walls each. Twelve bolts of 20 triangles.
        assert_eq!(
            meshes(&pipe, &["riser/fitting"])[0].indices.len() / 3,
            5 * 4 * 48
        );
        assert_eq!(meshes(&pipe, &["riser/bolt"])[0].indices.len() / 3, 12 * 20);
        let ring = |outer: f64, inner: f64, long: f64| {
            12. * (2. * PI / 24.).sin() * (outer * outer - inner * inner) * long
        };
        close(
            closed_volume(&meshes(&pipe, &["riser/fitting"])),
            2. * ring(0.13, 0.09, 0.03) + 3. * ring(0.108, 0.09, 0.07),
            1e-12,
        );
        closed_volume(&meshes(&pipe, &["riser/bolt"]));
        assert!(
            pipe.geometry
                .meshes
                .iter()
                .all(|mesh| mesh.material_slot == "steel")
        );
    }
    // A standoff shorter than the bend and flange need is a route error, not a bad mesh.
    let mut close_in: Value = serde_json::from_str(PIPE).unwrap();
    close_in["parameters"]["standoff"]["min"] = json!(0.05);
    let error = generate(&close_in.to_string(), &[("standoff", 0.08)]).unwrap_err();
    assert_eq!(error.code, "ITEM_ROUTE");
}

#[test]
fn the_washroom_duct_and_vent_build_as_one_item() {
    let duct = generate(DUCT, &[]).unwrap();
    assert_eq!(duct.geometry.parts.len(), 11);
    // A 1.67 m run, a 0.3 m bend down and 0.92 m of drop.
    let centreline = 1.67 + 0.92 - 2. * 0.3 + 0.3 * PI / 2.;
    let body = meshes(&duct, &["duct/body"])[0];
    close(
        body.uvs.iter().map(|uv| uv[1]).fold(0., f64::max),
        centreline,
        1e-9,
    );
    // The section's perimeter is 2 (0.34 + 0.3); the inner wall's is 0.032 shorter.
    close(
        body.uvs.iter().map(|uv| uv[0]).fold(0., f64::max),
        1.28,
        1e-12,
    );
    closed_volume(&meshes(&duct, &["duct/body", "duct/end"]));
    // Five seams, each a 0.012 collar 0.024 long around the 0.34 x 0.3 section.
    close(
        closed_volume(&meshes(&duct, &["duct/fitting"])),
        5. * (0.364 * 0.324 - 0.34 * 0.3) * 0.024,
        1e-12,
    );
    closed_volume(&meshes(&duct, &["duct/bracket"]));
    // Along the wall the section is 0.34 wide in x and 0.3 tall in z; after the bend it
    // hangs 0.3 deep in y. The collars add 0.012 each way.
    let (low, high) = (duct.geometry.bounds[0], duct.geometry.bounds[1]);
    close3(low, [0., -0.17 - 0.15 - 0.012, 1.2], 1e-9);
    close(high[1], 1.5 + 0.45, 1e-9);
    close(high[2], 1.92 + 0.4, 1e-9);
    // The slats lean out of the louver frame at x = 0.436.
    assert!(high[0] > 0.46 && high[0] < 0.47, "{high:?}");
    let seam = attachment(&duct, "duct/seam-4");
    close3(seam.0, [0.21, -0.17, 1.8], 1e-9);
    close3(seam.1, [0., 0., -1.], 1e-9);
    // Brackets reach the wall plane x = 0.
    let contacts: Vec<_> = duct
        .geometry
        .features
        .iter()
        .filter(|f| f.name == "bracket-contact")
        .collect();
    assert_eq!(contacts.len(), 2);
    assert!(
        contacts
            .iter()
            .all(|c| c.points.iter().all(|p| p[0].abs() < 1e-12))
    );
    close3(contacts[0].normal, [1., 0., 0.], 1e-12);
    // Lengthening the run moves the vent with the duct's start and keeps the bend's seam.
    let long = generate(DUCT, &[("run", 2.5)]).unwrap();
    close(long.geometry.bounds[1][1], 2.5 - 0.17 + 0.45, 1e-9);
    close3(attachment(&long, "duct/seam-4").0, [0.21, -0.17, 1.8], 1e-9);
    close3(
        attachment(&long, "duct/seam-1").0,
        [0.21, 2.5 - 0.17 - 0.012, 2.12],
        1e-9,
    );
    // Slats and louver frame read `ventY` and move; nothing else about them changes.
    assert_ne!(part_key(&duct, "slat-3"), part_key(&long, "slat-3"));
    assert_eq!(
        meshes(&duct, &["slat-3/side"])[0].positions,
        meshes(&long, &["slat-3/side"])[0].positions
    );
}

// ---- Bevels ---------------------------------------------------------------------------

fn solid(generator: Value) -> Result<GeneratedItem, ItemDiagnostic> {
    let text = json!({
        "format": "itemspec", "version": 2, "id": "test",
        "parameters": {"plan": {"type": "points", "default": [[0, 0], [4, 0], [4, 2], [2, 2], [2, 3], [0, 3]]}},
        "parts": [{"id": "s", "generator": generator}]
    })
    .to_string();
    generate(&text, &[])
}
fn groups(generated: &GeneratedItem) -> Vec<(&str, &str)> {
    generated
        .geometry
        .meshes
        .iter()
        .map(|m| (m.id.as_str(), m.material_slot.as_str()))
        .collect()
}
fn triangle_count(generated: &GeneratedItem) -> usize {
    generated
        .geometry
        .meshes
        .iter()
        .map(|m| m.indices.len() / 3)
        .sum()
}
/// The volume between two parallel sections `height` apart whose area is quadratic in between
/// (Simpson's rule is exact for it): a chamfer's frustum.
fn frustum(height: f64, area: impl Fn(f64) -> f64, reach: f64) -> f64 {
    height / 6. * (area(0.) + 4. * area(reach / 2.) + area(reach))
}

#[test]
fn a_bevel_rounds_a_box_inside_the_same_bounds_and_face_groups() {
    let (a, b, c, r) = (0.6, 0.3, 0.2, 0.02);
    let sharp = solid(json!({"kind": "box", "size": [a, b, c]})).unwrap();
    // No radius, however it is written, is the box as it was: the same key and meshes.
    let zero =
        solid(json!({"kind": "box", "size": [a, b, c], "bevel": 0, "bevelSegments": 2})).unwrap();
    assert_eq!(part_key(&zero, "s"), part_key(&sharp, "s"));
    assert_eq!(zero.geometry_key, sharp.geometry_key);
    assert_eq!(
        serde_json::to_vec(&zero.geometry.meshes).unwrap(),
        serde_json::to_vec(&sharp.geometry.meshes).unwrap()
    );
    let all = ["s/bottom", "s/side", "s/top"];
    let mut volumes = vec![];
    for segments in [1, 2] {
        let round =
            solid(json!({"kind": "box", "size": [a, b, c], "bevel": r, "bevelSegments": segments}))
                .unwrap();
        assert_ne!(part_key(&round, "s"), part_key(&sharp, "s"));
        assert_eq!(round.geometry.bounds, sharp.geometry.bounds);
        assert_eq!(groups(&round), groups(&sharp));
        assert_eq!(round.geometry.anchors, sharp.geometry.anchors);
        // Cheap: 60 triangles with one chord, 140 with two, against 12.
        assert_eq!(triangle_count(&sharp), 12);
        assert_eq!(triangle_count(&round), [60, 140][segments - 1]);
        // Closed, wound outward, with no degenerate triangle.
        volumes.push(closed_volume(&meshes(&round, &all)));
        // The caps are the faces drawn in by the radius, flat and facing up and down.
        let top = meshes(&round, &["s/top"])[0];
        assert!(top.normals.iter().all(|n| *n == [0., 0., 1.]));
        assert!(top.positions.iter().all(|p| p[2] == c
            && p[0] >= r - 1e-12
            && p[0] <= a - r + 1e-12
            && p[1] >= r - 1e-12
            && p[1] <= b - r + 1e-12));
        // The rims belong to the sides and carry on their mapping: v runs on past the wall's
        // height by the rim's arc at the top and before zero at the bottom.
        let side = meshes(&round, &["s/side"])[0];
        let v = side.uvs.iter().fold([f64::MAX, f64::MIN], |m, uv| {
            [m[0].min(uv[1]), m[1].max(uv[1])]
        });
        close(v[0], r - r * PI / 2., 1e-12);
        close(v[1], c - r + r * PI / 2., 1e-12);
    }
    // One chord is a chamfer: an octagonal belt between two frusta. An octagon drawn in by d
    // keeps (a - 2d)(b - 2d) less four corner triangles with legs r - d(2 - sqrt 2).
    let octagon = |d: f64| (a - 2. * d) * (b - 2. * d) - 2. * (r - d * (2. - 2f64.sqrt())).powi(2);
    close(
        volumes[0],
        octagon(0.) * (c - 2. * r) + 2. * frustum(r, octagon, r),
        1e-12,
    );
    // Two chords stand outside the chamfer and inside the box.
    assert!(volumes[1] > volumes[0] && volumes[1] < a * b * c);

    // Along one axis only the four edges that run that way are rounded: the section loses
    // four corner triangles of r^2 / 2 and the ends stay flat, in 28 triangles.
    for (axis, section, length) in [("x", b * c, a), ("y", a * c, b), ("z", a * b, c)] {
        let bar = solid(json!({"kind": "box", "size": [a, b, c], "bevel": r, "bevelAxis": axis}))
            .unwrap();
        assert_eq!(bar.geometry.bounds, sharp.geometry.bounds);
        assert_eq!(groups(&bar), groups(&sharp));
        assert_eq!(triangle_count(&bar), 28);
        close(
            closed_volume(&meshes(&bar, &all)),
            (section - 2. * r * r) * length,
            1e-12,
        );
        // Top and bottom keep their plan mapping, and the sides their distance round the box.
        let top = meshes(&bar, &["s/top"])[0];
        assert!(
            top.positions
                .iter()
                .zip(&top.uvs)
                .all(|(p, uv)| p[2] == c && *uv == [p[0], p[1]])
        );
        let side = meshes(&bar, &["s/side"])[0];
        let reach = side
            .uvs
            .iter()
            .fold([0f64; 2], |m, uv| [m[0].max(uv[0]), m[1].max(uv[1])]);
        if axis != "z" {
            close(reach[0], 2. * (a + b), 1e-12);
            close(reach[1], c - r + r * PI / 2., 1e-12);
            // The front face reads x by z, as a sharp box's does.
            let front: Vec<_> = (0..side.positions.len())
                .filter(|i| side.normals[*i] == [0., -1., 0.])
                .map(|i| (side.positions[i], side.uvs[i]))
                .collect();
            assert!(front.len() >= 4);
            assert!(front.iter().all(|(p, uv)| *uv == [p[0], p[2]]));
        }
    }

    // A radius that leaves no face, and a third chord, are refused at their field.
    for (bevel, segments, pointer) in [
        (0.1, 1, "/parts/0/generator/bevel"),
        (0.02, 3, "/parts/0/generator/bevelSegments"),
    ] {
        let error = solid(
            json!({"kind": "box", "size": [a, b, c], "bevel": bevel, "bevelSegments": segments}),
        )
        .unwrap_err();
        assert_eq!(
            (error.code.as_str(), error.pointer.as_str()),
            ("ITEM_GEOMETRY", pointer)
        );
    }
}

#[test]
fn a_bevel_rounds_the_rims_of_a_footprint_prism() {
    // An L of area 10 and perimeter 14 with five square corners and one re-entrant.
    let (h, r) = (0.3, 0.05);
    let prism = |bevel: f64| {
        solid(
            json!({"kind": "prism", "footprint": {"points": "plan"}, "height": h, "bevel": bevel}),
        )
        .unwrap()
    };
    let (sharp, round) = (prism(0.), prism(r));
    let plain =
        solid(json!({"kind": "prism", "footprint": {"points": "plan"}, "height": h})).unwrap();
    assert_eq!(part_key(&sharp, "s"), part_key(&plain, "s"));
    assert_eq!(round.geometry.bounds, sharp.geometry.bounds);
    assert_eq!(groups(&round), groups(&sharp));
    assert!(triangle_count(&round) > triangle_count(&sharp));
    let all = ["s/bottom", "s/side", "s/top"];
    close(closed_volume(&meshes(&sharp, &all)), 10. * h, 1e-12);
    // Drawn in by d the L keeps 10 - 14 d + (5 - 1) d^2.
    let area = |d: f64| 10. - 14. * d + 4. * d * d;
    close(
        closed_volume(&meshes(&round, &all)),
        10. * (h - 2. * r) + 2. * frustum(r, area, r),
        1e-12,
    );
    // The 2 m edge between two square corners cannot lose more than a meter at each end.
    let error =
        solid(json!({"kind": "prism", "footprint": {"points": "plan"}, "height": 3, "bevel": 1.1}))
            .err()
            .map(|error| (error.code, error.pointer));
    assert_eq!(
        error,
        Some(("ITEM_GEOMETRY".into(), "/parts/0/generator/bevel".into()))
    );
}

#[test]
fn a_bevel_rounds_a_panel_front_and_its_openings_and_keeps_its_features() {
    let (w, h, t, r) = (2., 1.5, 0.2, 0.015);
    let (ow, oh) = (0.6, 0.5);
    let panel = |bevel: Value| {
        solid(
            json!({"kind": "panel", "width": w, "height": h, "thickness": t, "bevel": bevel,
            "openings": [{"id": "window", "width": ow, "height": oh,
                "u": {"from": "start", "offset": 0.5}, "v": {"from": "start", "offset": 0.4}}]}),
        )
    };
    let (sharp, round) = (panel(json!(0)).unwrap(), panel(json!(r)).unwrap());
    assert_eq!(round.geometry.bounds, sharp.geometry.bounds);
    assert_eq!(groups(&round), groups(&sharp));
    // Base, top, opening edge and sill are reported where they were, and so is the anchor.
    assert_eq!(round.geometry.features, sharp.geometry.features);
    assert_eq!(round.geometry.anchors, sharp.geometry.anchors);
    assert!(triangle_count(&round) > triangle_count(&sharp));
    assert!(triangle_count(&round) <= triangle_count(&sharp) + 16);
    let all = ["s/front", "s/back", "s/edge", "s/reveal"];
    close(
        closed_volume(&meshes(&sharp, &all)),
        (w * h - ow * oh) * t,
        1e-12,
    );
    // The front shrinks by the radius and the opening grows by it, over the radius' depth.
    let face = |d: f64| (w - 2. * d) * (h - 2. * d) - (ow + 2. * d) * (oh + 2. * d);
    close(
        closed_volume(&meshes(&round, &all)),
        face(0.) * (t - r) + frustum(r, face, r),
        1e-12,
    );
    // The front stays one flat face with its metric mapping; the rims join edge and reveal.
    let front = meshes(&round, &["s/front"])[0];
    assert!(front.normals.iter().all(|n| *n == [0., -1., 0.]));
    assert!(
        front
            .positions
            .iter()
            .zip(&front.uvs)
            .all(|(p, uv)| p[1] == 0. && *uv == [p[0], p[2]])
    );
    close(area(front), face(r), 1e-12);
    // An opening nearer the edge than both rims need is refused.
    let error = solid(
        json!({"kind": "panel", "width": w, "height": h, "thickness": t, "bevel": r,
        "openings": [{"id": "window", "width": ow, "height": oh,
            "u": {"from": "start", "offset": 0.025}, "v": {"from": "start", "offset": 0.4}}]}),
    )
    .unwrap_err();
    assert_eq!(error.code, "ITEM_OPENING");
    // An arched recess is rounded too, and the solid stays closed.
    let niche = solid(
        json!({"kind": "panel", "width": w, "height": h, "thickness": t,
        "bevel": r, "bevelSegments": 2,
        "openings": [{"id": "niche", "shape": "arch", "mode": "recess", "depth": 0.08,
            "width": 0.5, "height": 0.6, "u": {"from": "center", "offset": 0},
            "v": {"from": "start", "offset": 0.35}}]}),
    )
    .unwrap();
    let volume = closed_volume(&meshes(&niche, &all));
    assert!(volume < w * h * t && volume > w * h * t - 0.5 * 0.6 * 0.08 - 0.01);
}

/// Every triangle has area and every vertex normal is a unit vector on its triangle's side.
fn sound(mesh: &ItemMesh) {
    assert_eq!(mesh.positions.len(), mesh.normals.len(), "{}", mesh.id);
    assert_eq!(mesh.positions.len(), mesh.uvs.len(), "{}", mesh.id);
    for corners in triangles(mesh) {
        let [a, b, c] = corners.map(|i| mesh.positions[i]);
        let facing = cross(sub(b, a), sub(c, a));
        assert!(length(facing) > 1e-12, "{}: degenerate triangle", mesh.id);
        for corner in corners {
            close(length(mesh.normals[corner]), 1., 1e-9);
            assert!(
                dot(facing, mesh.normals[corner]) > 0.,
                "{}: a vertex normal opposes its triangle",
                mesh.id
            );
        }
    }
}

#[test]
fn an_arc_sweeps_a_section_round_a_plan_arc_as_one_smooth_surface() {
    // A slab 0.25 m thick and 3 m high round a quarter turn of radius 0.55 m, in 8 chords.
    let (radius, t, h, n) = (0.55, 0.25, 3., 8usize);
    let slab = json!([[-t, 0], [0, 0], [0, h], [-t, h]]);
    let arc = |more: Value| {
        let mut generator =
            json!({"kind": "arc", "radius": radius, "turn": 90, "segments": n, "section": slab});
        generator
            .as_object_mut()
            .unwrap()
            .extend(more.as_object().unwrap().clone());
        solid(generator)
    };
    let built = arc(json!({})).unwrap();
    assert_eq!(groups(&built), [("s/end", "end"), ("s/side", "side")]);
    // It starts at the origin along +x and ends a radius on, running along +y.
    close3(built.geometry.bounds[0], [0.; 3], 1e-12);
    close3(built.geometry.bounds[1], [radius, radius, h], 1e-12);
    // In plan it is n wedges between two radii: n * sin(turn / n) / 2 * (R^2 - r^2).
    let step = PI / 2. / n as f64;
    let plan = n as f64 * step.sin() / 2. * (radius * radius - (radius - t) * (radius - t));
    close(
        closed_volume(&meshes(&built, &["s/side", "s/end"])),
        plan * h,
        1e-12,
    );
    // Four faces of n quads, each vertex named once along the arc; two caps.
    let side = meshes(&built, &["s/side"])[0];
    assert_eq!(side.indices.len() / 3, 4 * n * 2);
    assert_eq!(side.positions.len(), 4 * 2 * (n + 1));
    assert_eq!(triangle_count(&built), 4 * n * 2 + 2 * 2);
    // Its outer and inner faces are smooth: every normal there points along its vertex's own
    // radius about the axis at (0, R), outward on the outside and inward on the inside.
    let chord = 2. * radius * (step / 2.).sin();
    let mut outer = 0;
    for ((p, normal), uv) in side.positions.iter().zip(&side.normals).zip(&side.uvs) {
        if normal[2] != 0. {
            continue;
        }
        let spoke = [p[0], p[1] - radius];
        let reach = spoke[0].hypot(spoke[1]);
        let along = (normal[0] * spoke[0] + normal[1] * spoke[1]) / reach;
        let outside = (reach - radius).abs() < 1e-12;
        close(along, if outside { 1. } else { -1. }, 1e-12);
        if outside {
            outer += 1;
            // Pattern coordinates: the distance along the chords of the arc, and the height.
            let k = (p[0] / radius).clamp(-1., 1.).asin() / step;
            close(uv[0], k.round() * chord, 1e-9);
            close(uv[1], t + p[2], 1e-12);
        }
    }
    assert_eq!(outer, 2 * (n + 1));
    // A pattern anchor moves the coordinates, as a wall before the arc measures its own.
    let anchored = arc(json!({"patternAnchor": [-9.25, -3.35]})).unwrap();
    let u = meshes(&anchored, &["s/side"])[0]
        .uvs
        .iter()
        .fold([f64::MAX, f64::MIN], |m, uv| {
            [m[0].min(uv[0]), m[1].max(uv[0])]
        });
    close(u[0], 9.25, 1e-12);
    close(u[1], 9.25 + n as f64 * chord, 1e-12);
    assert_eq!(anchored.geometry.anchors[0].origin, [-9.25, 0., -3.35]);
    // `base` and `top` follow the arc at the section's foot and head; a ring where it is put.
    let ringed = arc(json!({"rings": [{"name": "eave-above", "at": [0, h + 0.3]}]})).unwrap();
    let names: Vec<&str> = ringed
        .geometry
        .features
        .iter()
        .map(|f| f.name.as_str())
        .collect();
    assert_eq!(names, ["base", "top", "eave-above", "start", "end"]);
    // `start` and `end` are the section where the arc begins and ends, facing along it.
    let end = &ringed.geometry.features[4];
    assert!(end.closed && end.points.len() == 4);
    close3(end.points[1], [radius, radius, 0.], 1e-12);
    close3(end.normal, [0., 1., 0.], 1e-12);
    close3(ringed.geometry.features[3].normal, [-1., 0., 0.], 1e-12);
    for (feature, (out, up)) in
        ringed
            .geometry
            .features
            .iter()
            .zip([(-t, 0.), (0., h), (0., h + 0.3)])
    {
        assert_eq!(feature.points.len(), n + 1);
        for p in &feature.points {
            close(p[0].hypot(p[1] - radius), radius + out, 1e-12);
            close(p[2], up, 1e-12);
        }
        close3(feature.normal, [0.5f64.sqrt(), -(0.5f64.sqrt()), 0.], 1e-12);
    }
    // An open section is one face group without ends: a wall's outer face, split into rows
    // no taller than a meter, is n by 3 quads of shared vertices.
    let face = solid(
        json!({"kind": "arc", "radius": radius, "turn": 90, "segments": n,
        "closed": false, "step": 1, "section": [[0, 0], [0, h]]}),
    )
    .unwrap();
    assert_eq!(groups(&face), [("s/side", "side")]);
    let wall = meshes(&face, &["s/side"])[0];
    sound(wall);
    assert_eq!(wall.indices.len() / 3, n * 3 * 2);
    assert_eq!(wall.positions.len(), (n + 1) * 4);
    close(area(wall), n as f64 * chord * h, 1e-12);
    // A bevel rounds the section's corners in one or two chords a corner, inside the solid.
    for (segments, strips) in [(1, 4), (2, 8)] {
        let round = arc(json!({"bevel": 0.02, "bevelSegments": segments})).unwrap();
        assert_eq!(round.geometry.bounds, built.geometry.bounds);
        assert_eq!(
            meshes(&round, &["s/side"])[0].indices.len() / 3,
            (4 + strips) * n * 2
        );
        let volume = closed_volume(&meshes(&round, &["s/side", "s/end"]));
        assert!(volume < plan * h && volume > plan * h - 4. * 0.02 * 0.02 * 2.);
    }
    // A turn, a section through the axis and a bevel longer than half an edge are refused.
    for (more, pointer) in [
        (json!({"turn": 0}), "/parts/0/generator/turn"),
        (json!({"radius": 0.2}), "/parts/0/generator/section"),
        (json!({"bevel": 0.13}), "/parts/0/generator/bevel"),
        (json!({"segments": 65}), "/parts/0/generator/segments"),
    ] {
        let error = arc(more).unwrap_err();
        assert_eq!(
            (error.code.as_str(), error.pointer.as_str()),
            ("ITEM_GEOMETRY", pointer)
        );
    }
}

#[test]
fn arrises_round_a_corner_between_two_panels_and_jambs_an_openings_sides() {
    let (w, h, t, r) = (2., 1.5, 0.2, 0.03);
    let half = 0.5f64.sqrt();
    let wall = |more: Value| {
        let mut generator = json!({"kind": "panel", "width": w, "height": h, "thickness": t});
        generator
            .as_object_mut()
            .unwrap()
            .extend(more.as_object().unwrap().clone());
        generator
    };
    let plain = solid(wall(json!({}))).unwrap();
    // A radius without a turn, or a turn without a radius, is the panel as it was.
    for more in [json!({"arris": r}), json!({"arrisStart": 1, "arrisEnd": 1})] {
        let same = solid(wall(more)).unwrap();
        assert_eq!(part_key(&same, "s"), part_key(&plain, "s"));
        assert_eq!(
            serde_json::to_vec(&same.geometry.meshes).unwrap(),
            serde_json::to_vec(&plain.geometry.meshes).unwrap()
        );
    }
    // Two walls meet at a square corner: the first ends where the second, turned a quarter
    // turn about that point, starts. Each rounds its own half of the corner in one chord.
    let corner = generate(
        &item(json!([
            {"id": "a", "generator": wall(json!({"arris": r, "arrisEnd": 1}))},
            {"id": "b", "generator": wall(json!({"arris": r, "arrisStart": 1})),
             "placement": {"position": [w, 0, 0], "rotation": [0, 0, 90]}}
        ])),
        &[],
    )
    .unwrap();
    close3(corner.geometry.bounds[0], [0.; 3], 1e-12);
    close3(corner.geometry.bounds[1], [w, w, h], 1e-12);
    let transforms = corner.geometry.world_transforms().unwrap();
    let placed = |part: &str, group: &str| {
        let (node, matrix) = corner
            .geometry
            .nodes
            .iter()
            .zip(&transforms)
            .find(|(node, _)| node.name.as_deref() == Some(part))
            .unwrap();
        let mesh = node
            .meshes
            .iter()
            .map(|index| &corner.geometry.meshes[*index])
            .find(|mesh| mesh.id == format!("{part}/{group}"))
            .unwrap();
        sound(mesh);
        (mesh, *matrix)
    };
    // The middle of the round, where the two halves meet with one normal: on the bisector,
    // r (sqrt 2 - 1) from the corner.
    let middle = [w - r * (1. - half), r * (1. - half)];
    for part in ["a", "b"] {
        let (front, matrix) = placed(part, "front");
        let turned: Vec<[f64; 3]> = front
            .positions
            .iter()
            .zip(&front.normals)
            .filter(|(_, n)| **n != [0., -1., 0.])
            .map(|(p, n)| {
                close3(matrix.direction(*n), [half, -half, 0.], 1e-12);
                matrix.point(*p)
            })
            .collect();
        assert_eq!(turned.len(), 2, "{part}");
        for p in turned {
            close(p[0], middle[0], 1e-12);
            close(p[1], middle[1], 1e-12);
        }
        // The flat front gives up the radius, and the chord takes its place.
        let chord = (r * half).hypot(r * (1. - half));
        close(area(front), (w - r + chord) * h, 1e-12);
        // The end face starts behind the round, and the top and bottom lose its corner.
        let (edge, _) = placed(part, "edge");
        let corner_cut = (1. - half) * r * r;
        close(
            area(edge),
            2. * (w * t - corner_cut) + h * t + h * (t - r),
            1e-12,
        );
    }
    // An opening keeps clear of a rounded end by the round and the margin.
    let window = |offset: f64, more: Value| {
        let mut opening = json!({"id": "window", "width": 0.6, "height": 0.5,
            "u": {"from": "start", "offset": offset}, "v": {"from": "start", "offset": 0.02}});
        opening
            .as_object_mut()
            .unwrap()
            .extend(more.as_object().unwrap().clone());
        opening
    };
    let error = solid(wall(
        json!({"arris": r, "arrisStart": 1, "openings": [window(0.04, json!({}))]}),
    ))
    .unwrap_err();
    assert_eq!(error.code, "ITEM_OPENING");
    assert!(
        solid(wall(
            json!({"arris": r, "arrisStart": 1, "openings": [window(0.05, json!({}))]})
        ))
        .is_ok()
    );
    // More round than the panel is thick, and a bevel beside an arris, are refused.
    for more in [
        json!({"arris": 0.25, "arrisEnd": 1}),
        json!({"arris": r, "arrisEnd": 1, "bevel": 0.01}),
    ] {
        let error = solid(wall(more)).unwrap_err();
        assert_eq!(
            (error.code.as_str(), error.pointer.as_str()),
            ("ITEM_GEOMETRY", "/parts/0/generator/arris")
        );
    }

    // Rounded jambs: an opening 2 cm above the panel's foot keeps its place; each upright
    // arris gives way to a quarter round in two chords, and the solid stays closed.
    let (ow, oh, j) = (0.6, 0.5, 0.03);
    let sharp = solid(wall(json!({"openings": [window(0.5, json!({}))]}))).unwrap();
    let round = solid(wall(
        json!({"openings": [window(0.5, json!({"jambs": j}))]}),
    ))
    .unwrap();
    assert_eq!(round.geometry.bounds, sharp.geometry.bounds);
    assert_eq!(groups(&round), groups(&sharp));
    assert_eq!(round.geometry.features, sharp.geometry.features);
    assert_eq!(triangle_count(&round), triangle_count(&sharp) + 2 * (4 + 4));
    let all = ["s/front", "s/back", "s/edge", "s/reveal"];
    // Each jamb loses its corner square less the two triangles the chords keep.
    close(
        closed_volume(&meshes(&round, &all)),
        (w * h - ow * oh) * t - 2. * oh * j * j * (1. - half),
        1e-12,
    );
    // The rounds are part of the front, turning to the reveals' normals.
    let front = meshes(&round, &["s/front"])[0];
    let turning: Vec<[f64; 3]> = front
        .normals
        .iter()
        .copied()
        .filter(|n| *n != [0., -1., 0.])
        .collect();
    assert!(turning.contains(&[1., 0., 0.]) && turning.contains(&[-1., 0., 0.]));
    assert!(turning.contains(&[half, -half, 0.]) && turning.contains(&[-half, -half, 0.]));
    // A jamb's round needs its radius of wall beside it; an arch has no upright arris to its head.
    let error = solid(wall(
        json!({"openings": [window(0.04, json!({"jambs": j}))]}),
    ))
    .unwrap_err();
    assert_eq!(error.code, "ITEM_OPENING");
    let error = solid(wall(
        json!({"openings": [window(0.5, json!({"jambs": j, "shape": "arch"}))]}),
    ))
    .unwrap_err();
    assert_eq!(
        (error.code.as_str(), error.pointer.as_str()),
        ("ITEM_OPENING", "/parts/0/generator/openings/0/jambs")
    );
}

#[test]
fn generation_is_deterministic_and_cancellable() {
    for text in [WALL, WALL_LOW, PIPE, DUCT] {
        let definition = item::parse("fixture.item.json", text).unwrap();
        let first = item::generate(&definition, &Values::new(), &absent, &|| false).unwrap();
        let second = item::generate(&definition, &Values::new(), &absent, &|| false).unwrap();
        assert_eq!(
            serde_json::to_vec(&first).unwrap(),
            serde_json::to_vec(&second).unwrap()
        );
        let positions: usize = first
            .geometry
            .meshes
            .iter()
            .map(|m| m.positions.len() * 24)
            .sum();
        assert!(first.retained_bytes() > positions);
        // Every emitted slot is bound or has a fallback, and every mesh belongs to a part.
        let owned: usize = first.geometry.parts.iter().map(|p| p.meshes.len()).sum();
        assert_eq!(owned, first.geometry.meshes.len());
        let slots: BTreeMap<_, _> = first
            .geometry
            .meshes
            .iter()
            .map(|m| (&m.material_slot, ()))
            .collect();
        assert!(
            slots
                .keys()
                .all(|slot| first.geometry.imported_materials.contains_key(*slot))
        );
        assert_eq!(
            item::generate(&definition, &Values::new(), &absent, &|| true)
                .unwrap_err()
                .code,
            "ITEM_CANCELLED"
        );
        let prepared = item::prepare(&definition, &Values::new(), &absent, &|| false).unwrap();
        assert_eq!(prepared.build(&|| true).unwrap_err().code, "ITEM_CANCELLED");
        assert_eq!(
            prepared.build(&|| false).unwrap().geometry_key,
            first.geometry_key
        );
        // The definition survives a serialize and parse round trip unchanged in meaning.
        let again = item::parse(
            "fixture.item.json",
            &serde_json::to_string(&definition).unwrap(),
        )
        .unwrap();
        let reparsed = item::generate(&again, &Values::new(), &absent, &|| false).unwrap();
        assert_eq!(reparsed.geometry_key, first.geometry_key);
    }
}

#[test]
fn a_seat_and_an_arc_recess_leave_no_wall_behind_what_fills_them() {
    // A seat 1 m by 0.5 m and 5 cm deep, 2 mm from the panel's corner, where a plain opening
    // must keep 2 cm.
    let seat = |mode: &str, u: f64| {
        json!({"id": "band", "mode": mode, "depth": 0.05, "seat": true, "width": 1., "height": 0.5,
               "u": {"from": "start", "offset": u}, "v": {"from": "start", "offset": 0.002}})
    };
    let built = generate(&panel(json!([seat("recess", 0.002)])), &[]).unwrap();
    close(
        closed_volume(&meshes(
            &built,
            &["wall/front", "wall/back", "wall/edge", "wall/reveal"],
        )),
        3. * 2. * 0.2 - 1. * 0.5 * 0.05,
        1e-12,
    );
    // No wall faces lie in front of the seat's floor, 5 cm back.
    let front = meshes(&built, &["wall/front"])[0];
    for [a, b, c] in triangles(front) {
        let middle = scale(
            add(
                add(front.positions[a], front.positions[b]),
                front.positions[c],
            ),
            1. / 3.,
        );
        assert!(
            !(middle[0] > 0.002 && middle[0] < 1.002 && middle[2] > 0.002 && middle[2] < 0.502),
            "{middle:?}"
        );
    }
    // It reports no opening edge or sill: what lies in it carries its own.
    assert!(
        built
            .geometry
            .features
            .iter()
            .all(|f| f.name != "opening-edge" && f.name != "sill"),
        "{:?}",
        built.geometry.features
    );
    // Without `seat`, the same recess must keep 2 cm from the edge; a seat cuts no hole.
    let mut plain = seat("recess", 0.002);
    plain["seat"] = json!(false);
    let mut through = seat("through", 0.5);
    through.as_object_mut().unwrap().remove("depth");
    for (openings, pointer) in [
        (json!([plain]), "/parts/0/generator/openings/0"),
        (json!([through]), "/parts/0/generator/openings/0/seat"),
    ] {
        let error = generate(&panel(openings), &[]).unwrap_err();
        assert_eq!(
            (error.code.as_str(), error.pointer.as_str()),
            ("ITEM_OPENING", pointer)
        );
    }

    // An arc's recess sets 0.5 m of its outer face 5 cm back, and the face above keeps `v`.
    let (radius, t, h, n) = (0.55, 0.25, 3., 8usize);
    let generator = |recesses: Value| {
        solid(
            json!({"kind": "arc", "radius": radius, "turn": 90, "segments": n,
                     "section": [[-t, 0], [0, 0], [0, h], [-t, h]], "recesses": recesses}),
        )
    };
    let recessed = generator(json!([{"from": 1, "to": 1.5, "depth": 0.05, "when": 1}])).unwrap();
    let wedge = |outer: f64, inner: f64| {
        n as f64 * (PI / 2. / n as f64).sin() / 2. * (outer * outer - inner * inner)
    };
    close(
        closed_volume(&meshes(&recessed, &["s/side", "s/end"])),
        wedge(radius, radius - t) * h - wedge(radius, radius - 0.05) * 0.5,
        1e-12,
    );
    let side = meshes(&recessed, &["s/side"])[0];
    let mut above = 0;
    for ((p, normal), uv) in side.positions.iter().zip(&side.normals).zip(&side.uvs) {
        let out = (p[0] * p[0] + (p[1] - radius) * (p[1] - radius)).sqrt() - radius;
        if normal[2] == 0. && out.abs() < 1e-9 && p[2] > 1.5 + 1e-9 {
            // The bottom (t long) and the face up to here, not the recess's floor and head.
            close(uv[1], t + p[2], 1e-9);
            above += 1;
        }
    }
    assert!(above > 0);
    // A recess reaching an open section's start starts the section at its back; one that
    // leaves its face fails.
    let open = solid(
        json!({"kind": "arc", "radius": radius, "turn": 90, "segments": n,
                            "closed": false, "section": [[0, 0], [0, h]],
                            "recesses": [{"from": 0, "to": 1, "depth": 0.05}]}),
    )
    .unwrap();
    close(open.geometry.bounds[0][2], 0., 1e-12);
    let error = generator(json!([{"from": 2.8, "to": 3.2, "depth": 0.05}])).unwrap_err();
    assert_eq!(
        (error.code.as_str(), error.pointer.as_str()),
        ("ITEM_GEOMETRY", "/parts/0/generator/recesses")
    );
}

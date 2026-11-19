//! Every Kowloon native generator draws valid, deterministic item meshes from its defaults,
//! varies with its seed when it reads one, and refuses parameters it does not know.
use serde_json::{Value, json};
use superspec_engine::{
    itemspec::definition::{ItemMesh, NativeGenerators, Values},
    worldspec::themes::kowloon::items::Natives,
};

fn values(entries: &[(&str, Value)]) -> Values {
    entries
        .iter()
        .map(|(key, value)| ((*key).to_owned(), value.clone()))
        .collect()
}

/// Every attribute of every mesh, bit for bit, so equality means identical output.
fn fingerprint(meshes: &[ItemMesh]) -> Vec<(String, String, Vec<u64>, Vec<u32>)> {
    meshes
        .iter()
        .map(|m| {
            let floats = m
                .positions
                .iter()
                .chain(&m.normals)
                .flatten()
                .chain(m.uvs.iter().flatten())
                .chain(m.colors.iter().flatten())
                .chain(m.tangents.iter().flatten())
                .chain(m.additional_uvs.values().flatten().flatten())
                .map(|v| v.to_bits())
                .collect();
            (
                m.id.clone(),
                m.material_slot.clone(),
                floats,
                m.indices.clone(),
            )
        })
        .collect()
}

fn generate(natives: &Natives, name: &str, parameters: &Values) -> Result<Vec<ItemMesh>, String> {
    natives
        .generate(name, parameters, &|| false)
        .map(|output| output.meshes)
}

fn check(name: &str, meshes: &[ItemMesh]) {
    assert!(!meshes.is_empty(), "{name} drew no meshes");
    for m in meshes {
        let count = m.positions.len();
        assert!(count > 0, "{name}/{} has no vertices", m.id);
        assert_eq!(m.normals.len(), count, "{name}/{} normals", m.id);
        assert_eq!(m.uvs.len(), count, "{name}/{} uvs", m.id);
        assert_eq!(m.colors.len(), count, "{name}/{} colors", m.id);
        assert!(
            m.tangents.is_empty() || m.tangents.len() == count,
            "{name}/{} tangents",
            m.id
        );
        assert!(
            !m.indices.is_empty() && m.indices.len() % 3 == 0,
            "{name}/{} indices form triangles",
            m.id
        );
        assert!(
            m.indices.iter().all(|i| (*i as usize) < count),
            "{name}/{} indices stay within its vertices",
            m.id
        );
        assert!(!m.material_slot.is_empty(), "{name}/{} slot", m.id);
        // A scene refuses a receiver with a triangle that has no area to map a material over.
        for corners in m.indices.chunks_exact(3) {
            let [a, b, c] = [0, 1, 2].map(|k| m.positions[corners[k] as usize]);
            let (u, v) = (
                [b[0] - a[0], b[1] - a[1], b[2] - a[2]],
                [c[0] - a[0], c[1] - a[1], c[2] - a[2]],
            );
            let cross = [
                u[1] * v[2] - u[2] * v[1],
                u[2] * v[0] - u[0] * v[2],
                u[0] * v[1] - u[1] * v[0],
            ];
            assert!(
                cross.iter().map(|x| x * x).sum::<f64>().sqrt() >= 1e-9,
                "{name}/{} has a triangle without area",
                m.id
            );
        }
        assert!(
            m.positions
                .iter()
                .flatten()
                .all(|v| v.is_finite() && v.abs() < 100.),
            "{name}/{} positions are finite and local",
            m.id
        );
        assert!(
            m.normals.iter().flatten().all(|v| v.is_finite())
                && m.colors.iter().flatten().all(|v| v.is_finite()),
            "{name}/{} normals and colors are finite",
            m.id
        );
        assert!(
            m.uvs.iter().flatten().all(|v| v.is_finite()),
            "{name}/{} uvs are finite",
            m.id
        );
    }
}

#[test]
fn every_generator_draws_deterministic_valid_meshes() {
    let natives = Natives::new();
    let names = natives.names();
    assert!(
        names.len() >= 40,
        "registry lists {} generators",
        names.len()
    );
    let empty = Values::new();
    let mut failures = vec![];
    for name in &names {
        assert!(
            natives
                .identity(name)
                .is_some_and(|id| id.starts_with(name.as_str())),
            "{name} has an identity"
        );
        let first = match generate(&natives, name, &empty) {
            Ok(meshes) => meshes,
            Err(error) => {
                failures.push(format!("{name}: {error}"));
                continue;
            }
        };
        check(name, &first);
        let second = generate(&natives, name, &empty).unwrap();
        assert!(
            fingerprint(&first) == fingerprint(&second),
            "{name} is deterministic"
        );
    }
    assert!(
        failures.is_empty(),
        "defaults failed:\n{}",
        failures.join("\n")
    );
}

#[test]
fn seeded_generators_vary_with_their_seed() {
    let natives = Natives::new();
    let mut seeded = 0;
    for name in natives.names() {
        // Only the tiled counter draws seeded tile shades; its default timber top does not.
        let fixed: &[(&str, Value)] = match name.as_str() {
            "kowloon/counter" => &[("finish", json!("tiled"))],
            _ => &[],
        };
        let seeded_values = |seed: Value| {
            let mut entries = fixed.to_vec();
            entries.push(("seed", seed));
            values(&entries)
        };
        let base = generate(&natives, &name, &seeded_values(json!("a")));
        let Ok(base) = base else {
            // Unseeded generators refuse the parameter like any other unknown one.
            assert!(
                base.unwrap_err().contains("unknown parameter seed"),
                "{name} refuses seed as unknown"
            );
            continue;
        };
        seeded += 1;
        let differs = (0..16).any(|n| {
            let other = generate(&natives, &name, &seeded_values(json!(n)))
                .unwrap_or_else(|e| panic!("{name} seed {n}: {e}"));
            fingerprint(&other) != fingerprint(&base)
        });
        assert!(differs, "{name} reads a seed but never varies with it");
    }
    assert!(seeded >= 20, "only {seeded} generators are seeded");
}

#[test]
fn unknown_parameters_and_bad_values_are_refused() {
    let natives = Natives::new();
    for name in natives.names() {
        let error = generate(&natives, &name, &values(&[("no-such-parameter", json!(1))]))
            .expect_err(&name);
        assert!(
            error.contains("unknown parameter no-such-parameter"),
            "{name}: {error}"
        );
    }
    assert!(
        natives
            .generate("kowloon/no-such-fixture", &Values::new(), &|| false)
            .is_err()
    );
    for (name, key, value) in [
        ("kowloon/window", "kind", json!("porthole")),
        ("kowloon/window", "frame", json!("green")),
        ("kowloon/window", "width", json!("wide")),
        ("kowloon/riser-duct", "height", json!(1)),
        ("kowloon/meter-bank", "count", json!(2.5)),
        ("kowloon/grocer-prop", "model", json!("piano")),
    ] {
        assert!(
            generate(&natives, name, &values(&[(key, value.clone())])).is_err(),
            "{name} refuses {key} = {value}"
        );
    }
}

#[test]
fn choices_and_dimensions_reach_the_fixture() {
    let natives = Natives::new();
    let extent = |meshes: &[ItemMesh], axis: usize| {
        let values = meshes
            .iter()
            .flat_map(|m| m.positions.iter().map(move |p| p[axis]));
        let (low, high) = values.fold((f64::MAX, f64::MIN), |(l, h), v| (l.min(v), h.max(v)));
        high - low
    };
    for kind in ["casement", "slider", "timber", "louver"] {
        let meshes = generate(
            &natives,
            "kowloon/window",
            &values(&[("kind", json!(kind)), ("width", json!(1.8))]),
        )
        .unwrap_or_else(|e| panic!("{kind}: {e}"));
        check(kind, &meshes);
        assert!(
            extent(&meshes, 0) >= 1.8 - 1e-6,
            "{kind} window spans its width"
        );
    }
    let short = generate(
        &natives,
        "kowloon/downpipe",
        &values(&[("height", json!(2))]),
    )
    .unwrap();
    let tall = generate(
        &natives,
        "kowloon/downpipe",
        &values(&[("height", json!(5))]),
    )
    .unwrap();
    assert!((extent(&tall, 2) - extent(&short, 2) - 3.).abs() < 0.05);
    // Wall fixtures stand in front of the wall plane, toward -Y (windows recess behind it).
    for name in [
        "kowloon/downpipe",
        "kowloon/pipe-elbow",
        "kowloon/sign",
        "kowloon/blade-sign",
        "kowloon/wall-vent",
        "kowloon/roller-shutter",
        "kowloon/balcony",
        "kowloon/cage",
        "kowloon/aircon",
        "kowloon/shelving",
    ] {
        let meshes = generate(&natives, name, &Values::new()).unwrap();
        let behind = meshes
            .iter()
            .flat_map(|m| &m.positions)
            .map(|p| p[1])
            .fold(f64::MIN, f64::max);
        assert!(behind <= 0.011, "{name} reaches {behind} m behind the wall");
        assert!(
            meshes
                .iter()
                .flat_map(|m| &m.positions)
                .any(|p| p[1] < -0.02),
            "{name} stands out from the wall"
        );
    }
    // An open shop is as wide as its opening and lies behind the wall, to its depth; its
    // stock stands on the pavement in front, within the frontage.
    let room = generate(
        &natives,
        "kowloon/shop-room",
        &values(&[("width", json!(6)), ("depth", json!(3.1))]),
    )
    .unwrap();
    check("shop-room", &room);
    // (its shutter's guides stand 6 cm onto the piers at either side).
    assert!(
        (extent(&room, 0) - 6.12).abs() < 0.05,
        "{}",
        extent(&room, 0)
    );
    let back = room
        .iter()
        .flat_map(|m| &m.positions)
        .map(|p| p[1])
        .fold(f64::MIN, f64::max);
    assert!((back - 3.1).abs() < 0.01, "the room is {back} m deep");
    for trade in [
        "provisions",
        "grocer",
        "greengrocer",
        "hardware",
        "herbalist",
    ] {
        let stocked = generate(
            &natives,
            "kowloon/shop-room",
            &values(&[("trade", json!(trade)), ("width", json!(2.6))]),
        )
        .unwrap_or_else(|e| panic!("{trade}: {e}"));
        let triangles: usize = stocked.iter().map(|m| m.indices.len() / 3).sum();
        assert!(
            triangles > 2000,
            "{trade} is furnished: {triangles} triangles"
        );
    }
    let stock = generate(
        &natives,
        "kowloon/street-stock",
        &values(&[("width", json!(6))]),
    )
    .unwrap();
    assert!(extent(&stock, 0) <= 6. && extent(&stock, 0) > 4.);
    // (the rack by the jamb stands 8 cm into the threshold).
    assert!(
        stock
            .iter()
            .flat_map(|m| &m.positions)
            .all(|p| p[1] <= 0.09 && p[1] > -1.1)
    );
    // A pavement lies in front of its wall, as long as it and its extensions, its top at 0
    // and the road its kerb height below.
    let paved = generate(
        &natives,
        "kowloon/pavement",
        &values(&[
            ("length", json!(8)),
            ("depth", json!(1.5)),
            ("height", json!(0.15)),
            ("after", json!(1.5)),
            ("drain", json!(false)),
        ]),
    )
    .unwrap();
    assert!((extent(&paved, 0) - 9.5).abs() < 0.02);
    assert!((extent(&paved, 1) - 1.5).abs() < 0.02);
    // (each stone has settled by up to 6 mm).
    assert!((extent(&paved, 2) - 0.15).abs() < 0.007);
    let slots: Vec<&str> = paved.iter().map(|m| m.material_slot.as_str()).collect();
    assert_eq!(slots, ["paving", "kerb"]);
    // A cornice is as long as its wall at its outer edge with square ends, and longer by its
    // projection at a mitred external corner.
    let square = generate(
        &natives,
        "kowloon/cornice",
        &values(&[("length", json!(4))]),
    )
    .unwrap();
    let mitred = generate(
        &natives,
        "kowloon/cornice",
        &values(&[("length", json!(4)), ("miterEnd", json!(1))]),
    )
    .unwrap();
    assert!((extent(&square, 0) - 4.).abs() < 1e-6);
    assert!(
        (extent(&mitred, 0) - 4.11).abs() < 0.02,
        "{}",
        extent(&mitred, 0)
    );
    // A conduit climbs, runs and climbs again; a ladder and a rail are as tall and long as asked.
    let conduit = generate(
        &natives,
        "kowloon/conduit",
        &values(&[
            ("rise", json!(2)),
            ("run", json!(-1.5)),
            ("rise2", json!(3)),
        ]),
    )
    .unwrap();
    assert!((extent(&conduit, 2) - 5.).abs() < 0.15 && (extent(&conduit, 0) - 1.5).abs() < 0.15);
    let rail = generate(&natives, "kowloon/rail", &values(&[("length", json!(5))])).unwrap();
    assert!((extent(&rail, 0) - 5.).abs() < 0.1);
    // Awning poles reach the ground below the canvas.
    let poled = generate(
        &natives,
        "kowloon/tarp",
        &values(&[("poles", json!(2)), ("stand", json!(2.7))]),
    )
    .unwrap();
    let low = poled
        .iter()
        .flat_map(|m| &m.positions)
        .map(|p| p[2])
        .fold(f64::MAX, f64::min);
    assert!((low + 2.7).abs() < 1e-6, "poles stand at {low}");
    // A window without glazing draws no glass; glazing bars add frame.
    let open = generate(
        &natives,
        "kowloon/window",
        &values(&[("glazing", json!(false))]),
    )
    .unwrap();
    assert!(open.iter().all(|m| m.material_slot != "glass"));
    // Unbaked weathering receivers keep their recorded paint.
    let red = generate(
        &natives,
        "kowloon/roller-shutter",
        &values(&[("color", json!("#c03020"))]),
    )
    .unwrap();
    assert!(
        red.iter()
            .flat_map(|m| &m.colors)
            .any(|c| c[0] > c[1] * 2. && c[0] > c[2] * 2.),
        "shutter curtains keep their paint"
    );
}

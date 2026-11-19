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
fn open_shops_and_street_stock_stay_light() {
    let natives = Natives::new();
    let vertices = |name: &str, entries: &[(&str, Value)]| -> usize {
        generate(&natives, name, &values(entries))
            .unwrap()
            .iter()
            .map(|m| m.positions.len())
            .sum()
    };
    let room = vertices(
        "kowloon/shop-room",
        &[("trade", json!("provisions")), ("width", json!(6))],
    );
    let stock = vertices("kowloon/street-stock", &[("width", json!(6))]);
    println!("provisions shop room (6 m): {room} vertices; street stock (6 m): {stock} vertices");
    // A packed shop of baked goods was 164 000 vertices and its street stock 22 000.
    assert!(room < 60_000 && stock < 12_000);
    // Its lit tubes are a slot of their own, bright and uncoloured by the room.
    let shop = generate(
        &natives,
        "kowloon/shop-room",
        &values(&[("trade", json!("provisions")), ("width", json!(6))]),
    )
    .unwrap();
    let tubes: Vec<&ItemMesh> = shop
        .iter()
        .filter(|m| m.material_slot == "tube-light")
        .collect();
    assert_eq!(tubes.len(), 1);
    assert!(tubes[0].colors.iter().all(|c| c[0] > 0.9 && c[2] > 0.9));
    let slots: Vec<&str> = shop.iter().map(|m| m.material_slot.as_str()).collect();
    for slot in ["interior", "concrete", "shop-floor", "stock"] {
        assert!(slots.contains(&slot), "{slot} in {slots:?}");
    }
    // The room's floor is a slot of its own, 3 cm above its anchor (clear of the ground the
    // shop stands on) and as large as the room; the ceiling stays concrete.
    let floor: Vec<&[f64; 3]> = shop
        .iter()
        .filter(|m| m.material_slot == "shop-floor")
        .flat_map(|m| &m.positions)
        .collect();
    assert!(floor.iter().all(|p| (p[2] - 0.03).abs() < 1e-6));
    let reach = |axis: usize| {
        floor.iter().map(|p| p[axis]).fold(f64::MIN, f64::max)
            - floor.iter().map(|p| p[axis]).fold(f64::MAX, f64::min)
    };
    assert!((reach(0) - 6.).abs() < 1e-6 && (reach(1) - 3.1).abs() < 1e-6);
    assert!(
        shop.iter()
            .filter(|m| m.material_slot == "concrete")
            .flat_map(|m| &m.positions)
            .all(|p| p[2] > 2.)
    );
    // Stock colours fade: half their saturation by default, all of it at 1.
    let saturation = |fade: f64| {
        let meshes = generate(
            &natives,
            "kowloon/street-stock",
            &values(&[("width", json!(6)), ("fade", json!(fade))]),
        )
        .unwrap();
        let colors: Vec<&[f64; 4]> = meshes.iter().flat_map(|m| &m.colors).collect();
        colors
            .iter()
            .map(|c| {
                c[..3].iter().cloned().fold(f64::MIN, f64::max)
                    - c[..3].iter().cloned().fold(f64::MAX, f64::min)
            })
            .sum::<f64>()
            / colors.len() as f64
    };
    let (vivid, faded, grey) = (saturation(0.), saturation(0.5), saturation(1.));
    assert!(
        faded < vivid * 0.62 && grey < vivid * 0.25,
        "{vivid} {faded} {grey}"
    );
    // Heaps of crates, cartons or both fill their width and stay within a metre of the wall.
    for kind in ["crates", "cartons", "mixed"] {
        let heap = generate(
            &natives,
            "kowloon/street-stock",
            &values(&[("width", json!(1.5)), ("kind", json!(kind))]),
        )
        .unwrap();
        check(kind, &heap);
        assert!(heap.iter().all(|m| m.material_slot == "stock"));
        assert!(
            heap.iter()
                .flat_map(|m| &m.positions)
                .all(|p| p[0].abs() <= 0.8 && p[1] <= 0.09 && p[1] > -1.1 && p[2] >= 0.),
            "{kind} stays in its place"
        );
    }
    assert!(
        generate(
            &natives,
            "kowloon/street-stock",
            &values(&[("kind", json!("pianos"))])
        )
        .is_err()
    );
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

#[test]
fn planned_gates_doors_and_hung_goods_reach_their_fixtures() {
    let natives = Natives::new();
    let bay = |entries: &[(&str, Value)]| {
        let meshes = generate(&natives, "kowloon/ground-bay", &values(entries)).unwrap();
        check("ground-bay", &meshes);
        meshes
    };
    let triangles =
        |meshes: &[ItemMesh]| -> usize { meshes.iter().map(|m| m.indices.len() / 3).sum() };
    // A gate's bars and backing: the defaults are the seeded gate, bit for bit.
    let gate = bay(&[("kind", json!("gate"))]);
    let same = bay(&[
        ("kind", json!("gate")),
        ("gateBars", json!(1)),
        ("gateBacking", json!(0)),
    ]);
    assert_eq!(fingerprint(&gate), fingerprint(&same));
    let thick = bay(&[("kind", json!("gate")), ("gateBars", json!(2))]);
    assert_eq!(triangles(&gate), triangles(&thick));
    assert_ne!(fingerprint(&gate), fingerprint(&thick));
    let backed = bay(&[
        ("kind", json!("gate")),
        ("gate", json!("#808080")),
        ("gateBacking", json!(1.5)),
    ]);
    assert!(triangles(&backed) > triangles(&gate));
    let light = |meshes: &[ItemMesh]| {
        meshes
            .iter()
            .flat_map(|m| &m.colors)
            .filter(|c| c[0] > 0.27 && (c[0] - c[1]).abs() < 0.02 && (c[1] - c[2]).abs() < 0.02)
            .count()
    };
    assert!(
        light(&backed) > light(&gate) + 8,
        "sheet leaves lighter than the lattice stand behind it"
    );
    for bad in [("gateBars", json!(4)), ("gateBacking", json!(3))] {
        assert!(
            generate(
                &natives,
                "kowloon/ground-bay",
                &values(&[("kind", json!("gate")), bad])
            )
            .is_err()
        );
    }
    // A named door's leaves have as many panels as the plan says; none named leaves three.
    let door = |panels: usize| {
        bay(&[
            ("kind", json!("door")),
            ("door", json!("double")),
            ("panels", json!(panels)),
        ])
    };
    assert_eq!(fingerprint(&door(0)), {
        fingerprint(&bay(&[("kind", json!("door")), ("door", json!("double"))]))
    });
    assert!(triangles(&door(2)) < triangles(&door(0)));
    assert!(triangles(&door(4)) > triangles(&door(0)));
    assert!(
        generate(
            &natives,
            "kowloon/ground-bay",
            &values(&[("kind", json!("door")), ("panels", json!(5))])
        )
        .is_err()
    );
    // Seeded painted leaves keep their paint: unbaked, they are not left white.
    for seed in 0..12 {
        let entrance = generate(
            &natives,
            "kowloon/entrance",
            &values(&[("seed", json!(seed)), ("posters", json!(false))]),
        )
        .unwrap();
        let paint: Vec<&[f64; 4]> = entrance
            .iter()
            .filter(|m| m.material_slot == "paint")
            .flat_map(|m| &m.colors)
            .collect();
        let white = paint
            .iter()
            .filter(|c| c[..3].iter().all(|v| *v > 0.9))
            .count();
        assert!(
            white * 5 < paint.len(),
            "seed {seed}: {white} of {} painted vertices are white",
            paint.len()
        );
    }
    // A back door without louvres is a plain leaf.
    let plain = generate(
        &natives,
        "kowloon/back-door",
        &values(&[("louvres", json!(false))]),
    )
    .unwrap();
    check("back-door", &plain);
    let vented = generate(&natives, "kowloon/back-door", &Values::new()).unwrap();
    assert!(triangles(&plain) + 20 <= triangles(&vented));
    // Hung goods: with a clear length they hang below it on cords, down to 1.05 m over the
    // floor at most, and leave the way in at the middle of the opening free.
    let room = |entries: &[(&str, Value)]| {
        let mut all = vec![("width", json!(6)), ("height", json!(2.7))];
        all.extend(entries.iter().cloned());
        generate(&natives, "kowloon/shop-room", &values(&all)).unwrap()
    };
    let points = |meshes: &[ItemMesh]| -> std::collections::BTreeSet<[i64; 3]> {
        meshes
            .iter()
            .filter(|m| m.material_slot == "stock")
            .flat_map(|m| &m.positions)
            .map(|p| p.map(|v| (v * 1e5).round() as i64))
            .collect()
    };
    let bare = points(&room(&[("hanging", json!(false))]));
    let hung: Vec<[f64; 3]> = points(&room(&[("hangClear", json!(1.0))]))
        .difference(&bare)
        .map(|p| p.map(|v| v as f64 / 1e5))
        .collect();
    assert!(hung.len() > 200, "{} hung vertices", hung.len());
    let (floor, head) = (0.03, 0.03 + 2.7);
    let line = head - 0.2;
    for p in &hung {
        assert!(p[2] >= floor + 1.05 - 1e-6, "hangs to {}", p[2]);
        // Above the clear length there are only the cords' ends, on the line.
        assert!(
            p[2] <= head - 1.0 + 1e-6 || (p[2] - line).abs() < 1e-6,
            "a piece at {} above the clear length",
            p[2]
        );
        assert!(p[0].abs() > 0.45, "goods over the way in at x = {}", p[0]);
        assert!(
            p[1] > 0. && p[1] < 0.3,
            "in the opening's plane: y = {}",
            p[1]
        );
    }
    assert!(
        generate(
            &natives,
            "kowloon/shop-room",
            &values(&[("hangDrop", json!(9))])
        )
        .is_err()
    );
}

/// The positions of one slot's meshes.
fn slot_points(meshes: &[ItemMesh], slot: &str) -> Vec<[f64; 3]> {
    meshes
        .iter()
        .filter(|m| m.material_slot == slot)
        .flat_map(|m| m.positions.iter().copied())
        .collect()
}
fn lowest(points: &[[f64; 3]], axis: usize) -> f64 {
    points.iter().map(|p| p[axis]).fold(f64::MAX, f64::min)
}
fn highest(points: &[[f64; 3]], axis: usize) -> f64 {
    points.iter().map(|p| p[axis]).fold(f64::MIN, f64::max)
}

#[test]
fn cloth_hangs_between_its_fixings_and_sheets_tilt() {
    let natives = Natives::new();
    let tarp = |entries: &[(&str, Value)]| {
        let meshes = generate(&natives, "kowloon/sagging-tarp", &values(entries)).unwrap();
        check("sagging-tarp", &meshes);
        meshes
    };
    // A slack sheet hangs lower than a taut one, by about its sag at the front edge and belly.
    let taut = tarp(&[
        ("sag", json!(0)),
        ("tilt", json!(0)),
        ("ties", json!(false)),
    ]);
    let slack = tarp(&[
        ("sag", json!(0.3)),
        ("tilt", json!(0)),
        ("ties", json!(false)),
    ]);
    let (taut_low, slack_low) = (
        lowest(&slot_points(&taut, "tarp"), 2),
        lowest(&slot_points(&slack, "tarp"), 2),
    );
    assert!(
        (taut_low + 0.4).abs() < 0.01,
        "a taut sheet falls its drop: {taut_low}"
    );
    assert!(
        slack_low < taut_low - 0.15,
        "{slack_low} against {taut_low}"
    );
    // It reaches its depth in front of the wall, and its poles stand on the ground (a little
    // off plumb), each tied back to the wall above the sheet.
    let poled = tarp(&[("stand", json!(2.7)), ("poles", json!(3))]);
    let steel = slot_points(&poled, "metal");
    assert!((lowest(&steel, 2) + 2.7).abs() < 0.005);
    assert!(
        highest(&steel, 2) > 0.2,
        "the stays' wall plates stand above the sheet"
    );
    assert!((lowest(&slot_points(&poled, "tarp"), 1) + 1.33).abs() < 0.08);
    let unbraced = tarp(&[
        ("stand", json!(2.7)),
        ("poles", json!(3)),
        ("braces", json!(false)),
    ]);
    assert!(highest(&slot_points(&unbraced, "metal"), 2) < 0.15);
    // Thirty stripes are thirty bands of two colours; a framed awning has no posts.
    let striped = tarp(&[
        ("stripes", json!(30)),
        ("frame", json!(true)),
        ("stand", json!(2.7)),
        ("color", json!("#9a3a2a")),
        ("stripe", json!("#d8d0b8")),
        ("valance", json!(0.25)),
        ("scallops", json!(15)),
        ("sag", json!(0)),
        ("tilt", json!(0)),
    ]);
    let cloth: Vec<&ItemMesh> = striped
        .iter()
        .filter(|m| m.material_slot == "tarp")
        .collect();
    let reddish = cloth
        .iter()
        .flat_map(|m| &m.colors)
        .filter(|c| c[0] > c[2] * 1.8)
        .count();
    let pale = cloth
        .iter()
        .flat_map(|m| &m.colors)
        .filter(|c| c[0] < c[2] * 1.5)
        .count();
    assert!(reddish > 500 && pale > 500, "{reddish} red, {pale} pale");
    assert!(
        lowest(&slot_points(&striped, "metal"), 2) > -1.5,
        "a frame has no posts"
    );
    // A scalloped hem rises between its lobes: fewer points hang at its full depth.
    let straight = tarp(&[
        ("stripes", json!(30)),
        ("valance", json!(0.25)),
        ("sag", json!(0)),
        ("tilt", json!(0)),
        ("frame", json!(true)),
    ]);
    let deep = |meshes: &[ItemMesh]| {
        slot_points(meshes, "tarp")
            .iter()
            .filter(|p| p[2] < -0.4 - 0.245)
            .count()
    };
    assert!(
        deep(&striped) * 2 < deep(&straight),
        "{} {}",
        deep(&striped),
        deep(&straight)
    );
    assert!((lowest(&slot_points(&striped, "tarp"), 2) + 0.65).abs() < 0.02);
    assert!(
        generate(
            &natives,
            "kowloon/sagging-tarp",
            &values(&[("stripes", json!(97)), ("scallops", json!(89))])
        )
        .is_err(),
        "stripes and scallops that share no column are refused"
    );
    // A sheet canopy projects its depth; its corners fall by seeded amounts, so its drip edge
    // is not level, and its rust is in its tints.
    let canopy = |entries: &[(&str, Value)]| {
        let meshes = generate(&natives, "kowloon/sheet-canopy", &values(entries)).unwrap();
        check("sheet-canopy", &meshes);
        meshes
    };
    let sheet = slot_points(&canopy(&[("width", json!(1.2))]), "sheet");
    assert!(
        (lowest(&sheet, 1) + 0.5).abs() < 1e-6,
        "projects {}",
        lowest(&sheet, 1)
    );
    let uneven = (0..8).any(|seed| {
        let sheet = slot_points(
            &canopy(&[("width", json!(1.2)), ("seed", json!(seed))]),
            "sheet",
        );
        let (left, right) = (
            sheet
                .iter()
                .filter(|p| p[1] < -0.49 && p[0] < -0.5)
                .map(|p| p[2])
                .fold(f64::MIN, f64::max),
            sheet
                .iter()
                .filter(|p| p[1] < -0.49 && p[0] > 0.5)
                .map(|p| p[2])
                .fold(f64::MIN, f64::max),
        );
        (left - right).abs() > 0.02
    });
    assert!(uneven, "no seed tilts the sheet");
    let redness = |rusted: f64| {
        let meshes = canopy(&[("rusted", json!(rusted))]);
        let colors: Vec<&[f64; 4]> = meshes
            .iter()
            .filter(|m| m.material_slot == "sheet")
            .flat_map(|m| &m.colors)
            .collect();
        colors.iter().map(|c| c[0] - c[2]).sum::<f64>() / colors.len() as f64
    };
    assert!(redness(1.) > redness(0.) + 0.03);
}

#[test]
fn cables_sag_round_corners_and_hardware_is_bolted() {
    let natives = Natives::new();
    let cable = |entries: &[(&str, Value)]| {
        let meshes = generate(&natives, "kowloon/cable", &values(entries)).unwrap();
        check("cable", &meshes);
        meshes
    };
    // One cable spans its length and hangs its sag below its fixings.
    let one = cable(&[("span", json!(4)), ("sag", json!(0.3))]);
    let wire = slot_points(&one, "cable");
    assert!((highest(&wire, 0) - 4.).abs() < 0.01 && lowest(&wire, 0).abs() < 0.01);
    assert!(
        (lowest(&wire, 2) + 0.3).abs() < 0.04,
        "hangs {}",
        lowest(&wire, 2)
    );
    assert!(highest(&wire, 1) < 0., "it stays in front of the wall");
    // A bundle has more wire; a rise lifts the far end; a negative span runs left.
    let triangles = |meshes: &[ItemMesh], slot: &str| -> usize {
        meshes
            .iter()
            .filter(|m| m.material_slot == slot)
            .map(|m| m.indices.len() / 3)
            .sum()
    };
    let bundle = cable(&[("span", json!(4)), ("count", json!(3))]);
    assert_eq!(triangles(&bundle, "cable"), 3 * triangles(&one, "cable"));
    let left = slot_points(&cable(&[("span", json!(-2)), ("rise", json!(1))]), "cable");
    assert!((lowest(&left, 0) + 2.).abs() < 0.01 && (highest(&left, 2) - 1.).abs() < 0.02);
    // Its far end can leave the wall for a fixture that stands in front of it.
    let out = slot_points(&cable(&[("out", json!(0.8))]), "cable");
    assert!(
        (lowest(&out, 1) + 0.8).abs() < 0.02,
        "reaches {}",
        lowest(&out, 1)
    );
    // Round a corner it carries on behind the wall plane, along the next wall, which stands
    // the corner's radius beyond where this one ends.
    let turned = slot_points(
        &cable(&[
            ("span", json!(5)),
            ("corner", json!(2)),
            ("cornerRadius", json!(0.5)),
            ("sag", json!(0.1)),
        ]),
        "cable",
    );
    let after = 5. - 2. - 0.5 * std::f64::consts::FRAC_PI_2;
    assert!(
        (highest(&turned, 1) - 0.5 - after).abs() < 0.02,
        "{}",
        highest(&turned, 1)
    );
    assert!(
        (highest(&turned, 0) - 2.53).abs() < 0.02,
        "{}",
        highest(&turned, 0)
    );
    assert!(
        generate(
            &natives,
            "kowloon/cable",
            &values(&[("span", json!(2)), ("corner", json!(1.9))])
        )
        .is_err(),
        "a cable must reach past its corner"
    );
    // Junction boxes at the fixings add steel.
    let boxed = cable(&[("boxes", json!("both"))]);
    assert!(triangles(&boxed, "metal") > triangles(&cable(&[]), "metal") + 100);
    let junction = generate(&natives, "kowloon/junction-box", &Values::new()).unwrap();
    check("junction-box", &junction);
    let steel = slot_points(&junction, "metal");
    assert!(highest(&steel, 1) <= 1e-9 && lowest(&steel, 1) < -0.07);
    // Pipe clips are bolted either side of the pipe, on the wall.
    let pipe = |entries: &[(&str, Value)]| {
        let meshes = generate(&natives, "kowloon/downpipe", &values(entries)).unwrap();
        check("downpipe", &meshes);
        meshes
    };
    let (plain, clipped) = (pipe(&[]), pipe(&[("clips", json!(1))]));
    let count = |meshes: &[ItemMesh]| meshes.iter().map(|m| m.indices.len() / 3).sum::<usize>();
    assert!(
        count(&clipped) > count(&plain) + 300,
        "three clips on three metres"
    );
    let all: Vec<[f64; 3]> = clipped
        .iter()
        .flat_map(|m| m.positions.iter().copied())
        .collect();
    assert!(highest(&all, 1) <= 1e-9 && highest(&all, 0) > lowest(&all, 0) + 0.2);
    // A level board hangs from one pole above it: no outrigger at its foot, as an upright
    // board has.
    let board = |horizontal: bool| {
        let meshes = generate(
            &natives,
            "kowloon/board-sign",
            &values(&[
                ("horizontal", json!(horizontal)),
                ("width", json!(if horizontal { 1.9 } else { 0.9 })),
                ("height", json!(if horizontal { 0.6 } else { 3. })),
            ]),
        )
        .unwrap();
        check("board-sign", &meshes);
        slot_points(&meshes, "metal")
    };
    let wall = |points: &[[f64; 3]]| {
        points
            .iter()
            .filter(|p| p[1] > -0.03)
            .map(|p| p[2])
            .fold(f64::MAX, f64::min)
    };
    assert!(wall(&board(true)) > 0.6, "its fixings are above it");
    assert!(wall(&board(false)) < 0.1);
    // Roof clutter keeps to the sides it is given.
    let roof = |entries: &[(&str, Value)]| {
        let meshes = generate(&natives, "kowloon/roof-clutter", &values(entries)).unwrap();
        check("roof-clutter", &meshes);
        meshes
    };
    let vents = roof(&[
        ("pipes", json!(0)),
        ("pots", json!(0)),
        ("width", json!(9)),
        ("depth", json!(6)),
    ]);
    let stubs = slot_points(&vents, "metal");
    assert!(
        lowest(&stubs, 0) > 3.5 && highest(&stubs, 0) < 4.5,
        "vents stand along the right"
    );
    assert!(highest(&stubs, 2) > 0.4 && lowest(&stubs, 2) >= 0.);
    let piped = roof(&[
        ("vents", json!(0)),
        ("pots", json!(0)),
        ("pipeSide", json!("left")),
        ("pipes", json!(2)),
    ]);
    assert!(highest(&slot_points(&piped, "metal"), 0) < -3.9);
    let slung = roof(&[("cable", json!(true))]);
    assert!(slung.iter().any(|m| m.material_slot == "cable"));
    assert!(roof(&[]).iter().all(|m| m.material_slot != "cable"));
    assert!(
        generate(
            &natives,
            "kowloon/roof-clutter",
            &values(&[("ventSide", json!("middle"))])
        )
        .is_err()
    );
}

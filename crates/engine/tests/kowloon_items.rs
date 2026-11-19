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
    // A pedestrian avenue is as long and wide as asked, its road `height` below its top; its
    // median planter stands `planterRise` above the paving with its soil `soilDepth` below
    // that, no slab is laid under the planter, and bollards close its ends.
    let avenue = generate(
        &natives,
        "kowloon/pedestrian-avenue",
        &values(&[
            ("length", json!(24)),
            ("width", json!(12)),
            ("height", json!(0.1)),
            ("medianOffset", json!(5)),
            ("medianWidth", json!(2)),
            ("medianStart", json!(2)),
            ("medianEnd", json!(22)),
        ]),
    )
    .unwrap();
    check("kowloon/pedestrian-avenue", &avenue);
    assert!((extent(&avenue, 0) - 24.).abs() < 1e-6);
    assert!((extent(&avenue, 1) - 12.).abs() < 1e-6);
    let slot = |name: &str| -> Vec<[f64; 3]> {
        avenue
            .iter()
            .filter(|m| m.material_slot == name)
            .flat_map(|m| m.positions.clone())
            .collect()
    };
    let slots: Vec<&str> = avenue.iter().map(|m| m.material_slot.as_str()).collect();
    for name in ["paving", "kerb", "soil", "metal", "timber"] {
        assert!(slots.contains(&name), "the avenue has {name}");
    }
    let low = avenue
        .iter()
        .flat_map(|m| &m.positions)
        .map(|p| p[2])
        .fold(f64::MAX, f64::min);
    assert!((low + 0.1).abs() < 1e-6, "the road is {low} below");
    let soil = slot("soil");
    assert!(soil.iter().all(|p| (p[2] - 0.16).abs() < 1e-9));
    let (soil_y0, soil_y1) = soil
        .iter()
        .fold((f64::MAX, f64::MIN), |(a, b), p| (a.min(p[1]), b.max(p[1])));
    assert!((soil_y0 + 5.8).abs() < 1e-9 && (soil_y1 + 4.2).abs() < 1e-9);
    let planter_top = slot("kerb")
        .iter()
        .filter(|p| p[1] > -6.01 && p[1] < -3.99 && p[0] > 2.5 && p[0] < 21.5)
        .map(|p| p[2])
        .fold(f64::MIN, f64::max);
    assert!(
        (planter_top - 0.3).abs() < 1e-9,
        "the planter rises {planter_top}"
    );
    let mut slabs = 0;
    for m in avenue.iter().filter(|m| m.material_slot == "paving") {
        for corners in m.indices.chunks_exact(3) {
            let [a, b, c] = [0, 1, 2].map(|k| m.positions[corners[k] as usize]);
            let centre = [0, 1].map(|k| (a[k] + b[k] + c[k]) / 3.);
            if a[2] > -0.01 && b[2] > -0.01 && c[2] > -0.01 {
                slabs += 1;
                assert!(
                    !(centre[0] > 3. && centre[0] < 21. && centre[1] > -5.9 && centre[1] < -4.1),
                    "a slab's top at {centre:?} lies under the planter"
                );
            }
        }
    }
    assert!(slabs > 600, "{slabs} slab top triangles");
    let tall = slot("metal").iter().filter(|p| p[2] > 0.75).count();
    assert!(tall > 0, "bollards stand at the ends");
    let plain = generate(
        &natives,
        "kowloon/pedestrian-avenue",
        &values(&[
            ("medianWidth", json!(0)),
            ("benches", json!(0)),
            ("bollardSpacing", json!(0)),
        ]),
    )
    .unwrap();
    let plain_slots: Vec<&str> = plain.iter().map(|m| m.material_slot.as_str()).collect();
    assert_eq!(plain_slots, ["paving", "kerb"]);
    for refused in [
        values(&[("medianWidth", json!(0.5))]),
        values(&[("medianOffset", json!(13.5))]),
        values(&[("medianStart", json!(20)), ("medianEnd", json!(10))]),
        values(&[("soilDepth", json!(0.4))]),
    ] {
        assert!(
            generate(&natives, "kowloon/pedestrian-avenue", &refused).is_err(),
            "{refused:?} is refused"
        );
    }
    // Litter bins stand past the ends of each bench station, one on either side of the
    // planter, about 0.97 m tall.
    let binned = generate(
        &natives,
        "kowloon/pedestrian-avenue",
        &values(&[
            ("length", json!(24)),
            ("width", json!(12)),
            ("medianOffset", json!(5)),
            ("medianWidth", json!(2)),
            ("medianStart", json!(2)),
            ("medianEnd", json!(22)),
            ("benches", json!(2)),
            ("benchFirst", json!(8)),
            ("bins", json!(true)),
        ]),
    )
    .unwrap();
    check("kowloon/pedestrian-avenue", &binned);
    let lids: Vec<[f64; 3]> = binned
        .iter()
        .filter(|m| m.material_slot == "metal")
        .flat_map(|m| m.positions.clone())
        .filter(|p| p[2] > 0.9 && p[2] < 1.)
        .collect();
    for (x, y) in [(6.65, -3.6), (9.35, -6.4), (13.65, -3.6), (16.35, -6.4)] {
        assert!(
            lids.iter()
                .any(|p| (p[0] - x).abs() < 0.1 && (p[1] - y).abs() < 0.1),
            "a bin stands at {x}, {y}"
        );
    }
    assert!(lids.iter().all(|p| p[2] <= 0.971));
    // What stands on the paving reports its `foot` without drawing: a closed ring on the
    // paving plane round every bollard, bin and bench support and round the planter, each
    // point at the foot of a drawn piece.
    let binned_values = values(&[
        ("length", json!(24)),
        ("width", json!(12)),
        ("medianOffset", json!(5)),
        ("medianWidth", json!(2)),
        ("medianStart", json!(2)),
        ("medianEnd", json!(22)),
        ("benches", json!(2)),
        ("benchFirst", json!(8)),
        ("bins", json!(true)),
    ]);
    let feet = natives.features("kowloon/pedestrian-avenue", &binned_values);
    // (2 stations of 2 benches on 2 supports, 4 bins, 7 bollards at either end, the planter).
    assert_eq!(feet.len(), 8 + 4 + 14 + 1, "{} feet", feet.len());
    let standing: Vec<[f64; 3]> = binned
        .iter()
        .filter(|m| m.material_slot == "metal" || m.material_slot == "kerb")
        .flat_map(|m| m.positions.clone())
        .filter(|p| p[2] < 0. && p[2] > -0.031)
        .collect();
    for foot in &feet {
        assert_eq!(foot.name, "foot");
        assert!(foot.closed && foot.points.len() >= 4 && foot.width > 0.1);
        assert_eq!(foot.normal, [0., 0., 1.]);
        for point in &foot.points {
            assert_eq!(point[2], 0., "a foot lies on the paving plane");
            assert!(
                standing
                    .iter()
                    .any(|p| (p[0] - point[0]).hypot(p[1] - point[1]) < 0.045),
                "a foot at {point:?} is at a drawn piece"
            );
        }
    }
    // The planter's ring is its outer outline; a bollard's is its shaft's girth.
    assert!(feet.iter().any(|f| {
        f.width == 0.2
            && f.points == vec![[2., -6., 0.], [22., -6., 0.], [22., -4., 0.], [2., -4., 0.]]
    }));
    let wide = |width: f64| {
        feet.iter()
            .filter(|f| (f.width - width).abs() < 1e-9)
            .count()
    };
    assert_eq!((wide(0.15), wide(0.4)), (14 + 8, 4));
    // A street lamp reports its base plate's outline, for the paving of the item under it.
    let plate = natives.features("kowloon/street-lamp", &Values::new());
    assert_eq!(plate.len(), 1);
    assert_eq!(plate[0].name, "foot");
    assert_eq!(
        plate[0].points,
        vec![
            [-0.2, -0.2, 0.],
            [0.2, -0.2, 0.],
            [0.2, 0.2, 0.],
            [-0.2, 0.2, 0.]
        ]
    );
    let lamp_foot: Vec<[f64; 3]> = generate(&natives, "kowloon/street-lamp", &Values::new())
        .unwrap()
        .iter()
        .flat_map(|m| m.positions.clone())
        .filter(|p| p[2] == 0.)
        .collect();
    for point in &plate[0].points {
        assert!(lamp_foot.contains(point), "the plate's corner {point:?}");
    }
    // Nothing stands on a bare avenue, and a refused one reports nothing.
    let bare = values(&[
        ("medianWidth", json!(0)),
        ("benches", json!(0)),
        ("bollardSpacing", json!(0)),
    ]);
    assert!(
        natives
            .features("kowloon/pedestrian-avenue", &bare)
            .is_empty()
    );
    assert!(
        natives
            .features(
                "kowloon/pedestrian-avenue",
                &values(&[("medianWidth", json!(0.5))])
            )
            .is_empty()
    );
    // The bench timber and the bins are tinted near neutral, so their materials decide.
    let spread = |r: f64, g: f64, b: f64| (r.max(g).max(b) - r.min(g).min(b)) / r.max(g).max(b);
    for m in binned.iter().filter(|m| m.material_slot == "timber") {
        assert!(m.colors.iter().all(|c| spread(c[0], c[1], c[2]) < 0.3));
    }
    // Unstaggered, the slabs lie in a grid: every slab top starts on one of the same column
    // lines, which staggered courses offset by half a slab; the bollards take their paint.
    let column_starts = |meshes: &[ItemMesh]| -> std::collections::BTreeSet<i64> {
        meshes
            .iter()
            .filter(|m| m.material_slot == "paving")
            .flat_map(|m| m.positions.iter())
            .filter(|p| p[2] > -0.01)
            .map(|p| (p[0] * 100.).round() as i64)
            .collect()
    };
    let grid_values = |stagger: bool| {
        values(&[
            ("length", json!(9)),
            ("width", json!(6)),
            ("medianWidth", json!(0)),
            ("benches", json!(0)),
            ("slab", json!(0.6)),
            ("course", json!(0.6)),
            ("bands", json!(0)),
            ("stagger", json!(stagger)),
            ("bollardColor", json!("#101010")),
        ])
    };
    let grid = generate(&natives, "kowloon/pedestrian-avenue", &grid_values(false)).unwrap();
    check("kowloon/pedestrian-avenue", &grid);
    let staggered = generate(&natives, "kowloon/pedestrian-avenue", &grid_values(true)).unwrap();
    assert!(
        column_starts(&grid).len() * 3 < column_starts(&staggered).len() * 2,
        "{} grid against {} staggered x positions",
        column_starts(&grid).len(),
        column_starts(&staggered).len()
    );
    let darkest = |meshes: &[ItemMesh]| {
        meshes
            .iter()
            .filter(|m| m.material_slot == "metal")
            .flat_map(|m| m.colors.iter())
            .map(|c| c[0] + c[1] + c[2])
            .fold(f64::MAX, f64::min)
    };
    let plain_bollards = generate(
        &natives,
        "kowloon/pedestrian-avenue",
        &values(&[("medianWidth", json!(0)), ("benches", json!(0))]),
    )
    .unwrap();
    assert!(
        darkest(&grid) < darkest(&plain_bollards) * 0.5,
        "the bollards are painted near black"
    );
    // A parked bicycle stands on its wheels about a metre high and 1.9 m long, along +X; a
    // hand cart's load stands on its deck. Both keep to their two slots.
    let bounds_of = |meshes: &[ItemMesh], axis: usize| {
        let points = all_points(meshes);
        (lowest(&points, axis), highest(&points, axis))
    };
    let bicycle = generate(&natives, "kowloon/street-cart", &Values::new()).unwrap();
    check("kowloon/street-cart", &bicycle);
    let (low, high) = bounds_of(&bicycle, 2);
    assert!(
        low.abs() < 0.01 && (0.95..1.1).contains(&high),
        "{low}..{high}"
    );
    let (front_x, back_x) = bounds_of(&bicycle, 0);
    assert!(
        (1.6..2.0).contains(&(back_x - front_x)),
        "{front_x}..{back_x}"
    );
    let (near, far) = bounds_of(&bicycle, 1);
    assert!(near > -0.35 && far < 0.35, "{near}..{far}");
    let cart = generate(
        &natives,
        "kowloon/street-cart",
        &values(&[("kind", json!("trolley")), ("load", json!(1))]),
    )
    .unwrap();
    check("kowloon/street-cart", &cart);
    for meshes in [&bicycle, &cart] {
        assert!(
            meshes
                .iter()
                .all(|m| m.material_slot == "metal" || m.material_slot == "stock")
        );
    }
    let load: Vec<[f64; 3]> = cart
        .iter()
        .filter(|m| m.material_slot == "stock")
        .flat_map(|m| m.positions.clone())
        .collect();
    assert!(!load.is_empty() && load.iter().all(|p| p[2] >= 0.17 - 1e-9));
    let vertices: usize = bicycle.iter().chain(&cart).map(|m| m.positions.len()).sum();
    assert!(vertices < 20_000, "{vertices} vertices");
    assert!(
        generate(
            &natives,
            "kowloon/street-cart",
            &values(&[("kind", json!("scooter"))])
        )
        .is_err()
    );
    // A street lamp: a pole `height` tall whose swan-neck arm reaches toward -Y and holds the
    // luminaire's diffuser (`lamp`) under its housing; a lantern's glass stands on the pole's
    // head; a banner hangs on brackets toward +X; two arms reach either way.
    let lamp = generate(&natives, "kowloon/street-lamp", &Values::new()).unwrap();
    check("kowloon/street-lamp", &lamp);
    let slots: Vec<&str> = lamp.iter().map(|m| m.material_slot.as_str()).collect();
    assert_eq!(slots, ["metal", "lamp"]);
    let vertices: usize = lamp.iter().map(|m| m.positions.len()).sum();
    assert!(vertices < 20_000, "{vertices} vertices");
    let bounds = |meshes: &[ItemMesh], slot: &str, k: usize| {
        meshes
            .iter()
            .filter(|m| slot.is_empty() || m.material_slot == slot)
            .flat_map(|m| &m.positions)
            .fold((f64::MAX, f64::MIN), |(a, b), p| (a.min(p[k]), b.max(p[k])))
    };
    let (low, high) = bounds(&lamp, "", 2);
    assert!(
        low.abs() < 1e-9 && high > 6.4 && high < 6.6,
        "{low}..{high}"
    );
    let (near, far) = bounds(&lamp, "", 1);
    assert!((near + 1.85).abs() < 1e-6 && far < 0.25, "{near}..{far}");
    let (glass_low, glass_high) = bounds(&lamp, "lamp", 2);
    assert!(glass_low > 6.2 && glass_high < 6.45);
    let (glass_near, _) = bounds(&lamp, "lamp", 1);
    assert!(glass_near < -1.7);
    let double = generate(
        &natives,
        "kowloon/street-lamp",
        &values(&[
            ("arms", json!(2)),
            ("reach", json!(1)),
            ("height", json!(8)),
        ]),
    )
    .unwrap();
    let (near, far) = bounds(&double, "", 1);
    assert!((near + 1.55).abs() < 1e-6 && (far - 1.55).abs() < 1e-6);
    let lantern = generate(
        &natives,
        "kowloon/street-lamp",
        &values(&[
            ("style", json!("lantern")),
            ("banner", json!(true)),
            ("height", json!(4)),
        ]),
    )
    .unwrap();
    check("kowloon/street-lamp", &lantern);
    let slots: Vec<&str> = lantern.iter().map(|m| m.material_slot.as_str()).collect();
    assert_eq!(slots, ["metal", "banner", "lamp"]);
    let (glass_low, glass_high) = bounds(&lantern, "lamp", 2);
    assert!((glass_low - 4.14).abs() < 1e-9 && (glass_high - 4.62).abs() < 1e-9);
    let (banner_x0, banner_x1) = bounds(&lantern, "banner", 0);
    assert!(banner_x0 > 0.05 && banner_x1 < 0.75);
    for refused in [
        values(&[("style", json!("cobra"))]),
        values(&[("banner", json!(true)), ("height", json!(3))]),
        values(&[("arms", json!(3))]),
    ] {
        assert!(
            generate(&natives, "kowloon/street-lamp", &refused).is_err(),
            "{refused:?} is refused"
        );
    }
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
    // Hung goods: with a clear length they hang below it on cords, plain goods down to 1.05 m
    // over the floor at most, and leave the way in at the middle of the opening free.
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
    let hung_points = |entries: &[(&str, Value)]| -> Vec<[f64; 3]> {
        points(&room(entries))
            .difference(&bare)
            .map(|p| p.map(|v| v as f64 / 1e5))
            .collect()
    };
    let hung = hung_points(&[("hangClear", json!(1.0)), ("hangStyle", json!("plain"))]);
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
    // Bold goods (the default) hang in a taller band, down to 0.85 m over the floor, with
    // longer pieces at the opening's sides that stop 0.35 m further down; the way in keeps 2 m.
    let bold = hung_points(&[("hangClear", json!(1.0))]);
    assert!(bold.len() > 200, "{} bold hung vertices", bold.len());
    let side = |p: &[f64; 3]| p[0].abs() > 3. - 1.05;
    for p in &bold {
        let lowest = if side(p) { 0.5 } else { 0.85 };
        assert!(
            p[2] >= floor + lowest - 1e-6,
            "hangs to {} at x = {}",
            p[2],
            p[0]
        );
        assert!(p[0].abs() > 0.45, "goods over the way in at x = {}", p[0]);
        if p[0].abs() < 0.6 {
            assert!(
                p[2] >= floor + 2. - 1e-6,
                "{} under the way's headroom",
                p[2]
            );
        }
    }
    let low = |keep: &dyn Fn(&[f64; 3]) -> bool, set: &[[f64; 3]]| {
        set.iter()
            .filter(|p| keep(p))
            .map(|p| p[2])
            .fold(f64::MAX, f64::min)
    };
    let all = |_: &[f64; 3]| true;
    assert!(low(&side, &bold) < low(&|p: &[f64; 3]| !side(p), &bold) - 0.1);
    assert!(low(&all, &bold) < low(&all, &hung) - 0.1);
    // And they are coloured: their stock is more saturated, with fewer pale vertices.
    let colours = |entries: &[(&str, Value)]| -> (f64, f64) {
        let meshes = room(entries);
        let colors: Vec<&[f64; 4]> = meshes
            .iter()
            .filter(|m| m.material_slot == "stock")
            .flat_map(|m| &m.colors)
            .collect();
        let chroma = colors
            .iter()
            .map(|c| {
                c[..3].iter().cloned().fold(0., f64::max)
                    - c[..3].iter().cloned().fold(1., f64::min)
            })
            .sum::<f64>()
            / colors.len() as f64;
        let pale = colors
            .iter()
            .filter(|c| c[..3].iter().all(|v| *v > 0.45))
            .count();
        (chroma, pale as f64 / colors.len() as f64)
    };
    let (plain_chroma, plain_pale) =
        colours(&[("hangClear", json!(1.0)), ("hangStyle", json!("plain"))]);
    let (bold_chroma, bold_pale) = colours(&[("hangClear", json!(1.0))]);
    assert!(
        bold_chroma > plain_chroma,
        "{bold_chroma} <= {plain_chroma}"
    );
    assert!(bold_pale <= plain_pale, "{bold_pale} > {plain_pale}");
    for bad in [
        ("hangDrop", json!(9)),
        ("hangSides", json!(2)),
        ("hangStyle", json!("loud")),
    ] {
        assert!(generate(&natives, "kowloon/shop-room", &values(&[bad])).is_err());
    }
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
    // A scalloped hem rises between its lobes: less of its width (in centimetre columns,
    // whatever the hem's bound edge adds) hangs at its full depth.
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
            .map(|p| (p[0] * 100.).round() as i64)
            .collect::<std::collections::BTreeSet<_>>()
            .len()
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

#[test]
fn a_lit_window_shows_its_room_in_depth() {
    let natives = Natives::new();
    let window = |entries: &[(&str, Value)]| {
        let mut all = vec![
            ("width", json!(1.8)),
            ("height", json!(1.5)),
            ("recess", json!(0.25)),
            ("glow", json!("#ffe7ad")),
        ];
        all.extend(entries.iter().cloned());
        generate(&natives, "kowloon/window", &values(&all)).unwrap()
    };
    // Unset, and on an unlit window, the room changes nothing.
    let flat = window(&[("lit", json!(true))]);
    assert_eq!(
        fingerprint(&flat),
        fingerprint(&window(&[("lit", json!(true)), ("room", json!(0))]))
    );
    assert_eq!(
        fingerprint(&window(&[])),
        fingerprint(&window(&[("room", json!(2.5))]))
    );
    assert!(flat.iter().all(|m| m.material_slot != "room-lit"));
    for seed in 0..6 {
        let lit = window(&[
            ("lit", json!(true)),
            ("room", json!(2.5)),
            ("seed", json!(seed)),
        ]);
        check("lit room", &lit);
        assert!(lit.iter().all(|m| m.material_slot != "glass"));
        // Back wall, side walls and ceiling stand behind the recess, 2.5 m deep, and reach past
        // the opening; the floor lies below the sill and the ceiling above the head.
        let walls = slot_points(&lit, "room-lit");
        let depth = |p: &[f64; 3]| p[1].abs();
        assert!(walls.iter().all(|p| depth(p) >= 0.25 - 1e-6), "{seed}");
        let far = walls.iter().map(depth).fold(0., f64::max);
        assert!((far - 2.75).abs() < 1e-6, "{seed}: {far}");
        assert!(lowest(&walls, 0) < -0.9 - 0.1 && highest(&walls, 0) > 0.9 + 0.1);
        assert!(lowest(&walls, 2) < -0.5 && highest(&walls, 2) > 1.5);
        // Its walls are lit, warm and darker than white: the lamp's colour, softened.
        let colors: Vec<&[f64; 4]> = lit
            .iter()
            .filter(|m| m.material_slot == "room-lit")
            .flat_map(|m| &m.colors)
            .collect();
        assert!(
            colors
                .iter()
                .all(|c| c[0] >= c[2] && c[0] < 0.9 && c[0] > 0.02),
            "{seed}"
        );
        // A tube under the ceiling, in the lamps' own slot, well inside the room.
        let tube = slot_points(&lit, "tube-light");
        assert!(!tube.is_empty() && tube.iter().all(|p| depth(p) > 0.8 && p[2] > 1.2));
    }
    assert!(
        generate(
            &natives,
            "kowloon/window",
            &values(&[("lit", json!(true)), ("room", json!(0.3))])
        )
        .is_err()
    );
}

fn triangles(meshes: &[ItemMesh]) -> usize {
    meshes.iter().map(|m| m.indices.len() / 3).sum()
}

#[test]
fn tank_shape_cage_welding_and_bracket_plates_reach_their_fixtures() {
    let natives = Natives::new();
    let top = |meshes: &[ItemMesh]| {
        meshes
            .iter()
            .flat_map(|m| &m.positions)
            .map(|p| p[2])
            .fold(f64::MIN, f64::max)
    };
    // A shaped tank: a 1.4 m shell on a 0.72 m stand under a 0.24 m conical lid, its access
    // ladder's head 0.16 m over the collar; the seeded tank stays under 2 m.
    let seeded = generate(&natives, "kowloon/water-tank", &Values::new()).unwrap();
    let shaped = |entries: &[(&str, Value)]| {
        let mut all = vec![
            ("height", json!(1.4)),
            ("stand", json!(0.72)),
            ("lid", json!(0.24)),
            ("ribs", json!(7)),
        ];
        all.extend(entries.iter().cloned());
        generate(&natives, "kowloon/water-tank", &values(&all))
    };
    let tall = shaped(&[]).unwrap();
    check("shaped tank", &tall);
    assert!(top(&seeded) < 2., "{}", top(&seeded));
    assert!(
        (top(&tall) - (0.72 + 1.4 + 0.24 + 0.16)).abs() < 0.02,
        "{}",
        top(&tall)
    );
    // Ribs and a lid add to the shell; without them the shaped tank keeps three hoops.
    let plain = generate(
        &natives,
        "kowloon/water-tank",
        &values(&[("height", json!(1.4)), ("stand", json!(0.72))]),
    )
    .unwrap();
    assert!(triangles(&tall) > triangles(&plain));
    assert!((top(&plain) - (0.72 + 1.4 + 0.16)).abs() < 0.02);
    for bad in [
        ("lid", json!(2)),
        ("ribs", json!(1.5)),
        ("height", json!(9)),
    ] {
        assert!(shaped(&[bad.clone()]).is_err(), "{bad:?}");
    }
    assert!(
        generate(
            &natives,
            "kowloon/water-tank",
            &values(&[("kind", json!("box")), ("lid", json!(0.2))])
        )
        .is_err()
    );
    // Finer, closer cage bars: more of them, each thinner; unset, the welding is the seed's.
    let cage = |entries: &[(&str, Value)]| generate(&natives, "kowloon/cage", &values(entries));
    let welded = cage(&[]).unwrap();
    assert_eq!(
        fingerprint(&welded),
        fingerprint(&cage(&[("louvred", json!(false))]).unwrap())
    );
    let fine = cage(&[
        ("bar", json!(0.008)),
        ("rail", json!(0.02)),
        ("spacing", json!(0.09)),
        ("rust", json!(0.3)),
        ("color", json!("#c9c3b2")),
    ])
    .unwrap();
    check("fine cage", &fine);
    assert!(triangles(&fine) > triangles(&welded));
    for bad in [
        ("bar", json!(0.2)),
        ("spacing", json!(0.01)),
        ("rust", json!(2)),
    ] {
        assert!(cage(&[bad.clone()]).is_err(), "{bad:?}");
    }
    // A caged unit turned louvred stands on the cage floor, its casing inside the cage.
    for seed in 0..4 {
        let unit = cage(&[
            ("contents", json!("aircon")),
            ("louvred", json!(true)),
            ("width", json!(0.98)),
            ("height", json!(0.68)),
            ("depth", json!(0.52)),
            ("seed", json!(seed)),
        ])
        .unwrap();
        check("caged unit", &unit);
        let casing = slot_points(&unit, "aircon");
        assert!(!casing.is_empty(), "{seed}");
        assert!(
            lowest(&casing, 2) >= 0.04 && highest(&casing, 2) < 0.68,
            "{seed}"
        );
        assert!(lowest(&casing, 1) > -0.52 - 1e-6, "{seed}");
    }
    // Bolted plates on the bracket arms, only when asked for.
    for name in ["kowloon/aircon", "kowloon/condenser"] {
        let bare = generate(&natives, name, &Values::new()).unwrap();
        assert_eq!(
            fingerprint(&bare),
            fingerprint(&generate(&natives, name, &values(&[("plates", json!(false))])).unwrap())
        );
        let plated = generate(&natives, name, &values(&[("plates", json!(true))])).unwrap();
        check(name, &plated);
        assert!(triangles(&plated) > triangles(&bare), "{name}");
    }
}

#[test]
fn a_slack_tarp_is_creased_from_its_fixings_and_shaded_in_its_troughs() {
    let natives = Natives::new();
    let tarp = |entries: &[(&str, Value)]| {
        let meshes = generate(&natives, "kowloon/sagging-tarp", &values(entries)).unwrap();
        check("sagging-tarp", &meshes);
        meshes
    };
    // Across the sheet near its hem (away from its hemmed sides and ropes): per column of
    // vertices, the lowest point and its tint's lightness.
    let band = |meshes: &[ItemMesh]| {
        let mut columns = std::collections::BTreeMap::<i64, (f64, f64)>::new();
        for m in meshes.iter().filter(|m| m.material_slot == "tarp") {
            for (p, c) in m.positions.iter().zip(&m.colors) {
                if (p[1] + 1.05).abs() < 0.03 && p[0].abs() < 1.4 {
                    let entry = columns
                        .entry((p[0] / 0.05).round() as i64)
                        .or_insert((f64::MAX, 0.));
                    if p[2] < entry.0 {
                        *entry = (p[2], c[0] + c[1] + c[2]);
                    }
                }
            }
        }
        columns.into_values().collect::<Vec<_>>()
    };
    let taut = tarp(&[("sag", json!(0)), ("tilt", json!(0))]);
    let slack = tarp(&[("sag", json!(0.3)), ("tilt", json!(0))]);
    // Its creases' troughs are shaded: the slack sheet's tints there vary far more.
    let spread = |meshes: &[ItemMesh]| {
        let light: Vec<f64> = band(meshes).iter().map(|c| c.1).collect();
        let high = light.iter().copied().fold(f64::MIN, f64::max);
        (high - light.iter().copied().fold(f64::MAX, f64::min)) / high
    };
    assert!(
        spread(&slack) > spread(&taut) + 0.08,
        "{} against {}",
        spread(&slack),
        spread(&taut)
    );
    // Its creases and gathered folds sink its front half in troughs: across the sheet its
    // height dips again and again, where a smooth sag dips once.
    let dips = |meshes: &[ItemMesh]| {
        let z: Vec<f64> = band(meshes).iter().map(|c| c.0).collect();
        z.windows(3)
            .filter(|w| w[1] < w[0] - 1e-4 && w[1] < w[2] - 1e-4)
            .count()
    };
    assert!(dips(&slack) >= 4, "{} dips", dips(&slack));
    // Its seed moves the folds.
    assert_ne!(
        fingerprint(&slack),
        fingerprint(&tarp(&[
            ("sag", json!(0.3)),
            ("tilt", json!(0)),
            ("seed", json!(5))
        ]))
    );
}

#[test]
fn a_party_neighbour_is_a_stained_wall_or_a_slice_of_a_building() {
    let natives = Natives::new();
    let party = |entries: &[(&str, Value)]| {
        let meshes = generate(&natives, "kowloon/party", &values(entries)).unwrap();
        check("party", &meshes);
        meshes
    };
    // A wall stands 8 cm out of the wall plane (the front faces -y), past both ends of its
    // edge, about as high as the parapet top it is given.
    let wall = party(&[("length", json!(7)), ("top", json!(13))]);
    let points = slot_points(&wall, "wall");
    assert!((lowest(&points, 1) + 0.08).abs() < 1e-9);
    assert!((lowest(&points, 0) + 0.06).abs() < 1e-9 && (highest(&points, 0) - 7.06).abs() < 1e-9);
    assert!((12.6..=13.95).contains(&highest(&points, 2)));
    assert!(slot_points(&wall, "pane").is_empty());
    // A stub is a slice 1.1 to 1.9 m deep with windows on its faces, at the storeys it is
    // given: two storeys of 3.3 and 3 m under a parapet of 0.4 to 1 m.
    let stub = party(&[
        ("kind", json!("stub")),
        ("length", json!(7)),
        ("own", json!(2)),
    ]);
    let points = slot_points(&stub, "wall");
    assert!(
        (-1.9..=-1.1).contains(&lowest(&points, 1)),
        "{}",
        lowest(&points, 1)
    );
    assert!(
        (6.7..=7.3).contains(&highest(&points, 2)),
        "{}",
        highest(&points, 2)
    );
    assert!(!slot_points(&stub, "pane").is_empty() && !slot_points(&stub, "frame").is_empty());
    // Its seed picks its storeys when none are given.
    let tops: std::collections::BTreeSet<i64> = (0..8)
        .map(|seed| {
            let meshes = party(&[("kind", json!("stub")), ("seed", json!(seed))]);
            (highest(&slot_points(&meshes, "wall"), 2) * 10.) as i64
        })
        .collect();
    assert!(tops.len() > 2, "{tops:?}");
    assert!(
        generate(
            &natives,
            "kowloon/party",
            &values(&[("kind", json!("tower"))])
        )
        .is_err()
    );
    // A stub can be given its depth, and its front's ground storey left blank for shops: no
    // window or door below the first floor on its front, the storeys above as before.
    let shops = party(&[
        ("kind", json!("stub")),
        ("length", json!(7)),
        ("own", json!(3)),
        ("depth", json!(3.5)),
        ("shops", json!(true)),
        ("seed", json!(4)),
    ]);
    assert!((lowest(&slot_points(&shops, "wall"), 1) + 3.5).abs() < 1e-9);
    let front_low = |meshes: &[ItemMesh]| {
        ["pane", "frame"]
            .iter()
            .flat_map(|slot| slot_points(meshes, slot))
            .filter(|p| p[1] < -3.45 && p[2] < 3.)
            .count()
    };
    assert_eq!(front_low(&shops), 0);
    let lane = party(&[
        ("kind", json!("stub")),
        ("length", json!(7)),
        ("own", json!(3)),
        ("depth", json!(3.5)),
        ("seed", json!(4)),
    ]);
    assert!(front_low(&lane) > 0);
    assert!(slot_points(&shops, "pane").iter().any(|p| p[2] > 3.5));
    for refused in [json!(0.5), json!(13)] {
        assert!(
            generate(
                &natives,
                "kowloon/party",
                &values(&[("kind", json!("stub")), ("depth", refused)])
            )
            .is_err()
        );
    }
    // An end that faces a street (`gable`) has columns of windows across its depth, barred
    // ones in its ground storey and a soil stack up past its parapet; the other end and the
    // front stay as they were, and every window still reports its sill.
    let gabled = party(&[
        ("kind", json!("stub")),
        ("length", json!(7)),
        ("own", json!(3)),
        ("depth", json!(3.5)),
        ("shops", json!(true)),
        ("seed", json!(4)),
        ("gable", json!("end")),
    ]);
    let end_points = |meshes: &[ItemMesh], slot: &str| -> Vec<[f64; 3]> {
        slot_points(meshes, slot)
            .into_iter()
            .filter(|p| p[0] > 7.001)
            .collect()
    };
    let (plain_panes, street_panes) = (end_points(&shops, "pane"), end_points(&gabled, "pane"));
    assert!(
        street_panes.len() > plain_panes.len(),
        "{} panes on the street end against {}",
        street_panes.len(),
        plain_panes.len()
    );
    assert!(lowest(&street_panes, 2) < 3. && lowest(&plain_panes, 2) > 3.);
    // (the stack stands off the end, from the ground to over the parapet).
    let stack = end_points(&gabled, "metal");
    assert!(
        lowest(&stack, 2) < 0.1 && highest(&stack, 2) > highest(&slot_points(&gabled, "wall"), 2)
    );
    assert!(end_points(&shops, "metal").iter().all(|p| p[0] < 7.2));
    let untouched = |meshes: &[ItemMesh]| -> Vec<u64> {
        slot_points(meshes, "pane")
            .iter()
            .filter(|p| p[0] < 7.001)
            .flat_map(|p| p.map(f64::to_bits))
            .collect()
    };
    assert_eq!(untouched(&shops), untouched(&gabled));
    let sills = |entries: &[(&str, Value)]| {
        natives
            .features("kowloon/party", &values(entries))
            .into_iter()
            .filter(|f| f.name == "sill" && f.face == "end")
            .collect::<Vec<_>>()
    };
    let gable_entries = [
        ("kind", json!("stub")),
        ("length", json!(7)),
        ("own", json!(3)),
        ("depth", json!(3.5)),
        ("shops", json!(true)),
        ("seed", json!(4)),
        ("gable", json!("end")),
    ];
    let reported = sills(&gable_entries);
    assert!(reported.len() >= 3, "{} sills", reported.len());
    for sill in &reported {
        // Each lies on the end's plane, 6 cm under a drawn pane.
        assert!(sill.points.iter().all(|p| (p[0] - 7.).abs() < 1e-9));
        let (z, y) = (
            sill.points[0][2],
            (sill.points[0][1] + sill.points[1][1]) / 2.,
        );
        assert!(
            street_panes
                .iter()
                .any(|p| (p[1] - y).abs() < 0.6 && p[2] > z && p[2] - z < 0.2),
            "a sill at {y}, {z} is under a pane"
        );
    }
    assert!(
        generate(
            &natives,
            "kowloon/party",
            &values(&[("kind", json!("stub")), ("gable", json!("front"))])
        )
        .is_err()
    );
}

/// Every position of every mesh.
fn all_points(meshes: &[ItemMesh]) -> Vec<[f64; 3]> {
    meshes
        .iter()
        .flat_map(|m| m.positions.iter().copied())
        .collect()
}

#[test]
fn roof_kit_lamps_and_caged_units_take_their_options() {
    let natives = Natives::new();
    let make = |name: &str, entries: &[(&str, Value)]| {
        let meshes = generate(&natives, name, &values(entries)).unwrap();
        check(name, &meshes);
        meshes
    };
    // Grab rails: the stiles rise 1 m over the ladder's head and come down behind the wall.
    let ladder = make("kowloon/ladder", &[("height", json!(2.35))]);
    assert_eq!(
        fingerprint(&ladder),
        fingerprint(&make(
            "kowloon/ladder",
            &[("height", json!(2.35)), ("grab", json!(0))]
        ))
    );
    let grab = make(
        "kowloon/ladder",
        &[("height", json!(2.35)), ("grab", json!(1.0))],
    );
    let points = all_points(&grab);
    assert!(
        (highest(&points, 2) - (3.35 + 0.022)).abs() < 0.01,
        "{}",
        highest(&points, 2)
    );
    assert!(highest(&all_points(&ladder), 2) < 2.45);
    // Behind the wall (+Y; the ladder stands at its front) the rails reach down to the roof.
    assert!(points.iter().any(|p| p[1] > 0.1 && p[2] < 2.4));
    for bad in [json!(0.1), json!(2)] {
        assert!(generate(&natives, "kowloon/ladder", &values(&[("grab", bad)])).is_err());
    }
    // An aerial grows with its size and is guyed by its stays; left alone it is the seed's.
    let aerial = make("kowloon/aerial", &[]);
    assert_eq!(
        fingerprint(&aerial),
        fingerprint(&make(
            "kowloon/aerial",
            &[("size", json!(1)), ("stays", json!(0))]
        ))
    );
    let reach = |meshes: &[ItemMesh]| {
        all_points(meshes)
            .iter()
            .filter(|p| p[2] > 1.)
            .map(|p| p[0].hypot(p[1]))
            .fold(0., f64::max)
    };
    let large = make("kowloon/aerial", &[("size", json!(1.5))]);
    assert!(
        reach(&large) > reach(&aerial) * 1.3,
        "{} {}",
        reach(&large),
        reach(&aerial)
    );
    let guyed = make(
        "kowloon/aerial",
        &[
            ("stays", json!(3)),
            ("spread", json!(0.7)),
            ("bearing", json!(90)),
        ],
    );
    let feet: Vec<_> = all_points(&guyed)
        .into_iter()
        .filter(|p| p[2] < 0.03 && p[0].hypot(p[1]) > 0.5)
        .collect();
    assert!(!feet.is_empty());
    assert!(feet.iter().all(|p| p[0].hypot(p[1]) < 0.7 + 0.08));
    // The first stay points along +Y; none of them along -Y.
    assert!(feet.iter().any(|p| p[1] > 0.6) && feet.iter().all(|p| p[1] > -0.45));
    for bad in [
        ("size", json!(4)),
        ("stays", json!(5)),
        ("spread", json!(0.1)),
    ] {
        assert!(generate(&natives, "kowloon/aerial", &values(&[bad])).is_err());
    }
    // A tank's ladder turns to the side it is given; its shell takes a tint.
    let head = |meshes: &[ItemMesh]| {
        let points = all_points(meshes);
        let top = highest(&points, 2);
        let head: Vec<_> = points.into_iter().filter(|p| p[2] > top - 0.02).collect();
        let n = head.len() as f64;
        [
            head.iter().map(|p| p[0]).sum::<f64>() / n,
            head.iter().map(|p| p[1]).sum::<f64>() / n,
        ]
    };
    let seeded = head(&make("kowloon/water-tank", &[]));
    assert!(seeded[0] < -0.5 && seeded[1].abs() < 0.05, "{seeded:?}");
    let street = head(&make("kowloon/water-tank", &[("ladder", json!(270))]));
    assert!(street[1] < -0.5 && street[0].abs() < 0.05, "{street:?}");
    let shade = |meshes: &[ItemMesh]| {
        let colors: Vec<_> = meshes
            .iter()
            .filter(|m| m.material_slot == "brushed-metal")
            .flat_map(|m| &m.colors)
            .collect();
        assert!(!colors.is_empty());
        let n = colors.len() as f64;
        [0, 2].map(|k| colors.iter().map(|c| c[k]).sum::<f64>() / n)
    };
    let plain = shade(&make("kowloon/water-tank", &[]));
    assert!((plain[0] - plain[1]).abs() < 1e-9);
    let warm = shade(&make("kowloon/water-tank", &[("shell", json!("#f7f0e6"))]));
    assert!(warm[0] > warm[1], "{warm:?}");
    for bad in [
        vec![("ladder", json!(400))],
        vec![("kind", json!("box")), ("shell", json!("#ffffff"))],
    ] {
        assert!(generate(&natives, "kowloon/water-tank", &values(&bad)).is_err());
    }
    // A round bulkhead: a domed glass on a round plate, wider and taller than the boxed one.
    let boxed = make("kowloon/wall-lamp", &[]);
    assert_eq!(
        fingerprint(&boxed),
        fingerprint(&make("kowloon/wall-lamp", &[("style", json!("bulkhead"))]))
    );
    let round = make("kowloon/wall-lamp", &[("style", json!("round"))]);
    let span = |meshes: &[ItemMesh], slot: &str, axis: usize| {
        let points = slot_points(meshes, slot);
        highest(&points, axis) - lowest(&points, axis)
    };
    assert!(span(&round, "pane", 2) > span(&boxed, "pane", 2) + 0.03);
    assert!((span(&round, "pane", 0) - 0.173).abs() < 0.01);
    // The glass stands out of the wall (to -Y), off its plate.
    assert!(highest(&slot_points(&round, "pane"), 1) < -0.02);
    assert!(
        generate(
            &natives,
            "kowloon/wall-lamp",
            &values(&[("style", json!("square"))])
        )
        .is_err()
    );
    // A door bay can carry the round lamp over its door.
    let door = |lamp: &str| {
        make(
            "kowloon/ground-bay",
            &[
                ("kind", json!("door")),
                ("door", json!("double")),
                ("lamp", json!(lamp)),
            ],
        )
    };
    let (boxed_door, round_door) = (door("bulkhead"), door("round"));
    assert_ne!(fingerprint(&boxed_door), fingerprint(&round_door));
    assert!(triangles(&round_door) > triangles(&boxed_door));
    assert!(
        generate(
            &natives,
            "kowloon/ground-bay",
            &values(&[("lamp", json!("neon"))])
        )
        .is_err()
    );
    // A cage holding a unit has no middle rail across the unit's face (an empty cage's crosses
    // the middle of its front).
    let rails_across = |cage: &[ItemMesh], [z0, z1]: [f64; 2]| {
        cage.iter()
            .filter(|m| m.material_slot == "paint")
            .flat_map(|m| {
                m.indices.chunks_exact(3).map(|t| {
                    t.iter()
                        .map(|i| m.positions[*i as usize])
                        .collect::<Vec<_>>()
                })
            })
            .filter(|t| {
                t.iter()
                    .all(|p| p[1] < -0.52 + 0.04 && p[2] > z0 && p[2] < z1)
                    && lowest(t, 0) < 0.
                    && highest(t, 0) > 0.
            })
            .count()
    };
    let empty = make(
        "kowloon/cage",
        &[
            ("contents", json!("empty")),
            ("width", json!(0.98)),
            ("height", json!(0.68)),
            ("depth", json!(0.52)),
        ],
    );
    assert!(rails_across(&empty, [0.1, 0.5]) > 0);
    for seed in 0..6 {
        for height in [0.68, 0.6] {
            let cage = make(
                "kowloon/cage",
                &[
                    ("contents", json!("aircon")),
                    ("width", json!(0.98)),
                    ("height", json!(height)),
                    ("depth", json!(0.52)),
                    ("seed", json!(seed)),
                ],
            );
            let casing = slot_points(&cage, "aircon");
            let (x0, x1) = (lowest(&casing, 0), highest(&casing, 0));
            let (z0, z1) = (lowest(&casing, 2), highest(&casing, 2));
            assert!(x0 < 0. && x1 > 0.);
            let across = rails_across(&cage, [z0 + 0.02, z1 - 0.02]);
            assert_eq!(
                across, 0,
                "seed {seed}, height {height}: a rail over the unit"
            );
        }
    }
}

#[test]
fn the_riser_duct_can_stand_close_on_bolted_brackets_and_reports_its_wall_fixings() {
    use superspec_engine::itemspec::definition::Reach;
    let natives = Natives::new();
    let far_values = values(&[("height", json!(9))]);
    let close_values = values(&[("height", json!(9)), ("standoff", json!(0.06))]);
    let far = generate(&natives, "kowloon/riser-duct", &far_values).unwrap();
    let close = generate(&natives, "kowloon/riser-duct", &close_values).unwrap();
    check("kowloon/riser-duct standing close", &close);
    for bad in [0.02, 0.5] {
        let error = generate(
            &natives,
            "kowloon/riser-duct",
            &values(&[("height", json!(9)), ("standoff", json!(bad))]),
        )
        .unwrap_err();
        assert!(error.contains("standoff"), "{error}");
    }
    // The steel, not the runoff stains it draws on the wall.
    let points = |meshes: &[ItemMesh]| -> Vec<[f64; 3]> {
        meshes
            .iter()
            .filter(|m| m.material_slot.starts_with("vent-"))
            .flat_map(|m| m.positions.clone())
            .collect()
    };
    let out = |meshes: &[ItemMesh]| points(meshes).iter().map(|p| p[1]).fold(0., f64::min);
    // Toward -Y: the default body stands 0.32 m off in a cradle reaching past a metre; close,
    // its 0.62 m deep body stands 6 cm off and the hatch handle, 7 cm proud of it, is the
    // furthest out.
    assert!(out(&far) < -1.0, "{}", out(&far));
    assert!(
        (out(&close) + 0.06 + 0.62 + 0.0875).abs() < 0.005,
        "{}",
        out(&close)
    );
    // Behind the body's middle only its edge rings stand proud of its back, 3 cm into the 6 cm
    // gap to the wall.
    let (lo, hi) = (1.05, 9. - 0.72);
    let behind: Vec<f64> = points(&close)
        .iter()
        .filter(|p| p[0].abs() < 0.38 && p[2] > lo + 0.05 && p[2] < hi - 0.05)
        .map(|p| p[1])
        .collect();
    assert!(behind.iter().all(|y| *y <= -0.03 + 1e-9), "{behind:?}");
    assert!(
        points(&close)
            .iter()
            .any(|p| (p[1] + 0.06).abs() < 1e-9 && (p[2] - lo).abs() < 1e-9 && p[0].abs() < 0.4)
    );
    // Both report the wall features the duct items do, on the wall plane, for receivers within
    // 1 m: a pair of brackets at most every 1.4 m, evenly within each third of the body between
    // its edge rings (7.23 m: two levels a third).
    for (parameters, levels) in [(&far_values, 2), (&close_values, 6)] {
        let features = natives.features("kowloon/riser-duct", parameters);
        let count = |name: &str| features.iter().filter(|f| f.name == name).count();
        assert_eq!(count("fixture"), 1);
        assert_eq!(count("fixing"), 2 * levels + 4);
        assert_eq!(count("rust"), 2 * levels + 2);
        assert_eq!(count("drip"), 2);
        assert!(features.iter().all(|f| {
            f.reach == Reach::Item
                && f.within == 1.
                && f.points.iter().all(|p| p[1] == 0.)
                && (f.zone.is_some() == (f.name == "rust" || f.name == "drip"))
        }));
    }
    // Every close fixing lies across steel the duct bolts to the wall there: a plate's face.
    let all = points(&close);
    for fixing in natives
        .features("kowloon/riser-duct", &close_values)
        .iter()
        .filter(|f| f.name == "fixing")
    {
        let [a, b] = [fixing.points[0], fixing.points[1]];
        assert!(
            all.iter().any(|p| {
                p[0] >= a[0].min(b[0]) - 1e-9
                    && p[0] <= a[0].max(b[0]) + 1e-9
                    && (p[2] - a[2]).abs() <= 0.1 + 1e-9
                    && (-0.017..=-0.013).contains(&p[1])
            }),
            "no plate under {a:?}–{b:?}"
        );
    }
}

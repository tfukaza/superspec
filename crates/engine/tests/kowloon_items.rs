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

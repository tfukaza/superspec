use serde::Deserialize;
use std::collections::BTreeMap;
use superspec_aml::material::Material;
use superspec_engine::worldspec::themes::kowloon::kit_materials::Library;
#[derive(Deserialize)]
struct Case {
    initial: Vec<Material>,
    prefix: Option<String>,
    night: bool,
    bindings: serde_json::Value,
    materials: Vec<Material>,
}
#[derive(Deserialize)]
struct Golden {
    cases: Vec<Case>,
}
pub fn check_reference_kit_materials() {
    let golden: Golden = serde_json::from_str(include_str!(
        "../../../tests/fixtures/rust/kit-materials.json"
    ))
    .unwrap();
    for (i, c) in golden.cases.into_iter().enumerate() {
        let initial = c.initial.clone();
        let mut library = Library::new(&c.initial, c.prefix.as_deref(), c.night).unwrap();
        for _ in 0..2 {
            let bindings = library.bind("wall", &["substrate".into()]).unwrap();
            assert_eq!(
                serde_json::to_value(bindings).unwrap(),
                c.bindings,
                "bindings case {i}"
            );
        }
        let expected = c
            .materials
            .into_iter()
            .map(|m| (m.id.clone(), m))
            .collect::<BTreeMap<_, _>>();
        assert_eq!(library.materials, expected, "definitions case {i}");
        assert_eq!(c.initial, initial);
    }
}
#[test]
fn kit_bindings_preserve_authored_pbr_roles_and_night_glazing() {
    check_reference_kit_materials();
}

#[test]
fn building_sampling_slots_do_not_shadow_authored_names() {
    use superspec_engine::worldspec::themes::kowloon::building::materials::Appearance;
    let wall = Material {
        id: "metal".into(),
        color: "#d4c7b0".into(),
        repeat: 1.7,
        ..Material::default()
    };
    let metal = Material {
        id: "kit-metal".into(),
        color: "#215bca".into(),
        repeat: 3.2,
        metalness: 0.8,
        ..Material::default()
    };
    let mut library = Library::new(&[wall.clone(), metal.clone()], Some("kit"), false).unwrap();
    let appearance = Appearance::new(&mut library, "metal", &[]).unwrap();
    assert_eq!(appearance.inputs["wall"].color, wall.color);
    assert_eq!(appearance.inputs["wall"].repeat, 1.7);
    assert_eq!(appearance.inputs["metal"].color, metal.color);
    assert_eq!(appearance.inputs["metal"].repeat, 3.2);
    assert_eq!(appearance.inputs["metal"].metalness, 0.8);
    assert_eq!(appearance.bindings["wall"], "metal");
    assert_eq!(appearance.bindings["metal"], "kit-metal");
    assert_eq!(library.materials["metal"], wall);
    assert_eq!(library.materials["kit-metal"], metal);
    let ceramic = Material {
        id: "kit-wall-ceramic".into(),
        height_texture: Some("height.png".into()),
        height_scale: Some(0.1),
        ..Material::default()
    };
    let mut displaced = Library::new(&[wall, ceramic], Some("kit"), false).unwrap();
    assert!(
        Appearance::new(&mut displaced, "metal", &[])
            .err()
            .unwrap()
            .contains("displacement")
    );
}
#[test]
fn invalid_reserved_and_missing_materials_fail_without_poisoning_bindings() {
    let wall = Material {
        id: "wall".into(),
        ..Material::default()
    };
    assert!(Library::new(&[wall.clone(), wall.clone()], None, false).is_err());
    assert!(
        Library::new(
            &[Material {
                id: "$kowloon/paint".into(),
                ..wall.clone()
            }],
            None,
            false
        )
        .is_err()
    );
    assert!(
        Library::new(
            &[Material {
                roughness: f64::NAN,
                ..wall.clone()
            }],
            None,
            false
        )
        .is_err()
    );
    let mut library = Library::new(&[wall], None, false).unwrap();
    let before = library.materials.clone();
    assert!(library.bind("missing", &[]).is_err());
    assert!(library.bind("wall", &["missing".into()]).is_err());
    assert_eq!(library.materials, before);
    assert_eq!(library.bind("wall", &[]).unwrap().materials["wall"], "wall");
}

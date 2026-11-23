use superspec_aml::material::Material;
use superspec_engine::worldspec::themes::kowloon::kit_materials::Library;

#[test]
fn kit_bindings_are_repeatable_keep_authored_inputs_and_fall_back_to_shipped_finishes() {
    let initial = vec![
        Material {
            id: "wall".into(),
            color: "#caaf92".into(),
            repeat: 2.7,
            roughness: 0.8,
            metalness: 0.05,
            ..Material::default()
        },
        Material {
            id: "substrate".into(),
            color: "#777777".into(),
            repeat: 1.3,
            roughness: 0.95,
            metalness: 0.,
            ..Material::default()
        },
    ];
    for (prefix, night) in [(None, false), (Some("kit"), true)] {
        let copy = initial.clone();
        let mut library = Library::new(&copy, prefix, night).unwrap();
        let first =
            serde_json::to_value(library.bind("wall", &["substrate".into()]).unwrap()).unwrap();
        let second =
            serde_json::to_value(library.bind("wall", &["substrate".into()]).unwrap()).unwrap();
        assert_eq!(first, second, "binding twice is stable");
        assert_eq!(copy, initial, "authored inputs are not mutated");
        assert_eq!(library.materials["wall"], initial[0]);
        assert_eq!(library.materials["substrate"], initial[1]);
        // Slots no authored material declares bind the shipped catalog finishes.
        for slot in ["kitchen-worktop", "market-hero-props"] {
            let id = format!("builtin:{slot}");
            assert_eq!(first["materials"][slot], serde_json::json!(id));
            let shipped = superspec_aml::material_catalog::material(&id);
            assert!(shipped.is_some(), "{id} is a shipped material");
            assert_eq!(library.materials.get(&id).cloned(), shipped);
        }
        for id in first["materials"].as_object().unwrap().values() {
            assert!(
                library.materials.contains_key(id.as_str().unwrap()),
                "every binding names a defined material: {id}"
            );
        }
    }
}

#[test]
fn wall_sampling_slots_do_not_shadow_authored_names() {
    use superspec_engine::worldspec::themes::kowloon::wall_appearance::Appearance;
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
        id: "metal".into(),
        height_texture: Some("height.png".into()),
        height_scale: Some(0.1),
        ..Material::default()
    };
    let mut displaced = Library::new(&[ceramic], Some("kit"), false).unwrap();
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

use superspec_engine::{compile, compile_collision_with_assets};

#[test]
fn strips_and_tree_rows_follow_the_aml_and_keep_the_physical_base() {
    let aml = r#"<architecture version="1" id="block">
      <box id="ground" width="8" depth="5" height="0.1"/>
      <city>
        <road id="crossing" position="10 20 1" heading="90" length="8" width="3"/>
        <avenue id="walk" position="8 2 0.105" length="32" width="8.2"/>
        <sidewalk id="kerb" position="10 20 1.15" length="8" width="1.5"/>
        <tree id="elm" position="8 3 0.3" count="3" step="-7 0 0"/>
        <lamp id="lantern" position="12 2 0.15"/>
      </city></architecture>"#;
    let scene = compile(aml, 1).unwrap();
    let meshes = &scene.manifest.meshes;
    let road = meshes
        .iter()
        .find(|m| m.object_id == "city/crossing")
        .unwrap();
    for (got, want) in road
        .bounds
        .iter()
        .flatten()
        .zip([10., 20., 0.9, 13., 28., 1.])
    {
        assert!((got - want).abs() < 1e-5, "{:?}", road.bounds);
    }
    let tree = |i| {
        meshes
            .iter()
            .find(|m| m.object_id == format!("city/elm/{i}"))
            .unwrap()
    };
    assert!((tree(1).bounds[0][0] - tree(3).bounds[0][0] - 14.).abs() < 1e-5);
    assert_eq!(tree(1).source.line, 7);
    for id in ["walk", "kerb", "lantern"] {
        assert!(meshes.iter().any(|m| m.object_id == format!("city/{id}")));
    }
    let collision = compile_collision_with_assets(aml, 1, &|_| unreachable!()).unwrap();
    assert_eq!(scene.manifest.colliders, collision.manifest.colliders);
    assert!(
        collision
            .manifest
            .meshes
            .iter()
            .all(|m| !m.object_id.starts_with("city/"))
    );
}

#[test]
fn corner_building_resolves_its_nested_item_definitions() {
    let aml = include_str!("../src/worldspec/fixtures/corner-avenue.aml");
    let scene = compile(aml, 1).unwrap();
    assert!(
        scene
            .manifest
            .meshes
            .iter()
            .any(|m| m.object_id == "city/corner" && m.bounds[1][2] > 5.)
    );
}

#[test]
fn bad_rows_fail_at_the_placement_and_can_be_corrected() {
    let bad = "<architecture version=\"1\" id=\"block\"><city>\n<tree id=\"row\" count=\"3\" position=\"0 0 0\"/></city></architecture>";
    let error = superspec_profiles::parse(bad).unwrap_err();
    assert_eq!((error.code, error.source.line), ("AML_CITY", 2));
    let good = bad.replace("count=", "step=\"2 0 0\" count=");
    let parsed = superspec_profiles::parse(&good).unwrap();
    let serialized = superspec_profiles::serialize(&parsed).unwrap();
    assert!(serialized.contains("step=\"2 0 0\""));
    assert!(superspec_profiles::parse(&good.replace("count=\"3", "count=\"1025")).is_err());
}

#[test]
fn row_ids_and_dimensions_are_checked_before_rendering() {
    for body in [
        r#"<tree id="a" position="0 0 0" count="2" step="4 0 0"/><lamp id="a/1" position="3 0 0"/>"#,
        r#"<avenue id="narrow" width="1" position="0 0 0"/>"#,
    ] {
        let aml =
            format!(r#"<architecture version="1" id="test"><city>{body}</city></architecture>"#);
        let e = compile_collision_with_assets(&aml, 0, &|_| unreachable!()).unwrap_err();
        assert_eq!(e.code, "AML_CITY");
    }
}

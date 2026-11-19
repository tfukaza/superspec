use superspec_engine::{compile, compile_collision_with_assets, compile_with_assets};

fn compile_stocked_city(aml: &str) -> superspec_engine::Scene {
    let advertised: std::collections::BTreeSet<String> =
        serde_json::from_str(&superspec_engine::library_assets(aml)).unwrap();
    compile_with_assets(aml, 1, &|id| {
        assert!(advertised.contains(id), "Missing city dependency: {id}");
        let name = superspec_catalog::image_name(id).expect("shipped stock texture");
        std::fs::read(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../assets/materials")
                .join(name),
        )
        .map_err(|e| e.to_string())
    })
    .unwrap()
}

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
        scene
            .manifest
            .batches
            .iter()
            .find(|b| {
                b.object_id == format!("city/elm/{i}") && b.foliage.as_ref().unwrap().mass.is_some()
            })
            .unwrap()
    };
    for (i, x) in [(1, 8.), (2, 1.), (3, -6.)] {
        let batch = tree(i);
        assert_eq!(scene.batches.placements[batch.instance_start * 10], x);
        assert_eq!(batch.source.line, 7);
        assert!(
            !batch
                .foliage
                .as_ref()
                .unwrap()
                .mass
                .as_ref()
                .unwrap()
                .sites
                .is_empty()
        );
    }
    let lighting = scene.manifest.lighting.as_ref().unwrap();
    assert!(!lighting.contours.enabled);
    assert!(lighting.ambient_occlusion.enabled);
    assert!(lighting.lights.iter().any(|light| light.casts_shadow));
    assert!(scene.manifest.batches.iter().any(|b| {
        b.object_id == "city/walk"
            && b.foliage
                .as_ref()
                .unwrap()
                .mass
                .as_ref()
                .is_some_and(|m| m.look == "sprays")
    }));
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
    let scene = compile_stocked_city(aml);
    // Leaf-site counts from the original ItemSpec avenue's lane-plants document.
    let mass = |owner: &str| {
        scene
            .manifest
            .batches
            .iter()
            .filter(|b| b.object_id == owner)
            .find_map(|b| b.foliage.as_ref()?.mass.as_ref())
            .unwrap()
    };
    for (i, sites) in [1844, 1880, 1982, 1978].into_iter().enumerate() {
        let canopy = mass(&format!("city/median/{}", i + 1));
        assert_eq!(
            (canopy.look.as_str(), canopy.sites.len()),
            ("leaves", sites)
        );
    }
    assert_eq!(mass("city/understory/1").sites.len(), 608);
    let bed = mass("city/promenade");
    assert_eq!((bed.look.as_str(), bed.sites.len()), ("sprays", 52194));
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

#[test]
fn ordinary_four_storey_lots_generate_distinct_fitted_frontages() {
    for (seed, width, corners, fronts) in [(8, 14, true, 2), (9, 6, false, 1)] {
        let aml = format!(
            r#"<architecture version="1" id="test"><city>
          <building id="lot" position="0 0 0" floors="4" seed="{seed}" width="{width}" depth="6"/>
        </city></architecture>"#
        );
        let scene = compile_stocked_city(&aml);
        let meshes = &scene.manifest.meshes;
        let part = |path: &str| meshes.iter().any(|m| m.id.contains(path));
        assert_eq!(part("/part/cornerSign-"), corners);
        assert!(part("/part/streetSign-0/"));
        assert_eq!(part("/part/streetSign-1/"), fronts == 2);
        assert!(part("/lettering"));
        // The rear edge spans the authored width, independently of shop fittings and overhangs.
        assert!(meshes.iter().any(|m| m.id.contains("/part/side-2/")
            && m.bounds[1][0] - m.bounds[0][0] >= width as f64 - 0.75));
    }
}

#[test]
fn invalid_city_lots_fail_at_the_building() {
    for attrs in [
        r#"depth="5.9""#,
        r#"width="14.1""#,
        r#"seed="32""#,
        r#"seed="-1""#,
        r#"model="reference-corner" floors="8""#,
    ] {
        let aml = format!(
            "<architecture version=\"1\" id=\"test\"><city>\n<building id=\"lot\" position=\"0 0 0\" {attrs}/></city></architecture>"
        );
        let err = superspec_profiles::parse(&aml).unwrap_err();
        assert_eq!((err.code, err.source.line), ("AML_CITY", 2));
    }
}

#[test]
fn twelve_storey_infill_lot_fits_the_item_budget() {
    let aml = r#"<architecture version="1" id="test"><city>
        <building id="tower" position="0 0 0" seed="31" floors="12" width="14" depth="14"/>
    </city></architecture>"#;
    let scene = compile_stocked_city(aml);
    for part in ["/part/party-2/", "/part/party-3/", "/part/streetSign-0/"] {
        assert!(
            scene.manifest.meshes.iter().any(|m| m.id.contains(part)),
            "{part}"
        );
    }
    assert!(scene.manifest.meshes.iter().any(|m| m.bounds[1][2] > 36.0));
}
#[test]
fn concrete_members_keep_the_placement_and_all_six_weathering_surfaces() {
    let aml = r#"<architecture version="1" id="block"><city>
      <pillar id="pier" position="10 20 1" width="0.6" depth="0.8" height="3"/>
      <wall id="screen" position="1 2 0" heading="90" width="4" depth="0.25" height="2"/>
    </city></architecture>"#;
    let scene = compile(aml, 1).unwrap();
    for (id, bounds) in [
        ("city/pier", [10., 20., 1., 10.6, 20.8, 4.]),
        ("city/screen", [0.75, 2., 0., 1., 6., 2.]),
    ] {
        let mut meshes = scene.manifest.meshes.iter().filter(|m| m.object_id == id);
        let member = meshes.next().unwrap();
        assert!(meshes.next().is_none());
        assert_eq!(member.index_count, 36);
        for (value, expected) in member.bounds.iter().flatten().zip(bounds) {
            assert!((value - expected).abs() < 1e-5, "{id}: {:?}", member.bounds);
        }
        let normals: std::collections::BTreeSet<_> = scene.normals
            [3 * member.vertex_start..3 * (member.vertex_start + member.vertex_count)]
            .chunks_exact(3)
            .map(|n| [n[0].round() as i8, n[1].round() as i8, n[2].round() as i8])
            .collect();
        assert_eq!(normals.len(), 6);
    }
    assert!(scene.manifest.colliders.is_empty());
    let bad = aml.replace("height=\"3\"", "height=\"-1\"");
    let err = superspec_profiles::parse(&bad).unwrap_err();
    assert_eq!((err.code, err.source.line), ("AML_CITY", 2));
}

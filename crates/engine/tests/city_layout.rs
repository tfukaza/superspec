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
fn service_runs_follow_vertical_and_horizontal_legs() {
    let aml = r#"<architecture version="1" id="services"><city>
      <pipe-run id="riser" position="1 0 2" points="0 0 0; 0 0 2; 4 0 2" supports="-0.25 0 0; 0 0 0.25"/>
      <duct-run id="supply" position="1 0 2" points="0 0 0; 0 0 2; 4 0 2; 4 2 2"/>
    </city></architecture>"#;
    let scene = compile_stocked_city(aml);
    for (owner, skin) in [
        ("city/riser", "builtin:wall-rust"),
        ("city/supply", "builtin:galvanized-duct"),
    ] {
        let body = scene
            .manifest
            .meshes
            .iter()
            .find(|m| {
                m.object_id == owner
                    && m.material.as_deref() == Some(skin)
                    && m.bounds[1][0] - m.bounds[0][0] > 3.
            })
            .unwrap();
        assert!(body.bounds[0][2] <= 2.001 && body.bounds[1][2] >= 4.);
        assert!(
            scene
                .manifest
                .meshes
                .iter()
                .any(|m| m.object_id == owner && m.material.as_deref() == Some("builtin:metal"))
        );
    }
    assert!(scene.manifest.colliders.is_empty());
}

#[test]
fn ceiling_girder_seats_its_plates_and_braces_at_both_ends() {
    for length in [1.6, 6.3] {
        let scene = compile_stocked_city(&format!(
            r#"<architecture version="1" id="girder"><city>
              <ceiling-girder id="span" position="0 0 0" length="{length}"/>
            </city></architecture>"#
        ));
        let meshes = &scene.manifest.meshes;
        for (name, x) in [("bearing-0", 0.), ("bearing-1", length - 0.018)] {
            let mut bounds = [[f64::INFINITY; 3], [f64::NEG_INFINITY; 3]];
            for mesh in meshes.iter().filter(|m| m.id.contains(name)) {
                for axis in 0..3 {
                    bounds[0][axis] = bounds[0][axis].min(mesh.bounds[0][axis]);
                    bounds[1][axis] = bounds[1][axis].max(mesh.bounds[1][axis]);
                }
            }
            for (actual, expected) in
                bounds
                    .iter()
                    .flatten()
                    .zip([x, -0.115, -0.58, x + 0.018, 0.115, 0.])
            {
                assert!(
                    (actual - expected).abs() < 0.0001,
                    "{name}: {actual} vs {expected}"
                );
            }
        }
        for (name, x) in [("knee-0", 0.), ("knee-1", length - 0.65)] {
            let knee = meshes
                .iter()
                .find(|m| m.id.contains(name) && m.id.ends_with("/body"))
                .unwrap();
            assert!(knee.bounds[0][0] >= x && knee.bounds[1][0] <= x + 0.65);
            assert!(knee.bounds[0][2] < -0.52 && knee.bounds[1][2] >= -0.20);
        }
        assert!(meshes.iter().any(|m| m.id.contains("/web/")));
        assert!(scene.manifest.colliders.is_empty());
    }
}

#[test]
fn ceiling_girder_hangers_reach_only_the_surveyed_soffit_stations() {
    let aml = r#"<architecture version="1" id="hung"><city>
        <ceiling-girder id="hung" position="0 0 0" length="9"
            soffitGap="0.23" hangerStations="2.25 0 0; 4.5 0 0; 6.75 0 0"/>
        <ceiling-girder id="flush" position="0 3 0" length="9"/>
    </city></architecture>"#;
    let scene = compile_stocked_city(aml);
    let meshes = &scene.manifest.meshes;
    for (i, station) in [2.25, 4.5, 6.75].into_iter().enumerate() {
        let id = format!("soffit-foot-{i}/top");
        let foot = meshes.iter().find(|m| m.id.ends_with(&id)).unwrap();
        assert!((foot.bounds[0][2] - 0.23).abs() < 1e-5);
        assert!((foot.bounds[1][2] - 0.23).abs() < 1e-5);
        assert!((foot.bounds[0][0] + foot.bounds[1][0] - 2. * station).abs() < 1e-5);
        for side in 0..2 {
            let id = format!("hanger-strap-{i}-{side}/side");
            let strap = meshes.iter().find(|m| m.id.ends_with(&id)).unwrap();
            assert!((strap.bounds[0][2] - 0.012).abs() < 1e-5);
            assert!((strap.bounds[1][2] - 0.218).abs() < 1e-5);
        }
    }
    assert!(
        !meshes
            .iter()
            .any(|m| m.object_id == "city/flush" && m.bounds[1][2] > 0.00001)
    );
    assert!(scene.manifest.colliders.is_empty());
}

#[test]
fn extraction_grille_recesses_the_blades_between_the_backer_and_rim() {
    let scene = compile_stocked_city(
        r#"<architecture version="1" id="extraction"><city>
      <wall-extraction-grille id="grille" position="0 0 0"/>
    </city></architecture>"#,
    );
    let part = |name: &str| {
        scene
            .manifest
            .meshes
            .iter()
            .find(|mesh| mesh.object_id == "city/grille" && mesh.id.ends_with(name))
            .unwrap()
    };
    let blades = part("/blades");
    let frame = part("/frame");
    assert!(part("/seal").bounds[1][1].abs() < 1e-6);
    assert!(frame.bounds[0][1] < blades.bounds[0][1] - 0.005);
    assert!(part("/well").bounds[1][1] > blades.bounds[1][1] + 0.008);
    assert!((blades.bounds[1][0] - blades.bounds[0][0] - 0.596).abs() < 1e-5);
    assert_eq!(
        frame.material.as_deref(),
        Some("builtin:extraction-grille-frame")
    );
    assert!(scene.manifest.colliders.is_empty());
}

#[test]
fn sign_lamp_faces_outward_with_a_recessed_optic_and_wall_contact() {
    let scene = compile_stocked_city(
        r#"<architecture version="1" id="passage"><city>
      <passage-sign-lamp id="sign-light" position="0 0 2.2"/>
    </city></architecture>"#,
    );
    let part = |name: &str| {
        scene
            .manifest
            .meshes
            .iter()
            .find(|mesh| mesh.object_id == "city/sign-light" && mesh.id.ends_with(name))
            .unwrap()
    };
    let plate = part("/backplate");
    let hood = part("/hood");
    let lens = part("/glass");
    assert!(plate.bounds[1][1].abs() < 1e-5);
    assert!(plate.bounds[1][0] - plate.bounds[0][0] > 0.44);
    assert!(hood.bounds[0][1] < lens.bounds[0][1] - 0.02);
    assert!(lens.bounds[0][2] < 2.15 && lens.bounds[0][2] > 2.12);
    assert_eq!(lens.material.as_deref(), Some("builtin:sign-lamp-opal"));
    assert!(scene.manifest.colliders.is_empty());
}

#[test]
fn ceiling_fixtures_hang_below_the_mount_and_keep_the_guard_outside_the_glass() {
    let scene = compile_stocked_city(
        r#"<architecture version="1" id="ceiling"><city>
      <ceiling-bulkhead id="cage" position="0 0 3"/>
      <ceiling-twin-strip id="strip" position="2 0 3" length="1.5"/>
    </city></architecture>"#,
    );
    let meshes = &scene.manifest.meshes;
    assert!(meshes.iter().all(|m| m.bounds[1][2] <= 3.0001));
    let dome = meshes
        .iter()
        .filter(|m| {
            m.object_id == "city/cage"
                && m.material.as_deref() == Some("builtin:bulkhead-prismatic-glass")
        })
        .map(|m| m.bounds[0][2])
        .reduce(f64::min)
        .expect("bulkhead lens");
    assert!(meshes.iter().any(|m| m.object_id == "city/cage"
        && m.material.as_deref() == Some("builtin:fixture-enamel")
        && m.bounds[0][2] < dome - 0.01));
    let tubes: Vec<_> = meshes
        .iter()
        .filter(|m| {
            m.object_id == "city/strip" && m.material.as_deref() == Some("builtin:opal-diffuser")
        })
        .collect();
    assert_eq!(tubes.len(), 2);
    assert!(
        tubes
            .iter()
            .all(|m| (m.bounds[1][0] - m.bounds[0][0] - 1.44).abs() < 0.001)
    );
    assert!(tubes.iter().any(|m| m.bounds[1][1] < 0.) && tubes.iter().any(|m| m.bounds[0][1] > 0.));
    assert!(scene.manifest.colliders.is_empty());
}

#[test]
fn procedural_wall_dresses_an_existing_collider_without_changing_it() {
    let base = r#"<architecture version="1" id="wall">
      <box id="brush" position="2 3 0" width="0.5" depth="4" height="2.5" visible="false"/>
    </architecture>"#;
    let aml = base.replace(
        "</architecture>",
        r#"<city>
          <procedural-wall id="dressed" position="2.5 3 0" heading="90" width="4" depth="0.5" height="2.5" seed="7"/>
          <procedural-wall id="plain" position="8 3 0" width="4" depth="0.5" height="2.5" dressing="false"/>
        </city></architecture>"#,
    );
    let scene = compile_stocked_city(&aml);
    let material_for = |owner: &str, material: &str| {
        scene
            .manifest
            .meshes
            .iter()
            .find(|mesh| mesh.object_id == owner && mesh.material.as_deref() == Some(material))
    };
    let concrete = material_for("city/dressed", "builtin:concrete").unwrap();
    for (value, expected) in concrete
        .bounds
        .iter()
        .flatten()
        .zip([2., 3., 0., 2.5, 7., 2.5])
    {
        assert!((value - expected).abs() < 1e-5);
    }
    assert!(material_for("city/dressed", "builtin:wall-posters").is_some());
    assert!(material_for("city/plain", "builtin:wall-posters").is_none());
    assert!(
        scene
            .manifest
            .assets
            .iter()
            .any(|asset| asset.id == "superspec-material:posters-atlas.jpg")
    );
    let collision = compile_collision_with_assets(&aml, 1, &|_| unreachable!()).unwrap();
    let original = compile_collision_with_assets(base, 1, &|_| unreachable!()).unwrap();
    assert_eq!(collision.manifest.colliders, original.manifest.colliders);
    assert_eq!(scene.manifest.colliders, original.manifest.colliders);
}

#[test]
fn strips_and_tree_rows_follow_the_aml_and_keep_the_physical_base() {
    let aml = r#"<architecture version="1" id="block">
      <box id="ground" width="8" depth="5" height="0.1"/>
      <city>
        <road id="crossing" position="10 20 1" heading="90" length="8" width="3"/>
        <avenue id="walk" position="8 2 0.105" length="32" width="8.2"/>
        <sidewalk id="kerb" position="10 20 1.15" length="8" width="1.5"/>
        <tree id="elm" position="8 3 0.3" count="3" step="-7 0 0" scale="0.45"/>
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
        for level in scene
            .manifest
            .batches
            .iter()
            .filter(|b| b.object_id == batch.object_id)
        {
            let start = level.instance_start * 10;
            assert_eq!(&scene.batches.placements[start..start + 3], &[x, 3., 0.3]);
            assert_eq!(&scene.batches.placements[start + 7..start + 10], &[0.45; 3]);
        }
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
    let aml = include_str!("support/corner-avenue.aml");
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
fn grass_field_loads_the_local_companion_once_for_a_row() {
    let aml = r#"<architecture version="1" id="beds"><city>
        <grass-field id="court" src="planting/court.glb" position="1 2 3" count="2" step="4 0 0"/>
      </city></architecture>"#;
    let reads = std::cell::Cell::new(0);
    let scene = compile_with_assets(aml, 1, &|path| {
        assert_eq!(path, "planting/court.glb");
        reads.set(reads.get() + 1);
        Ok(include_bytes!("../src/worldspec/themes/kowloon/items/kowloon-weed-paint.glb").to_vec())
    })
    .unwrap();
    assert_eq!(reads.get(), 1);
    let [first, second] = scene.manifest.meshes.as_slice() else {
        panic!("two placed meshes")
    };
    assert!((second.bounds[0][0] - first.bounds[0][0] - 4.).abs() < 1e-5);
    assert_eq!(second.material, first.material);
    assert!(scene.manifest.colliders.is_empty());
    assert!(compile(aml, 1).unwrap_err().message.contains("court.glb"));
    let invalid = aml.replace("planting/court.glb", "../court.glb");
    assert_eq!(compile(&invalid, 1).unwrap_err().code, "AML_CITY");
}

#[test]
fn weed_rows_preserve_metric_roots_scale_and_heading() {
    let aml = r#"<architecture version="1" id="weeds"><city>
      <weed id="local" position="0 0 0"/>
      <weed id="row" position="7 2 1" heading="90" scale="0.5" count="2" step="3 0 0"/>
    </city></architecture>"#;
    let scene = compile(aml, 1).unwrap();
    let bounds = |owner: &str| {
        let (mut low, mut high) = ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]);
        for mesh in scene
            .manifest
            .meshes
            .iter()
            .filter(|m| m.object_id == owner)
        {
            for k in 0..3 {
                low[k] = low[k].min(mesh.bounds[0][k]);
                high[k] = high[k].max(mesh.bounds[1][k]);
            }
        }
        assert!(low[0].is_finite(), "{owner}");
        (low, high)
    };
    let (low, high) = bounds("city/local");
    for (id, x) in [("city/row/1", 7.), ("city/row/2", 10.)] {
        let (a, b) = bounds(id);
        let min = [x - 0.5 * high[1], 2. + 0.5 * low[0], 1. + 0.5 * low[2]];
        let max = [x - 0.5 * low[1], 2. + 0.5 * high[0], 1. + 0.5 * high[2]];
        for k in 0..3 {
            assert!((a[k] - min[k]).abs() < 1e-5 && (b[k] - max[k]).abs() < 1e-5);
        }
    }
    assert!(scene.manifest.colliders.is_empty());
    assert_eq!(
        scene.manifest.meshes.len(),
        6,
        "paint and blades on each root"
    );
    assert_eq!(
        scene.manifest.assets.len(),
        2,
        "shared pigment and leaf normals"
    );
    let clusters: Vec<_> = scene
        .manifest
        .meshes
        .iter()
        .filter(|m| m.id.contains("/underpainting/"))
        .collect();
    assert_eq!(clusters.len(), 3);
    for mesh in clusters {
        let material = scene
            .manifest
            .materials
            .iter()
            .find(|m| Some(&m.id) == mesh.material.as_ref())
            .unwrap();
        assert_ne!(material.texture, material.normal_texture);
        assert_eq!(material.normal_scale, Some(0.56));
        for texture in [&material.texture, &material.normal_texture] {
            assert!(
                scene
                    .manifest
                    .assets
                    .iter()
                    .any(|a| Some(&a.id) == texture.as_ref())
            );
        }
        let tangents = &scene.manifest.mesh_tangents[&mesh.id];
        assert_eq!(tangents.len(), mesh.vertex_count);
        assert!(tangents.iter().all(|t| t[3].abs() == 1.));
    }
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
fn roast_frontage_uses_the_food_atlas_behind_clear_display_glass() {
    let scene = compile_stocked_city(
        r#"<architecture version="1" id="market"><city>
        <building id="restaurant" position="0 0 0" floors="4" seed="4" front-trade="roast"/>
    </city></architecture>"#,
    );
    let bay = "/side-0/part/ground/ground/part/shop-0/shop-0/";
    for material in [
        "builtin:restaurant-food",
        "builtin:restaurant-tiles",
        "builtin:kitchen-metal",
        "builtin:kitchen-worktop",
        "builtin:display-glass",
        "builtin:market-hero-props",
    ] {
        assert!(
            scene
                .manifest
                .meshes
                .iter()
                .any(|mesh| mesh.id.contains(bay) && mesh.material.as_deref() == Some(material)),
            "{material}"
        );
    }
    assert!(
        !scene
            .manifest
            .meshes
            .iter()
            .any(|m| m.id.contains(bay) && m.id.contains("/part/stock/"))
    );
    let glass = scene
        .manifest
        .materials
        .iter()
        .find(|m| m.id == "builtin:display-glass")
        .unwrap();
    assert_eq!(glass.opacity, Some(0.055));
}

#[test]
fn invalid_city_lots_fail_at_the_building() {
    for attrs in [
        r#"footprint="0 0; 0 7; 9 7; 10 0""#,
        r#"footprint="0 0; 10 7; 10 0; 0 7""#,
        r#"footprint="0 0; 10 0; 11 7""#,
        r#"footprint="0 0; 10 0; 10 0; 0 7""#,
        r#"footprint="0 0; 10 0; 9 7; 0 7" width="10""#,
        r#"footprint="0 0; 10 0; 9 7; 0 7" model="reference-corner""#,
        r#"depth="5.9""#,
        r#"width="14.1""#,
        r#"seed="32""#,
        r#"seed="-1""#,
        r#"model="reference-corner" floors="8""#,
        r#"front-trade="unknown""#,
    ] {
        let aml = format!(
            "<architecture version=\"1\" id=\"test\"><city>\n<building id=\"lot\" position=\"0 0 0\" {attrs}/></city></architecture>"
        );
        let err = superspec_profiles::parse(&aml).unwrap_err();
        assert_eq!((err.code, err.source.line), ("AML_CITY", 2));
    }
}

#[test]
fn city_lot_corners_drive_the_roof_and_round_trip_in_local_metres() {
    let aml = r#"<architecture version="1" id="test"><city>
        <building id="corner" position="2 1 0" floors="4" seed="4" footprint="0 0; 10 0; 8.0001 7; 0 7"/>
    </city></architecture>"#;
    let canonical =
        superspec_profiles::serialize(&superspec_profiles::parse(aml).unwrap()).unwrap();
    assert!(canonical.contains(r#"footprint="0 0; 10 0; 8 7; 0 7""#));
    let scene = compile_stocked_city(&canonical);
    let roof = scene
        .manifest
        .meshes
        .iter()
        .find(|mesh| mesh.id.ends_with("/roof/roof/top"))
        .unwrap();
    let mut points = scene.positions
        [roof.vertex_start * 3..(roof.vertex_start + roof.vertex_count) * 3]
        .chunks_exact(3);
    // In the world frame the east roof edge slopes from x=12 to x=10 over 7 m of depth.
    let back: Vec<_> = points.clone().filter(|v| v[1] > 7.5).collect();
    assert!(!back.is_empty() && back.iter().all(|v| v[0] < 10.2));
    assert!(points.any(|v| v[0] > 11.5 && v[1] < 2.));
}

#[test]
fn repeated_building_designs_are_instanced_from_their_first_copy() {
    let aml = r#"<architecture version="1" id="test"><city>
        <building id="a" position="0 0 0" floors="3" seed="9"/>
        <building id="b" position="30 4 0" heading="90" floors="3" seed="9" count="2" step="0 15 0"/>
        <building id="c" position="-30 0 0" floors="4" seed="4"/>
    </city></architecture>"#;
    let scene = compile_stocked_city(aml);
    let meshes = |owner: &str| {
        scene
            .manifest
            .meshes
            .iter()
            .filter(|m| m.object_id == owner)
            .count()
    };
    let batches = &scene.manifest.batches;
    // The repeated design's batches share an ordered list of independently movable placements.
    assert!(!batches.is_empty());
    assert!(batches.iter().all(|b| b.object_id == "city/a"
        && b.template.as_deref() == Some(b.id.as_str())
        && b.id.starts_with("city/a/")));
    let placements = &scene.batches.placements;
    let first = &batches[0];
    let at = |index: usize| {
        &placements[(first.instance_start + index) * superspec_model::batches::PLACEMENT_FLOATS..]
            [..superspec_model::batches::PLACEMENT_FLOATS]
    };
    assert_eq!(first.instance_count, 3);
    assert_eq!(
        scene.manifest.batch_placement_owners["city/a"],
        ["city/a", "city/b/1", "city/b/2"]
    );
    assert_eq!(at(0), [0., 0., 0., 0., 0., 0., 1., 1., 1., 1.]);
    // The second copy: turned a quarter about Z and moved to its own origin.
    let second = at(1);
    assert!(
        (second[5] - std::f32::consts::FRAC_1_SQRT_2).abs() < 1e-6
            && (second[6] - std::f32::consts::FRAC_1_SQRT_2).abs() < 1e-6
    );
    assert!((second[0] - 30.).abs() < 1e-4 && (second[1] - 4.).abs() < 1e-4);
    // Kowloon buildings have no tangents, colliders or decals: the whole design is instanced, its
    // tinted lettering included, and the second copy owns no meshes. The design placed once stays
    // ordinary meshes.
    assert_eq!(
        (meshes("city/a"), meshes("city/b/1"), meshes("city/b/2")),
        (0, 0, 0)
    );
    assert!(meshes("city/c") > 100);
    assert!(batches.iter().any(|b| b.id.contains("/lettering")));
    // Every vertex tint of the template is kept in the batch buffers.
    assert!(scene.batches.colors.iter().any(|c| *c != 1.));
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
fn plant_edits_reuse_only_intrinsic_designs_and_match_a_fresh_scene() {
    let aml = r#"<architecture version="1" id="trees"><city>
        <tree id="single" position="0 0 0"/>
        <tree id="row" position="8 0 0" count="2" step="0 4 0"/>
        <shrub id="hedge" position="5 2 0" count="2" step="2 0 0"/>
    </city></architecture>"#;
    let session = superspec_engine::Session::default();
    let no_assets = |path: &str| Err(format!("unexpected companion {path}"));
    let before = session.compile(aml, 1, &no_assets).unwrap();
    let stats = session.bake_stats();
    // The tree at station 1 has its own seed, unlike identical station-0 trees or shrubs.
    assert_eq!((stats.plant_hits, stats.plant_misses), (2, 3));
    let changed = aml.replace(
        r#"id="single" position="0 0 0""#,
        r#"id="moved" position="7 8 2" heading="65" scale="0.65""#,
    );
    let edited = session.compile(&changed, 2, &no_assets).unwrap();
    let stats = session.bake_stats();
    assert_eq!((stats.plant_hits, stats.plant_misses), (7, 3));
    let group = edited
        .manifest
        .batches
        .iter()
        .filter(|b| b.object_id == "city/moved")
        .collect::<Vec<_>>();
    assert_eq!(group.len(), 4);
    for level in group {
        assert_eq!(&edited.batches.view(level).placements[..3], &[7., 8., 2.]);
    }
    assert!(
        before
            .manifest
            .batches
            .iter()
            .all(|b| b.object_id != "city/moved")
    );
    assert_eq!(
        edited.packet(),
        superspec_engine::compile(&changed, 2).unwrap().packet()
    );
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
#[test]
fn enclosed_battens_replace_both_tubes_and_fit_short_and_long_covers() {
    let scene = compile_stocked_city(
        r#"<architecture version="1" id="batten"><city>
      <ceiling-twin-strip id="short" position="0 0 0" length="0.4" enclosed="true"/>
      <ceiling-twin-strip id="long" position="0 1 0" length="2.4" enclosed="true"/>
    </city></architecture>"#,
    );
    assert!(scene.manifest.colliders.is_empty());
    for (owner, length) in [("city/short", 0.4), ("city/long", 2.4)] {
        let fixture: Vec<_> = scene
            .manifest
            .meshes
            .iter()
            .filter(|m| m.object_id == owner)
            .collect();
        let lens = fixture
            .iter()
            .find(|m| m.material.as_deref() == Some("builtin:batten-opal-glass"))
            .expect("enclosed cover has an opal lens");
        assert!((lens.bounds[1][0] - lens.bounds[0][0] - length + 0.060).abs() < 0.001);
        assert!((lens.bounds[0][2] + 0.10455).abs() < 0.001);
        assert!(
            fixture.iter().all(|m| m.bounds[1][2] <= 0.0001
                && m.material.as_deref() != Some("builtin:opal-diffuser"))
        );
        for slot in [
            "builtin:wall-cable",
            "builtin:cabinet-zinc",
            "builtin:batten-ivory",
            "builtin:fixture-enamel",
        ] {
            assert!(
                fixture.iter().any(|m| m.material.as_deref() == Some(slot)),
                "{owner} lacks {slot}"
            );
        }
    }
}

#[test]
fn eight_storey_kowloon_building_fits_the_placed_vertex_allowance() {
    let scene = compile_stocked_city(
        r#"<architecture version="1" id="tall-building">
        <city><building id="prototype" position="0 0 0" floors="8" seed="8"/></city>
    </architecture>"#,
    );
    assert!(scene.manifest.vertex_count <= 1_000_000);
    assert!(
        scene
            .manifest
            .meshes
            .iter()
            .any(|mesh| mesh.bounds[1][2] >= 24.)
    );
}

use superspec_engine::{Scene, compile};

fn scene(body: &str) -> Scene {
    compile(
        &format!(r#"<architecture version="1" id="test">{body}</architecture>"#),
        1,
    )
    .unwrap()
}
fn measure(scene: &Scene) -> (f64, f64) {
    let mut volume = 0.;
    let mut area = 0.;
    for mesh in &scene.manifest.meshes {
        if !scene.manifest.colliders.contains(&mesh.id) {
            continue;
        }
        for t in scene.indices[mesh.index_start..mesh.index_start + mesh.index_count]
            .as_chunks::<3>()
            .0
        {
            let p = t.map(|i| {
                let start = (mesh.vertex_start + i as usize) * 3;
                [
                    scene.positions[start] as f64,
                    scene.positions[start + 1] as f64,
                    scene.positions[start + 2] as f64,
                ]
            });
            use superspec_geometry::math::{cross, dot, length, sub};
            volume += dot(p[0], cross(p[1], p[2])) / 6.;
            area += length(cross(sub(p[1], p[0]), sub(p[2], p[0]))) / 2.;
        }
    }
    (volume, area)
}
fn near(actual: f64, expected: f64) {
    assert!((actual - expected).abs() < 1e-5, "{actual} != {expected}");
}
fn ray(scene: &Scene, origin: [f64; 3], direction: [f64; 3]) -> f64 {
    use superspec_geometry::math::*;
    let mut distance = f64::INFINITY;
    for mesh in &scene.manifest.meshes {
        if !scene.manifest.colliders.contains(&mesh.id) {
            continue;
        }
        for t in scene.indices[mesh.index_start..mesh.index_start + mesh.index_count]
            .as_chunks::<3>()
            .0
        {
            let p = t.map(|i| {
                let offset = (mesh.vertex_start + i as usize) * 3;
                [
                    scene.positions[offset] as f64,
                    scene.positions[offset + 1] as f64,
                    scene.positions[offset + 2] as f64,
                ]
            });
            let e1 = sub(p[1], p[0]);
            let e2 = sub(p[2], p[0]);
            let h = cross(direction, e2);
            let den = dot(e1, h);
            if den.abs() < 1e-10 {
                continue;
            }
            let s = sub(origin, p[0]);
            let u = dot(s, h) / den;
            let q = cross(s, e1);
            let v = dot(direction, q) / den;
            let d = dot(e2, q) / den;
            if u >= -1e-7 && v >= -1e-7 && u + v <= 1. + 1e-7 && d > 1e-7 {
                distance = distance.min(d);
            }
        }
    }
    distance
}

#[test]
fn physical_composition_removes_buried_faces() {
    for (kind, volume, area) in [
        ("union", 12., 32.),
        ("assembly", 12., 32.),
        ("intersect", 4., 16.),
        ("subtract", 4., 16.),
    ] {
        let output = scene(&format!(
            r#"<{kind} id="parts"><box id="a" width="2" depth="2" height="2"/><box id="b" width="2" depth="2" height="2" position="1 0 0"/></{kind}>"#
        ));
        let (v, a) = measure(&output);
        near(v, volume);
        near(a, area);
    }
}
#[test]
fn holed_extrusion_and_reflected_nonuniform_instance() {
    let output = scene(
        r#"<profile id="p" points="0 0;4 0;4 4;0 4"><hole points="1 1;1 3;3 3;3 1"/></profile><extrusion id="e" profile="p" distance="3"/>"#,
    );
    let (v, a) = measure(&output);
    near(v, 36.);
    near(a, 96.);
    let output = scene(
        r#"<definition id="part"><box id="a"/></definition><instance id="one" ref="part" scale="-2 3 4" position="2 0 0"/>"#,
    );
    near(measure(&output).0, 24.);
    let mesh = output
        .manifest
        .meshes
        .iter()
        .find(|m| m.id == "one/a")
        .unwrap();
    assert_eq!(mesh.bounds, [[0., 0., 0.], [2., 3., 4.]]);
}
#[test]
fn wall_opening_is_a_physical_cut() {
    let output = scene(
        r#"<path id="p" points="0 0 0;4 0 0"/><wall id="w" path="p" height="3" thickness="0.2"/><opening id="door" host="w" center="2 1" width="1" height="2"/>"#,
    );
    near(measure(&output).0, 2.);
    for (x, z) in [(2., 1.), (1.51, 0.01), (2.49, 1.99)] {
        assert!(ray(&output, [x, -1., z], [0., 1., 0.]).is_infinite());
    }
    for (x, z) in [(1.49, 1.), (2.51, 1.), (2., 2.01)] {
        near(ray(&output, [x, -1., z], [0., 1., 0.]), 0.9);
    }
}

#[test]
fn stair_undersides_retain_distinct_physical_volumes() {
    for (underside, thickness, volume) in [
        ("filled", 0.3, 2.7),
        ("sloped", 0.3, 1.2),
        ("stepped", 0.3, 0.9),
        ("separate", 0.1, 0.3),
        ("custom", 0.3, 1.65),
    ] {
        let custom = if underside == "custom" {
            r#" underside-profile="soffit""#
        } else {
            ""
        };
        let output = scene(&format!(
            r#"<path id="p" points="0 0 0;3 0 1"/><path id="soffit" points="0 -0.2 0;3 0.3 0"/><stairs id="s" path="p" width="1" count="5" thickness="{thickness}" underside="{underside}"{custom}/>"#
        ));
        near(measure(&output).0, volume);
        for i in 0..5 {
            for fraction in [0.2, 0.8] {
                near(
                    ray(
                        &output,
                        [(i as f64 + fraction) * 0.6, 0., 2.],
                        [0., 0., -1.],
                    ),
                    2. - (i + 1) as f64 * 0.2,
                );
            }
        }
        if underside == "separate" {
            assert!(ray(&output, [-1., 0., 0.05], [1., 0., 0.]).is_infinite());
        }
    }
    let descending = scene(
        r#"<path id="p" points="0 0 1;3 0 0"/><stairs id="s" path="p" width="1" count="5"/>"#,
    );
    for i in 0..5 {
        for fraction in [0.2, 0.8] {
            near(
                ray(
                    &descending,
                    [(i as f64 + fraction) * 0.6, 0., 2.],
                    [0., 0., -1.],
                ),
                2. - (5 - i) as f64 * 0.2,
            );
        }
    }
}

#[test]
fn maintained_circulation_fixture() {
    let output = compile(
        include_str!("../../../tests/fixtures/aml/circulation.aml"),
        1,
    )
    .unwrap();
    assert!(output.manifest.meshes.iter().any(|m| m.id == "sloped-rail"));
}

#[test]
fn terrain_grading_preserves_road_crossfall_transitions_and_host_frame() {
    let heights = vec!["0"; 81].join(" ");
    for placement in ["", r#"position="10 20 3" rotation="0 0 90" scale="2 1 2""#] {
        let output = scene(&format!(
            r#"<terrain id="ground" columns="9" rows="9" heights="{heights}" {placement}/><path id="road" points="1 4 1;7 4 4"/><grade id="g" host="ground" path="road" width="2" cross-slope="0.25" transition="2"/>"#
        ));
        if placement.is_empty() {
            for (y, z) in [
                (3., 2.25),
                (4., 2.5),
                (5., 2.75),
                (1., 0.),
                (7., 0.),
                (2., 1.125),
                (6., 1.375),
            ] {
                near(ray(&output, [4., y, 10.], [0., 0., -1.]), 10. - z);
            }
        } else {
            near(ray(&output, [5., 28., 10.], [0., 0., -1.]), 1.5);
        }
    }
}

#[test]
fn terrain_png_and_closure_preserve_local_heights() {
    let output = scene(
        r#"<terrain id="ground" columns="2" rows="2" heights="1 1 1 1" cell-size="2 2" base="0" position="0 0 10" scale="1 1 2"/>"#,
    );
    near(measure(&output).0, 8.);
    let raster = superspec_geometry::heightmap::decode(include_bytes!(
        "../../../tests/fixtures/aml/terrain-course/hill.png"
    ))
    .unwrap();
    assert_eq!((raster.width, raster.height), (5, 5));
    assert_eq!(raster.samples.iter().map(|v| *v as u32).sum::<u32>(), 2395);
    let png = include_bytes!("../../../tests/fixtures/aml/asymmetric-heightmap.png");
    for flip in [false, true] {
        let source = format!(
            r#"<architecture version="1" id="map"><terrain id="ground" heightmap="height.png" min-height="1" max-height="4" cell-size="2 3" flip-y="{flip}" position="10 20 5" scale="1 1 2"/></architecture>"#
        );
        let compiled = superspec_engine::compile_with_assets(&source, 0, &|path| {
            assert_eq!(path, "height.png");
            Ok(png.to_vec())
        })
        .unwrap();
        for (x, y, z) in [
            (0., 0., if flip { 3. } else { 1. }),
            (2., 0., if flip { 4. } else { 2. }),
            (0., 3., if flip { 1. } else { 3. }),
            (2., 3., if flip { 2. } else { 4. }),
        ] {
            near(
                ray(&compiled, [10. + x, 20. + y, 20.], [0., 0., -1.]),
                20. - (5. + 2. * z),
            );
        }
        for broken in [
            png[..png.len() - 1].to_vec(),
            [png.as_slice(), &[0]].concat(),
        ] {
            let failure =
                superspec_engine::compile_with_assets(&source, 1, &|_| Ok(broken.clone()))
                    .unwrap_err();
            assert!(failure.objects.contains(&"ground".into()));
            assert!(failure.source.offset > 0);
        }
    }
}

#[test]
fn maintained_cad_and_terrain_keep_architectural_metadata() {
    let cad = compile(
        include_str!("../../../tests/fixtures/aml/architecture-cad.aml"),
        1,
    )
    .unwrap();
    assert!(
        cad.manifest
            .meshes
            .iter()
            .any(|m| m.visible && m.material.as_deref() == Some("accent"))
    );
    let terrain = superspec_engine::compile_with_assets(
        include_str!("../../../tests/fixtures/aml/terrain-course.aml"),
        1,
        &|path| {
            assert_eq!(path, "terrain-course/hill.png");
            Ok(include_bytes!("../../../tests/fixtures/aml/terrain-course/hill.png").to_vec())
        },
    )
    .unwrap();
    let context = terrain.manifest.architecture.as_ref().unwrap();
    assert_eq!(context.volumes.len(), 5);
    assert_eq!(
        context
            .metadata
            .iter()
            .filter(|n| n.kind == "route")
            .count(),
        3
    );
    assert_eq!(terrain.manifest.navigation_validation, "not-run");
}

#[test]
fn opening_and_detail_use_host_coordinates_and_notches_reach_silhouettes() {
    for host in [
        r#"<wall id="host" path="axis" thickness="0.2" height="3"/>"#,
        r#"<assembly id="host"><box id="body" width="4" depth="0.2" height="3" position="0 -0.1 0"/></assembly>"#,
    ] {
        let source = format!(
            r#"<path id="axis" points="0 0 0;4 0 0"/><coordinate-frame id="shift" position="1 0 0"/>{host}<opening id="cut" host="host" center="1 1.5" width="0.5" height="1" frame="shift"/>"#
        );
        let output = scene(&source);
        assert!(ray(&output, [2., -1., 1.5], [0., 1., 0.]).is_infinite());
        near(ray(&output, [1., -1., 1.5], [0., 1., 0.]), 0.9);
        let detail = scene(
            &(source
                + r#"<panel id="panel" opening="cut" depth="0.05" offset="-0.1" position="0 0.5 0"/>"#),
        );
        near(ray(&detail, [2., -1., 1.5], [0., 1., 0.]), 1.4);
    }
    let invalid = r#"<architecture version="1" id="x"><path id="axis" points="0 0 2;4 0 2"/><wall id="host" path="axis" height="3"/><opening id="notch" host="host" center="2 1.5" width="0.5" height="0.8" mode="notch"/></architecture>"#;
    let error = compile(invalid, 1).unwrap_err();
    assert_eq!(error.code, "AML_OPENING");
    assert!(error.objects.contains(&"notch".into()));
    scene(
        r#"<path id="axis" points="0 0 0;4 0 0"/><path id="top" points="0 3 0;4 1 0"/><wall id="host" path="axis" top="top"/><opening id="notch" host="host" center="3.5 1.2" width="0.5" height="0.8" mode="notch"/>"#,
    );
    let corner = scene(
        r#"<path id="axis" points="0 0 0;2 0 0;2 2 0"/><wall id="host" path="axis" thickness="0.2" height="3"/><opening id="hole" host="host" center="2 1.5" width="1" height="1"/>"#,
    );
    near(ray(&corner, [2.05, -1., 1.5], [0., 1., 0.]), 1.5);
}

#[test]
fn face_material_partitions_do_not_change_canonical_collision() {
    let body = r##"<material id="accent" color="#ff0000"/><box id="body" width="4" depth="2" height="1"/><profile id="paint" points="1 0.5;3 0.5;3 1.5;1 1.5"/>"##;
    let original = scene(body);
    let painted = scene(&format!(
        r#"{body}<face-material id="finish" host="body" face="top" profile="paint" projection="xy" material="accent"/>"#
    ));
    near(measure(&painted).0, 8.);
    near(measure(&painted).1, 28.);
    let collider = |s: &Scene| {
        s.manifest
            .meshes
            .iter()
            .filter(|m| s.manifest.colliders.contains(&m.id))
            .flat_map(|m| {
                s.positions[m.vertex_start * 3..(m.vertex_start + m.vertex_count) * 3].to_vec()
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(collider(&original), collider(&painted));
    let mut area = 0.;
    for m in painted
        .manifest
        .meshes
        .iter()
        .filter(|m| m.visible && m.material.as_deref() == Some("accent"))
    {
        for triangle in painted.indices[m.index_start..m.index_start + m.index_count]
            .as_chunks::<3>()
            .0
        {
            use superspec_geometry::math::{cross, length, sub};
            let p = triangle.map(|i| {
                let start = (m.vertex_start + i as usize) * 3;
                std::array::from_fn(|j| painted.positions[start + j] as f64)
            });
            area += length(cross(sub(p[1], p[0]), sub(p[2], p[0]))) / 2.;
        }
    }
    near(area, 2.);
    let separate = scene(
        r#"<assembly id="a"><box id="hidden" visible="false"/><box id="decoration" collidable="false" position="3 0 0"/><box id="moving" dynamic="true" position="0.5 0 0"/></assembly>"#,
    );
    near(measure(&separate).0, 2.);
    near(ray(&separate, [0.25, -1., 0.5], [0., 1., 0.]), 1.);
    assert!(ray(&separate, [3.5, -1., 0.5], [0., 1., 0.]).is_infinite());
    assert!(
        !separate
            .manifest
            .meshes
            .iter()
            .any(|m| m.visible && m.id == "hidden")
    );
    assert!(
        separate
            .manifest
            .meshes
            .iter()
            .any(|m| m.visible && m.id == "decoration")
    );
}

#[test]
fn invalid_architecture_returns_source_errors_and_allows_recovery() {
    for body in [
        r#"<box id="bad" mystery="1"/>"#,
        r#"<wall id="bad" path="missing"/>"#,
        r#"<profile id="p" points="0 0;1 0;1 1;0 1"/><wall id="bad" path="p"/>"#,
        r#"<coordinate-frame id="a" frame="b"/><coordinate-frame id="b" frame="a"/><box id="bad" frame="a"/>"#,
        r#"<datum id="a" datum="b"/><datum id="b" datum="a"/>"#,
        r#"<definition id="a"><instance id="again" ref="a"/></definition><instance id="bad" ref="a"/>"#,
        r#"<box id="bad" scale="1 0 1"/>"#,
        r#"<path id="p" points="0 0 0;3 0 1"/><stairs id="bad" path="p" count="5" maximum-riser="0.2"/>"#,
        r#"<union id="bad"><box id="a"/><box id="b" dynamic="true"/></union>"#,
    ] {
        let source = format!(r#"<architecture version="1" id="test">{body}</architecture>"#);
        let error = compile(&source, 1).expect_err(body);
        assert_eq!(error.severity, "error", "{body}");
        assert!(!error.objects.is_empty(), "{error:?}");
        assert!(
            error
                .source
                .end
                .is_some_and(|end| end > error.source.offset),
            "{error:?}"
        );
        assert!(scene(r#"<box id="good"/>"#).manifest.success);
    }
}

#[test]
fn public_architectural_fixture_keeps_holes_and_nested_frame_placement() {
    let output = compile(
        include_str!("../../../tests/fixtures/aml/rust-architecture.aml"),
        0,
    )
    .unwrap();
    assert!(ray(&output, [2., 2., 4.], [0., 0., -1.]).is_infinite());
    near(ray(&output, [0.5, 2., 4.], [0., 0., -1.]), 2.);
    assert_eq!(
        output
            .manifest
            .meshes
            .iter()
            .find(|m| m.id == "placed/body")
            .unwrap()
            .bounds,
        [[7., 1., 0.], [8., 3., 3.]]
    );
    let context = serde_json::to_value(output.manifest.architecture).unwrap();
    let object = context["objects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["id"] == "placed/body")
        .unwrap();
    assert_eq!(object["sourceId"], "body");
    assert_eq!(object["instances"], serde_json::json!(["placed"]));
}

#[test]
fn excessive_generation_and_dependency_depth_fail_before_large_allocations() {
    let declarations = (0..100)
        .map(|i| format!(r#"<coordinate-frame id="a{i:03}" frame="a{:03}"/>"#, i + 1))
        .collect::<String>();
    for body in [
        r#"<sphere id="big" segments="16384"/>"#.to_owned(),
        declarations,
        r#"<path id="p"><arc center="0 0 0" radii="1 1" sweep="90"/></path>"#.into(),
    ] {
        let source = format!(
            r#"<architecture version="1" id="limits" curve-tolerance="1e-100">{body}</architecture>"#
        );
        let error = compile(&source, 0).unwrap_err();
        assert!(
            error.message.contains("budget") || error.message.contains("depth"),
            "{error:?}"
        );
    }
    scene(r#"<sphere id="normal" segments="16"/>"#);
    let groups = (0..45)
        .map(|i| format!(r#"<group id="g{i}" scale="10000000 10000000 10000000">"#))
        .collect::<String>();
    let source = format!(
        r#"<architecture version="1" id="overflow">{groups}{}</architecture>"#,
        "</group>".repeat(45)
    );
    assert_eq!(compile(&source, 0).unwrap_err().code, "AML_TRANSFORM");
}

#[test]
fn material_channels_uvs_and_asset_bytes_survive_native_compilation() {
    let png = include_bytes!("../../../tests/fixtures/aml/asymmetric-heightmap.png");
    let source = include_str!("../../../tests/fixtures/aml/rust-materials.aml");
    let output = superspec_engine::compile_with_assets(source, 0, &|path| {
        assert_eq!(path, "asymmetric-heightmap.png");
        Ok(png.to_vec())
    })
    .unwrap();
    let material = output
        .manifest
        .materials
        .iter()
        .find(|m| m.id == "paint")
        .unwrap();
    assert_eq!(material.emissive.as_deref(), Some("#112233"));
    assert_eq!(material.emissive_strength, Some(0.3));
    assert_eq!(material.opacity, Some(0.8));
    assert_eq!(output.asset_bytes, png);
    assert_eq!(output.manifest.assets.len(), 1);
    assert_eq!(output.manifest.asset_byte_count, png.len());
    assert_eq!(
        superspec_engine::compile_with_assets(source, 0, &|_| Ok(png[..8].to_vec()))
            .unwrap_err()
            .code,
        "AML_ASSET"
    );
    let mesh = output
        .manifest
        .meshes
        .iter()
        .find(|m| m.id == "surface")
        .unwrap();
    for i in mesh.vertex_start..mesh.vertex_start + mesh.vertex_count {
        if output.normals[i * 3 + 2] > 0.9 {
            near(
                output.uvs[i * 2] as f64,
                output.positions[i * 3] as f64 * 2.,
            );
            near(
                output.uvs[i * 2 + 1] as f64,
                output.positions[i * 3 + 1] as f64 * 2.,
            );
        }
    }
    for attributes in [
        r#"opacity="0.8" alpha-cutoff="0.5""#,
        r#"emissive-strength="1""#,
        r#"normal-scale="1""#,
        r#"repeat="0""#,
    ] {
        let source = format!(
            r#"<architecture version="1" id="x"><material id="bad" {attributes}/><box id="box" material="bad"/></architecture>"#
        );
        assert_eq!(compile(&source, 0).unwrap_err().code, "AML_MATERIAL");
    }
}

#[test]
fn connections_require_the_named_playable_physical_floor() {
    let source = r#"<architecture version="1" id="x"><profile id="p" points="0 0;2 0;2 2;0 2"/><roof id="a" profile="p"/><slab id="b" profile="p" position="3 0 0" playable="false"/><connection id="c" from="a" to="b"/></architecture>"#;
    assert_eq!(
        compile(source, 0).unwrap_err().code,
        "AML_CONNECTION_SUPPORT"
    );
    assert!(
        compile(
            &source.replace(r#"<roof id="a""#, r#"<roof playable="true" id="a""#),
            0
        )
        .is_ok()
    );
}

#[test]
fn intersected_clearance_reports_failure_and_recovers() {
    let source = r#"<architecture version="1" id="x"><profile id="p" points="0 0;2 0;2 2;0 2"/><box id="obstacle" position="0.5 0.5 0.5"/><clearance id="space" profile="p" height="2"/></architecture>"#;
    let failure = compile(source, 1).unwrap_err();
    assert_eq!(failure.code, "AML_CLEARANCE");
    assert!(failure.spatial.is_some());
    let source = source.replace(r#"<box id="obstacle" position="0.5 0.5 0.5"/>"#, "");
    assert!(compile(&source, 2).is_ok());
}

use serde_json::json;
use std::collections::BTreeMap;
use superspec_engine::materialspec::program::{self, *};

fn source(text: &str) -> Sources {
    Sources {
        entry: "main.ms".into(),
        modules: BTreeMap::from([("main.ms".into(), text.into())]),
    }
}
fn examples(entry: &str) -> Sources {
    Sources {
        entry: entry.into(),
        modules: BTreeMap::from([
            ("common.ms".into(), COMMON_HELPERS.into()),
            ("chipped-concrete.ms".into(), CHIPPED_CONCRETE.into()),
            ("painted-metal.ms".into(), PAINTED_METAL.into()),
        ]),
    }
}
#[test]
fn imported_helpers_parameters_and_inspectable_fields_drive_real_channels() {
    for entry in ["chipped-concrete.ms", "painted-metal.ms"] {
        let p = program::compile(&examples(entry)).unwrap();
        assert_eq!(p.metadata().dependencies, ["common.ms"]);
        assert!(p.metadata().parameters.len() >= 4);
        let sample = Sample {
            surface: [0.031, 0.149],
            footprint: 0.0001,
            ..Sample::default()
        };
        let inputs = Inputs {
            seed: 17,
            ..Inputs::default()
        };
        let a = p.evaluate(&inputs, &sample).unwrap();
        let b = p.evaluate(&inputs, &sample).unwrap();
        assert_eq!(
            serde_json::to_vec(&a).unwrap(),
            serde_json::to_vec(&b).unwrap()
        );
        assert!(a.roughness > 0.0 && a.roughness <= 1.0);
        assert!(!a.fields.is_empty());
        let changed = p
            .evaluate(
                &Inputs {
                    seed: 18,
                    ..Inputs::default()
                },
                &sample,
            )
            .unwrap();
        assert_ne!(
            serde_json::to_vec(&a).unwrap(),
            serde_json::to_vec(&changed).unwrap()
        );
        let raster = p
            .bake(
                &BakeRequest {
                    width: 12,
                    height: 8,
                    domain: Domain {
                        u: [0.12, 0., 0.],
                        v: [0., 0.08, 0.],
                        ..Domain::default()
                    },
                    inspection: Some(
                        if entry.starts_with("chipped") {
                            "chips"
                        } else {
                            "flakes"
                        }
                        .into(),
                    ),
                },
                &inputs,
            )
            .unwrap();
        assert_eq!(raster.base_color.len(), 12 * 8 * 4);
        assert_eq!(raster.normal.len(), 12 * 8 * 4);
        assert_eq!(raster.inspection.unwrap().len(), 12 * 8 * 4);
        assert!(raster.height_meters.iter().all(|v| v.is_finite()));
        assert!(
            raster
                .normal
                .chunks_exact(4)
                .any(|v| v[0] != 128 || v[1] != 128)
        );
    }
}

#[test]
fn compilation_errors_keep_module_utf16_location_and_recover() {
    let missing = source("material 1; // 🧱\nimport \"./absent.ms\"; output base_color = vec3(1);");
    let error = program::compile(&missing).err().unwrap().remove(0);
    assert_eq!(error.code, "MS_IMPORT_MISSING");
    assert_eq!(error.source.module, "main.ms");
    assert_eq!(error.source.location.line, 2);
    assert_eq!(
        error.source.location.offset,
        missing.modules["main.ms"]
            .split("\"./absent")
            .next()
            .unwrap()
            .encode_utf16()
            .count()
    );
    for (text, code) in [
        ("material 2; output base_color = vec3(1);", "MS_VERSION"),
        ("material 1; output base_color = true;", "MS_TYPE"),
        ("material 1; output base_color = absent;", "MS_NAME"),
        (
            "material 1; param x: scalar = 3 range(0,1); output base_color = vec3(x);",
            "MS_PARAMETER",
        ),
        (
            "material 1; fn loop(x: scalar) -> scalar = loop(x); output base_color = vec3(loop(1));",
            "MS_CYCLE",
        ),
    ] {
        assert_eq!(
            program::compile(&source(text)).err().unwrap()[0].code,
            code,
            "{text}"
        );
    }
    let mut cycle = source("material 1; import \"./b.ms\"; output base_color=vec3(1);");
    cycle
        .modules
        .insert("b.ms".into(), "material 1; import \"./main.ms\";".into());
    assert_eq!(
        program::compile(&cycle).err().unwrap()[0].code,
        "MS_IMPORT_CYCLE"
    );
    let good = program::compile(&source("material 1; output base_color=vec3(0.2);")).unwrap();
    assert_eq!(
        good.evaluate(&Inputs::default(), &Sample::default())
            .unwrap()
            .base_color,
        [0.2; 3]
    );
}

#[test]
fn parameter_changes_reuse_program_with_strict_types_ranges_and_recovery() {
    let p=program::compile(&source("material 1; param tint: vec3=vec3(0.2) range(0,1); param wet: bool=false; output base_color=tint; output roughness=select(wet,0.1,0.8);")).unwrap();
    for value in [json!([1, 2, 3]), json!("red"), json!([0.1, 0.2])] {
        let inputs = Inputs {
            parameters: BTreeMap::from([("tint".into(), value)]),
            ..Inputs::default()
        };
        assert_eq!(
            p.evaluate(&inputs, &Sample::default()).unwrap_err().code,
            "MS_PARAMETER"
        );
    }
    let inputs = Inputs {
        parameters: BTreeMap::from([
            ("tint".into(), json!([0.1, 0.3, 0.7])),
            ("wet".into(), json!(true)),
        ]),
        ..Inputs::default()
    };
    let changed = p.evaluate(&inputs, &Sample::default()).unwrap();
    assert_eq!(changed.base_color, [0.1, 0.3, 0.7]);
    assert_eq!(changed.roughness, 0.1);
    assert_eq!(
        p.evaluate(&Inputs::default(), &Sample::default())
            .unwrap()
            .roughness,
        0.8
    );
}

#[test]
fn physical_coordinates_named_seeds_and_footprint_have_observable_semantics() {
    let p=program::compile(&source("material 1; param size: scalar=0.1 range(0.001,2); let n=noise(surface/size,seed(\"pores\")); output base_color=vec3(n);")).unwrap();
    let a = p
        .evaluate(
            &Inputs::default(),
            &Sample {
                surface: [0.037, 0.021],
                ..Sample::default()
            },
        )
        .unwrap();
    let doubled = Inputs {
        parameters: BTreeMap::from([("size".into(), json!(0.2))]),
        ..Inputs::default()
    };
    let b = p
        .evaluate(
            &doubled,
            &Sample {
                surface: [0.074, 0.042],
                ..Sample::default()
            },
        )
        .unwrap();
    assert_eq!(
        a.base_color, b.base_color,
        "physical feature size follows meters, not image resolution"
    );
    let distant = p
        .evaluate(
            &Inputs::default(),
            &Sample {
                surface: [0.037, 0.021],
                footprint: 1.0,
                ..Sample::default()
            },
        )
        .unwrap();
    assert_eq!(distant.base_color, [0.5; 3]);
    assert_ne!(a.base_color, distant.base_color);
}

#[test]
fn bake_and_sample_share_color_height_channel_and_mapping_conventions() {
    let p=program::compile(&source("material 1; output base_color=vec3(0.25); output roughness=0.4; output metalness=0.7; output height=surface.x*0.1;")).unwrap();
    let b = p
        .bake(
            &BakeRequest {
                width: 4,
                height: 3,
                domain: Domain {
                    u: [2., 0., 0.],
                    ..Domain::default()
                },
                inspection: None,
            },
            &Inputs::default(),
        )
        .unwrap();
    assert_eq!(&b.base_color[..4], &[137, 137, 137, 255]);
    assert_eq!(&b.properties[..4], &[255, 102, 179, 255]);
    assert!(
        b.normal
            .chunks_exact(4)
            .all(|p| (113..=116).contains(&p[0]) && p[1] == 128 && p[2] >= 253)
    );
    assert!((b.height_meters[0] - 0.025).abs() < 1e-12);
    assert!((b.height_meters[3] - 0.175).abs() < 1e-12);
    assert_eq!(b.height_meters[..4], b.height_meters[4..8]);
}

#[test]
fn image_inputs_decode_linear_color_and_runoff_respects_finite_edges() {
    let p=program::compile(&source("material 1; output base_color=sample_color(\"grime\",uv); output roughness=runoff(0.1,2.0);")).unwrap();
    assert!(p.metadata().required_inputs.contains(&"image:grime".into()));
    let mut inputs = Inputs {
        images: BTreeMap::from([(
            "grime".into(),
            Image {
                width: 1,
                height: 1,
                rgba: vec![128, 128, 128, 255],
            },
        )]),
        edges: vec![Edge {
            start: [-0.5, 0., 1.],
            end: [0.5, 0., 1.],
        }],
        ..Inputs::default()
    };
    let sample = Sample {
        world: [0., 0., 0.5],
        normal: [0., 1., 0.],
        ..Sample::default()
    };
    let under = p.evaluate(&inputs, &sample).unwrap();
    assert!((under.base_color[0] - 0.21586).abs() < 0.00001);
    assert!(under.roughness > 0.1);
    let outside = p
        .evaluate(
            &inputs,
            &Sample {
                world: [2., 0., 0.5],
                ..sample.clone()
            },
        )
        .unwrap();
    assert_eq!(outside.roughness, 0.0);
    inputs.images.get_mut("grime").unwrap().rgba.pop();
    assert_eq!(p.evaluate(&inputs, &sample).unwrap_err().code, "MS_INPUT");
    inputs.images.get_mut("grime").unwrap().rgba.push(255);
    assert_eq!(
        p.evaluate(&inputs, &sample).unwrap().base_color,
        under.base_color
    );
}

#[test]
fn invalid_math_and_resource_requests_fail_without_poisoning_program() {
    let p = program::compile(&source(
        "material 1; param divisor: scalar=1; output base_color=vec3(1/divisor);",
    ))
    .unwrap();
    let bad = Inputs {
        parameters: BTreeMap::from([("divisor".into(), json!(0))]),
        ..Inputs::default()
    };
    assert_eq!(
        p.evaluate(&bad, &Sample::default()).unwrap_err().code,
        "MS_NONFINITE"
    );
    assert!(
        p.bake(
            &BakeRequest {
                width: usize::MAX,
                height: 2,
                domain: Domain::default(),
                inspection: None
            },
            &Inputs::default()
        )
        .is_err()
    );
    assert!(
        p.evaluate(
            &Inputs::default(),
            &Sample {
                footprint: f64::NAN,
                ..Sample::default()
            }
        )
        .is_err()
    );
    assert_eq!(
        p.evaluate(&Inputs::default(), &Sample::default())
            .unwrap()
            .base_color,
        [1.; 3]
    );
}

#[test]
fn relative_import_graph_is_precise_and_rejects_conflicts_and_root_escape() {
    let mut sources = Sources {
        entry: "materials/main.ms".into(),
        modules: BTreeMap::from([
            (
                "materials/main.ms".into(),
                "material 1; import \"../shared/base.ms\"; output base_color=coat;".into(),
            ),
            (
                "shared/base.ms".into(),
                "material 1; let coat=vec3(0.4,0.3,0.2);".into(),
            ),
            ("unused.ms".into(), "not a valid program".into()),
        ]),
    };
    let p = program::compile(&sources).unwrap();
    assert_eq!(
        p.source_modules().map(|(name, _)| name).collect::<Vec<_>>(),
        ["materials/main.ms", "shared/base.ms"]
    );
    sources
        .modules
        .insert("shared/base.ms".into(), "material 1; let noise=0.5;".into());
    assert_eq!(
        program::compile(&sources).err().unwrap()[0].code,
        "MS_DUPLICATE"
    );
    sources.modules.insert(
        "materials/main.ms".into(),
        "material 1; import \"../../outside.ms\"; output base_color=vec3(1);".into(),
    );
    assert_eq!(
        program::compile(&sources).err().unwrap()[0].code,
        "MS_IMPORT_PATH"
    );
    for text in [
        "material 1; let text=\"not a numeric field\"; output base_color=vec3(1);",
        "material 1; output base_color=vec3(true);",
        "material 1; output base_color=sample_color(\"\",uv);",
    ] {
        assert_eq!(
            program::compile(&source(text)).err().unwrap()[0].code,
            "MS_TYPE"
        );
    }
}

#[test]
fn syntax_and_expanding_helper_budgets_fail_safely_and_cancellation_recovers() {
    let long = format!(
        "material 1; output base_color=vec3({});",
        std::iter::repeat_n("0.001", 300)
            .collect::<Vec<_>>()
            .join("+")
    );
    assert_eq!(
        program::compile(&source(&long)).err().unwrap()[0].code,
        "MS_LIMIT"
    );
    let mut text =
        "material 1; param expensive: bool=false; fn f0(x: scalar) -> scalar=x;".to_owned();
    for i in 1..16 {
        text.push_str(&format!(
            "fn f{i}(x: scalar) -> scalar=(f{}(x)+f{}(x))*0.5;",
            i - 1,
            i - 1
        ));
    }
    text.push_str("output base_color=vec3(select(expensive,f15(0.5),0.5));");
    let p = program::compile(&source(&text)).unwrap();
    let inputs = Inputs {
        parameters: BTreeMap::from([("expensive".into(), json!(true))]),
        ..Inputs::default()
    };
    assert_eq!(
        p.evaluate(&inputs, &Sample::default()).unwrap_err().code,
        "MS_LIMIT"
    );
    let request = BakeRequest {
        width: 2,
        height: 2,
        domain: Domain::default(),
        inspection: None,
    };
    assert_eq!(
        p.bake_cancellable(&request, &Inputs::default(), &|| true)
            .unwrap_err()
            .code,
        "MS_CANCELLED"
    );
    assert_eq!(
        p.bake(
            &BakeRequest {
                inspection: Some("unknown".into()),
                ..request.clone()
            },
            &Inputs::default()
        )
        .unwrap_err()
        .code,
        "MS_INSPECTION"
    );
    assert_eq!(
        p.bake(&request, &Inputs::default())
            .unwrap()
            .base_color
            .len(),
        16
    );
}

/// The acceptance-only Wasm crate calls this exact fixture and compares its bytes with native output.
pub fn reference_program_result() -> Vec<u8> {
    let mut results = Vec::new();
    for entry in ["chipped-concrete.ms", "painted-metal.ms"] {
        let program = program::compile(&examples(entry)).unwrap();
        let inputs = Inputs {
            seed: 81,
            ..Inputs::default()
        };
        let samples = (0..6)
            .map(|i| {
                program
                    .evaluate(
                        &inputs,
                        &Sample {
                            uv: [i as f64 / 6., 0.4],
                            surface: [i as f64 * 0.021, 0.017],
                            object: [i as f64 * 0.021, 0., 0.017],
                            world: [i as f64 * 0.021 + 4., 2., 0.017],
                            footprint: 0.0004,
                            ..Sample::default()
                        },
                    )
                    .unwrap()
            })
            .collect::<Vec<_>>();
        let bake = program
            .bake(
                &BakeRequest {
                    width: 9,
                    height: 7,
                    domain: Domain {
                        u: [0.09, 0., 0.],
                        v: [0., 0.07, 0.],
                        ..Domain::default()
                    },
                    inspection: Some(
                        if entry.starts_with("chipped") {
                            "chips"
                        } else {
                            "flakes"
                        }
                        .into(),
                    ),
                },
                &inputs,
            )
            .unwrap();
        results.push(json!({"entry":entry,"samples":samples,"bake":bake}));
    }
    let image_program=program::compile(&source("material 1; let data=sample_data(\"source\",uv); output base_color=sample_color(\"source\",uv); output roughness=data.g; output metalness=data.b; output emissive=sample_color(\"source\",uv)*4;")).unwrap();
    let inputs = Inputs {
        images: BTreeMap::from([(
            "source".into(),
            Image {
                width: 2,
                height: 1,
                rgba: vec![0, 80, 200, 255, 240, 160, 20, 255],
            },
        )]),
        ..Inputs::default()
    };
    results.push(json!({"image":image_program.bake(&BakeRequest{width:5,height:3,domain:Domain::default(),inspection:Some("data".into())},&inputs).unwrap()}));
    serde_json::to_vec(&results).unwrap()
}

#[test]
fn portable_program_fixture_covers_the_same_native_and_wasm_evaluator() {
    let bytes = reference_program_result();
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(json.as_array().unwrap().len(), 3);
    for record in &json.as_array().unwrap()[..2] {
        assert_eq!(record["samples"].as_array().unwrap().len(), 6);
        for channel in [
            "baseColor",
            "normal",
            "properties",
            "emissive",
            "inspection",
        ] {
            assert_eq!(record["bake"][channel].as_array().unwrap().len(), 9 * 7 * 4);
        }
        assert_eq!(
            record["bake"]["heightMeters"].as_array().unwrap().len(),
            9 * 7
        );
    }
    assert_eq!(
        json[2]["image"]["baseColor"].as_array().unwrap().len(),
        5 * 3 * 4
    );
    assert!(json[2]["image"]["emissiveStrength"].as_f64().unwrap() > 1.);
    if let Ok(path) = std::env::var("MATERIAL_PROGRAM_PARITY_OUTPUT") {
        std::fs::write(path, bytes).unwrap();
    }
}

#[test]
fn hdr_emission_and_normal_inspection_preserve_the_evaluated_channels() {
    let p=program::compile(&source("material 1; output base_color=vec3(0.2); output height=surface.x*0.1; output emissive=vec3(4,2,0);")).unwrap();
    assert_eq!(
        p.evaluate(&Inputs::default(), &Sample::default())
            .unwrap()
            .emissive,
        [4., 2., 0.]
    );
    let baked = p
        .bake(
            &BakeRequest {
                width: 3,
                height: 2,
                domain: Domain::default(),
                inspection: Some("normal".into()),
            },
            &Inputs::default(),
        )
        .unwrap();
    assert_eq!(baked.emissive_strength, 4.);
    assert_eq!(&baked.emissive[..4], &[255, 188, 0, 255]);
    assert_eq!(baked.inspection.as_ref().unwrap(), &baked.normal);
}

#[test]
fn a_sample_batch_enforces_its_shared_work_allowance_without_mutating_the_program() {
    let p = program::compile(&source("material 1; output base_color=vec3(0.2);")).unwrap();
    let inputs = Inputs::default();
    let mut batch = p.evaluator_with_budget(&inputs, 5).unwrap();
    assert_eq!(
        batch.evaluate(&Sample::default()).unwrap().base_color,
        [0.2; 3]
    );
    assert_eq!(
        batch.evaluate(&Sample::default()).unwrap().base_color,
        [0.2; 3]
    );
    assert_eq!(
        batch.evaluate(&Sample::default()).unwrap_err().code,
        "MS_LIMIT"
    );
    assert!(p.evaluator_with_budget(&inputs, 0).is_err());
    assert!(
        p.evaluator(&inputs)
            .unwrap()
            .evaluate(&Sample::default())
            .is_ok()
    );
}

#[test]
fn uv_footprint_is_dimensionless_while_surface_feature_sizes_remain_in_meters() {
    let p = program::compile(&source(
        "material 1; output base_color=vec3(noise(uv*2,seed(\"UV\")));",
    ))
    .unwrap();
    let request = BakeRequest {
        width: 8,
        height: 8,
        domain: Domain::default(),
        inspection: None,
    };
    let a = p.bake(&request, &Inputs::default()).unwrap();
    let b = p
        .bake(
            &BakeRequest {
                domain: Domain {
                    u: [4., 0., 0.],
                    v: [0., 4., 0.],
                    ..Domain::default()
                },
                ..request
            },
            &Inputs::default(),
        )
        .unwrap();
    assert_eq!(
        a.base_color, b.base_color,
        "a UV-space field has the same sample coverage at the same resolution regardless of physical domain size"
    );
    let sample = Sample {
        uv: [0.13, 0.28],
        footprint: 100.,
        uv_footprint: Some(0.),
        ..Sample::default()
    };
    let resolved = p.evaluate(&Inputs::default(), &sample).unwrap();
    let filtered = p
        .evaluate(
            &Inputs::default(),
            &Sample {
                uv_footprint: Some(1.),
                ..sample
            },
        )
        .unwrap();
    assert_ne!(resolved.base_color, filtered.base_color);
    assert_eq!(filtered.base_color, [0.5; 3]);
}

#[test]
fn imported_unused_helpers_do_not_require_their_images_but_called_helpers_do() {
    let sources=Sources{entry:"main.ms".into(),modules:BTreeMap::from([
        ("main.ms".into(),"material 1; import \"./helpers.ms\"; output base_color=vec3(coverage(surface));".into()),
        ("helpers.ms".into(),"material 1; fn unused(p:vec2)->vec3=sample_color(\"optional.png\",p); fn coverage(p:vec2)->scalar=noise(p,seed(\"mask\"));".into()),
    ])};
    let p = program::compile(&sources).unwrap();
    assert!(
        !p.metadata()
            .required_inputs
            .contains(&"image:optional.png".into())
    );
    assert!(p.metadata().required_inputs.contains(&"seed".into()));
    assert!(p.evaluate(&Inputs::default(), &Sample::default()).is_ok());
    let mut used = sources;
    used.modules.insert(
        "main.ms".into(),
        "material 1; import \"./helpers.ms\"; output base_color=unused(surface);".into(),
    );
    let p = program::compile(&used).unwrap();
    assert!(
        p.metadata()
            .required_inputs
            .contains(&"image:optional.png".into())
    );
    assert_eq!(
        p.evaluate(&Inputs::default(), &Sample::default())
            .unwrap_err()
            .code,
        "MS_INPUT"
    );
}

#[test]
fn retention_charge_includes_repeated_source_marks_and_unused_owned_sources() {
    let mut source =
        String::from("material 1; fn blend(amount:scalar)->scalar=clamp(amount,0,1);\n");
    for i in 0..20 {
        source.push_str(&format!("let field{i}=blend({i}.0/20.0);\n"));
    }
    source.push_str("output base_color=vec3(field19);");
    let short = Sources {
        entry: "main.ms".into(),
        modules: BTreeMap::from([("main.ms".into(), source.clone())]),
    };
    let long_name = format!("{}/main.ms", "directory".repeat(100));
    let mut long = Sources {
        entry: long_name.clone(),
        modules: BTreeMap::from([(long_name.clone(), source)]),
    };
    let a = program::compile(&short).unwrap();
    let b = program::compile(&long).unwrap();
    assert!(
        b.retained_bytes() >= a.retained_bytes() + 40 * (long_name.len() - short.entry.len()),
        "each field retains its own declaration and expression source marks, beyond the source map"
    );
    long.modules
        .insert("not-imported.ms".into(), " ".repeat(40_000));
    let c = program::compile(&long).unwrap();
    assert!(
        c.retained_bytes() >= b.retained_bytes() + 40_000,
        "the Program owns supplied but unimported text too"
    );
    assert_eq!(
        a.evaluate(&Inputs::default(), &Sample::default())
            .unwrap()
            .base_color,
        c.evaluate(&Inputs::default(), &Sample::default())
            .unwrap()
            .base_color
    );
}

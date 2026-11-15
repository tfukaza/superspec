//! Noise, power, triplanar, varied image sampling and receiver bounds primitives, plus the
//! cheaper evaluation modes bakes use. Expectations come from closed forms and constructed images.
use std::collections::BTreeMap;
use superspec_engine::materialspec::program::{
    self, BakeRequest, Domain, FieldValue, Image, Inputs, Program, Sample, Sources,
};

fn ms(text: &str) -> Program {
    program::compile(&Sources {
        entry: "main.ms".into(),
        modules: BTreeMap::from([("main.ms".into(), text.into())]),
    })
    .unwrap_or_else(|e| panic!("{e:?}"))
}

fn field(inputs: &Inputs, sample: &Sample, expression: &str) -> Result<FieldValue, String> {
    let program = program::compile(&Sources {
        entry: "main.ms".into(),
        modules: BTreeMap::from([(
            "main.ms".into(),
            format!("material 1; let x = {expression}; output base_color = vec3(0.5);"),
        )]),
    })
    .map_err(|e| e[0].message.clone())?;
    let out = program.evaluate(inputs, sample).map_err(|e| e.message)?;
    Ok(out.fields["x"].clone())
}

fn scalar(expression: &str) -> f64 {
    match field(&Inputs::default(), &Sample::default(), expression).unwrap() {
        FieldValue::Scalar(v) => v,
        other => panic!("{other:?}"),
    }
}

fn vec3(inputs: &Inputs, sample: &Sample, expression: &str) -> [f64; 3] {
    match field(inputs, sample, expression).unwrap() {
        FieldValue::Vec3(v) => v,
        other => panic!("{other:?}"),
    }
}

fn close(a: [f64; 3], b: [f64; 3], tolerance: f64) -> bool {
    (0..3).all(|i| (a[i] - b[i]).abs() <= tolerance)
}

#[test]
fn hash_is_stable_per_lattice_cell_and_seed() {
    let a = scalar("hash(vec2(3.2, -1.7), 5.0)");
    assert_eq!(
        a,
        scalar("hash(vec2(3.9, -1.1), 5.0)"),
        "the same floor cell"
    );
    assert_ne!(a, scalar("hash(vec2(4.2, -1.7), 5.0)"), "a neighbor cell");
    assert_ne!(a, scalar("hash(vec2(3.2, -1.7), 6.0)"), "another seed");
    assert_ne!(
        a,
        scalar("hash(vec3(3.2, -1.7, 1.0), 5.0)"),
        "a third coordinate"
    );
    for value in [
        a,
        scalar("hash(2.0, 1.0)"),
        scalar("hash(vec3(-9.0, 4.0, 7.0), 0.0)"),
    ] {
        assert!((0.0..1.0).contains(&value), "{value}");
    }
}

#[test]
fn value_noise_is_smooth_bounded_and_fades_with_the_footprint() {
    let at = |x: f64, footprint: f64| match field(
        &Inputs::default(),
        &Sample {
            world: [x, 0.3, 0.7],
            footprint,
            ..Sample::default()
        },
        "value_noise(world, 2.0)",
    )
    .unwrap()
    {
        FieldValue::Scalar(v) => v,
        other => panic!("{other:?}"),
    };
    let mut previous = at(0., 0.);
    for step in 1..200 {
        let value = at(step as f64 * 0.01, 0.);
        assert!((0.0..=1.0).contains(&value));
        assert!((value - previous).abs() < 0.05, "continuous at step {step}");
        previous = value;
    }
    // A footprint of a whole feature size or more leaves only the mean.
    assert_eq!(at(0.37, 1.0), 0.5);
    let partial = at(0.37, 0.5);
    assert!((partial - 0.5).abs() < (at(0.37, 0.) - 0.5).abs() + 1e-12);
}

#[test]
fn pow_and_exp_follow_their_closed_forms() {
    assert!((scalar("pow(0.5, 3.0)") - 0.125).abs() < 1e-15);
    assert!((scalar("pow(9.0, 0.5)") - 3.).abs() < 1e-15);
    assert!((scalar("exp(-1.0)") - (-1f64).exp()).abs() < 1e-15);
    assert!(
        field(&Inputs::default(), &Sample::default(), "pow(-0.5, 2.0)")
            .unwrap_err()
            .contains("nonnegative")
    );
    assert!(
        field(&Inputs::default(), &Sample::default(), "exp(300.0)")
            .unwrap_err()
            .contains("overflow")
    );
}

#[test]
fn triplanar_weights_raise_the_normal_and_normalize() {
    let w = |n: [f64; 3], k: f64| {
        vec3(
            &Inputs::default(),
            &Sample::default(),
            &format!(
                "triplanar_weights(vec3({}, {}, {}), {k:?})",
                n[0], n[1], n[2]
            ),
        )
    };
    assert!(close(w([0., 0., 1.], 4.), [0., 0., 1.], 1e-15));
    assert!(close(w([1., -1., 0.], 1.), [0.5, 0.5, 0.], 1e-15));
    let (a, b) = (0.6f64.powi(4), 0.8f64.powi(4));
    assert!(close(
        w([0.6, 0., -0.8], 4.),
        [a / (a + b), 0., b / (a + b)],
        1e-12
    ));
}

/// An image whose texels alternate between two bytes in a 2x2 checker, or a solid byte.
fn image(size: usize, bytes: [u8; 2]) -> Image {
    let mut rgba = Vec::with_capacity(size * size * 4);
    for y in 0..size {
        for x in 0..size {
            let byte = bytes[(x + y) % 2];
            rgba.extend([byte, byte, byte, 255]);
        }
    }
    Image {
        width: size,
        height: size,
        rgba,
    }
}

fn inputs(images: &[(&str, Image)]) -> Inputs {
    Inputs {
        images: images
            .iter()
            .map(|(id, image)| ((*id).to_owned(), image.clone()))
            .collect(),
        ..Inputs::default()
    }
}

fn sample_at(world: [f64; 3], footprint: f64) -> Sample {
    Sample {
        world,
        footprint,
        ..Sample::default()
    }
}

#[test]
fn varied_sampling_keeps_a_solid_image_and_decodes_srgb() {
    let solid = inputs(&[("solid", image(8, [128, 128]))]);
    // The sRGB transfer function, from its definition.
    let linear = ((128. / 255. + 0.055) / 1.055f64).powf(2.4);
    for (x, y) in [(0.1, 0.2), (3.7, -2.4), (11.3, 5.9)] {
        for correction in ["0.0", "1.0"] {
            let color = vec3(
                &solid,
                &sample_at([x, y, 0.], 0.),
                &format!(
                    "varied_color(\"solid\", vec2(world.x, world.y), 3.0, 6.283, {correction})"
                ),
            );
            assert!(close(color, [linear; 3], 1e-6), "{color:?}");
        }
    }
}

#[test]
fn varied_sampling_restores_contrast_and_filters_to_the_mean() {
    // Black and white texels: the linear mean is one half.
    let checker = inputs(&[("checker", image(16, [0, 255]))]);
    let spread = |correction: &str, footprint: f64| {
        let values: Vec<f64> = (0..400)
            .map(|i| {
                let world = [
                    (i % 20) as f64 * 0.173 + 0.01,
                    (i / 20) as f64 * 0.191 + 0.02,
                    0.,
                ];
                vec3(
                    &checker,
                    &sample_at(world, footprint),
                    &format!(
                        "varied_color(\"checker\", vec2(world.x, world.y), 1.0, 0.32, {correction})"
                    ),
                )[0]
            })
            .collect();
        let mean = values.iter().sum::<f64>() / values.len() as f64;
        let variance = values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / values.len() as f64;
        (mean, variance)
    };
    let (_, blended) = spread("0.0", 0.);
    let (_, corrected) = spread("1.0", 0.);
    assert!(corrected > blended * 1.05, "{corrected} vs {blended}");
    // A footprint covering many texels samples the coarsest levels: the image mean.
    let (mean, variance) = spread("1.0", 4.);
    assert!(
        (mean - 0.5).abs() < 0.02 && variance < 1e-3,
        "{mean} {variance}"
    );
    // The same coordinate gives the same color.
    let again = |seed: &str| {
        vec3(
            &checker,
            &sample_at([1.234, 5.678, 0.], 0.),
            &format!("varied_color(\"checker\", vec2(world.x, world.y), {seed}, 6.283, 0.0)"),
        )
    };
    assert_eq!(again("1.0"), again("1.0"));
    assert!(
        field(
            &checker,
            &Sample::default(),
            "varied_color(\"missing\", uv, 1.0, 0.3, 1.0)"
        )
        .unwrap_err()
        .contains("not supplied")
    );
}

#[test]
fn receiver_bounds_are_sample_inputs_and_bakes_supply_their_rectangle() {
    let sample = Sample {
        bounds_min: [-1., 0., 0.5],
        bounds_max: [2., 3., 1.5],
        ..Sample::default()
    };
    let extent = vec3(&Inputs::default(), &sample, "bounds_max - bounds_min");
    assert_eq!(extent, [3., 3., 1.]);
    let program =
        ms("material 1; let e = bounds_max - bounds_min; output base_color = clamp(e, 0.0, 1.0);");
    assert!(
        program
            .metadata()
            .required_inputs
            .contains(&"bounds_max".to_owned())
    );
    let baked = program
        .bake(
            &BakeRequest {
                width: 2,
                height: 2,
                domain: Domain {
                    u: [0.5, 0., 0.],
                    v: [0., 0., 0.25],
                    ..Domain::default()
                },
                inspection: Some("e".into()),
            },
            &Inputs::default(),
        )
        .unwrap();
    // The inspection map normalizes nothing for vectors: 0.5 m by 0 m by 0.25 m encodes directly.
    let texel = &baked.inspection.unwrap()[..4];
    assert_eq!(texel, [128, 0, 64, 255]);
}

/// Small deterministic stand-ins for the recipes' images, so the fixture stays portable.
fn fixture_image(size: usize, salt: u32) -> Image {
    let mut rgba = Vec::with_capacity(size * size * 4);
    for i in 0..size * size {
        let h = (i as u32).wrapping_mul(2654435761) ^ salt.wrapping_mul(40503);
        rgba.extend([
            (h & 255) as u8,
            ((h >> 8) & 255) as u8,
            ((h >> 16) & 255) as u8,
            255,
        ]);
    }
    Image {
        width: size,
        height: size,
        rgba,
    }
}

/// The Kowloon ports and the primitives they use, evaluated on a swatch bake and on a crate's
/// creases. Native tests and the Wasm acceptance artifact must produce the same bytes.
pub fn kowloon_program_result() -> Vec<u8> {
    let modules: BTreeMap<String, String> = [
        (
            "kowloon-noise.material.ts",
            include_str!("../src/materialspec/program/examples/kowloon-noise.material.ts"),
        ),
        (
            "kowloon-concrete.material.ts",
            include_str!("../src/materialspec/program/examples/kowloon-concrete.material.ts"),
        ),
        (
            "kowloon-rusted-steel.material.ts",
            include_str!("../src/materialspec/program/examples/kowloon-rusted-steel.material.ts"),
        ),
    ]
    .into_iter()
    .map(|(name, text)| (name.to_owned(), text.to_owned()))
    .collect();
    let images: BTreeMap<String, Image> = [
        ("textures/concrete-plain.png", 24, 1),
        ("textures/concrete-fine-sand.png", 20, 2),
        ("superspec-material:building-plaster-plain.png", 16, 3),
        ("superspec-material:rust.png", 18, 4),
    ]
    .into_iter()
    .map(|(name, size, salt)| (name.to_owned(), fixture_image(size, salt)))
    .collect();
    // A 0.4 m crate: its creases drive edge wear, rims and the round-over.
    let (positions, indices) = {
        let (min, max) = ([0., 0., 0.], [0.4, 0.3, 0.25]);
        let corner = |x: usize, y: usize, z: usize| {
            [
                [min[0], max[0]][x],
                [min[1], max[1]][y],
                [min[2], max[2]][z],
            ]
        };
        let faces: [[[usize; 3]; 4]; 6] = [
            [[0, 0, 1], [1, 0, 1], [1, 1, 1], [0, 1, 1]],
            [[0, 1, 0], [1, 1, 0], [1, 0, 0], [0, 0, 0]],
            [[0, 0, 0], [1, 0, 0], [1, 0, 1], [0, 0, 1]],
            [[1, 1, 0], [0, 1, 0], [0, 1, 1], [1, 1, 1]],
            [[0, 1, 0], [0, 0, 0], [0, 0, 1], [0, 1, 1]],
            [[1, 0, 0], [1, 1, 0], [1, 1, 1], [1, 0, 1]],
        ];
        let (mut positions, mut indices) = (vec![], vec![]);
        for quad in faces {
            let base = (positions.len() / 3) as u32;
            for [x, y, z] in quad {
                positions.extend(corner(x, y, z));
            }
            indices.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
        }
        (positions, indices)
    };
    let features = program::receiver_features(&positions, &indices);
    let mut records = Vec::new();
    for (entry, parameters) in [
        ("kowloon-concrete.material.ts", serde_json::json!({})),
        (
            "kowloon-concrete.material.ts",
            serde_json::json!({ "ground": true, "plaster": true, "chipAmount": 0.6 }),
        ),
        ("kowloon-rusted-steel.material.ts", serde_json::json!({})),
    ] {
        let compiled = program::compile(&Sources {
            entry: entry.into(),
            modules: modules.clone(),
        })
        .unwrap_or_else(|e| panic!("{e:?}"));
        let inputs = Inputs {
            parameters: serde_json::from_value(parameters.clone()).unwrap(),
            seed: 7,
            images: images.clone(),
            ..Inputs::default()
        };
        let bake = compiled
            .bake(
                &BakeRequest {
                    width: 6,
                    height: 5,
                    domain: Domain {
                        u: [0.8, 0., 0.],
                        v: [0., 0., 0.6],
                        ..Domain::default()
                    },
                    inspection: None,
                },
                &inputs,
            )
            .unwrap_or_else(|e| panic!("{entry}: {e:?}"));
        let mut evaluator = compiled
            .evaluator_with_features(&inputs, &features)
            .unwrap();
        let samples: Vec<_> = [
            ([0.2, 0.15, 0.25], [0., 0., 1.]),
            ([0.395, 0.15, 0.25], [0., 0., 1.]),
            ([0.4, 0.01, 0.12], [1., 0., 0.]),
            ([0.21, 0.0, 0.003], [0., -1., 0.]),
        ]
        .into_iter()
        .map(|(world, normal)| {
            let sample = Sample {
                world,
                object: world,
                normal,
                footprint: 0.004,
                surface: [world[0], world[1]],
                bounds_min: [0., 0., 0.],
                bounds_max: [0.4, 0.3, 0.25],
                ..Sample::default()
            };
            serde_json::to_value(evaluator.evaluate_channels(&sample, None).unwrap()).unwrap()
        })
        .collect();
        records.push(serde_json::json!({
            "entry": entry,
            "parameters": parameters,
            "bake": { "baseColor": bake.base_color, "normal": bake.normal, "properties": bake.properties, "height": bake.height_meters },
            "samples": samples,
        }));
    }
    serde_json::to_vec(&records).unwrap()
}

#[test]
fn portable_kowloon_fixture_covers_the_same_native_and_wasm_ports() {
    let bytes = kowloon_program_result();
    let records: Vec<serde_json::Value> = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(records.len(), 3);
    for record in &records {
        assert_eq!(record["samples"].as_array().unwrap().len(), 4);
        assert_eq!(
            record["bake"]["baseColor"].as_array().unwrap().len(),
            6 * 5 * 4
        );
    }
    // The crate edge rusts or rounds differently from its face center.
    let steel = &records[2]["samples"];
    assert_ne!(steel[0], steel[1]);
    assert_eq!(kowloon_program_result(), bytes, "repeatable");
    if let Ok(path) = std::env::var("MATERIAL_KOWLOON_PARITY_OUTPUT") {
        std::fs::write(path, bytes).unwrap();
    }
}

#[test]
fn bake_evaluation_modes_match_full_evaluation() {
    let program = ms(
        "material 1; let n = fbm(vec2(world.x, world.y) / 0.1, seed(\"a\"), 3); let unused = n * 2.0; \
         output base_color = vec3(n); output roughness = n; output height = n * 0.01;",
    );
    let inputs = Inputs::default();
    let mut evaluator = program.evaluator(&inputs).unwrap();
    for i in 0..20 {
        let sample = sample_at([i as f64 * 0.137, i as f64 * 0.071, 0.], 0.002);
        let full = evaluator.evaluate(&sample).unwrap();
        let channels = evaluator.evaluate_channels(&sample, None).unwrap();
        let inspected = evaluator
            .evaluate_channels(&sample, Some("unused"))
            .unwrap();
        assert_eq!(channels.base_color, full.base_color);
        assert_eq!(channels.roughness, full.roughness);
        assert_eq!(channels.height, full.height);
        assert!(
            channels.fields.is_empty(),
            "no field is computed unless inspected"
        );
        assert_eq!(inspected.fields.len(), 1);
        assert_eq!(inspected.fields["unused"], full.fields["unused"]);
        assert_eq!(evaluator.evaluate_height(&sample).unwrap(), full.height);
    }
}

#[test]
fn two_dimensional_noise_ignores_padding_left_by_constructors() {
    // A splat fills the unused third slot with its value, and cos maps the padding to 1; neither
    // may change 2D noise, which depends only on the two components it names.
    for operation in ["noise", "cellular", "value_noise"] {
        let splat = scalar(&format!("{operation}(vec2(1.5), 7)"));
        let explicit = scalar(&format!("{operation}(vec2(1.5, 1.5), 7)"));
        assert_eq!(splat, explicit, "{operation}");
        let cosine = scalar(&format!("{operation}(cos(vec2(0.3, 0.8)), 7)"));
        let written = scalar(&format!("{operation}(vec2(cos(0.3), cos(0.8)), 7)"));
        assert_eq!(cosine, written, "{operation}");
    }
    let splat = scalar("fbm(vec2(2.25), 3, 4)");
    assert_eq!(splat, scalar("fbm(vec2(2.25, 2.25), 3, 4)"));
}

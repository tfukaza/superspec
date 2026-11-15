//! Mask and profile primitives: remap, soft thresholds, distance bands and mask combinations.
//! Expectations are computed here from the documented formulas, not from the evaluator.
use std::collections::BTreeMap;
use superspec_engine::materialspec::program::{self, FieldValue, Inputs, Program, Sample, Sources};

fn ms(text: &str) -> Result<Program, Vec<program::ProgramDiagnostic>> {
    program::compile(&Sources {
        entry: "main.ms".into(),
        modules: BTreeMap::from([("main.ms".into(), text.into())]),
    })
}

/// Evaluates `expression` as the scalar field `x` at a sample with the given footprint.
fn eval(expression: &str, footprint: f64) -> Result<f64, String> {
    let program = ms(&format!(
        "material 1; let x = {expression}; output base_color = vec3(0.5);"
    ))
    .map_err(|e| e[0].message.clone())?;
    let out = program
        .evaluate(
            &Inputs::default(),
            &Sample {
                footprint,
                ..Sample::default()
            },
        )
        .map_err(|e| e.message)?;
    match out.fields["x"] {
        FieldValue::Scalar(v) => Ok(v),
        ref other => Err(format!("{other:?}")),
    }
}

fn smooth(t: f64) -> f64 {
    let t = t.clamp(0., 1.);
    t * t * (3. - 2. * t)
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-12
}

#[test]
fn remap_maps_ranges_with_an_explicit_mode() {
    assert!(near(
        eval("remap(\"clamp\", 0.5, 0.0, 2.0, 10.0, 20.0)", 0.).unwrap(),
        12.5
    ));
    assert!(near(
        eval("remap(\"clamp\", 3.0, 0.0, 2.0, 10.0, 20.0)", 0.).unwrap(),
        20.
    ));
    assert!(near(
        eval("remap(\"extrapolate\", 3.0, 0.0, 2.0, 10.0, 20.0)", 0.).unwrap(),
        25.
    ));
    // A descending output range inverts the value.
    assert!(near(
        eval("remap(\"clamp\", 0.25, 0.0, 1.0, 1.0, 0.0)", 0.).unwrap(),
        0.75
    ));
    // Vectors map componentwise.
    let program = ms(
        "material 1; let v = remap(\"clamp\", vec2(-1.0, 0.5), 0.0, 1.0, 0.0, 4.0); output base_color = vec3(0.5);",
    )
    .unwrap();
    let out = program
        .evaluate(&Inputs::default(), &Sample::default())
        .unwrap();
    assert_eq!(out.fields["v"], FieldValue::Vec2([0., 2.]));
    for reversed in [
        "remap(\"clamp\", 0.5, 1.0, 1.0, 0.0, 1.0)",
        "remap(\"clamp\", 0.5, 1.0, 0.0, 0.0, 1.0)",
    ] {
        assert!(
            eval(reversed, 0.)
                .unwrap_err()
                .contains("strictly increasing"),
            "{reversed}"
        );
    }
    let Err(errors) = ms(
        "material 1; let x = remap(\"wrap\", 0.5, 0.0, 1.0, 0.0, 1.0); output base_color = vec3(0.5);",
    ) else {
        panic!("an unknown remap mode must not compile");
    };
    assert_eq!(errors[0].code, "MS_TYPE");
    assert!(errors[0].message.contains("clamp"), "{}", errors[0].message);
}

#[test]
fn soft_threshold_rises_across_its_softness_and_feathers_with_the_value_variation() {
    for (value, expected) in [
        (0.5, 0.),
        (0.55, 0.),
        (0.6, 0.5),
        (0.625, smooth(0.75)),
        (0.65, 1.),
        (0.9, 1.),
    ] {
        let got = eval(&format!("soft_threshold({value}, 0.6, 0.1)"), 0.).unwrap();
        assert!(near(got, expected), "{value}: {got} != {expected}");
    }
    // A varying value widens the transition: at the same offset the mask is less decided.
    let sharp = eval(
        "soft_threshold(fbm(vec2(world.x, world.y) / 0.01, seed(\"a\"), 2), 0.5, 0.02)",
        0.,
    )
    .unwrap();
    let feathered = eval(
        "soft_threshold(fbm(vec2(world.x, world.y) / 0.01, seed(\"a\"), 2), 0.5, 0.02)",
        0.004,
    )
    .unwrap();
    assert!((0.0..=1.0).contains(&sharp) && (0.0..=1.0).contains(&feathered));
    assert!(
        (feathered - 0.5).abs() <= (sharp - 0.5).abs() + 1e-12,
        "{sharp} vs {feathered}"
    );
    assert!(
        eval("soft_threshold(0.5, 0.5, 0.0)", 0.)
            .unwrap_err()
            .contains("positive")
    );
}

#[test]
fn distance_band_falls_from_inner_to_outer_meters() {
    for (distance, expected) in [
        (0.0, 1.),
        (0.005, 1.),
        (0.0125, 1. - smooth(0.5)),
        (0.02, 0.),
        (1.0, 0.),
    ] {
        let got = eval(&format!("distance_band({distance}, 0.005, 0.02)"), 0.).unwrap();
        assert!(near(got, expected), "{distance}: {got} != {expected}");
    }
    // The distance's footprint softens both sides: a coarse sample just outside the band is partly covered.
    let coarse = eval(
        "distance_band(length(vec2(world.x, world.y) - vec2(0.021, 0.0)), 0.005, 0.02)",
        0.004,
    )
    .unwrap();
    let fine = eval(
        "distance_band(length(vec2(world.x, world.y) - vec2(0.021, 0.0)), 0.005, 0.02)",
        0.,
    )
    .unwrap();
    assert_eq!(fine, 0.);
    assert!(coarse > 0. && coarse < 0.1, "{coarse}");
    for bad in [
        "distance_band(0.1, 0.02, 0.02)",
        "distance_band(0.1, -0.01, 0.02)",
    ] {
        assert!(eval(bad, 0.).unwrap_err().contains("ordered"), "{bad}");
    }
}

#[test]
fn mask_combinations_clamp_their_inputs() {
    for (a, b) in [(0.25, 0.5), (0.0, 1.0), (1.0, 1.0), (-0.5, 2.0), (0.7, 0.3)] {
        let (x, y) = (f64::clamp(a, 0., 1.), f64::clamp(b, 0., 1.));
        let union = eval(&format!("soft_union({a}, {b})"), 0.).unwrap();
        let intersect = eval(&format!("mask_intersect({a}, {b})"), 0.).unwrap();
        let subtract = eval(&format!("mask_subtract({a}, {b})"), 0.).unwrap();
        assert!(near(union, 1. - (1. - x) * (1. - y)), "union {a} {b}");
        assert!(near(intersect, x * y), "intersect {a} {b}");
        assert!(near(subtract, x * (1. - y)), "subtract {a} {b}");
    }
}

#[test]
fn recipes_spell_the_primitives_in_camel_case() {
    let program = program::compile(&Sources {
        entry: "main.material.ts".into(),
        modules: BTreeMap::from([(
            "main.material.ts".into(),
            "import { defineMaterial, remap, softThreshold, distanceBand, softUnion, maskIntersect, maskSubtract, vec3 } from \"@superspec/material/v1\";\n\
             const a = softThreshold(0.6, 0.6, 0.1);\n\
             const b = distanceBand(0.0125, 0.005, 0.02);\n\
             const c = maskSubtract(softUnion(a, b), maskIntersect(a, b));\n\
             export default defineMaterial({ base: { baseColor: vec3(0.5, 0.5, 0.5), roughness: remap(\"clamp\", c, 0, 1, 0.2, 0.9) } });\n"
                .into(),
        )]),
    })
    .unwrap_or_else(|e| panic!("{e:?}"));
    let out = program
        .evaluate(&Inputs::default(), &Sample::default())
        .unwrap();
    let (a, b) = (0.5, 1. - smooth(0.5));
    let union = 1. - (1. - a) * (1. - b);
    let c = union * (1. - a * b);
    assert!(near(out.roughness, 0.2 + c * 0.7), "{}", out.roughness);
}

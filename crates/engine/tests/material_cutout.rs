//! Cutouts, authored masks, color ramps and the decal and layer recipe kinds: what each
//! evaluates to against values worked out apart from the evaluator, what a recipe declares
//! about itself, and the diagnostics of the ways each can be written wrong.
use std::sync::Arc;
use superspec_engine::materialspec::program::{
    self, Blend, Evaluator, FieldSelection, Image, Inputs, OutputSet, Program, RecipeKind, Sample,
    Sources,
};

fn compile(entry: &str, text: &str) -> Result<Program, Vec<program::ProgramDiagnostic>> {
    program::compile(&Sources {
        entry: entry.into(),
        modules: [(entry.to_owned(), text.to_owned())].into(),
    })
}

fn source(text: &str) -> Program {
    compile("main.ms", text).unwrap_or_else(|e| panic!("{e:?}"))
}

fn recipe(text: &str) -> Program {
    compile("main.material.ts", text).unwrap_or_else(|e| panic!("{e:?}"))
}

fn refused(entry: &str, text: &str) -> String {
    let errors = compile(entry, text).err().expect("the recipe is refused");
    errors
        .iter()
        .map(|error| error.message.clone())
        .collect::<Vec<_>>()
        .join("\n")
}

fn at(u: f64, v: f64) -> Sample {
    Sample {
        uv: [u, v],
        surface: [u, v],
        footprint: 0.001,
        uv_footprint: Some(0.001),
        ..Sample::default()
    }
}

/// The sRGB transfer function, from its definition.
fn decode(encoded: f64) -> f64 {
    if encoded <= 0.04045 {
        encoded / 12.92
    } else {
        ((encoded + 0.055) / 1.055).powf(2.4)
    }
}

const STOPS: [(f64, [f64; 3]); 4] = [
    (0.1, [0.9, 0.1, 0.]),
    (0.4, [0.2, 0.6, 0.3]),
    (0.4, [1., 1., 0.5]),
    (0.8, [0., 0.25, 1.]),
];

/// The ramp over [`STOPS`] at `t`, worked out from the definition of each mode.
fn table(mode: &str, t: f64) -> [f64; 3] {
    let t = t.clamp(STOPS[0].0, STOPS[3].0);
    let below = (0..4).rev().find(|stop| STOPS[*stop].0 <= t).unwrap();
    let mut color = STOPS[below].1;
    if !mode.ends_with("constant") && below < 3 {
        let (from, to) = (STOPS[below], STOPS[below + 1]);
        let part = (t - from.0) / (to.0 - from.0);
        color = std::array::from_fn(|c| from.1[c] * (1. - part) + to.1[c] * part);
    }
    if mode.starts_with("display") {
        color = color.map(decode);
    }
    color
}

#[test]
fn a_ramp_matches_a_table_computed_apart_from_it_in_every_mode() {
    for mode in ["linear", "constant", "display-linear", "display-constant"] {
        let stops: String = STOPS
            .iter()
            .map(|(position, [r, g, b])| format!(", {position}, vec3({r}, {g}, {b})"))
            .collect();
        let program = source(&format!(
            "material 1;\noutput base_color = ramp(\"{mode}\", uv.x{stops});"
        ));
        let inputs = Inputs::default();
        let mut evaluator: Evaluator = program.evaluator(&inputs).unwrap();
        let samples: Vec<Sample> = (0..=64).map(|i| at(i as f64 / 64., 0.5)).collect();
        for sample in &samples {
            let expected = table(mode, sample.uv[0]);
            let found = evaluator.evaluate(sample).unwrap().base_color;
            for channel in 0..3 {
                assert!(
                    (found[channel] - expected[channel]).abs() <= 1e-12,
                    "{mode} at {}: {found:?} for {expected:?}",
                    sample.uv[0]
                );
            }
        }
        // A row evaluated together gives the same bits as its samples one at a time.
        let mut row: Evaluator = program.evaluator(&inputs).unwrap();
        let texels: Vec<usize> = (0..samples.len()).collect();
        let together = row
            .evaluate_row_with_planes(
                &samples,
                &texels,
                OutputSet::ALL,
                &FieldSelection::default(),
                &mut Vec::new(),
                &[],
                &mut [],
            )
            .unwrap();
        for (sample, found) in samples.iter().zip(&together) {
            let alone = evaluator.evaluate(sample).unwrap().base_color;
            assert_eq!(
                found.base_color.map(f64::to_bits),
                alone.map(f64::to_bits),
                "{mode}"
            );
        }
        // The generated shader has the ramp's helper, and decodes only the display modes.
        let shader = program.shader().unwrap();
        assert!(shader.source.contains("vec3 ms_ramp4("), "{mode}");
        assert_eq!(
            shader.source.contains("ms_srgb_to_linear"),
            mode.starts_with("display"),
            "{mode}"
        );
    }
}

#[test]
fn a_ramp_refuses_stops_out_of_order_and_display_colors_out_of_range() {
    let inputs = Inputs::default();
    let unordered = source(
        "material 1;\noutput base_color = ramp(\"linear\", uv.x, 0.5, vec3(0), 0.2, vec3(1));",
    );
    let error = unordered
        .evaluator(&inputs)
        .unwrap()
        .evaluate(&at(0.3, 0.))
        .unwrap_err();
    assert!(error.message.contains("ascending order"), "{error}");
    let bright = source(
        "material 1;\noutput base_color = ramp(\"display-linear\", uv.x, 0, vec3(0), 1, vec3(1.5));",
    );
    let error = bright
        .evaluator(&inputs)
        .unwrap()
        .evaluate(&at(0.3, 0.))
        .unwrap_err();
    assert!(error.message.contains("sRGB-encoded"), "{error}");
    let message = refused(
        "main.ms",
        "material 1;\noutput base_color = ramp(\"cubic\", uv.x, 0, vec3(0), 1, vec3(1));",
    );
    assert!(message.contains("ramp selects one of linear"), "{message}");
    let message = refused(
        "main.ms",
        "material 1;\noutput base_color = ramp(\"linear\", uv.x, 0, vec3(0));",
    );
    assert!(message.contains("Invalid arguments for ramp"), "{message}");
}

const CUTOUT: &str = "material 1;
let coverage = uv.x * 2 - 0.5;
output base_color = vec3(0.5);
output opacity = alpha_cutout(coverage, 0.35);";

#[test]
fn a_cutout_declares_its_cutoff_and_its_opacity_is_the_clamped_coverage() {
    let program = source(CUTOUT);
    assert_eq!(program.metadata().alpha_cutoff, Some(0.35));
    let inputs = Inputs::default();
    let mut evaluator: Evaluator = program.evaluator(&inputs).unwrap();
    for (u, expected) in [(0., 0.), (0.5, 0.5), (0.6, 0.7), (1., 1.)] {
        let found = evaluator.evaluate(&at(u, 0.)).unwrap().opacity;
        assert!((found - expected).abs() < 1e-12, "{found} at {u}");
    }
    // The opacity may be a field the channel names; a program without a cutout declares none.
    let named = source(&CUTOUT.replace(
        "output opacity = alpha_cutout(coverage, 0.35);",
        "let cut = alpha_cutout(coverage, 0.35);\noutput opacity = cut;",
    ));
    assert_eq!(named.metadata().alpha_cutoff, Some(0.35));
    let plain = source(&CUTOUT.replace("alpha_cutout(coverage, 0.35)", "coverage"));
    assert_eq!(plain.metadata().alpha_cutoff, None);
    let metadata = serde_json::to_value(plain.metadata()).unwrap();
    for absent in [
        "alphaCutoff",
        "doubleSided",
        "kind",
        "size",
        "blend",
        "reads",
    ] {
        assert!(metadata.get(absent).is_none(), "{absent} in {metadata}");
    }
}

#[test]
fn a_cutout_is_refused_out_of_range_varying_or_anywhere_but_the_opacity() {
    for (cutoff, _) in [("1.5", ()), ("-0.1", ()), ("coverage", ()), ("uv.y", ())] {
        let message = refused("main.ms", &CUTOUT.replace("0.35", cutoff));
        assert!(
            message.contains("An alpha cutoff is a constant in [0, 1]."),
            "{cutoff}: {message}"
        );
    }
    let elsewhere = "valid only as the whole opacity channel";
    let message = refused(
        "main.ms",
        &CUTOUT.replace(
            "output base_color = vec3(0.5);",
            "output base_color = vec3(alpha_cutout(coverage, 0.5));",
        ),
    );
    assert!(message.contains(elsewhere), "{message}");
    let message = refused(
        "main.ms",
        &CUTOUT.replace(
            "alpha_cutout(coverage, 0.35);",
            "alpha_cutout(coverage, 0.35) * 0.5;",
        ),
    );
    assert!(message.contains(elsewhere), "{message}");
    let message = refused(
        "main.ms",
        "material 1;\noutput base_color = vec3(0.5);\noutput roughness = alpha_cutout(uv.x, 0.5);",
    );
    assert!(message.contains(elsewhere), "{message}");
}

/// A 4 by 2 mask whose red bytes are given by rows from the top.
fn mask() -> Inputs {
    let red = [0u8, 51, 102, 255, 255, 204, 153, 0];
    let rgba = red.iter().flat_map(|r| [*r, 9, 9, 9]).collect();
    let image = Image {
        width: 4,
        height: 2,
        rgba,
    };
    Inputs {
        images: [("tear".to_owned(), Arc::new(image))].into(),
        ..Inputs::default()
    }
}

#[test]
fn an_authored_mask_is_its_image_over_the_unit_square_and_nothing_outside() {
    let program =
        source("material 1;\nlet m = authored_mask(\"tear\", uv);\noutput base_color = vec3(m);");
    assert!(
        program
            .metadata()
            .required_inputs
            .contains(&"image:tear".to_owned())
    );
    let inputs = mask();
    let mut evaluator: Evaluator = program.evaluator(&inputs).unwrap();
    let mut value = |u: f64, v: f64| evaluator.evaluate(&at(u, v)).unwrap().base_color[0];
    let close = |found: f64, expected: f64| (found - expected).abs() < 1e-12;
    // Texel centers hold their bytes: V is up, so the top row is at v = 0.75.
    assert!(close(value(0.125, 0.75), 0.));
    assert!(close(value(0.375, 0.75), 51. / 255.));
    assert!(close(value(0.875, 0.25), 0.));
    assert!(close(value(0.125, 0.25), 1.));
    // Halfway between two centers is their mean; halfway between the rows too.
    assert!(close(value(0.25, 0.75), 25.5 / 255.));
    assert!(close(value(0.125, 0.5), 127.5 / 255.));
    // The edge texel continues to the edge of the square: nothing wraps around.
    assert!(close(value(0., 0.75), 0.));
    assert!(close(value(1., 0.75), 1.));
    assert!(close(value(1., 1.), 1.));
    // Outside the square there is no mask.
    for (u, v) in [
        (-0.01, 0.5),
        (1.01, 0.5),
        (0.5, -0.01),
        (0.5, 1.01),
        (3.9, 0.75),
    ] {
        assert_eq!(value(u, v), 0., "{u}, {v}");
    }
    // Without its image the mask fails, naming it.
    let missing = Inputs::default();
    let error = match program.evaluator(&missing) {
        Ok(mut evaluator) => evaluator.evaluate(&at(0.5, 0.5)).unwrap_err().to_string(),
        Err(error) => error.to_string(),
    };
    assert!(error.contains("'tear'"), "{error}");
    let shader = program.shader().unwrap();
    assert!(shader.source.contains("float ms_authored_mask("));
    assert!(
        shader
            .images
            .iter()
            .any(|image| image.name == "tear" && image.linear)
    );
}

const STICKER: &str = r#"import { defineDecal, alphaCutout, authoredMask, ramp, uv, vec3 } from "@superspec/material/v1";
const torn = authoredMask("tear", uv);
export default defineDecal({
  size: [0.3, 0.2],
  baseColor: ramp("constant", uv.x, 0, vec3(0.8, 0.1, 0.1), 0.5, vec3(0.9, 0.9, 0.8)),
  opacity: alphaCutout(torn, 0.5),
  roughness: 0.6,
  height: 0.0002,
  blend: { roughness: "max", height: "add" },
  doubleSided: true,
});
"#;

#[test]
fn a_decal_declares_its_size_blend_rules_cutout_and_sides() {
    let program = recipe(STICKER);
    let metadata = program.metadata();
    assert_eq!(metadata.kind, RecipeKind::Decal);
    assert_eq!(metadata.size, Some([0.3, 0.2]));
    assert_eq!(metadata.alpha_cutoff, Some(0.5));
    assert!(metadata.double_sided);
    assert_eq!(
        metadata.blend,
        [
            ("base_color".to_owned(), Blend::Mix),
            ("height".to_owned(), Blend::Add),
            ("roughness".to_owned(), Blend::Max),
        ]
        .into()
    );
    assert_eq!(
        metadata.outputs,
        ["base_color", "height", "opacity", "roughness"]
    );
    let json = serde_json::to_value(metadata).unwrap();
    assert_eq!(json["kind"], "decal");
    assert_eq!(json["blend"]["roughness"], "max");
    assert_eq!(json["alphaCutoff"], 0.5);
    assert_eq!(json["doubleSided"], true);
    // Its channels are its own values.
    let inputs = mask();
    let mut evaluator: Evaluator = program.evaluator(&inputs).unwrap();
    let left = evaluator.evaluate(&at(0.125, 0.25)).unwrap();
    assert_eq!(left.base_color, [0.8, 0.1, 0.1]);
    assert_eq!(
        (left.opacity, left.roughness, left.height),
        (1., 0.6, 0.0002)
    );
    let right = evaluator.evaluate(&at(0.875, 0.25)).unwrap();
    assert_eq!(right.base_color, [0.9, 0.9, 0.8]);
    assert_eq!(right.opacity, 0.);
    // A material may be drawn from both sides too.
    let leaf = recipe(
        r#"import { defineMaterial, vec3 } from "@superspec/material/v1";
export default defineMaterial({ base: { baseColor: vec3(0.1, 0.4, 0.1) }, doubleSided: true });
"#,
    );
    assert!(leaf.metadata().double_sided);
    assert_eq!(leaf.metadata().kind, RecipeKind::Material);
}

#[test]
fn a_decal_written_wrong_says_how() {
    let entry = "main.material.ts";
    let message = refused(entry, &STICKER.replace("size: [0.3, 0.2],", ""));
    assert!(message.contains("A decal needs size"), "{message}");
    let message = refused(entry, &STICKER.replace("[0.3, 0.2]", "[0.3, 0]"));
    assert!(message.contains("A decal needs size"), "{message}");
    let message = refused(entry, &STICKER.replace("\"max\"", "\"screen\""));
    assert!(
        message.contains("A blend rule is one of mix, add"),
        "{message}"
    );
    let message = refused(
        entry,
        &STICKER.replace("height: \"add\"", "metalness: \"add\""),
    );
    assert!(
        message.contains("Unknown property 'metalness' for blend"),
        "{message}"
    );
    let message = refused(
        entry,
        &STICKER.replace("doubleSided: true", "doubleSided: 1"),
    );
    assert!(message.contains("literal true or false"), "{message}");
    let message = refused(
        entry,
        &STICKER.replace("0.5),\n  roughness", "1.5),\n  roughness"),
    );
    assert!(
        message.contains("An alpha cutoff is a constant in [0, 1]."),
        "{message}"
    );
}

const GRIME: &str = r#"import { defineLayer, below, param, featureDistance, distanceBand, external, mul, vec3 } from "@superspec/material/v1";
const reach = param.scalar({ default: 0.3, range: [0.01, 2], unit: "m" });
const band = distanceBand(featureDistance("follow", 2), 0, reach);
export default defineLayer({
  coverage: mul(band, external("occlusion", 1)),
  baseColor: vec3(0.5, 0.25, 0.125),
  roughness: 0.25,
  height: mul(below.metalness, 0.001),
  blend: { baseColor: "multiply", roughness: "max", height: "add" },
});
"#;

#[test]
fn a_layer_combines_what_it_gives_with_the_surface_beneath_and_leaves_the_rest() {
    let program = recipe(GRIME);
    let metadata = program.metadata();
    assert_eq!(metadata.kind, RecipeKind::Layer);
    assert_eq!(metadata.outputs, ["base_color", "height", "roughness"]);
    assert_eq!(
        metadata.reads,
        ["base_color", "height", "metalness", "roughness"]
    );
    assert_eq!(metadata.named_features, ["follow"]);
    assert_eq!(metadata.blend["base_color"], Blend::Multiply);
    assert_eq!(serde_json::to_value(metadata).unwrap()["kind"], "layer");
    // Without planes the surface beneath is the fallback: mid grey, rough, flat, not metal.
    let inputs = Inputs::default();
    let line = program::NamedFeature {
        name: "follow".into(),
        points: vec![[-5., 0., 0.], [5., 0., 0.]],
        closed: false,
    };
    let named = program::NamedFeatures::new(std::slice::from_ref(&line)).unwrap();
    let features = program::FeatureSet::new(Vec::new());
    let mut evaluator: Evaluator = program
        .evaluator_with_feature_sets(&inputs, &features, &named)
        .unwrap();
    let sample = |distance: f64| Sample {
        world: [0., 0., distance],
        footprint: 0.,
        ..Sample::default()
    };
    // On the feature the layer covers the surface: each channel is its rule over the two.
    let on = evaluator.evaluate(&sample(0.)).unwrap();
    assert_eq!(on.base_color, [0.25, 0.125, 0.0625]);
    assert_eq!(on.roughness, 1., "the rougher of 1 beneath and 0.25");
    assert_eq!(on.height, 0.);
    // Past its reach it leaves the surface beneath as it was.
    let off = evaluator.evaluate(&sample(1.)).unwrap();
    assert_eq!(off.base_color, [0.5, 0.5, 0.5]);
    assert_eq!(off.roughness, 1.);
    // The shader plan reads the surface beneath and the occlusion as planes the host binds.
    let plan = program.shader_plan().unwrap();
    let planes: Vec<&str> = plan
        .cpu_fields
        .iter()
        .flat_map(|field| field.planes.iter().map(String::as_str))
        .collect();
    for name in [
        "base.base_color",
        "base.roughness",
        "base.height",
        "base.metalness",
        "occlusion",
    ] {
        assert!(planes.contains(&name), "{name} in {planes:?}");
    }
}

#[test]
fn a_layer_written_wrong_says_how() {
    let entry = "main.material.ts";
    let message = refused(
        entry,
        &GRIME.replace("coverage: mul(band, external(\"occlusion\", 1)),", ""),
    );
    assert!(message.contains("A layer needs a coverage."), "{message}");
    let message = refused(entry, &GRIME.replace("below.metalness", "below.emissive"));
    assert!(
        message.contains("The surface beneath has baseColor"),
        "{message}"
    );
    let message = refused(
        entry,
        &GRIME
            .replace("  baseColor: vec3(0.5, 0.25, 0.125),\n", "")
            .replace("  roughness: 0.25,\n", "")
            .replace("  height: mul(below.metalness, 0.001),\n", "")
            .replace(
                "blend: { baseColor: \"multiply\", roughness: \"max\", height: \"add\" },",
                "",
            ),
    );
    assert!(
        message.contains("A layer must change at least one channel."),
        "{message}"
    );
    // Only a layer has a surface beneath it.
    let message = refused(
        entry,
        r#"import { defineMaterial, below } from "@superspec/material/v1";
export default defineMaterial({ base: { baseColor: below.baseColor } });
"#,
    );
    assert!(
        message.contains("Only a layer (defineLayer) reads the surface beneath it."),
        "{message}"
    );
}

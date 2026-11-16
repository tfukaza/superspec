//! Planes are one mechanism for both evaluators: an external input is read from the plane bound
//! to its name at each sample's texel, a field whose plane holds the texel is read instead of
//! computed, and evaluating a row together charges exactly what evaluating its samples one at a
//! time with the same planes charges.
use superspec_engine::materialspec::program::{
    self, Evaluator, FieldSelection, Inputs, OutputSet, Outputs, Plane, Program, Sample, Sources,
    Type,
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

fn samples(count: usize) -> Vec<Sample> {
    (0..count)
        .map(|i| {
            let x = (i as f64 + 0.5) / count as f64;
            let world = [x * 1.2 - 0.6, 0.05, 0.3 + 0.4 * x];
            Sample {
                uv: [x, 0.25 + 0.5 * x],
                surface: [world[0], world[2]],
                object: world,
                world,
                normal: [0., 0., 1.],
                footprint: 0.004,
                uv_footprint: Some(0.004),
                gravity: [0., 0., -1.],
                bounds_min: [-0.6, -0.11, 0.],
                bounds_max: [0.6, 0.11, 1.1],
            }
        })
        .collect()
}

fn bits(outputs: &Outputs) -> Vec<u64> {
    let mut all: Vec<f64> = outputs.base_color.to_vec();
    all.extend([outputs.roughness, outputs.metalness, outputs.height]);
    all.into_iter().map(f64::to_bits).collect()
}

/// The row together (which must not decline when `together`), and its samples one at a time on
/// another evaluator with the same planes: equal results, and equal allowances left.
fn both(
    program: &Program,
    inputs: &Inputs,
    features: &[program::Feature],
    samples: &[Sample],
    texels: &[usize],
    planes: &[&Plane],
    outputs: OutputSet,
) -> (Vec<Outputs>, usize) {
    let selection = FieldSelection::default();
    let mut row: Evaluator = program.evaluator_with_features(inputs, features).unwrap();
    let mut alone: Evaluator = program.evaluator_with_features(inputs, features).unwrap();
    let mut unused = Vec::new();
    let together = row
        .evaluate_row_with_planes(
            samples,
            texels,
            outputs,
            &selection,
            &mut unused,
            planes,
            &mut [],
        )
        .unwrap_or_else(|e| panic!("{e}"));
    let singly: Vec<Outputs> = samples
        .iter()
        .zip(texels)
        .map(|(sample, texel)| {
            alone
                .evaluate_outputs_at(sample, *texel, planes, outputs, &selection, &mut [])
                .unwrap_or_else(|e| panic!("{e}"))
        })
        .collect();
    for (i, (a, b)) in together.iter().zip(&singly).enumerate() {
        assert_eq!(bits(a), bits(b), "sample {i}");
    }
    assert_eq!(
        row.allowance(),
        alone.allowance(),
        "a row charges what its samples charge one at a time"
    );
    (together, 64_000_000 - row.allowance())
}

const SHADED: &str = "material 1;
param gain: scalar = 0.5 range(0, 1);
let shade = external(\"shade\", 0.25);
let tint = external(\"base.color\", vec3(0.5, 0.25, 0.125));
let grain = noise(surface * 12, seed(\"g\")) * shade;
output base_color = tint * shade;
output roughness = clamp(grain * 0.5 + external(\"shade\", 0.25) * gain, 0, 1);
output height = shade * 0.001;";

#[test]
fn an_external_input_is_its_plane_at_each_sample_s_texel_or_the_fallback() {
    let program = source(SHADED);
    assert_eq!(
        program.metadata().externals,
        [
            program::External {
                name: "base.color".into(),
                kind: Type::Vec3
            },
            program::External {
                name: "shade".into(),
                kind: Type::Scalar
            },
        ]
    );
    assert!(
        program
            .metadata()
            .required_inputs
            .contains(&"plane:shade".to_owned())
    );
    assert!(
        program
            .metadata()
            .required_inputs
            .contains(&"plane:base.color".to_owned())
    );
    let inputs = Inputs::default();
    for count in [5, 64, 150] {
        let samples = samples(count);
        let texels: Vec<usize> = (0..count).map(|i| i * 3 + 1).collect();
        let size = count * 3 + 1;
        // Every fifth texel of the shade holds no value; the tint covers only the first half.
        let shade_of = |texel: usize| (texel % 5 != 0).then(|| (texel % 17) as f32 / 16.);
        let tint_of = |texel: usize| {
            (texel < size / 2).then(|| [(texel % 7) as f32 / 8., 0.5, (texel % 3) as f32 / 4.])
        };
        let shade = Plane::external(
            "shade",
            1,
            (0..size).map(|t| shade_of(t).unwrap_or(f32::NAN)).collect(),
        )
        .unwrap();
        let tint = Plane::external(
            "base.color",
            3,
            (0..size)
                .flat_map(|t| tint_of(t).unwrap_or([f32::NAN, 0., 0.]))
                .collect(),
        )
        .unwrap();
        assert!(program.evaluator(&inputs).unwrap().rows_evaluate_together());
        let (with, cost) = both(
            &program,
            &inputs,
            &[],
            &samples,
            &texels,
            &[&shade, &tint],
            OutputSet::ALL,
        );
        for (i, outputs) in with.iter().enumerate() {
            let shade = shade_of(texels[i]).map_or(0.25, f64::from);
            let tint = tint_of(texels[i]).map_or([0.5, 0.25, 0.125], |v| v.map(f64::from));
            assert_eq!(outputs.base_color, tint.map(|v| v * shade), "sample {i}");
            assert_eq!(outputs.height, shade * 0.001, "sample {i}");
        }
        // Without planes, and without a texel, every sample reads the fallbacks, at the same cost:
        // what a host binds never changes what a program may spend.
        let (without, fallback_cost) = both(
            &program,
            &inputs,
            &[],
            &samples,
            &texels,
            &[],
            OutputSet::ALL,
        );
        assert_eq!(cost, fallback_cost);
        let mut evaluator = program.evaluator(&inputs).unwrap();
        for (i, outputs) in without.iter().enumerate() {
            assert_eq!(outputs.base_color, [0.125, 0.0625, 0.03125], "sample {i}");
            let unplaced = evaluator
                .evaluate_outputs_at(
                    &samples[i],
                    usize::MAX,
                    &[&shade, &tint],
                    OutputSet::ALL,
                    &FieldSelection::default(),
                    &mut [],
                )
                .unwrap();
            assert_eq!(bits(&unplaced), bits(outputs), "sample {i} without a texel");
        }
    }
}

#[test]
fn a_plane_of_the_wrong_shape_and_planes_without_texels_fail_in_both_evaluators() {
    let program = source(SHADED);
    let inputs = Inputs::default();
    let samples = samples(70);
    let texels: Vec<usize> = (0..70).collect();
    let pairs = Plane::external("shade", 2, vec![0.5; 140]).unwrap();
    let selection = FieldSelection::default();
    let mut evaluator = program.evaluator(&inputs).unwrap();
    let mut unused = Vec::new();
    let error = evaluator
        .evaluate_row_with_planes(
            &samples,
            &texels,
            OutputSet::ALL,
            &selection,
            &mut unused,
            &[&pairs],
            &mut [],
        )
        .unwrap_err();
    assert_eq!(error.code, "MS_INPUT");
    assert!(
        error
            .message
            .contains("'shade' holds 2 components where the program reads 1"),
        "{error}"
    );
    assert_eq!(
        error.source.location.line, 3,
        "the read that failed: {error}"
    );
    let alone = evaluator
        .evaluate_outputs_at(
            &samples[0],
            0,
            &[&pairs],
            OutputSet::ALL,
            &selection,
            &mut [],
        )
        .unwrap_err();
    assert_eq!((alone.code, &alone.message), (error.code, &error.message));
    let error = evaluator
        .evaluate_row_with_planes(
            &samples,
            &texels[..69],
            OutputSet::ALL,
            &selection,
            &mut unused,
            &[&pairs],
            &mut [],
        )
        .unwrap_err();
    assert_eq!(error.code, "MS_INPUT");
    assert!(
        error.message.contains("a texel for every sample"),
        "{error}"
    );
    // The evaluator still works afterwards.
    evaluator.replenish(64_000_000);
    evaluator
        .evaluate_row(&samples, OutputSet::ALL, &selection, &mut unused)
        .unwrap();
}

#[test]
fn kept_field_planes_are_read_at_the_same_cost_one_sample_at_a_time() {
    let program = source(
        "material 1;
         let f = select(uv.x < 0.5, 1.0, 2.0);
         let n = fbm(vec2(uv.x * 9, uv.y * 7), seed(\"n\"), 4) * f;
         output base_color = vec3(f * 0.1, n * 0.1, 0.2);
         output roughness = f * 0.25 + n * 0.1;",
    );
    let inputs = Inputs::default();
    let index = |name: &str| {
        program
            .metadata()
            .fields
            .iter()
            .position(|f| f.name == name)
            .unwrap()
    };
    for count in [5, 64, 150] {
        let samples = samples(count);
        let texels: Vec<usize> = (0..count).map(|i| i * 2 + 1).collect();
        let mut planes = vec![
            program::FieldPlane::new(index("f"), 1, count * 2 + 1),
            program::FieldPlane::new(index("n"), 1, count * 2 + 1),
        ];
        let mut unused = Vec::new();
        program
            .evaluator(&inputs)
            .unwrap()
            .evaluate_row_with_planes(
                &samples,
                &texels,
                OutputSet::ALL,
                &FieldSelection::default(),
                &mut unused,
                &[],
                &mut planes,
            )
            .unwrap();
        assert_eq!(planes[1].stored(), count);
        let kept: Vec<&Plane> = planes.iter().collect();
        let (with, cost) = both(
            &program,
            &inputs,
            &[],
            &samples,
            &texels,
            &kept,
            OutputSet::ALL,
        );
        let (without, full) = both(
            &program,
            &inputs,
            &[],
            &samples,
            &texels,
            &[],
            OutputSet::ALL,
        );
        for (i, (a, b)) in with.iter().zip(&without).enumerate() {
            assert_eq!(
                bits(a),
                bits(b),
                "planes only change what is computed: sample {i}"
            );
        }
        assert!(
            cost + count * 32 < full,
            "the kept fractal noise is read, not computed: {cost} against {full}"
        );
    }
}

/// A fold of parallel creases, enough of them that a sample's queries could visit thousands.
fn pleats(count: usize) -> Vec<program::Feature> {
    (0..count)
        .map(|i| {
            let x = -0.6 + 1.2 * i as f64 / count as f64;
            program::Feature {
                start: [x, 0.05, 0.],
                end: [x, 0.05, 1.1],
                kind: program::FeatureKind::Convex,
                angle: 90.,
                normal: Some([0., 0., 1.]),
            }
        })
        .collect()
}

#[test]
fn a_program_that_could_reach_a_limit_reads_external_planes_but_no_kept_fields() {
    // Seventy queries over four thousand features: the bound on a sample's steps passes the
    // limit, so what a kept field spares must not decide whether a sample fails.
    let mut text = String::from(
        "material 1;\nlet worn = crease_distance(\"convex\", 30.0, 0.004);\nlet lit = external(\"shade\", 1.0);\n",
    );
    let sum: Vec<String> = (0..70)
        .map(|i| {
            format!(
                "crease_distance(\"convex\", 30.0, {:.4})",
                0.001 + i as f64 * 1e-5
            )
        })
        .collect();
    text += &format!(
        "output base_color = vec3(clamp(worn * 100, 0, 1) * lit);\noutput roughness = clamp(({}) * 2, 0, 1);\n",
        sum.join(" + ")
    );
    let program = source(&text);
    let inputs = Inputs::default();
    let features = pleats(4000);
    let count = 40;
    let samples = samples(count);
    let texels: Vec<usize> = (0..count).collect();
    assert!(
        !program
            .evaluator_with_features(&inputs, &features)
            .unwrap()
            .rows_evaluate_together()
    );
    let lit = program
        .metadata()
        .fields
        .iter()
        .position(|f| f.name == "lit")
        .unwrap();
    // A plane claiming `lit` is 0.5 everywhere, filled by a program whose field of that index
    // is: honored, it would change the color.
    let constant = source("material 1; let half = 0.5; output base_color = vec3(half);");
    let mut capture = [program::FieldPlane::new(lit, 1, count)];
    assert_eq!(lit, 0);
    let mut unused = Vec::new();
    constant
        .evaluator(&inputs)
        .unwrap()
        .evaluate_row_with_planes(
            &samples,
            &texels,
            OutputSet::ALL,
            &FieldSelection::default(),
            &mut unused,
            &[],
            &mut capture,
        )
        .unwrap();
    let [lit_plane] = capture;
    assert_eq!((lit_plane.field(), lit_plane.stored()), (Some(lit), count));
    let shade = Plane::external("shade", 1, (0..count).map(|t| t as f32 / 64.).collect()).unwrap();
    let (with, cost) = both(
        &program,
        &inputs,
        &features,
        &samples,
        &texels,
        &[&lit_plane, &shade],
        OutputSet::ALL,
    );
    let (plain, plain_cost) = both(
        &program,
        &inputs,
        &features,
        &samples,
        &texels,
        &[&shade],
        OutputSet::ALL,
    );
    assert_eq!(cost, plain_cost, "the kept field was not read");
    let (unlit, _) = both(
        &program,
        &inputs,
        &features,
        &samples,
        &texels,
        &[],
        OutputSet::ALL,
    );
    for i in 0..count {
        assert_eq!(bits(&with[i]), bits(&plain[i]), "sample {i}");
        let shade = i as f64 / 64.;
        assert_eq!(
            with[i].base_color,
            unlit[i].base_color.map(|v| v * shade),
            "the external plane was read: sample {i}"
        );
    }
}

#[test]
fn external_inputs_are_checked_when_a_program_compiles() {
    let failed = |text: &str| {
        let errors = compile("main.ms", text).err().expect("a rejected program");
        (errors[0].code, errors[0].message.clone())
    };
    let (code, message) = failed(
        "material 1; let a = external(\"k\", 1.0); output base_color = external(\"k\", vec3(1));",
    );
    assert_eq!(code, "MS_TYPE");
    assert!(message.contains("'k' is read as"), "{message}");
    for name in ["has space", "semi;colon", &"x".repeat(65)] {
        let (code, message) = failed(&format!(
            "material 1; output base_color = vec3(external(\"{name}\", 1.0));"
        ));
        assert_eq!(code, "MS_TYPE", "{name}");
        assert!(message.contains("at most 64"), "{message}");
    }
    let (code, _) = failed("material 1; output base_color = vec3(external(\"k\", true));");
    assert_eq!(code, "MS_TYPE");
    let (code, _) =
        failed("material 1; let k = \"k\"; output base_color = vec3(external(k, 1.0));");
    assert_eq!(code, "MS_TYPE");
    let many = |count: usize| {
        let reads: Vec<String> = (0..count)
            .map(|i| format!("external(\"e{i}\", 0.01)"))
            .collect();
        format!(
            "material 1; output base_color = vec3({});",
            reads.join(" + ")
        )
    };
    assert_eq!(source(&many(16)).metadata().externals.len(), 16);
    let (code, message) = failed(&many(17));
    assert_eq!(code, "MS_LIMIT");
    assert!(message.contains("at most 16 external inputs"), "{message}");
    // Parameter defaults are constants: no plane can supply one.
    let (code, _) =
        failed("material 1; param p: scalar = external(\"k\", 1.0); output base_color = vec3(p);");
    assert_eq!(code, "MS_PARAMETER");
}

#[test]
fn recipes_read_external_inputs_and_report_what_reads_them() {
    let recipe = r#"
import { defineMaterial, external, vec3, mul, add, surface, noise, seed, scale } from "@superspec/material/v1";

const occlusion = external("occlusion", 1);
const grain = noise(scale(surface, 9), seed("grain"));

export default defineMaterial({
  name: "Shaded",
  base: {
    baseColor: mul(vec3(0.5, 0.4, 0.3), occlusion),
    roughness: add(mul(grain, 0.5), 0.25),
    metalness: 0,
  },
});
"#;
    let program = compile("shaded.material.ts", recipe).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(
        program.metadata().externals,
        [program::External {
            name: "occlusion".into(),
            kind: Type::Scalar
        }]
    );
    let table = program.dependency_table();
    let color = &table.outputs["base_color"];
    assert_eq!(color.externals.iter().collect::<Vec<_>>(), ["occlusion"]);
    assert!(
        !color.intrinsic(),
        "a plane belongs to one receiver's texels"
    );
    assert!(table.outputs["roughness"].externals.is_empty());
    assert!(table.outputs["roughness"].intrinsic());
    let inputs = Inputs::default();
    let samples = samples(8);
    let texels: Vec<usize> = (0..8).collect();
    let plane = Plane::external("occlusion", 1, (0..8).map(|t| t as f32 / 8.).collect()).unwrap();
    let (shaded, _) = both(
        &program,
        &inputs,
        &[],
        &samples,
        &texels,
        &[&plane],
        OutputSet::ALL,
    );
    for (i, outputs) in shaded.iter().enumerate() {
        let occlusion = i as f64 / 8.;
        assert_eq!(outputs.base_color, [0.5, 0.4, 0.3].map(|v| v * occlusion));
    }
    // The GPU plan reads a field over an external input from a texture, like any CPU field, and
    // says which planes the texture follows.
    let plan = program.shader_plan().unwrap();
    let field = plan
        .cpu_fields
        .iter()
        .find(|field| field.planes == ["occlusion"])
        .expect("the field over the plane is baked on the CPU");
    assert_eq!(field.kind, Type::Scalar);
    assert_eq!(plan.cpu_outputs, ["base_color"]);
    // The record says what each output reads, for a host that redraws one texture per output.
    let color = &plan.reads.outputs["base_color"];
    assert_eq!(color.planes, ["occlusion"]);
    assert_eq!(color.cpu_fields, [field.name.clone()]);
    assert!(!color.intrinsic && !color.seed && color.sample.is_empty());
    let roughness = &plan.reads.outputs["roughness"];
    assert!(roughness.intrinsic && roughness.seed && !roughness.features && !roughness.edges);
    assert_eq!(roughness.sample, ["surface", "footprint"]);
    assert!(roughness.cpu_fields.is_empty() && roughness.planes.is_empty());
    assert!(plan.reads.outputs["metalness"].intrinsic);
    assert_eq!(
        plan.reads.outputs.keys().collect::<Vec<_>>(),
        plan.outputs.iter().collect::<Vec<_>>()
    );
    let json = serde_json::to_value(&plan).unwrap();
    assert_eq!(
        json["reads"]["outputs"]["base_color"]["cpuFields"][0],
        field.name
    );
    assert_eq!(json["reads"]["outputs"]["roughness"]["intrinsic"], true);
}

#[test]
fn the_shader_record_names_the_parameter_uniforms_and_live_arguments_each_value_reads() {
    let program = source(
        "material 1;
         param tint: scalar = 0.5 range(0, 1);
         param shine: scalar = 0.2 range(0, 1);
         let worn = crease_distance(\"convex\", 30.0, 0.05);
         output base_color = vec3(tint * normal.z);
         output roughness = clamp(shine + worn, 0, 1);
         output height = runoff(0.1, 0.5) * 0.001;",
    );
    let shader = program.shader().unwrap();
    let uniform = |name: &str| {
        let found = shader.parameters.iter().find(|p| p.name == name).unwrap();
        found.uniform.clone()
    };
    let reads = &shader.reads.outputs;
    assert_eq!(reads["base_color"].parameters, [uniform("tint")]);
    assert_eq!(reads["base_color"].sample, ["normal"]);
    assert!(!reads["base_color"].intrinsic);
    assert_eq!(reads["roughness"].parameters, [uniform("shine")]);
    assert!(reads["roughness"].features && !reads["roughness"].edges);
    assert_eq!(reads["roughness"].sample, ["world", "normal", "footprint"]);
    assert!(reads["height"].edges && reads["height"].parameters.is_empty());
    assert!(shader.reads.live.is_empty());
    // No output of it is intrinsic, so there is nothing to split.
    assert!(
        shader
            .entries
            .iter()
            .all(|entry| !entry.function.contains("intrinsic"))
    );
}

#[test]
fn a_program_with_both_kinds_of_outputs_has_an_entry_point_for_each() {
    let program = source(
        "material 1;
         let grain = fbm(surface * 12, seed(\"g\"), 3);
         output base_color = vec3(grain);
         output roughness = clamp(grain + crease_distance(\"convex\", 30.0, 0.05), 0, 1);
         output metalness = 0;",
    );
    let shader = program.shader().unwrap();
    let entry = |name: &str| {
        let found = shader.entries.iter().find(|entry| entry.function == name);
        found.unwrap_or_else(|| panic!("{name}"))
    };
    let intrinsic = entry("ms_evaluate_intrinsic");
    assert_eq!(intrinsic.outputs, ["base_color", "metalness"]);
    assert!(
        !intrinsic.source.contains("ms_crease_distance"),
        "it reads no receiver"
    );
    let remainder = entry("ms_evaluate_remainder");
    assert_eq!(remainder.outputs, ["roughness"]);
    assert!(remainder.source.contains("ms_crease_distance"));
    for name in &intrinsic.outputs {
        assert!(shader.reads.outputs[name].intrinsic, "{name}");
    }
    assert!(!shader.reads.outputs["roughness"].intrinsic);
}

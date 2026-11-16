//! Live and painted materials (roadmap stage 5, plan steps 1C and 1D): the painted shading model,
//! programs delivered beside their baked fallback, the sun from the lighting record, `shade_band`
//! with cast shadows folded in, instance, vertex and scene-field inputs, and what a bake may read.
//!
//! The expected values here are worked out from the documented formulas, not read back from the
//! evaluator. `MATERIAL_LIVE_GLSL_OUTPUT=<file>` makes `the_cpu_reference_fixture_is_written`
//! write the grid `tests/live-program-browser.mjs` evaluates with the generated GLSL.
use std::collections::BTreeMap;
use std::sync::Arc;
use superspec_engine::authoring::{Request, Session};
use superspec_engine::materialspec::program::{
    self, BakeRequest, Inputs, RenderBindings, Sample, SceneField, Shading, Sources,
};

fn compile(entry: &str, text: &str) -> Result<program::Program, Vec<program::ProgramDiagnostic>> {
    program::compile(&Sources {
        entry: entry.into(),
        modules: [(entry.to_owned(), text.to_owned())].into(),
    })
}

/// A painted ground: its paint mixes a shade and a lit color by the sun's shade band, bitten by
/// a world-anchored edge break. The base color, which the bake delivers as the fallback, is the
/// lit color and reads no render-only input.
const PAINTED: &str = r#"import { defineMaterial, param, vec3, mix, add, shadeBand, sunDirection, normal, edgeBreak } from "@superspec/material/v1";
const lit = param.vec3({ default: vec3(0.62, 0.55, 0.4), range: [0, 1] });
const shade = param.vec3({ default: vec3(0.18, 0.2, 0.32), range: [0, 1] });
const threshold = param.scalar({ default: 0.2, range: [-1, 1] });
const edge = edgeBreak({ size: 0.35, amount: 0.1 });
const factor = shadeBand(normal, sunDirection, add(threshold, edge.offset), 1.5, 0.05);
export default defineMaterial({
  shading: "painted",
  base: { baseColor: lit, roughness: 0.9 },
  paint: mix(shade, lit, factor),
});
"#;

fn smooth(edge0: f64, edge1: f64, x: f64) -> f64 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0., 1.);
    t * t * (3. - 2. * t)
}

fn bound(sun: [f64; 3]) -> Inputs {
    Inputs {
        render: Some(RenderBindings {
            sun_direction: sun,
            ..RenderBindings::default()
        }),
        ..Inputs::default()
    }
}

fn paint(program: &program::Program, inputs: &Inputs, sample: &Sample) -> [f64; 3] {
    match program.evaluate(inputs, sample).unwrap().fields["painted.color"] {
        program::FieldValue::Vec3(value) => value,
        ref other => panic!("{other:?}"),
    }
}

#[test]
fn a_painted_recipe_declares_its_shading_and_paint_and_its_shader_reads_the_sun() {
    let program = compile("painted.material.ts", PAINTED).unwrap_or_else(|e| panic!("{e:?}"));
    let metadata = program.metadata();
    assert_eq!(metadata.shading, Shading::Painted);
    assert_eq!(metadata.paint.as_deref(), Some("painted.color"));
    let json = serde_json::to_value(metadata).unwrap();
    assert_eq!(json["shading"], "painted");
    let shader = program.shader().unwrap();
    let painted = shader
        .painted
        .clone()
        .expect("a painted shader names its color");
    assert!(painted.starts_with("ms_f"), "{painted}");
    assert!(shader.source.contains(&format!("{painted} = ")));
    assert!(shader.source.contains("uniform vec3 msSunDirection;"));
    assert!(shader.source.contains("float ms_cast_shadow = 1.0;"));
    assert!(shader.source.contains("ms_shade_band("));
    let reads = shader.reads.painted.as_ref().unwrap();
    assert!(reads.sample.contains(&"sun") && reads.sample.contains(&"normal"));
    // The base color, which bakes, reads none of it.
    assert!(!shader.reads.outputs["base_color"].sample.contains(&"sun"));
    // A lit recipe's shader and metadata are as before.
    let lit = compile("lit.ms", "material 1;\noutput base_color = vec3(0.5);").unwrap();
    assert!(lit.shader().unwrap().painted.is_none());
    assert!(
        serde_json::to_value(lit.metadata())
            .unwrap()
            .get("shading")
            .is_none()
    );
    // paint needs painted shading, and shading is lit or painted.
    let errors = compile(
        "bad.material.ts",
        &PAINTED.replace("shading: \"painted\",", ""),
    )
    .err()
    .expect("paint without painted shading");
    assert!(errors[0].message.contains("painted"), "{errors:?}");
    let errors = compile(
        "bad.material.ts",
        &PAINTED.replace("\"painted\",", "\"toon\","),
    )
    .err()
    .expect("an unknown shading");
    assert_eq!(errors[0].source.location.line, 8, "{errors:?}");
}

#[test]
fn the_shade_boundary_follows_the_sun_on_the_cpu_and_cast_shadows_fold_in() {
    let program = compile("painted.material.ts", PAINTED).unwrap();
    let (lit, shade) = ([0.62, 0.55, 0.4], [0.18, 0.2, 0.32]);
    let mix = |t: f64| std::array::from_fn::<f64, 3, _>(|i| shade[i] + (lit[i] - shade[i]) * t);
    // A flat ground (normal +Z): the band reads the sun's height. The edge break is a bounded
    // offset (at most 0.1 either way), so a sun far above or below the threshold decides alone.
    let ground = Sample {
        world: [0.3, -0.2, 0.],
        footprint: 0.01,
        ..Sample::default()
    };
    let close = |found: [f64; 3], expected: [f64; 3]| {
        for (found, expected) in found.iter().zip(expected) {
            assert!((found - expected).abs() < 1e-12, "{found} {expected}");
        }
    };
    let high = [0., 0.6, 0.8];
    let low = [0., 0.8, -0.6];
    close(paint(&program, &bound(high), &ground), mix(1.));
    close(paint(&program, &bound(low), &ground), mix(0.));
    // Between, the factor is smoothstep(-0.05, 0.05, n.s - (0.2 + offset)): the offset is the
    // edge break's, so the sun that puts n.s at the threshold plus the offset gives one half.
    let offset = match program.evaluate(&bound(high), &ground).unwrap().fields["edge.offset"] {
        program::FieldValue::Scalar(value) => value,
        ref other => panic!("{other:?}"),
    };
    assert!(offset.abs() <= 0.1, "{offset}");
    let z = 0.2 + offset + 0.02;
    let sun = [0., (1. - z * z).sqrt(), z];
    let expected = mix(smooth(-0.05, 0.05, sun[2] - (0.2 + offset)));
    close(paint(&program, &bound(sun), &ground), expected);
    // The cast shadow is folded into the same factor by its minimum.
    let mut shadowed = bound(high);
    shadowed.render.as_mut().unwrap().cast_shadow = 0.25;
    close(paint(&program, &shadowed, &ground), mix(0.25));
    // A wall facing away from a sun overhead reads its own normal: n.s is zero, in shade.
    let wall = Sample {
        normal: [1., 0., 0.],
        ..ground.clone()
    };
    close(paint(&program, &bound(high), &wall), mix(0.));
    // Without bindings the paint is render-only: it says so where the sun is read.
    let error = program.evaluate(&Inputs::default(), &ground).unwrap_err();
    assert_eq!(error.code, "MS_INPUT");
    assert!(error.message.contains("the sun"), "{error}");
    assert_eq!(error.source.location.line, 6, "{error:?}");
}

#[test]
fn a_painted_material_bakes_its_fallback_and_carries_its_program() {
    let request = Request {
        entry: "painted.material.ts".into(),
        source: PAINTED.into(),
        resolution: 32,
        parameters: [("threshold".to_owned(), serde_json::json!(0.35))].into(),
        seed: 3,
        ..Request::default()
    };
    let mut session = Session::default();
    let built = session
        .build(&request, 1, &|name| Err(name.to_owned()), &|| false)
        .unwrap_or_else(|e| panic!("{e:?}"));
    let material = built.model.materials.values().next().unwrap();
    assert_eq!(
        material.shading,
        Some(superspec_types::material::Shading::Painted)
    );
    assert!(material.texture.is_some(), "the baked fallback");
    let program = material.program.as_ref().expect("the delivered program");
    assert!(program.valid());
    assert_eq!(program.mapping, "object");
    assert!(program.source.contains("void ms_evaluate()"));
    for needed in ["normal", "shadow", "sun", "world"] {
        assert!(
            program.requires.iter().any(|r| r == needed),
            "{needed}: {:?}",
            program.requires
        );
    }
    assert!(
        program
            .inputs
            .iter()
            .any(|input| input.symbol == "msSunDirection" && input.kind == "uniform")
    );
    // Uniforms carry the request's values: the parameter it set, the defaults of the others and
    // the edge break's named seed under the request's seed.
    let compiled = compile("painted.material.ts", PAINTED).unwrap();
    let shader = compiled.shader().unwrap();
    let uniform = |name: &str| {
        let symbol = &shader
            .parameters
            .iter()
            .find(|parameter| parameter.name == name)
            .unwrap()
            .uniform;
        program
            .uniforms
            .iter()
            .find(|uniform| &uniform.uniform == symbol)
            .unwrap()
            .value
            .clone()
    };
    assert_eq!(uniform("threshold"), [0.35]);
    assert_eq!(uniform("lit"), [0.62, 0.55, 0.4]);
    let seed = &shader.seeds[0];
    let seeded = program
        .uniforms
        .iter()
        .find(|uniform| uniform.uniform == seed.uniform)
        .unwrap();
    assert_eq!(seeded.kind, "uint");
    assert_eq!(
        seeded.value,
        [f64::from(program::seed_value(3, &seed.name))]
    );
    // A lit material carries neither; its identity is computed as before.
    let lit = Request {
        entry: "lit.ms".into(),
        source: "material 1;\noutput base_color = vec3(0.5);".into(),
        resolution: 32,
        ..Request::default()
    };
    let built = session
        .build(&lit, 2, &|name| Err(name.to_owned()), &|| false)
        .unwrap();
    let material = built.model.materials.values().next().unwrap();
    assert!(material.shading.is_none() && material.program.is_none());
    let json = serde_json::to_value(material).unwrap();
    assert!(json.get("shading").is_none() && json.get("program").is_none());
}

#[test]
fn a_bake_that_needs_the_sun_is_a_source_linked_diagnostic() {
    // The base color is baked; reading the sun there cannot be.
    let source = PAINTED.replace(
        "base: { baseColor: lit, roughness: 0.9 },",
        "base: { baseColor: mix(shade, lit, factor), roughness: 0.9 },",
    );
    let request = Request {
        entry: "sunny.material.ts".into(),
        source,
        resolution: 16,
        ..Request::default()
    };
    let errors = Session::default()
        .build(&request, 1, &|name| Err(name.to_owned()), &|| false)
        .err()
        .expect("the base color reads the sun");
    assert_eq!(errors[0].code, "MS_INPUT");
    assert!(errors[0].message.contains("the sun"), "{errors:?}");
    assert_eq!(errors[0].line, 6, "{errors:?}");
}

const PLAIN: &str = "material 1;
let tone = 0.4 + 0.2 * fbm(surface / 0.3, seed(\"tone\"), 3);
output base_color = vec3(tone, 0.5 * tone, 0.3);
output roughness = 0.6 + 0.2 * tone;
output height = 0.002 * tone;";

#[test]
fn with_default_bindings_a_program_that_reads_instance_inputs_bakes_as_before() {
    // Tint white, random zero, the root at the origin, every named attribute at the value its
    // call is typed by: the instance terms leave the bake exactly as the program without them.
    let instanced = PLAIN
        .replace(
            "output base_color = vec3(tone, 0.5 * tone, 0.3);",
            "output base_color = vec3(tone, 0.5 * tone, 0.3) * instance_tint * (1.0 - 0.3 * instance_random) * instance_attribute(\"tint\", vec3(1.0));",
        )
        .replace(
            "output roughness = 0.6 + 0.2 * tone;",
            "output roughness = 0.6 + 0.2 * tone + 0.1 * length(instance_root) + 0.05 * vertex_attribute(\"heightRatio\", 0.0);",
        );
    let plain = compile("plain.ms", PLAIN).unwrap();
    let instanced = compile("instanced.ms", &instanced).unwrap();
    let request = BakeRequest {
        width: 24,
        height: 16,
        domain: Default::default(),
        inspection: None,
    };
    let before = plain.bake(&request, &Inputs::default()).unwrap();
    let defaults = Inputs {
        render: Some(RenderBindings::default()),
        ..Inputs::default()
    };
    let after = instanced.bake(&request, &defaults).unwrap();
    assert_eq!(after.base_color, before.base_color);
    assert_eq!(after.properties, before.properties);
    assert_eq!(after.normal, before.normal);
    assert_eq!(after.emissive, before.emissive);
    // An authoring bake binds nothing: the instanced program's channels are render-only there.
    let error = instanced
        .bake(&request, &Inputs::default())
        .err()
        .expect("render-only without bindings");
    assert_eq!(error.code, "MS_INPUT");
    // And the plain program's dependencies, the bake key, read nothing render-only.
    let table = plain.dependency_table();
    assert!(table.outputs.values().all(|read| {
        !read
            .sample
            .names()
            .iter()
            .any(|name| ["time", "instance", "vertex", "sun", "field"].contains(name))
    }));
}

#[test]
fn render_bindings_reach_each_input_on_the_cpu() {
    let program = compile(
        "inputs.ms",
        "material 1;
let root = instance_root;
let tint = instance_tint;
let random = instance_random;
let placed = placed_world;
let ratio = vertex_attribute(\"heightRatio\", 0.0);
let pivot = instance_attribute(\"pivot\", vec3(0.0));
let clock = time;
let sun = sun_direction;
output base_color = vec3(0.5);
output emissive = abs(root + tint + placed + pivot + sun) * 0.0 + vec3(random + ratio + clock) * 0.0;",
    )
    .unwrap();
    let mut bindings = RenderBindings {
        time: 2.5,
        sun_direction: [0., 0., 1.],
        instance_root: [1., 2., 3.],
        instance_tint: [0.5, 0.25, 1.],
        instance_random: 0.75,
        placed_world: Some([4., 5., 6.]),
        ..RenderBindings::default()
    };
    bindings
        .vertex_attributes
        .insert("heightRatio".into(), vec![0.4]);
    bindings
        .instance_attributes
        .insert("pivot".into(), vec![0., 1., 0.]);
    let inputs = Inputs {
        render: Some(bindings),
        ..Inputs::default()
    };
    let fields = program
        .evaluate(&inputs, &Sample::default())
        .unwrap()
        .fields;
    use program::FieldValue::{Scalar, Vec3};
    assert_eq!(fields["root"], Vec3([1., 2., 3.]));
    assert_eq!(fields["tint"], Vec3([0.5, 0.25, 1.]));
    assert_eq!(fields["random"], Scalar(0.75));
    assert_eq!(fields["placed"], Vec3([4., 5., 6.]));
    assert_eq!(fields["ratio"], Scalar(0.4));
    assert_eq!(fields["pivot"], Vec3([0., 1., 0.]));
    assert_eq!(fields["clock"], Scalar(2.5));
    assert_eq!(fields["sun"], Vec3([0., 0., 1.]));
    // Unplaced, the drawn point is the sample's own; a missing attribute is its typed value.
    let fields = program
        .evaluate(
            &Inputs {
                render: Some(RenderBindings::default()),
                ..Inputs::default()
            },
            &Sample {
                world: [7., 8., 9.],
                ..Sample::default()
            },
        )
        .unwrap()
        .fields;
    assert_eq!(fields["placed"], Vec3([7., 8., 9.]));
    assert_eq!(fields["ratio"], Scalar(0.));
    assert_eq!(fields["tint"], Vec3([1., 1., 1.]));
    // The default sun is the lighting record's.
    let sun = superspec_types::lighting::Light::default().direction;
    assert_eq!(fields["sun"], Vec3(sun));
    // The shader lists what the renderer must bind.
    let shader = program.shader().unwrap();
    let listed: Vec<(&str, &str)> = shader
        .render_inputs
        .iter()
        .map(|input| (input.symbol.as_str(), input.kind))
        .collect();
    for expected in [
        ("msInstanceRoot", "derived"),
        ("msInstanceTint", "derived"),
        ("msInstanceRandom", "derived"),
        ("msPlacedWorld", "derived"),
        ("msInstance_pivot", "instance"),
        ("msVertex_heightRatio", "vertex"),
        ("msSunDirection", "uniform"),
        ("msTime", "uniform"),
    ] {
        assert!(listed.contains(&expected), "{expected:?} in {listed:?}");
    }
}

/// A 3 by 2 field over [0, 3] x [10, 12]: texel centers at x 0.5, 1.5, 2.5 and y 10.5, 11.5.
fn field() -> SceneField {
    SceneField {
        width: 3,
        height: 2,
        components: 4,
        min: [0., 10.],
        max: [3., 12.],
        values: vec![
            0., 0., 0., 1., 1., 0., 0., 1., 2., 0., 0., 1., // lower row: x = 0, 1, 2
            0., 4., 0., 1., 1., 4., 0., 1., 2., 4., 1., 1., // upper row
        ],
    }
}

#[test]
fn scene_fields_are_sampled_between_texel_centers_and_a_missing_one_is_reported_where_it_is_read() {
    let program = compile(
        "blade.ms",
        "material 1;
let ground = sample_field(\"ground.color\", instance_root.xy);
output base_color = vec3(0.5);
output emissive = ground * 0.0;",
    )
    .unwrap();
    let mut bindings = RenderBindings::default();
    bindings
        .fields
        .insert("ground.color".into(), Arc::new(field()));
    let at = |root: [f64; 3], bindings: &RenderBindings| {
        let mut bindings = bindings.clone();
        bindings.instance_root = root;
        let fields = program
            .evaluate(
                &Inputs {
                    render: Some(bindings),
                    ..Inputs::default()
                },
                &Sample::default(),
            )
            .unwrap()
            .fields;
        match fields["ground"] {
            program::FieldValue::Vec3(value) => value,
            ref other => panic!("{other:?}"),
        }
    };
    // At a texel center the texel; halfway between centers the mean; outside, the edge.
    assert_eq!(at([1.5, 10.5, 0.], &bindings), [1., 0., 0.]);
    assert_eq!(at([1., 11., 0.], &bindings), [0.5, 2., 0.]);
    assert_eq!(at([2.5, 11.5, 0.], &bindings), [2., 4., 1.]);
    assert_eq!(at([-5., 9., 0.], &bindings), [0., 0., 0.]);
    assert_eq!(at([2.25, 11.25, 0.], &bindings), [1.75, 3., 0.75 * 0.75]);
    // A renderer without the field: the diagnostic names it where it is read.
    let error = program
        .evaluate(
            &Inputs {
                render: Some(RenderBindings::default()),
                ..Inputs::default()
            },
            &Sample::default(),
        )
        .unwrap_err();
    assert_eq!(error.code, "MS_INPUT");
    assert!(error.message.contains("ground.color"), "{error}");
    assert_eq!(
        (error.source.location.line, error.source.location.column),
        (2, 14),
        "{error:?}"
    );
    // A bake has no scene: the same place, as a render-only input.
    let error = program
        .evaluate(&Inputs::default(), &Sample::default())
        .unwrap_err();
    assert!(error.message.contains("render-only"), "{error}");
    assert_eq!(error.source.location.line, 2);
    // A field the scene does not have at all is refused where it is written.
    let errors = compile(
        "blade.ms",
        "material 1;\nlet ground = sample_field(\"ground.height\", world.xy);\noutput base_color = vec3(0.5);",
    )
    .err()
    .unwrap();
    assert!(errors[0].message.contains("ground.color"), "{errors:?}");
    assert_eq!(errors[0].source.location.line, 2);
    // The shader binds each field as a texture and its rectangle.
    let shader = program.shader().unwrap();
    assert!(shader.source.contains("uniform highp sampler2D msField0;"));
    assert!(
        shader
            .source
            .contains("ms_sample_field(msField0, msFieldRect0, ")
    );
    let field_inputs: Vec<(&str, Option<&str>)> = shader
        .render_inputs
        .iter()
        .filter(|input| input.kind == "field")
        .map(|input| (input.symbol.as_str(), input.name.as_deref()))
        .collect();
    assert_eq!(
        field_inputs,
        [
            ("msField0", Some("ground.color")),
            ("msFieldRect0", Some("ground.color"))
        ]
    );
}

/// Painted programs whose color is one render input each, for the renderer's test that it binds
/// every input (`tests/live-program-browser.mjs`): by input name, the painted program's shader.
fn probes() -> serde_json::Value {
    let mut probes = serde_json::Map::new();
    for (name, paint) in [
        ("root", "instanceRoot"),
        ("tint", "instanceTint"),
        ("random", "vec3(instanceRandom)"),
        ("placed", "placedWorld"),
        ("height", "vec3(vertexAttribute(\"heightRatio\", 0))"),
        ("fallback", "vec3(vertexAttribute(\"bend\", 0.7))"),
        ("field", "sampleField(\"ground.color\", instanceRoot.xy)"),
        ("sun", "sunDirection"),
    ] {
        let source = format!(
            "import {{ defineMaterial, vec3, sampleField, instanceRoot, instanceTint, instanceRandom, placedWorld, vertexAttribute, sunDirection }} from \"@superspec/material/v1\";
export default defineMaterial({{ shading: \"painted\", base: {{ baseColor: vec3(0.5) }}, paint: {paint} }});
"
        );
        let program = compile(&format!("{name}.material.ts"), &source)
            .unwrap_or_else(|e| panic!("{name}: {e:?}"));
        probes.insert(
            name.into(),
            serde_json::to_value(program.shader().unwrap()).unwrap(),
        );
    }
    probes.into()
}

/// A program that reads every render input but the sun: the browser compares its GLSL with these
/// values too.
const INPUTS: &str = "material 1;
let ground = sample_field(\"ground.color\", instance_root.xy);
let mixed = ground * instance_tint + vec3(instance_random, vertex_attribute(\"heightRatio\", 0.0), 0.0) + 0.01 * placed_world;
output base_color = vec3(0.5);
output emissive = mixed;";

/// 48 samples across the shade boundary, each with its own bindings: a sun from below the horizon
/// to high, cast shadows on every third, and instance, vertex and field inputs that vary.
fn reference_grid() -> (Vec<Sample>, Vec<RenderBindings>) {
    let mut grid = Vec::new();
    let mut bindings_of = Vec::new();
    for j in 0..6 {
        for i in 0..8 {
            let azimuth = i as f64 * 0.7;
            let height = -0.2 + j as f64 * 0.2;
            let horizontal = (1. - height * height).sqrt();
            let sun = [
                horizontal * azimuth.cos(),
                horizontal * azimuth.sin(),
                height,
            ];
            let normal = {
                let n = [0.3 * (i as f64 - 3.5) / 3.5, 0.2, 1.];
                let length = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
                n.map(|v| v / length)
            };
            // No footprint: the shader draws each sample over a 2x2 block of pixels, whose screen
            // derivatives are zero, so neither side filters.
            let sample = Sample {
                world: [i as f64 * 0.37 - 1., j as f64 * 0.29 + 0.5, 0.1 * i as f64],
                normal,
                footprint: 0.,
                ..Sample::default()
            };
            let mut bindings = RenderBindings {
                sun_direction: sun,
                cast_shadow: if (i + j) % 3 == 0 { 0.4 } else { 1. },
                instance_root: [0.3 * i as f64, 10. + 0.35 * j as f64, 0.],
                instance_tint: [0.5 + 0.05 * i as f64, 1., 0.8],
                instance_random: 0.1 * j as f64,
                placed_world: Some([i as f64, j as f64, 1.]),
                ..RenderBindings::default()
            };
            bindings
                .vertex_attributes
                .insert("heightRatio".into(), vec![0.125 * i as f64]);
            bindings
                .fields
                .insert("ground.color".into(), Arc::new(field()));
            grid.push(sample);
            bindings_of.push(bindings);
        }
    }
    (grid, bindings_of)
}

/// A field of `program` at each sample of the grid, with that sample's bindings.
fn evaluate_grid(
    program: &program::Program,
    field: &str,
    grid: &[Sample],
    bindings_of: &[RenderBindings],
) -> Vec<[f64; 3]> {
    grid.iter()
        .zip(bindings_of)
        .map(|(sample, bindings)| {
            let inputs = Inputs {
                render: Some(bindings.clone()),
                ..Inputs::default()
            };
            match program.evaluate(&inputs, sample).unwrap().fields[field] {
                program::FieldValue::Vec3(value) => value,
                ref other => panic!("{other:?}"),
            }
        })
        .collect()
}

/// The grid the browser evaluates with the generated GLSL (`tests/live-program-browser.mjs`):
/// the painted recipe and a program that reads every other render input, each sample with its
/// bindings and the CPU's values.
#[test]
fn the_cpu_reference_fixture_is_written() {
    let painted = compile("painted.material.ts", PAINTED).unwrap();
    let inputs_program = compile("inputs.ms", INPUTS).unwrap();
    let (grid, bindings_of) = reference_grid();
    let painted_values = evaluate_grid(&painted, "painted.color", &grid, &bindings_of);
    let input_values = evaluate_grid(&inputs_program, "mixed", &grid, &bindings_of);
    // The fixture's own sanity: the grid crosses the boundary and holds cast shadows.
    assert!(painted_values.iter().any(|v| v[0] > 0.6) && painted_values.iter().any(|v| v[0] < 0.2));
    let shader = painted.shader().unwrap();
    let defaults: BTreeMap<String, serde_json::Value> = painted
        .metadata()
        .parameters
        .iter()
        .map(|parameter| (parameter.name.clone(), parameter.default.clone()))
        .collect();
    let fixture = serde_json::json!({
        "field": field(),
        "programs": [
            {
                "name": "painted",
                "shader": shader,
                "parameters": defaults,
                "seeds": shader.seeds.iter().map(|seed| program::seed_value(0, &seed.name)).collect::<Vec<_>>(),
                "values": painted_values,
                "symbol": shader.painted,
            },
            {
                "name": "inputs",
                "shader": inputs_program.shader().unwrap(),
                "parameters": {},
                "seeds": [],
                "values": input_values,
                "symbol": "ms_emissive",
            },
        ],
        "samples": grid,
        "bindings": bindings_of.iter().map(|bindings| serde_json::json!({
            "sun": bindings.sun_direction, "cast": bindings.cast_shadow, "root": bindings.instance_root,
            "tint": bindings.instance_tint, "random": bindings.instance_random, "placed": bindings.placed_world,
            "heightRatio": bindings.vertex_attributes["heightRatio"][0],
        })).collect::<Vec<_>>(),
        "probes": probes(),
    });
    if let Ok(path) = std::env::var("MATERIAL_LIVE_GLSL_OUTPUT") {
        std::fs::write(path, serde_json::to_vec(&fixture).unwrap()).unwrap();
    }
}

/// What native and Wasm must agree on byte for byte (`tests/material-program-browser.mjs`): the
/// CPU's live values on the reference grid as exact bits, the painted material's delivered program
/// and baked fallback (its identity hashes every map), and a bake with default bindings.
pub fn live_program_result() -> Vec<u8> {
    let painted = compile("painted.material.ts", PAINTED).unwrap();
    let inputs_program = compile("inputs.ms", INPUTS).unwrap();
    let (grid, bindings_of) = reference_grid();
    let bits = |values: Vec<[f64; 3]>| -> Vec<[u64; 3]> {
        values
            .into_iter()
            .map(|value| value.map(f64::to_bits))
            .collect()
    };
    let request = Request {
        entry: "painted.material.ts".into(),
        source: PAINTED.into(),
        resolution: 16,
        seed: 3,
        ..Request::default()
    };
    let built = Session::default()
        .build(&request, 1, &|name| Err(name.to_owned()), &|| false)
        .unwrap();
    let material = built.model.materials.values().next().unwrap();
    let instanced = compile(
        "instanced.ms",
        &PLAIN.replace(
            "output base_color = vec3(tone, 0.5 * tone, 0.3);",
            "output base_color = vec3(tone, 0.5 * tone, 0.3) * instance_tint * (1.0 - 0.3 * instance_random);",
        ),
    )
    .unwrap();
    let baked = instanced
        .bake(
            &BakeRequest {
                width: 12,
                height: 8,
                domain: Default::default(),
                inspection: None,
            },
            &Inputs {
                render: Some(RenderBindings::default()),
                ..Inputs::default()
            },
        )
        .unwrap();
    serde_json::to_vec(&serde_json::json!({
        "painted": bits(evaluate_grid(&painted, "painted.color", &grid, &bindings_of)),
        "inputs": bits(evaluate_grid(&inputs_program, "mixed", &grid, &bindings_of)),
        "material": material,
        "defaultBindings": [baked.base_color, baked.properties, baked.normal],
    }))
    .unwrap()
}

#[test]
fn portable_live_fixture_covers_the_same_native_and_wasm_evaluator() {
    let bytes = live_program_result();
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(json["painted"].as_array().unwrap().len(), 48);
    assert_eq!(json["material"]["shading"], "painted");
    if let Ok(path) = std::env::var("MATERIAL_LIVE_PARITY_OUTPUT") {
        std::fs::write(path, &bytes).unwrap();
    }
}

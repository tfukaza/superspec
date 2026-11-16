//! Render-only inputs (roadmap 1D): `time`, `instance`, `instanceW` and named instance and
//! vertex attributes exist only where a material is drawn. The shader reads them from what the
//! renderer binds; a bake of a value that reads one fails where it is read, and values that do not
//! read one bake as before.
use superspec_engine::authoring::{Request, Session};
use superspec_engine::materialspec::program::{self, Inputs, Sample, Sources};

fn compile(entry: &str, text: &str) -> Result<program::Program, Vec<program::ProgramDiagnostic>> {
    program::compile(&Sources {
        entry: entry.into(),
        modules: [(entry.to_owned(), text.to_owned())].into(),
    })
}

const PULSE: &str = "material 1;
let pulse = 0.5 + 0.5 * sin(time * 3.0 + instance_w);
let unused = instance.x;
output base_color = vec3(0.4, 0.3, 0.2);
output roughness = 0.7;
output emissive = vec3(pulse, 0.1, 0.0);";

#[test]
fn the_shader_reads_render_only_inputs_and_lists_what_the_renderer_binds() {
    let program = compile("pulse.ms", PULSE).unwrap();
    let shader = program.shader().unwrap();
    assert!(shader.source.contains("uniform float msTime;"));
    assert!(shader.source.contains("#define ms_time msTime"));
    assert!(
        shader
            .source
            .contains("#define ms_instance msInstanceValue")
    );
    assert!(shader.source.contains("ms_instance.w"));
    let symbols: Vec<(&str, &str, &str)> = shader
        .render_inputs
        .iter()
        .map(|input| (input.symbol.as_str(), input.kind, input.glsl))
        .collect();
    assert_eq!(
        symbols,
        [
            ("msInstanceValue", "instance", "vec4"),
            ("msTime", "uniform", "float")
        ]
    );
    assert!(
        shader
            .uniforms
            .iter()
            .any(|uniform| uniform.name == "msTime")
    );
    // `reads` keeps its shape: the render-only parts are sample names of the outputs that read
    // them, and such an output is not intrinsic.
    let emissive = &shader.reads.outputs["emissive"];
    assert!(emissive.sample.contains(&"time") && emissive.sample.contains(&"instance"));
    assert!(!emissive.intrinsic);
    assert!(shader.reads.outputs["roughness"].sample.is_empty());
    // A program that reads none lists none and serializes as before.
    let plain = compile("plain.ms", "material 1;\noutput base_color = vec3(0.5);").unwrap();
    let json = serde_json::to_value(plain.shader().unwrap()).unwrap();
    assert!(json.get("renderInputs").is_none());
}

#[test]
fn a_bake_of_a_value_that_reads_one_fails_where_it_is_read_and_the_others_bake() {
    let program = compile("pulse.ms", PULSE).unwrap();
    let inputs = Inputs::default();
    let error = program.evaluate(&inputs, &Sample::default()).unwrap_err();
    assert_eq!(error.code, "MS_INPUT");
    assert!(error.message.contains("render-only"), "{error}");
    assert_eq!(error.source.location.line, 2, "{error:?}");
    // Without the emissive output, the program bakes: the field that reads `instance` is never
    // evaluated.
    let quiet = PULSE.replace("output emissive = vec3(pulse, 0.1, 0.0);", "");
    let request = Request {
        entry: "quiet.ms".into(),
        source: quiet,
        resolution: 32,
        ..Request::default()
    };
    let mut session = Session::default();
    session
        .build(&request, 1, &|name| Err(format!("{name}")), &|| false)
        .unwrap_or_else(|e| panic!("{e:?}"));
    // With it, the build fails with the source-linked diagnostic.
    let request = Request {
        source: PULSE.into(),
        ..request
    };
    let errors = session
        .build(&request, 2, &|name| Err(format!("{name}")), &|| false)
        .err()
        .expect("the emissive map reads time");
    assert_eq!(errors[0].code, "MS_INPUT");
    assert!(errors[0].message.contains("render-only"), "{errors:?}");
}

const SWAY: &str = r#"import { defineMaterial, instanceAttribute, vertexAttribute, time, mix, mul, add, sin, vec3 } from "@superspec/material/v1";
const tip = vertexAttribute("heightRatio", 0);
const tint = instanceAttribute("tint", vec3(1));
const shade = add(0.8, mul(0.2, sin(add(time, instanceAttribute("random", 0)))));
export default defineMaterial({
  base: { baseColor: vec3(0.2, 0.5, 0.1) },
  channels: { baseColor: mul(mix(vec3(0.1, 0.3, 0.05), vec3(0.3, 0.6, 0.2), tip), mul(tint, shade)) },
});
"#;

#[test]
fn a_recipe_reads_named_instance_and_vertex_attributes() {
    let program = compile("sway.material.ts", SWAY).unwrap_or_else(|e| panic!("{e:?}"));
    let shader = program.shader().unwrap();
    for symbol in [
        "msVertex_heightRatio",
        "msInstance_tint",
        "msInstance_random",
        "ms_time",
    ] {
        assert!(shader.source.contains(symbol), "{symbol}");
    }
    let listed: Vec<(&str, &str)> = shader
        .render_inputs
        .iter()
        .map(|input| (input.symbol.as_str(), input.glsl))
        .collect();
    assert_eq!(
        listed,
        [
            ("msInstance_random", "float"),
            ("msInstance_tint", "vec3"),
            ("msTime", "float"),
            ("msVertex_heightRatio", "float"),
        ]
    );
    let error = program
        .evaluate(&Inputs::default(), &Sample::default())
        .unwrap_err();
    assert!(error.message.contains("render-only"), "{error}");
    // An attribute's name becomes a GLSL symbol: letters and digits only.
    let errors = compile(
        "bad.material.ts",
        &SWAY.replace("\"heightRatio\"", "\"height-ratio\""),
    )
    .err()
    .expect("refused");
    assert!(errors[0].message.contains("Attribute names"), "{errors:?}");
}

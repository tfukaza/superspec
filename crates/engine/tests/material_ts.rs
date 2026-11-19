use serde_json::json;
use std::collections::BTreeMap;
use superspec_engine::materialspec::{
    catalog,
    program::{
        self, CHIPPED_CONCRETE, CHIPPED_CONCRETE_TS as CONCRETE_TS, COMMON_HELPERS,
        COMMON_HELPERS_TS as COMMON_TS, EditOp, FieldValue, Inputs, Outputs, PAINTED_METAL,
        PAINTED_METAL_TS, Program, Sample, Sources, TILED_COURTYARD_TS, apply_edit, inspect_recipe,
    },
};

const SDK: &str = "@superspec/material/v1";

fn recipe(entry: &str, modules: &[(&str, &str)]) -> Sources {
    Sources {
        entry: entry.into(),
        modules: modules
            .iter()
            .map(|(name, text)| ((*name).to_owned(), (*text).to_owned()))
            .collect(),
    }
}

fn single(text: &str) -> Sources {
    recipe("main.material.ts", &[("main.material.ts", text)])
}

fn compile(text: &str) -> Program {
    program::compile(&single(text)).unwrap_or_else(|errors| {
        panic!(
            "{}",
            errors
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join("\n")
        )
    })
}

fn errors(text: &str) -> Vec<program::ProgramDiagnostic> {
    match program::compile(&single(text)) {
        Ok(_) => panic!("expected compilation to fail"),
        Err(errors) => errors,
    }
}

fn evaluate_with(program: &Program, sample: &Sample) -> Outputs {
    program
        .evaluate(&Inputs::default(), sample)
        .expect("sample evaluates")
}

fn at(surface: [f64; 2]) -> Sample {
    Sample {
        uv: surface,
        surface,
        world: [surface[0], surface[1], 0.0],
        ..Sample::default()
    }
}

fn scalar(outputs: &Outputs, name: &str) -> f64 {
    match &outputs.fields[name] {
        FieldValue::Scalar(value) => *value,
        other => panic!("{name} is {other:?}"),
    }
}

fn concrete_ts() -> Sources {
    recipe(
        "main.material.ts",
        &[
            ("main.material.ts", CONCRETE_TS),
            ("common.material.ts", COMMON_TS),
        ],
    )
}

#[test]
fn typescript_concrete_matches_the_ms_program_exactly() {
    let ts = program::compile(&concrete_ts()).unwrap_or_else(|e| panic!("{e:?}"));
    let ms = program::compile(&recipe(
        "main.ms",
        &[("main.ms", CHIPPED_CONCRETE), ("common.ms", COMMON_HELPERS)],
    ))
    .expect("ms compiles");
    let edges = vec![program::Edge {
        start: [0.0, 0.0, 0.5],
        end: [2.0, 0.0, 0.5],
    }];
    let inputs = Inputs {
        edges,
        ..Inputs::default()
    };
    for (index, point) in [
        [0.0, 0.0],
        [0.37, 0.12],
        [-1.2, 0.8],
        [3.3, -2.1],
        [0.05, 0.4],
    ]
    .into_iter()
    .enumerate()
    {
        let sample = Sample {
            footprint: if index % 2 == 0 { 0.0 } else { 0.01 },
            world: [point[0], point[1], 0.3],
            ..at(point)
        };
        let a = ts.evaluate(&inputs, &sample).unwrap();
        let b = ms.evaluate(&inputs, &sample).unwrap();
        assert_eq!(a.base_color, b.base_color, "base color at {point:?}");
        assert_eq!(a.roughness, b.roughness, "roughness at {point:?}");
        assert_eq!(a.metalness, b.metalness);
        assert_eq!(a.height, b.height, "height at {point:?}");
    }
    let metadata = ts.metadata();
    assert_eq!(metadata.dependencies, ["common.material.ts"]);
    let parameters: BTreeMap<_, _> = metadata
        .parameters
        .iter()
        .map(|p| (p.name.as_str(), (p.min, p.max)))
        .collect();
    assert_eq!(parameters["chipSize"], (Some(0.005), Some(0.5)));
    assert_eq!(parameters["tint"], (Some(0.0), Some(1.0)));
    assert!(metadata.required_inputs.contains(&"seed".to_owned()));
    assert!(metadata.required_inputs.contains(&"edges".to_owned()));
}

#[test]
fn parameter_overrides_reuse_the_program_and_validate_ranges() {
    let program = program::compile(&concrete_ts()).unwrap();
    let mut inputs = Inputs::default();
    inputs.parameters.insert("chipSize".into(), json!(0.2));
    let wide = program.evaluate(&inputs, &at([0.4, 0.4])).unwrap();
    let narrow = program
        .evaluate(&Inputs::default(), &at([0.4, 0.4]))
        .unwrap();
    assert_ne!(wide.height, narrow.height);
    inputs.parameters.insert("chipSize".into(), json!(9.0));
    assert!(program.evaluate(&inputs, &at([0.4, 0.4])).is_err());
    inputs.parameters.clear();
    inputs.parameters.insert("chip_size".into(), json!(0.1));
    assert!(
        program.evaluate(&inputs, &at([0.4, 0.4])).is_err(),
        "unknown name"
    );
}

#[test]
fn relative_imports_keep_private_helpers_in_their_defining_module() {
    let sources = recipe(
        "main.material.ts",
        &[
            (
                "main.material.ts",
                r#"import { defineMaterial, vec3 } from "@superspec/material/v1";
import { tone, amount } from "./a.material";
import { gloss } from "./b.material";
const hidden = 0.95;
export default defineMaterial({base:{baseColor:vec3(tone(0.1)),roughness:gloss,metalness:amount}});
"#,
            ),
            (
                "a.material.ts",
                r#"import { add, param } from "@superspec/material/v1";
import { weight } from "./nested/common.material";
const hidden = 0.2;
const privateBias = param.scalar({default:0.1,range:[0,1]});
const helper = (value: Scalar): Scalar => add(add(value, hidden), add(weight, privateBias));
export function tone(value: Scalar): Scalar { return helper(value); }
export const amount = param.scalar({default:0.3,range:[0,1]});
"#,
            ),
            (
                "b.material.ts",
                r#"import { add } from "@superspec/material/v1";
const hidden = 0.7;
const helper = (hidden: Scalar): Scalar => add(hidden, 0.1);
export const gloss = helper(hidden);
"#,
            ),
            (
                "nested/common.material.ts",
                "const hidden = 0.6; export const weight = 0.1;",
            ),
        ],
    );
    let program = program::compile(&sources).unwrap_or_else(|errors| panic!("{errors:?}"));
    let values = evaluate_with(&program, &Sample::default());
    assert!((values.base_color[0] - 0.5).abs() < 1e-12);
    assert!((values.roughness - 0.8).abs() < 1e-12);
    assert_eq!(values.metalness, 0.3);
    let parameter = program
        .metadata()
        .parameters
        .iter()
        .find(|parameter| parameter.name == "amount")
        .unwrap();
    assert_eq!(parameter.name, "amount");
    assert_eq!(parameter.source.module, "a.material.ts");
    let mut inputs = Inputs::default();
    inputs.parameters.insert(parameter.name.clone(), json!(0.6));
    inputs.parameters.insert("privateBias".into(), json!(0.2));
    assert_eq!(
        program
            .evaluate(&inputs, &Sample::default())
            .unwrap()
            .metalness,
        0.6
    );
    assert!(
        (program
            .evaluate(&inputs, &Sample::default())
            .unwrap()
            .base_color[0]
            - 0.6)
            .abs()
            < 1e-12
    );
}

#[test]
fn imports_report_unsupported_objects_languages_and_conflicting_host_parameters() {
    for initializer in ["layer({id:\"rim\",mask:0.4})", "tileAndGrout({})"] {
        let helper = format!(
            "import {{layer,tileAndGrout}} from \"{SDK}\"; export const shared = {initializer};"
        );
        let entry = format!(
            "import {{defineMaterial,vec3}} from \"{SDK}\"; import {{shared}} from \"./helper.material\"; export default defineMaterial({{base:{{baseColor:vec3(0.4)}}}});"
        );
        let errors = program::compile(&recipe(
            "main.material.ts",
            &[
                ("main.material.ts", &entry),
                ("helper.material.ts", &helper),
            ],
        ))
        .err()
        .unwrap();
        assert!(errors.iter().any(|error| error.code == "MT_SUBSET" && error.source.module == "main.material.ts"));
    }
    let ts = format!(
        "import {{defineMaterial,vec3}} from \"{SDK}\"; import {{value}} from \"./helper.ms\"; export default defineMaterial({{base:{{baseColor:vec3(value)}}}});"
    );
    let ms = "material 1; import \"./helper.material.ts\"; output base_color = vec3(value);";
    for sources in [
        recipe(
            "main.material.ts",
            &[
                ("main.material.ts", &ts),
                ("helper.ms", "material 1; field value = 0.4;"),
            ],
        ),
        recipe(
            "main.ms",
            &[
                ("main.ms", ms),
                ("helper.material.ts", "export const value = 0.4;"),
            ],
        ),
    ] {
        let errors = program::compile(&sources).err().unwrap();
        assert!(
            errors
                .iter()
                .any(|error| error.code == "MS_IMPORT_LANGUAGE"
                    && error.source.module == sources.entry),
            "{errors:?}"
        );
    }
    let entry = format!(
        "import {{defineMaterial,vec3,param}} from \"{SDK}\"; import {{value}} from \"./helper.material\"; const amount = param.scalar({{default:0.5}}); export default defineMaterial({{base:{{baseColor:vec3(value)}}}});"
    );
    let helper = format!(
        "import {{param}} from \"{SDK}\"; const amount = param.scalar({{default:0.4}}); export const value = amount;"
    );
    let errors = program::compile(&recipe(
        "main.material.ts",
        &[
            ("main.material.ts", &entry),
            ("helper.material.ts", &helper),
        ],
    ))
    .err()
    .unwrap();
    assert!(
        errors.iter().any(|error| error.code == "MS_DUPLICATE"),
        "host parameter IDs remain globally unique"
    );
}

#[test]
fn helper_parameters_shadow_module_group_and_layer_names() {
    let text = format!(
        "import {{defineMaterial,vec3,add,tileAndGrout,layer}} from \"{SDK}\"; const tiles=tileAndGrout({{}}); const rim=layer({{id:\"rim\",mask:0.3}}); function tone(tiles: Scalar,rim: Scalar): Scalar {{ return add(tiles,rim); }} export default defineMaterial({{base:{{baseColor:vec3(tone(0.2,0.3))}}}});"
    );
    assert_eq!(
        evaluate_with(&compile(&text), &Sample::default()).base_color,
        [0.5; 3]
    );
    let invalid = format!(
        "import {{defineMaterial,vec3}} from \"{SDK}\"; function helper(x: Scalar): Scalar {{ return x; }} function tone(helper: Scalar): Scalar {{ return helper(0.3); }} export default defineMaterial({{base:{{baseColor:vec3(tone(0.2))}}}});"
    );
    assert!(
        errors(&invalid)
            .iter()
            .any(|error| error.code == "MS_TYPE" && error.message.contains("not callable"))
    );
}

#[test]
fn relative_imports_reject_missing_exports_and_unimported_bindings_at_their_source() {
    let helper = "export const visible = 0.2; const hidden = 0.8; const privateHelper = (x: Scalar): Scalar => x;";
    for (import, expression, code, needle) in [
        ("missing", "0.4", "MS_IMPORT_NAME", "missing"),
        ("hidden", "0.4", "MS_IMPORT_NAME", "hidden"),
        ("visible", "hidden", "MS_NAME", "hidden"),
        ("visible", "privateHelper(0.4)", "MS_NAME", "privateHelper"),
    ] {
        let text = format!(
            "// 日本語 ✨\nimport {{ defineMaterial, vec3 }} from \"{SDK}\";\nimport {{ {import} }} from \"./helper.material\";\nexport default defineMaterial({{base:{{baseColor:vec3({expression})}}}});\n"
        );
        let sources = recipe(
            "main.material.ts",
            &[("main.material.ts", &text), ("helper.material.ts", helper)],
        );
        let diagnostics = program::compile(&sources)
            .err()
            .expect("invalid scope must fail");
        let diagnostic = diagnostics
            .iter()
            .find(|error| error.code == code)
            .unwrap_or_else(|| panic!("{diagnostics:?}"));
        assert_eq!(diagnostic.source.module, "main.material.ts");
        assert_eq!(
            diagnostic.source.location.offset,
            text[..text.find(needle).unwrap()].encode_utf16().count()
        );
    }
    let valid = format!(
        "import {{ defineMaterial, vec3 }} from \"{SDK}\";\nimport {{ visible }} from \"./helper.material\";\nexport default defineMaterial({{base:{{baseColor:vec3(visible)}}}});\n"
    );
    assert!(
        program::compile(&recipe(
            "main.material.ts",
            &[("main.material.ts", &valid), ("helper.material.ts", helper)]
        ))
        .is_ok()
    );

    for (text, code) in [
        (
            valid.replace("import { visible }", "import { visible as renamed }"),
            "MT_SUBSET",
        ),
        (
            valid.replace("export default", "const visible = 0.5; export default"),
            "MS_DUPLICATE",
        ),
        (
            valid.replace("import { visible }", "import { visible, visible }"),
            "MS_DUPLICATE",
        ),
        (
            valid.replace("./helper.material", "../helper.material"),
            "MS_IMPORT_PATH",
        ),
    ] {
        let diagnostics = program::compile(&recipe(
            "main.material.ts",
            &[("main.material.ts", &text), ("helper.material.ts", helper)],
        ))
        .err()
        .expect("invalid import must fail");
        assert!(
            diagnostics.iter().any(|error| error.code == code),
            "{diagnostics:?}"
        );
    }
}

fn layered(mask: &str, extra: &str) -> String {
    format!(
        r#"import {{ defineMaterial, layer, vec3 }} from "{SDK}";
export default defineMaterial({{
  base: {{ baseColor: vec3(0.2, 0.2, 0.2), roughness: 0.4, height: 0.01 }},
  layers: [
    layer({{ id: "wear", mask: {mask}, baseColor: vec3(1, 0.5, 0), roughness: 1, {extra} }}),
  ],
}});
"#
    )
}

#[test]
fn layers_blend_in_order_clamp_coverage_and_pass_omitted_channels_through() {
    let sample = at([0.0, 0.0]);
    let half = evaluate_with(&compile(&layered("0.5", "")), &sample);
    for (actual, expected) in half.base_color.iter().zip([0.6, 0.35, 0.1]) {
        assert!((actual - expected).abs() < 1e-12, "{actual} vs {expected}");
    }
    assert!((half.roughness - 0.7).abs() < 1e-12);
    assert_eq!(half.height, 0.01, "height was not named by the layer");
    assert_eq!(half.metalness, 0.0, "omitted channels keep their default");
    let over = evaluate_with(&compile(&layered("2", "")), &sample);
    assert_eq!(over.base_color, [1.0, 0.5, 0.0], "coverage clamps to one");
    let under = evaluate_with(&compile(&layered("-1", "")), &sample);
    assert_eq!(under.base_color, [0.2, 0.2, 0.2], "coverage clamps to zero");
    assert_eq!(scalar(&over, "layers.wear.coverage"), 1.0);
    assert_eq!(scalar(&half, "layers.wear.coverage"), 0.5);
}

#[test]
fn height_layers_add_a_delta_or_blend_toward_replacement() {
    let sample = at([0.0, 0.0]);
    let add = evaluate_with(&compile(&layered("0.5", "height: { add: -0.02 }")), &sample);
    assert!((add.height - 0.0).abs() < 1e-12, "0.01 + 0.5 * -0.02");
    let replace = evaluate_with(
        &compile(&layered("0.5", "height: { replace: 0.03 }")),
        &sample,
    );
    assert!(
        (replace.height - 0.02).abs() < 1e-12,
        "halfway from 0.01 to 0.03"
    );
}

#[test]
fn layers_apply_in_declared_order() {
    let text = format!(
        r#"import {{ defineMaterial, layer, vec3 }} from "{SDK}";
const first = layer({{ id: "first", mask: 1, roughness: 0.2 }});
export default defineMaterial({{
  base: {{ baseColor: vec3(0, 0, 0), roughness: 1 }},
  layers: [first, layer({{ id: "second", mask: 0.5, roughness: 0.6 }})],
}});
"#
    );
    let out = evaluate_with(&compile(&text), &at([0.0, 0.0]));
    assert!(
        (out.roughness - 0.4).abs() < 1e-12,
        "second blends from the first layer's 0.2"
    );
}

#[test]
fn channel_overrides_replace_the_final_value() {
    let text = format!(
        r#"import {{ defineMaterial, layer, vec3 }} from "{SDK}";
export default defineMaterial({{
  base: {{ baseColor: vec3(0.1, 0.1, 0.1), roughness: 0.5 }},
  layers: [layer({{ id: "a", mask: 1, roughness: 0.9 }})],
  channels: {{ roughness: 0.25 }},
}});
"#
    );
    assert_eq!(
        evaluate_with(&compile(&text), &at([0.0, 0.0])).roughness,
        0.25
    );
}

#[test]
fn tile_group_exposes_face_joint_chip_and_substrate_coverage() {
    let text = format!(
        r#"import {{ defineMaterial, layer, tileAndGrout, vec2, vec3 }} from "{SDK}";
const tiles = tileAndGrout({{ size: vec2(0.3, 0.3), groutWidth: 0.01 }});
export default defineMaterial({{
  base: {{ baseColor: vec3(0.8, 0.8, 0.8), roughness: 0.2 }},
  layers: [layer({{ id: "grout", mask: tiles.joint, baseColor: vec3(0.2, 0.2, 0.2), roughness: 0.9 }})],
}});
"#
    );
    let program = compile(&text);
    let center = evaluate_with(&program, &at([0.15, 0.15]));
    assert!(
        scalar(&center, "tiles.joint") < 1e-6,
        "tile centers have no joint"
    );
    assert!(scalar(&center, "tiles.face") > 0.999);
    assert_eq!(center.base_color, [0.8, 0.8, 0.8].map(|v| v));
    let joint = evaluate_with(&program, &at([0.3, 0.15]));
    assert!(
        scalar(&joint, "tiles.joint") > 0.999,
        "tile edges are joints"
    );
    assert!(scalar(&joint, "tiles.face") < 1e-6);
    assert!(scalar(&joint, "tiles.substrate") > 0.999);
    assert!(joint.base_color[0] < 0.21, "grout layer darkens joints");
    // A coarse footprint widens the joint transition without changing the tile layout.
    let coarse = evaluate_with(
        &program,
        &Sample {
            footprint: 0.05,
            ..at([0.3, 0.15])
        },
    );
    assert!(scalar(&coarse, "tiles.joint") > 0.5);
}

#[test]
fn staggered_tile_rows_shift_joints_by_the_row_offset() {
    let text = format!(
        r#"import {{ defineMaterial, tileAndGrout, vec2, vec3 }} from "{SDK}";
const tiles = tileAndGrout({{ size: vec2(0.3, 0.15), groutWidth: 0.01, rowOffset: 0.5 }});
export default defineMaterial({{ base: {{ baseColor: vec3(1, 1, 1) }}, channels: {{ roughness: tiles.joint }} }});
"#
    );
    let program = compile(&text);
    // Row 0: vertical joint at x = 0.3. Row 1 (y in [0.15, 0.3)): joint shifts by half a tile.
    assert!(evaluate_with(&program, &at([0.3, 0.07])).roughness > 0.99);
    assert!(evaluate_with(&program, &at([0.3, 0.22])).roughness < 0.01);
    assert!(evaluate_with(&program, &at([0.15, 0.22])).roughness > 0.99);
    // Negative coordinates use the same floor rule: row -1 is odd (shifted), row -2 is even.
    assert!(evaluate_with(&program, &at([-0.15, -0.07])).roughness > 0.99);
    assert!(evaluate_with(&program, &at([-0.3, -0.07])).roughness < 0.01);
    assert!(evaluate_with(&program, &at([-0.3, -0.22])).roughness > 0.99);
}

#[test]
fn group_defaults_and_named_seeds_are_independent_per_instance() {
    let text = format!(
        r#"import {{ defineMaterial, groundWeathering, vec3, add }} from "{SDK}";
const first = groundWeathering({{}});
const second = groundWeathering({{ scale: 0.3 }});
export default defineMaterial({{ base: {{ baseColor: vec3(1, 1, 1) }}, channels: {{ roughness: add(first.stain, second.stain) }} }});
"#
    );
    let program = compile(&text);
    let out = evaluate_with(
        &program,
        &Sample {
            world: [0.2, 0.7, 0.0],
            surface: [0.2, 0.7],
            ..Sample::default()
        },
    );
    assert_ne!(scalar(&out, "first.stain"), scalar(&out, "second.stain"));
    assert!(scalar(&out, "first.contact") > 0.99, "at ground level");
    let high = evaluate_with(
        &program,
        &Sample {
            world: [0.2, 0.7, 5.0],
            surface: [0.2, 0.7],
            ..Sample::default()
        },
    );
    assert_eq!(
        scalar(&high, "first.stain"),
        0.0,
        "contact fades with height"
    );
}

#[test]
fn runoff_group_requires_edges_and_reports_finite_edge_inputs() {
    let text = format!(
        r#"import {{ defineMaterial, runoffStreaks, vec3 }} from "{SDK}";
const streaks = runoffStreaks({{ width: 0.1, length: 1 }});
export default defineMaterial({{ base: {{ baseColor: vec3(1, 1, 1) }}, channels: {{ roughness: streaks.coverage }} }});
"#
    );
    let program = compile(&text);
    assert!(
        program
            .metadata()
            .required_inputs
            .contains(&"edges".to_owned())
    );
}

#[test]
fn live_renderer_operations_are_recorded_with_their_input_fields() {
    let text = format!(
        r#"import {{ defineMaterial, modulateDiffuse, grazingFalloff, vec3, noise, seed, surface }} from "{SDK}";
const grain = noise(surface, seed("grain"));
export default defineMaterial({{
  base: {{ baseColor: vec3(0.5, 0.5, 0.5) }},
  live: [modulateDiffuse({{ factor: 0.9, mask: grain }}), grazingFalloff({{ from: 0.2, to: 0.8 }})],
}});
"#
    );
    let program = compile(&text);
    let live = &program.metadata().live;
    assert_eq!(live.len(), 2);
    assert_eq!(live[0].id, "modulateDiffuse");
    assert_eq!(live[0].inputs["mask"], "live.0.mask");
    assert!(
        live[0]
            .requires
            .contains(&"diffuse-contribution".to_owned())
    );
    assert_eq!(live[1].id, "grazingFalloff");
    assert!(
        program
            .metadata()
            .fields
            .iter()
            .any(|f| f.name == "live.1.from"),
        "live inputs stay inspectable"
    );
    let serialized = serde_json::to_value(program.metadata()).unwrap();
    assert!(serialized["live"].is_array());
}

fn codes(errors: &[program::ProgramDiagnostic]) -> Vec<&'static str> {
    errors.iter().map(|e| e.code).collect()
}

#[test]
fn recipe_errors_are_source_linked_with_utf16_offsets_and_collected() {
    let text = format!(
        "// ✨ emoji before the errors\nimport {{ defineMaterial, vec3, nope }} from \"{SDK}\";\nconst a = 1;\nexport default defineMaterial({{ base: {{ baseColor: vec3(1) }} }});\n"
    );
    let found = errors(&text);
    // Import and expression errors are reported together, in source order.
    assert_eq!(codes(&found), ["MS_NAME"], "{found:?}");
    let name = &found[0];
    assert_eq!(name.source.module, "main.material.ts");
    assert_eq!(name.source.location.line, 2);
    // The emoji sits on the previous line; columns count UTF-16 units within the line.
    let expected = text.lines().nth(1).unwrap().find("nope").unwrap() + 1;
    assert_eq!(name.source.location.column, expected);
    let emoji_units = "// ✨ emoji before the errors\n".encode_utf16().count();
    assert_eq!(name.source.location.offset, emoji_units + expected - 1);
    assert_eq!(
        name.source.location.end.unwrap() - name.source.location.offset,
        4
    );
}

#[test]
fn lowering_and_type_errors_are_each_collected_in_one_pass() {
    // The `1 * 2` statement is a syntax-subset error found while parsing, so only it is reported.
    let parse_error = errors(&format!(
        "import {{ defineMaterial, vec3 }} from \"{SDK}\";\nconst a = 1 * 2;\nexport default defineMaterial({{ base: {{ baseColor: vec3(1) }} }});\n"
    ));
    assert_eq!(codes(&parse_error), ["MT_SUBSET"]);
    // Lowering collects independent statement errors.
    let lowering = errors(&format!(
        "import {{ defineMaterial, vec3, mix }} from \"{SDK}\";\nconst a = noise(1);\nconst b = vec3;\nexport default defineMaterial({{ base: {{ baseColor: vec3(1) }} }});\n"
    ));
    assert_eq!(lowering.len(), 2, "{lowering:?}");
    assert_eq!(lowering[0].source.location.line, 2);
    assert_eq!(lowering[1].source.location.line, 3);
    // The shared checker collects one error per bad definition.
    let typed = errors(&format!(
        "import {{ defineMaterial, vec3, mix }} from \"{SDK}\";\nconst a = mix(1, 2, 3, 4);\nconst b = vec3(1, 2);\nexport default defineMaterial({{ base: {{ baseColor: vec3(1) }}, channels: {{ roughness: a }} }});\n"
    ));
    assert_eq!(codes(&typed), ["MS_TYPE", "MS_TYPE"], "{typed:?}");
    assert_eq!(
        (typed[0].source.location.line, typed[1].source.location.line),
        (2, 3)
    );
}
#[test]
fn operators_misspellings_and_missing_imports_are_rejected_clearly() {
    let operator = errors(&format!(
        "import {{ defineMaterial, vec3 }} from \"{SDK}\";\nconst a = 1 + 2;\nexport default defineMaterial({{ base: {{ baseColor: vec3(1) }} }});\n"
    ));
    assert_eq!(codes(&operator), ["MT_SUBSET"]);
    assert!(operator[0].message.contains("add()"));

    let missing = errors(&format!(
        "import {{ defineMaterial, vec3 }} from \"{SDK}\";\nconst a = fbm(surface, seed(\"a\"), 2);\nexport default defineMaterial({{ base: {{ baseColor: vec3(1) }} }});\n"
    ));
    assert!(codes(&missing).contains(&"MS_NAME"));

    let snake = errors(&format!(
        "import {{ defineMaterial, vec3 }} from \"{SDK}\";\nconst a = sample_color(\"x\", vec2(0, 0));\nexport default defineMaterial({{ base: {{ baseColor: vec3(1) }} }});\n"
    ));
    assert!(snake.iter().any(|e| e.message.contains("sampleColor")));

    let version = errors(
        "import { defineMaterial } from \"@superspec/material/v9\";\nexport default defineMaterial({ base: {} });\n",
    );
    assert!(codes(&version).contains(&"MS_VERSION"));
}

#[test]
fn channel_type_mismatches_and_structure_errors_point_at_the_user_value() {
    let color_for_roughness =
        errors(&layered("0.5", "").replace("roughness: 0.4", "roughness: vec3(1, 1, 1)"));
    assert!(
        color_for_roughness.iter().any(|e| e.code == "MS_TYPE"),
        "{color_for_roughness:?}"
    );

    let missing_base = errors(&format!(
        "import {{ defineMaterial }} from \"{SDK}\";\nexport default defineMaterial({{ base: {{ roughness: 0.2 }} }});\n"
    ));
    assert!(codes(&missing_base).contains(&"MT_BASE"));

    let duplicate = errors(&format!(
        "import {{ defineMaterial, layer, vec3 }} from \"{SDK}\";\nexport default defineMaterial({{ base: {{ baseColor: vec3(1) }}, layers: [layer({{ id: \"a\", mask: 1 }}), layer({{ id: \"a\", mask: 1 }})] }});\n"
    ));
    assert!(codes(&duplicate).contains(&"MS_DUPLICATE"));

    let unknown_key = errors(&format!(
        "import {{ defineMaterial, layer, vec3 }} from \"{SDK}\";\nexport default defineMaterial({{ base: {{ baseColor: vec3(1) }}, layers: [layer({{ id: \"a\", mask: 1, rougness: 1 }})] }});\n"
    ));
    assert!(codes(&unknown_key).contains(&"MT_PROPERTY"));
}

#[test]
fn every_name_the_declarations_export_is_accepted_by_the_import_check() {
    let declarations = catalog::sdk::declarations();
    let exported: Vec<&str> = declarations
        .lines()
        .filter_map(|line| line.strip_prefix("export "))
        .filter_map(|rest| {
            let rest = rest
                .strip_prefix("declare function ")
                .or_else(|| rest.strip_prefix("declare const "))
                .or_else(|| rest.strip_prefix("interface "))
                .or_else(|| rest.strip_prefix("type "))?;
            Some(rest.split(|c: char| !c.is_alphanumeric()).next().unwrap())
        })
        .collect();
    assert!(exported.len() > 100, "{} names", exported.len());
    let names = catalog::sdk_exports();
    for name in exported {
        assert!(
            names.iter().any(|n| n == name),
            "{name} is exported but cannot be imported"
        );
    }
    let text = format!(
        "import {{ defineMaterial, tileAndGrout, vec3 }} from \"{SDK}\";\nimport type {{ TileAndGroutGroup }} from \"{SDK}\";\nconst tiles = tileAndGrout({{}});\nexport default defineMaterial({{ base: {{ baseColor: vec3(1) }}, channels: {{ roughness: tiles.joint }} }});\n"
    );
    compile(&text);
}

#[test]
fn receiver_dependent_groups_declare_the_receiver_features_they_query() {
    let text = format!(
        "import {{ defineMaterial, edgeWear, vec3 }} from \"{SDK}\";\nconst wear = edgeWear({{ width: 0.02 }});\nexport default defineMaterial({{ base: {{ baseColor: vec3(1) }}, channels: {{ roughness: wear.coverage }} }});\n"
    );
    let program = compile(&text);
    assert!(
        program
            .metadata()
            .required_inputs
            .contains(&"features".to_owned())
    );
    // Every catalog group is available now that receiver features exist.
    assert!(
        catalog::groups()
            .iter()
            .all(|group| group.unavailable.is_none())
    );
}

#[test]
fn parameter_and_helper_errors_use_the_shared_checker() {
    let bad_default = errors(&format!(
        "import {{ defineMaterial, param, vec3, surface, noise, seed }} from \"{SDK}\";\nconst p = param.scalar({{ default: 5, range: [0, 1] }});\nexport default defineMaterial({{ base: {{ baseColor: vec3(1) }} }});\n"
    ));
    assert!(codes(&bad_default).contains(&"MS_PARAMETER"));
    let cycle = errors(&format!(
        "import {{ defineMaterial, vec3, add }} from \"{SDK}\";\nconst f = (x: Scalar): Scalar => g(x);\nconst g = (x: Scalar): Scalar => f(x);\nexport default defineMaterial({{ base: {{ baseColor: vec3(1) }}, channels: {{ roughness: f(1) }} }});\n"
    )
    .replace("{ defineMaterial, vec3, add }", "{ defineMaterial, vec3, add, Scalar }"));
    assert!(codes(&cycle).contains(&"MS_CYCLE"), "{cycle:?}");
}

#[test]
fn syntax_outside_the_subset_is_rejected_without_execution() {
    for (text, fragment) in [
        ("const a = `x`;\n", "Template"),
        ("let a = 1;\n", "const"),
        ("const a = () => 1;\n", "return type"),
        ("class A {}\n", "Expected"),
        ("const a = 1 ? 2 : 3;\n", "Operator"),
    ] {
        let found = errors(text);
        assert!(
            found.iter().any(|e| e.message.contains(fragment)),
            "{text:?}: {found:?}"
        );
    }
}

#[test]
fn recompiling_corrected_source_recovers_after_a_failure() {
    let good = layered("0.5", "");
    assert!(program::compile(&single(&good)).is_ok());
    let broken = good.replace("0.5", "0.5 +");
    assert!(program::compile(&single(&broken)).is_err());
    assert!(program::compile(&single(&good)).is_ok());
}

#[test]
fn ms_diagnostics_are_unchanged_by_the_typescript_front_end() {
    let broken = recipe(
        "main.ms",
        &[(
            "main.ms",
            "material 1;\nlet a = fbm(1, 2);\noutput base_color = vec3(a);\n",
        )],
    );
    let Err(found) = program::compile(&broken) else {
        panic!("expected a type error");
    };
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].code, "MS_TYPE");
}

#[test]
fn generated_sdk_declares_every_operation_and_stays_in_sync_with_the_catalog() {
    let files = catalog::sdk::files();
    let names: Vec<_> = files.iter().map(|f| f.name).collect();
    assert_eq!(
        names,
        [
            "index.d.ts",
            "ports.json",
            "ai-reference.md",
            "tsconfig.json"
        ]
    );
    let declarations = &files[0].contents;
    for operation in catalog::operations() {
        let name = catalog::ts_name(operation.id);
        assert!(
            declarations.contains(&format!("export declare function {name}("))
                || declarations.contains(&format!("export declare function {name}<")),
            "{name}"
        );
    }
    for operator in catalog::operators() {
        assert!(
            declarations.contains(&format!("export declare function {}(", operator.id))
                || declarations.contains(&format!("export declare function {}<", operator.id))
        );
    }
    for group in catalog::groups() {
        assert!(declarations.contains(&format!("export declare function {}(", group.id)));
    }
    assert!(declarations.contains("readonly xz: Vec2<S>;"));
    // Vectors carry coordinate spaces; positions in different spaces only meet through a check.
    assert!(declarations.contains("export declare const world: Vec3<\"world\">;"));
    assert!(declarations.contains("export declare const normal: Vec3<\"direction\">;"));
    assert!(declarations.contains("export declare function sub<S0 extends Space, S1 extends Space>(a: Vec3<S0>, b: Vec3<S1> & Check<S0, S1>): Vec3<Combine<S0, S1>>;"));
    assert!(!declarations.contains(": any"));
    let ports: serde_json::Value = serde_json::from_str(&files[1].contents).unwrap();
    assert_eq!(ports["catalogVersion"], 1);
    assert_eq!(ports["operations"].as_array().unwrap().len(), 61);
    assert!(declarations.contains("export declare function zoneCoord(a: string): Vec3<\"none\">;"));
    // An external input's name is a literal, and its type follows the fallback.
    assert!(declarations.contains("export declare function external("));
    assert!(declarations.contains(
        "export declare function creaseDistance(a: \"convex\" | \"concave\" | \"boundary\" | \"exposed\" | \"any\" | \"seam\" | \"contact\", b: ScalarInput, c: ScalarInput): Scalar;"
    ));
    assert!(files[2].contents.contains("tileAndGrout"));
}

#[test]
fn every_catalog_group_expands_to_its_declared_outputs() {
    for group in catalog::groups().iter().filter(|g| g.unavailable.is_none()) {
        let text = format!(
            "import {{ defineMaterial, {id}, vec3, clamp }} from \"{SDK}\";\nconst g = {id}({{}});\nexport default defineMaterial({{ base: {{ baseColor: vec3(1) }}, channels: {{ roughness: clamp(g.{first}, 0, 1) }} }});\n",
            id = group.id,
            first = group.outputs[0].name
        );
        let program = compile(&text);
        for output in &group.outputs {
            assert!(
                program
                    .metadata()
                    .fields
                    .iter()
                    .any(|f| f.name == format!("g.{}", output.name)),
                "{} lacks {}",
                group.id,
                output.name
            );
        }
        // Every group evaluates at representative samples without runtime failure.
        for sample in [at([0.1, 0.2]), at([-1.3, 2.6]), at([5.0, -0.4])] {
            let sample = Sample {
                world: [sample.surface[0], sample.surface[1], 0.1],
                ..sample
            };
            let inputs = Inputs {
                edges: vec![program::Edge {
                    start: [0.0, 0.0, 0.2],
                    end: [3.0, 0.0, 0.2],
                }],
                ..Inputs::default()
            };
            program.evaluate(&inputs, &sample).expect(group.id);
        }
    }
}

// ---- Source-preserving edits ----

const ANNOTATED: &str = r#"import { defineMaterial, layer, param, vec3, mix, surface, fbm, seed, smoothstep } from "@superspec/material/v1";

// Chip scale in meters; keep this comment through every edit.
const chipSize = param.scalar({ default: 0.08, range: [0.005, 0.5], unit: "m" });
const tint = param.vec3({ default: vec3(0.4, 0.4, 0.4) });
const noise1 = fbm(surface, seed("a"), 3); /* shared mask */
const mask = smoothstep(0.4, 0.6, noise1);
const rim = layer({ id: "rim", mask: 0.25, roughness: 0.3 });

export default defineMaterial({
  base: { baseColor: tint, roughness: mix(0.9, 0.5, mask) },
  layers: [
    // first
    layer({ id: "stain", mask, baseColor: vec3(0.1, 0.1, 0.1) }),
    rim,
    layer({ id: "dust", mask: noise1, roughness: 0.8 }),
  ],
  channels: { height: mix(0, 0.01, mask) },
});
"#;

fn annotated() -> Sources {
    single(ANNOTATED)
}

fn utf16_span(text: &str, needle: &str, nth: usize) -> (usize, usize) {
    let byte = text
        .match_indices(needle)
        .nth(nth)
        .expect("needle present")
        .0;
    let start = text[..byte].encode_utf16().count();
    (start, start + needle.encode_utf16().count())
}

fn applied(op: EditOp) -> program::Edited {
    apply_edit(&annotated(), "main.material.ts", &op).unwrap_or_else(|e| panic!("{e}"))
}

fn diff_lines(before: &str, after: &str) -> Vec<(String, String)> {
    before
        .lines()
        .zip(after.lines())
        .filter(|(a, b)| a != b)
        .map(|(a, b)| (a.to_owned(), b.to_owned()))
        .collect()
}

#[test]
fn inspection_lists_parameters_fields_layers_groups_and_their_use_spans() {
    let inspection = inspect_recipe(&annotated(), "main.material.ts").unwrap();
    assert_eq!(inspection.sdk, "@superspec/material/v1");
    let chip = &inspection.parameters[0];
    assert_eq!(
        (chip.name.as_str(), chip.range, chip.unit.as_deref()),
        ("chipSize", Some([0.005, 0.5]), Some("m"))
    );
    assert_eq!(chip.default_text, "0.08");
    assert_eq!(inspection.parameters[1].default_text, "vec3(0.4, 0.4, 0.4)");
    let mask = inspection.fields.iter().find(|f| f.name == "mask").unwrap();
    assert_eq!(
        mask.references.len(),
        3,
        "base roughness, the stain layer shorthand and the height channel"
    );
    let ids: Vec<_> = inspection.layers.iter().map(|l| l.id.as_str()).collect();
    assert_eq!(ids, ["stain", "rim", "dust"]);
    assert_eq!(inspection.layers[1].declared_as.as_deref(), Some("rim"));
    assert_eq!(inspection.layers[2].mask.as_ref().unwrap().text, "noise1");
    assert_eq!(inspection.base.len(), 2);
    assert_eq!(inspection.channels[0].name, "height");
    // Spans are UTF-16 offsets that slice the source back to the reported text.
    let units: Vec<u16> = ANNOTATED.encode_utf16().collect();
    let slice = |s: program::Span| String::from_utf16(&units[s.start..s.end]).unwrap();
    assert_eq!(slice(chip.default), "0.08");
    assert_eq!(
        slice(inspection.layers[2].mask.as_ref().unwrap().span),
        "noise1"
    );
}

#[test]
fn a_literal_edit_changes_only_that_literal_and_keeps_comments() {
    let (start, _) = utf16_span(ANNOTATED, "0.25", 0);
    let edited = applied(EditOp::SetLiteral {
        offset: start,
        value: 0.75,
    });
    assert_eq!(edited.patches.len(), 1);
    assert_eq!(edited.patches[0].text, "0.75");
    let changes = diff_lines(ANNOTATED, &edited.source);
    assert_eq!(changes.len(), 1);
    assert!(changes[0].1.contains("mask: 0.75") && changes[0].0.contains("mask: 0.25"));
    assert!(
        edited
            .source
            .contains("// Chip scale in meters; keep this comment through every edit.")
    );
    assert!(edited.source.contains("/* shared mask */"));
}

#[test]
fn a_literal_edit_rejects_offsets_that_are_not_numbers() {
    let (start, _) = utf16_span(ANNOTATED, "chipSize", 0);
    assert!(
        apply_edit(
            &annotated(),
            "main.material.ts",
            &EditOp::SetLiteral {
                offset: start,
                value: 1.0
            }
        )
        .is_err()
    );
}

#[test]
fn parameter_defaults_and_ranges_edit_in_place() {
    let scalar = applied(EditOp::SetParameterDefault {
        name: "chipSize".into(),
        value: json!(0.12),
    });
    assert!(scalar.source.contains("default: 0.12, range: [0.005, 0.5]"));
    let vector = applied(EditOp::SetParameterDefault {
        name: "tint".into(),
        value: json!([0.5, 0.25, 0]),
    });
    assert!(vector.source.contains("default: vec3(0.5, 0.25, 0) }"));
    let wider = applied(EditOp::SetParameterRange {
        name: "chipSize".into(),
        range: Some([0.001, 1.0]),
    });
    assert!(wider.source.contains("range: [0.001, 1]"));
    let added = applied(EditOp::SetParameterRange {
        name: "tint".into(),
        range: Some([0.0, 1.0]),
    });
    assert!(
        added
            .source
            .contains("default: vec3(0.4, 0.4, 0.4), range: [0, 1] }")
    );
    let removed = applied(EditOp::SetParameterRange {
        name: "chipSize".into(),
        range: None,
    });
    assert!(
        removed
            .source
            .contains("param.scalar({ default: 0.08, unit: \"m\" })"),
        "{}",
        removed.source
    );
    // The default must satisfy the range, and the checker runs before a patch is returned.
    let failure = apply_edit(
        &annotated(),
        "main.material.ts",
        &EditOp::SetParameterDefault {
            name: "chipSize".into(),
            value: json!(9),
        },
    )
    .unwrap_err();
    assert_eq!(failure.diagnostics[0].code, "MS_PARAMETER");
}

#[test]
fn reconnecting_one_use_of_a_shared_field_changes_only_that_use() {
    let inspection = inspect_recipe(&annotated(), "main.material.ts").unwrap();
    let mask = inspection.fields.iter().find(|f| f.name == "mask").unwrap();
    let base_use = mask.references[0];
    let edited = applied(EditOp::Reconnect {
        start: base_use.start,
        end: base_use.end,
        expression: "noise1".into(),
    });
    let changes = diff_lines(ANNOTATED, &edited.source);
    assert_eq!(changes.len(), 1, "{changes:?}");
    assert!(
        changes[0].0.contains("mix(0.9, 0.5, mask)")
            && changes[0].1.contains("mix(0.9, 0.5, noise1)")
    );
    // The other two uses are untouched and the field is still shared by them.
    let after = inspect_recipe(&single(&edited.source), "main.material.ts").unwrap();
    assert_eq!(
        after
            .fields
            .iter()
            .find(|f| f.name == "mask")
            .unwrap()
            .references
            .len(),
        2
    );
}

#[test]
fn an_incompatible_reconnection_returns_diagnostics_and_no_patch() {
    let inspection = inspect_recipe(&annotated(), "main.material.ts").unwrap();
    let base_use = inspection
        .fields
        .iter()
        .find(|f| f.name == "mask")
        .unwrap()
        .references[0];
    // A color cannot drive the scalar interpolation weight.
    let failure = apply_edit(
        &annotated(),
        "main.material.ts",
        &EditOp::Reconnect {
            start: base_use.start,
            end: base_use.end,
            expression: "tint".into(),
        },
    )
    .unwrap_err();
    assert_eq!(failure.diagnostics[0].code, "MS_TYPE");
    // Replacement text must be exactly one expression.
    for bad in ["mask; evil()", "1 +", ""] {
        assert!(
            apply_edit(
                &annotated(),
                "main.material.ts",
                &EditOp::Reconnect {
                    start: base_use.start,
                    end: base_use.end,
                    expression: bad.into()
                },
            )
            .is_err(),
            "{bad:?}"
        );
    }
    // A range that is not exactly one expression is rejected.
    assert!(
        apply_edit(
            &annotated(),
            "main.material.ts",
            &EditOp::Reconnect {
                start: base_use.start,
                end: base_use.end + 1,
                expression: "noise1".into()
            },
        )
        .is_err()
    );
}

#[test]
fn layers_can_be_added_removed_and_reordered_without_touching_comments() {
    let added = applied(EditOp::AddLayer {
        source: "layer({ id: \"soot\", mask: 0.1, roughness: 1 })".into(),
        index: Some(1),
    });
    let ids = |text: &str| -> Vec<String> {
        inspect_recipe(&single(text), "main.material.ts")
            .unwrap()
            .layers
            .into_iter()
            .map(|l| l.id)
            .collect()
    };
    assert_eq!(ids(&added.source), ["stain", "soot", "rim", "dust"]);
    assert!(added.source.contains("// first"));

    let appended = applied(EditOp::AddLayer {
        source: "layer({ id: \"end\", mask: 0 })".into(),
        index: None,
    });
    assert_eq!(ids(&appended.source), ["stain", "rim", "dust", "end"]);

    let removed_inline = applied(EditOp::RemoveLayer { id: "dust".into() });
    assert_eq!(ids(&removed_inline.source), ["stain", "rim"]);
    let removed_declared = applied(EditOp::RemoveLayer { id: "rim".into() });
    assert_eq!(ids(&removed_declared.source), ["stain", "dust"]);
    assert!(
        !removed_declared.source.contains("const rim"),
        "an unused layer constant goes with it"
    );
    assert!(removed_declared.source.contains("// first"));

    let moved = applied(EditOp::MoveLayer {
        id: "dust".into(),
        to: 0,
    });
    assert_eq!(ids(&moved.source), ["dust", "stain", "rim"]);
    assert!(
        moved.source.contains("// first"),
        "separators stay in place"
    );
    assert_eq!(
        applied(EditOp::MoveLayer {
            id: "stain".into(),
            to: 0
        })
        .patches
        .len(),
        0
    );

    assert!(
        apply_edit(
            &annotated(),
            "main.material.ts",
            &EditOp::RemoveLayer { id: "nope".into() }
        )
        .is_err()
    );
    assert!(
        apply_edit(
            &annotated(),
            "main.material.ts",
            &EditOp::MoveLayer {
                id: "dust".into(),
                to: 9
            }
        )
        .is_err()
    );
}

#[test]
fn removing_a_declared_layer_preserves_neighboring_statements_and_comments() {
    for declaration in [
        "const rim = layer({ id: \"rim\", mask: 0.3, roughness: 0.9 });",
        "export const rim = layer({ id: \"rim\", mask: 0.3, roughness: 0.9 });",
    ] {
        let text = format!(
            "import {{ defineMaterial, layer, vec3 }} from \"{SDK}\";\n\
             const before = 0.1; /* preserve 日本語; */ {declaration} const after = 0.7; // preserve author note\n\
             export default defineMaterial({{ base: {{ baseColor: vec3(0.4) }}, layers: [rim] }});\n"
        );
        let edited = apply_edit(
            &single(&text),
            "main.material.ts",
            &EditOp::RemoveLayer { id: "rim".into() },
        )
        .unwrap();
        assert_eq!(
            edited.source,
            text.replace(declaration, "").replace("[rim]", "[]")
        );
        assert!(
            inspect_recipe(&single(&edited.source), "main.material.ts")
                .unwrap()
                .layers
                .is_empty()
        );
    }
}

#[test]
fn layer_edits_never_remove_an_unrelated_invalid_array_item() {
    let text = format!(
        "import {{defineMaterial,layer,vec3}} from \"{SDK}\"; const bad=0.4; export default defineMaterial({{base:{{baseColor:vec3(0.3)}},layers:[bad,layer({{id:\"rim\",mask:0.3}})]}});"
    );
    let sources = single(&text);
    for operation in [
        EditOp::RemoveLayer { id: "rim".into() },
        EditOp::MoveLayer {
            id: "rim".into(),
            to: 0,
        },
    ] {
        assert!(apply_edit(&sources, "main.material.ts", &operation).is_err());
        assert_eq!(sources.modules["main.material.ts"], text);
    }
    let corrected = single(&text.replace("bad,layer", "layer"));
    let removed = apply_edit(
        &corrected,
        "main.material.ts",
        &EditOp::RemoveLayer { id: "rim".into() },
    )
    .unwrap();
    assert!(
        inspect_recipe(&single(&removed.source), "main.material.ts")
            .unwrap()
            .layers
            .is_empty()
    );
}

#[test]
fn adding_a_layer_creates_the_layers_array_when_missing() {
    let text = format!(
        "import {{ defineMaterial, layer, vec3 }} from \"{SDK}\";\nexport default defineMaterial({{\n  base: {{ baseColor: vec3(1) }},\n}});\n"
    );
    let edited = apply_edit(
        &single(&text),
        "main.material.ts",
        &EditOp::AddLayer {
            source: "layer({ id: \"a\", mask: 1 })".into(),
            index: None,
        },
    )
    .unwrap();
    assert!(
        edited
            .source
            .contains("layers: [layer({ id: \"a\", mask: 1 })]"),
        "{}",
        edited.source
    );
}

#[test]
fn edit_patches_use_utf16_offsets_after_non_ascii_text() {
    let text = format!(
        "// 日本語 ✨\nimport {{ defineMaterial, vec3 }} from \"{SDK}\";\nexport default defineMaterial({{ base: {{ baseColor: vec3(0.5), roughness: 0.25 }} }});\n"
    );
    let sources = single(&text);
    let (start, end) = utf16_span(&text, "0.25", 0);
    let edited = apply_edit(
        &sources,
        "main.material.ts",
        &EditOp::SetLiteral {
            offset: start,
            value: 0.5,
        },
    )
    .unwrap();
    assert_eq!(
        (edited.patches[0].start, edited.patches[0].end),
        (start, end)
    );
    assert!(edited.source.contains("roughness: 0.5 }"));
    assert!(edited.source.starts_with("// 日本語 ✨\n"));
}

#[test]
fn authoring_sessions_build_a_typescript_recipe_with_its_metadata() {
    use superspec_engine::authoring::{Request, Session};
    let mut session = Session::default();
    let request = Request {
        entry: "main.material.ts".into(),
        source: CONCRETE_TS.into(),
        modules: BTreeMap::from([("common.material.ts".into(), COMMON_TS.into())]),
        resolution: 32,
        ..Request::default()
    };
    let output = session
        .build(
            &request,
            1,
            &|name| Err(format!("no asset {name}")),
            &|| false,
        )
        .unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(output.metadata["entry"], "main.material.ts");
    assert!(
        output.metadata["parameters"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["name"] == "chipSize")
    );
    // A broken edit reports every diagnostic and a corrected one builds again.
    let broken = Request {
        source: CONCRETE_TS.replace("seed(\"aggregate\")", "seed(1)"),
        ..request.clone()
    };
    let Err(diagnostics) = session.build(&broken, 2, &|_| Err(String::new()), &|| false) else {
        panic!("expected diagnostics");
    };
    assert_eq!(diagnostics[0].source_name, "main.material.ts");
    assert!(
        session
            .build(
                &request,
                3,
                &|name| Err(format!("no asset {name}")),
                &|| false
            )
            .is_ok()
    );
}
#[test]
fn typescript_painted_metal_matches_the_ms_program_including_boolean_selection() {
    let ts = program::compile(&recipe(
        "main.material.ts",
        &[
            ("main.material.ts", PAINTED_METAL_TS),
            ("common.material.ts", COMMON_TS),
        ],
    ))
    .unwrap_or_else(|e| panic!("{e:?}"));
    let ms = program::compile(&recipe(
        "main.ms",
        &[("main.ms", PAINTED_METAL), ("common.ms", COMMON_HELPERS)],
    ))
    .unwrap();
    for polished in [false, true] {
        let mut ts_inputs = Inputs::default();
        ts_inputs
            .parameters
            .insert("polished".into(), json!(polished));
        let mut ms_inputs = Inputs::default();
        ms_inputs
            .parameters
            .insert("polished".into(), json!(polished));
        for point in [[0.0, 0.0], [0.013, 0.07], [-0.4, 0.22], [1.7, -0.9]] {
            let a = ts.evaluate(&ts_inputs, &at(point)).unwrap();
            let b = ms.evaluate(&ms_inputs, &at(point)).unwrap();
            assert_eq!(a.base_color, b.base_color);
            assert_eq!(a.roughness, b.roughness, "polished={polished} at {point:?}");
            assert_eq!(a.metalness, b.metalness);
            assert_eq!(a.height, b.height);
        }
    }
}

#[test]
fn the_tiled_courtyard_example_exercises_groups_layers_and_live_operations() {
    let program = program::compile(&single(TILED_COURTYARD_TS)).unwrap_or_else(|e| panic!("{e:?}"));
    let metadata = program.metadata();
    for field in [
        "tiles.face",
        "tiles.joint",
        "tiles.chip",
        "tiles.substrate",
        "ground.stain",
        "streaks.coverage",
        "layers.grout.coverage",
        "layers.stain.roughness",
    ] {
        assert!(
            metadata.fields.iter().any(|f| f.name == field),
            "missing {field}"
        );
    }
    assert_eq!(metadata.live.len(), 1);
    let joint = program
        .evaluate(&Inputs::default(), &at([0.3, 0.07]))
        .unwrap();
    let face = program
        .evaluate(&Inputs::default(), &at([0.15, 0.07]))
        .unwrap();
    assert!(
        joint.base_color[0] < face.base_color[0],
        "grout is darker than the tile face"
    );
    assert!(joint.height < face.height, "joints sit below the tile face");
}

/// The acceptance-only Wasm crate calls this exact fixture and compares its bytes with native output.
pub fn typescript_program_result() -> Vec<u8> {
    use program::{BakeRequest, Domain, Edge};
    let mut results = Vec::new();
    let studies = [
        (
            "chipped-concrete",
            recipe(
                "main.material.ts",
                &[
                    ("main.material.ts", CONCRETE_TS),
                    ("common.material.ts", COMMON_TS),
                ],
            ),
            "chips",
        ),
        (
            "painted-metal",
            recipe(
                "main.material.ts",
                &[
                    ("main.material.ts", PAINTED_METAL_TS),
                    ("common.material.ts", COMMON_TS),
                ],
            ),
            "flakes",
        ),
        ("tiled-courtyard", single(TILED_COURTYARD_TS), "tiles.joint"),
    ];
    for (name, sources, field) in studies {
        let program = program::compile(&sources).unwrap_or_else(|e| panic!("{name}: {e:?}"));
        let inputs = Inputs {
            seed: 81,
            edges: vec![Edge {
                start: [3.9, 2.0, 0.5],
                end: [4.4, 2.0, 0.5],
            }],
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
                    inspection: Some(field.into()),
                },
                &inputs,
            )
            .unwrap();
        results.push(json!({"entry": name, "metadata": program.metadata(), "samples": samples, "bake": bake}));
    }
    // Structured edits are byte-for-byte deterministic across targets, including UTF-16 offsets.
    let text = format!("// 日本語\n{CONCRETE_TS}");
    let sources = recipe(
        "main.material.ts",
        &[
            ("main.material.ts", text.as_str()),
            ("common.material.ts", COMMON_TS),
        ],
    );
    let inspection = inspect_recipe(&sources, "main.material.ts").unwrap();
    let default = apply_edit(
        &sources,
        "main.material.ts",
        &EditOp::SetParameterDefault {
            name: "chipSize".into(),
            value: json!(0.12),
        },
    )
    .unwrap();
    let chips = inspection
        .fields
        .iter()
        .find(|f| f.name == "chips")
        .unwrap();
    let reconnect = apply_edit(
        &sources,
        "main.material.ts",
        &EditOp::Reconnect {
            start: chips.references[0].start,
            end: chips.references[0].end,
            expression: "aggregate".into(),
        },
    )
    .unwrap();
    results.push(json!({"inspection": inspection, "setDefault": default, "reconnect": reconnect}));
    serde_json::to_vec(&results).unwrap()
}

#[test]
fn portable_typescript_fixture_covers_the_same_native_and_wasm_front_end() {
    let bytes = typescript_program_result();
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let records = json.as_array().unwrap();
    assert_eq!(records.len(), 4);
    for record in &records[..3] {
        assert_eq!(record["samples"].as_array().unwrap().len(), 6);
        assert_eq!(record["bake"]["width"], 9);
    }
    assert!(
        records[2]["metadata"]["live"]
            .as_array()
            .is_some_and(|live| live.len() == 1)
    );
    assert_eq!(
        records[3]["setDefault"]["patches"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    // Offsets count the UTF-16 units of the multi-byte comment on the first line.
    let default_patch = &records[3]["setDefault"]["patches"][0];
    assert!(default_patch["start"].as_u64().unwrap() > "// 日本語\n".encode_utf16().count() as u64);
    assert_eq!(
        typescript_program_result(),
        bytes,
        "the fixture is deterministic"
    );
    if let Ok(path) = std::env::var("MATERIAL_TS_PARITY_OUTPUT") {
        std::fs::write(path, bytes).unwrap();
    }
}

#[test]
fn coordinate_space_annotations_compile_and_are_ignored() {
    // Spaces only type-check in TypeScript: generic helpers and spaced annotations compile to the
    // same program as their plain forms.
    let spaced = compile(&format!(
        r#"import {{ defineMaterial, mul, add, noise, seed, world, normal, vec3, type Vec3, type Space }} from "{SDK}";
const twice = <S extends Space>(p: Vec3<S>): Vec3<S> => mul(p, 2);
function lift<S extends Space, T extends Space>(p: Vec3<S>, d: Vec3<T>): Vec3<S> {{ return add(p, mul(d, 0.01)); }}
const at: Vec3<"world"> = lift(twice(world), normal);
export default defineMaterial({{ base: {{ baseColor: vec3(noise(at, seed("n"))) }} }});
"#
    ));
    let plain = compile(&format!(
        r#"import {{ defineMaterial, mul, add, noise, seed, world, normal, vec3, type Vec3 }} from "{SDK}";
const twice = (p: Vec3): Vec3 => mul(p, 2);
function lift(p: Vec3, d: Vec3): Vec3 {{ return add(p, mul(d, 0.01)); }}
const at: Vec3 = lift(twice(world), normal);
export default defineMaterial({{ base: {{ baseColor: vec3(noise(at, seed("n"))) }} }});
"#
    ));
    let sample = Sample {
        world: [0.3, 0.7, 0.2],
        normal: [0., 0., 1.],
        ..at([0.3, 0.7])
    };
    assert_eq!(
        evaluate_with(&spaced, &sample).base_color,
        evaluate_with(&plain, &sample).base_color
    );
    let unknown = errors(&format!(
        r#"import {{ defineMaterial, world, vec3, type Vec3 }} from "{SDK}";
const at: Vec3<"screen"> = world;
export default defineMaterial({{ base: {{ baseColor: vec3(0.5) }} }});
"#
    ));
    assert!(
        unknown[0].message.contains("coordinate space"),
        "{unknown:?}"
    );
    let bound = errors(&format!(
        r#"import {{ defineMaterial, mul, vec3, type Vec3 }} from "{SDK}";
const f = <S extends string>(p: Vec3<S>): Vec3<S> => mul(p, 2);
export default defineMaterial({{ base: {{ baseColor: f(vec3(0.5)) }} }});
"#
    ));
    assert!(bound[0].message.contains("extends Space"), "{bound:?}");
}

fn group_field(text: &str, sample: &Sample, name: &str) -> FieldValue {
    let program = compile(&format!(
        "import {{ defineMaterial, rectangularGrid, periodicStrips, directionalCoordinates, vec2, vec3, surface }} from \"{SDK}\";\n{text}\nexport default defineMaterial({{ base: {{ baseColor: vec3(0.5) }} }});\n"
    ));
    program.evaluate(&Inputs::default(), sample).unwrap().fields[name].clone()
}

fn near(value: FieldValue, expected: &[f64]) {
    let actual: Vec<f64> = match value {
        FieldValue::Scalar(v) => vec![v],
        FieldValue::Vec2(v) => v.to_vec(),
        FieldValue::Vec3(v) => v.to_vec(),
        FieldValue::Bool(v) => vec![f64::from(u8::from(v))],
    };
    assert_eq!(actual.len(), expected.len());
    for (a, e) in actual.iter().zip(expected) {
        assert!((a - e).abs() < 1e-9, "{actual:?} != {expected:?}");
    }
}

#[test]
fn rectangular_grids_floor_negative_cells_and_stagger_odd_rows() {
    let grid = "const g = rectangularGrid({ size: vec2(0.3, 0.3), rowOffset: 0.5 });";
    // Row 1 is odd, so x shifts by half a cell: -0.05 m lands 0.1 m into column 0.
    let odd = at([-0.05, 0.31]);
    near(group_field(grid, &odd, "g.cell"), &[0., 1.]);
    near(group_field(grid, &odd, "g.local"), &[0.1, 0.01]);
    near(group_field(grid, &odd, "g.sideDistance"), &[0.01]);
    near(
        group_field(grid, &odd, "g.cornerDistance"),
        &[(0.1f64.powi(2) + 0.01f64.powi(2)).sqrt()],
    );
    // Row 0 is not shifted; negative coordinates floor to column -1.
    let even = at([-0.05, 0.02]);
    near(group_field(grid, &even, "g.cell"), &[-1., 0.]);
    near(group_field(grid, &even, "g.local"), &[0.25, 0.02]);
    // The cell value is stable inside a cell and changes between cells.
    let a = group_field(grid, &at([0.05, 0.05]), "g.random");
    assert_eq!(a, group_field(grid, &at([0.25, 0.25]), "g.random"));
    assert_ne!(a, group_field(grid, &at([0.35, 0.05]), "g.random"));
}

#[test]
fn periodic_strips_integrate_joint_coverage_over_the_footprint() {
    let strips = "const s = periodicStrips({ position: surface.x, period: 1, gap: 0.1 });";
    let with = |x: f64, footprint: f64| Sample {
        footprint,
        ..at([x, 0.])
    };
    // Point samples: joints are centered on whole meters, 0.1 m wide.
    near(group_field(strips, &with(0.02, 0.), "s.joint"), &[1.]);
    near(group_field(strips, &with(-0.04, 0.), "s.joint"), &[1.]);
    near(group_field(strips, &with(0.5, 0.), "s.joint"), &[0.]);
    near(group_field(strips, &with(0.5, 0.), "s.solid"), &[1.]);
    // A 0.2 m footprint centered on a joint covers it entirely: half joint, half solid.
    near(group_field(strips, &with(0., 0.2), "s.joint"), &[0.5]);
    // A footprint spanning whole periods averages to the joint share anywhere.
    near(group_field(strips, &with(0.37, 2.), "s.joint"), &[0.1]);
}

#[test]
fn directional_coordinates_follow_a_direction_projected_onto_the_surface() {
    let coordinates =
        "const d = directionalCoordinates({ direction: vec3(1, 1, 0.5), scale: vec2(2, 0.5) });";
    let sample = Sample {
        world: [1., 0., 0.3],
        normal: [0., 0., 1.],
        ..at([1., 0.])
    };
    // The direction projects to (1, 1, 0) / sqrt(2); across is z x along = (-1, 1, 0) / sqrt(2).
    let half = 0.5f64.sqrt();
    near(group_field(coordinates, &sample, "d.along"), &[half / 2.]);
    near(
        group_field(coordinates, &sample, "d.across"),
        &[-half / 0.5],
    );
    near(
        group_field(coordinates, &sample, "d.coordinates"),
        &[half / 2., -half / 0.5],
    );
    // A direction along the normal has no surface direction: both coordinates are zero.
    let vertical = "const d = directionalCoordinates({ direction: vec3(0, 0, 1) });";
    near(group_field(vertical, &sample, "d.coordinates"), &[0., 0.]);
}

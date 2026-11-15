use std::collections::BTreeMap;
use superspec_engine::materialspec::program::{self, Sources};

fn sources(entry: &str, modules: &[(&str, &str)]) -> Sources {
    Sources {
        entry: entry.into(),
        modules: modules
            .iter()
            .map(|(path, text)| ((*path).into(), (*text).into()))
            .collect::<BTreeMap<_, _>>(),
    }
}

#[test]
fn parameter_annotations_survive_compilation_and_match_source_inspection() {
    let shared = r#"import { param } from "@superspec/material/v1";
export const chipWidth = param.scalar({ default: 0.08, range: [0.005, 0.5], unit: "m", label: "Chip width" });
"#;
    let entry = r#"import { defineMaterial, param, vec3, mul, select } from "@superspec/material/v1";
import { chipWidth } from "./shared.material.ts";
const tint = param.vec3({ default: vec3(0.2, 0.3, 0.4), unit: "linear RGB", label: "Surface tint" });
const enabled = param.bool({ default: true, label: "Enable finish" });
const roughness = param.scalar({ default: 0.6 });
export default defineMaterial({ base: { baseColor: mul(tint, select(enabled, chipWidth, 1)), roughness } });
"#;
    let source = sources(
        "main.material.ts",
        &[("main.material.ts", entry), ("shared.material.ts", shared)],
    );
    let program = program::compile(&source).unwrap();
    let metadata = serde_json::to_value(program.metadata()).unwrap();
    let parameters = metadata["parameters"].as_array().unwrap();
    let scale = parameters
        .iter()
        .find(|parameter| parameter["source"]["module"] == "shared.material.ts")
        .unwrap();
    assert_eq!(scale["name"], "chipWidth");
    assert_eq!(scale["unit"], "m");
    assert_eq!(scale["label"], "Chip width");
    assert_eq!(scale["default"], 0.08);
    assert_eq!(scale["min"], 0.005);
    assert_eq!(scale["max"], 0.5);
    assert_eq!(scale["source"]["location"]["line"], 2);
    let inspected = program::inspect_recipe(&source, "shared.material.ts").unwrap();
    assert_eq!(
        inspected.parameters[0].unit.as_deref(),
        scale["unit"].as_str()
    );
    assert_eq!(
        inspected.parameters[0].label.as_deref(),
        scale["label"].as_str()
    );
    let by_name = |name: &str| parameters.iter().find(|p| p["name"] == name).unwrap();
    assert_eq!(by_name("tint")["unit"], "linear RGB");
    assert_eq!(by_name("tint")["label"], "Surface tint");
    assert_eq!(
        by_name("tint")["default"],
        serde_json::json!([0.2, 0.3, 0.4])
    );
    assert_eq!(by_name("enabled")["label"], "Enable finish");
    assert!(by_name("enabled").get("unit").is_none());
    assert!(by_name("roughness").get("unit").is_none());
    assert!(by_name("roughness").get("label").is_none());
    program
        .evaluate(&Default::default(), &Default::default())
        .unwrap();
    let built = superspec_engine::authoring::Session::default()
        .build(
            &superspec_engine::authoring::Request {
                entry: source.entry.clone(),
                source: entry.into(),
                modules: source.modules.clone(),
                resolution: 16,
                ..Default::default()
            },
            1,
            &|_| panic!("this material has no source images"),
            &|| false,
        )
        .unwrap();
    assert_eq!(built.metadata["parameters"], metadata["parameters"]);
}

#[test]
fn legacy_parameters_omit_annotations_and_invalid_annotations_keep_source_diagnostics() {
    let source = sources(
        "main.ms",
        &[(
            "main.ms",
            "material 1; param roughness: scalar = 0.6; output base_color = vec3(0.5); output roughness = roughness;",
        )],
    );
    let program = program::compile(&source).unwrap();
    let parameter = serde_json::to_value(&program.metadata().parameters[0]).unwrap();
    assert!(parameter.get("unit").is_none());
    assert!(parameter.get("label").is_none());
    for property in ["unit", "label"] {
        let text = format!(
            "import {{ defineMaterial, param, vec3 }} from \"@superspec/material/v1\";\n\
             // 🙂 UTF-16 source marker\n\
             const size = param.scalar({{ default: 0.1, {property}: 42 }});\n\
             export default defineMaterial({{ base: {{ baseColor: vec3(size) }} }});"
        );
        let source = sources("main.material.ts", &[("main.material.ts", &text)]);
        let diagnostics = program::compile(&source).err().unwrap();
        let error = diagnostics
            .iter()
            .find(|error| error.code == "MT_PROPERTY")
            .unwrap();
        assert_eq!(error.source.module, "main.material.ts");
        assert_eq!(error.source.location.line, 3);
        assert_eq!(
            error.source.location.offset,
            text[..text.find("42").unwrap()].encode_utf16().count()
        );
        assert!(error.message.contains(property));
    }
}

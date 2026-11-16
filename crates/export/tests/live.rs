//! A painted material in a GLB (roadmap stage 5, plan step 1C): the baked PBR maps every glTF
//! importer shows, the shading model and the per-pixel program under `extras.materialSpec` for a
//! SuperSpec runtime, and the export report naming what an importer without them skips.
use serde_json::Value;
use superspec_engine::authoring::{Request, Session};
use superspec_export::{Options, instance_glb, live_report};

const PAINTED: &str = r#"import { defineMaterial, param, vec3, mix, shadeBand, sunDirection, normal } from "@superspec/material/v1";
const lit = param.vec3({ default: vec3(0.62, 0.55, 0.4), range: [0, 1] });
export default defineMaterial({
  shading: "painted",
  base: { baseColor: lit },
  paint: mix(vec3(0.18, 0.2, 0.32), lit, shadeBand(normal, sunDirection, 0.2, 1.5, 0.05)),
});
"#;

fn document(glb: &[u8]) -> Value {
    let length = u32::from_le_bytes(glb[12..16].try_into().unwrap()) as usize;
    serde_json::from_slice(&glb[20..20 + length]).unwrap()
}

#[test]
fn a_painted_material_exports_its_baked_fallback_with_its_program_in_the_extras() {
    let request = Request {
        entry: "painted.material.ts".into(),
        source: PAINTED.into(),
        resolution: 16,
        ..Request::default()
    };
    let built = Session::default()
        .build(&request, 1, &|name| Err(name.to_owned()), &|| false)
        .unwrap_or_else(|e| panic!("{e:?}"));
    let glb = instance_glb(&built.model, &Options::default()).unwrap();
    let doc = document(&glb);
    let material = &doc["materials"][0];
    // Any importer: the baked maps.
    assert!(material["pbrMetallicRoughness"]["baseColorTexture"].is_object());
    assert!(material["normalTexture"].is_object());
    // Nothing a plain importer must understand.
    assert!(doc.get("extensionsRequired").is_none_or(|required| {
        !required
            .as_array()
            .unwrap()
            .iter()
            .any(|name| name.as_str().unwrap().contains("materialSpec"))
    }));
    let spec = &material["extras"]["materialSpec"];
    assert_eq!(spec["version"], 1);
    assert_eq!(spec["shading"], "painted");
    let program = &spec["program"];
    assert_eq!(program["version"], 1);
    assert!(
        program["source"]
            .as_str()
            .unwrap()
            .contains("void ms_evaluate()")
    );
    assert!(program["painted"].as_str().unwrap().starts_with("ms_f"));
    let requires: Vec<&str> = program["requires"]
        .as_array()
        .unwrap()
        .iter()
        .map(|value| value.as_str().unwrap())
        .collect();
    assert!(
        requires.contains(&"sun") && requires.contains(&"shadow"),
        "{requires:?}"
    );
    assert!(
        program["uniforms"]
            .as_array()
            .unwrap()
            .iter()
            .any(|uniform| {
                uniform["type"] == "vec3"
                    && uniform["value"] == serde_json::json!([0.62, 0.55, 0.4])
            })
    );
    // The export report names what an importer without the payload skips.
    let materials: Vec<_> = built.model.materials.values().cloned().collect();
    let report = live_report(&materials);
    assert_eq!(report.len(), 1, "{report:?}");
    assert!(report[0].contains("painted shading"), "{report:?}");
    assert!(
        report[0].contains("per-pixel program (reads "),
        "{report:?}"
    );
    assert!(report[0].contains("shows the baked maps"), "{report:?}");
}

#[test]
fn a_lit_material_exports_and_reports_as_before() {
    let request = Request {
        entry: "lit.ms".into(),
        source: "material 1;\noutput base_color = vec3(0.5);".into(),
        resolution: 16,
        ..Request::default()
    };
    let built = Session::default()
        .build(&request, 1, &|name| Err(name.to_owned()), &|| false)
        .unwrap();
    let doc = document(&instance_glb(&built.model, &Options::default()).unwrap());
    assert!(doc["materials"][0].get("extras").is_none());
    let materials: Vec<_> = built.model.materials.values().cloned().collect();
    assert!(live_report(&materials).is_empty());
}

use base64::{Engine, engine::general_purpose::STANDARD};
use serde_json::{Value, json};
use std::{
    fs,
    path::Path,
    process::{Command, Output},
};

fn run(command: &str, source: &Path, options: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_superspec-studio"))
        .arg(command)
        .arg(source)
        .args(options)
        .env("PATH", "")
        .output()
        .unwrap()
}

fn succeeded(output: &Output) {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
}

/// The JSON document and binary chunk of a GLB.
fn glb_parts(bytes: &[u8]) -> (Value, &[u8]) {
    assert_eq!(&bytes[..4], b"glTF");
    let length = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
    (
        serde_json::from_slice(&bytes[20..20 + length]).unwrap(),
        &bytes[28 + length..],
    )
}

/// Every embedded image of a GLB, decoded, in image order.
fn glb_pixels(bytes: &[u8]) -> Vec<Vec<u8>> {
    let (document, binary) = glb_parts(bytes);
    document["images"]
        .as_array()
        .unwrap()
        .iter()
        .map(|image| {
            let view = &document["bufferViews"][image["bufferView"].as_u64().unwrap() as usize];
            let start = view["byteOffset"].as_u64().unwrap_or(0) as usize;
            let length = view["byteLength"].as_u64().unwrap() as usize;
            superspec_geometry::image::decode(&binary[start..start + length])
                .unwrap()
                .rgba
        })
        .collect()
}

fn manifest(packet: &[u8]) -> Value {
    assert_eq!(&packet[..4], b"LSP3");
    let length = u32::from_le_bytes(packet[4..8].try_into().unwrap()) as usize;
    serde_json::from_slice(&packet[8..8 + length]).unwrap()
}

#[test]
fn raw_modules_parameters_and_item_definitions_use_the_native_authoring_engine() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("main.ms");
    fs::write(&source, "material 1; import \"./common.ms\"; param tint: vec3 = vec3(0.2,0.3,0.4); output base_color = finish(tint); output roughness = 0.7;").unwrap();
    fs::write(
        temp.path().join("common.ms"),
        "material 1; fn finish(color: vec3) -> vec3 = color * 0.5;",
    )
    .unwrap();
    let parameters = temp.path().join("values.json");
    fs::write(&parameters, r#"{"tint":[0.8,0.2,0.1]}"#).unwrap();
    let first = run("scene", &source, &["--resolution", "16"]);
    succeeded(&first);
    let changed = run(
        "scene",
        &source,
        &[
            "--resolution",
            "16",
            "--parameters",
            parameters.to_str().unwrap(),
        ],
    );
    succeeded(&changed);
    assert_ne!(
        first.stdout, changed.stdout,
        "supplied parameters affect actual encoded material bytes"
    );
    let scene = manifest(&changed.stdout);
    assert!(scene["success"].as_bool().unwrap());
    assert!(!scene["assets"].as_array().unwrap().is_empty());
    let glb = run("glb", &source, &["--resolution", "16"]);
    succeeded(&glb);
    assert_eq!(&glb.stdout[..4], b"glTF");
    assert_eq!(
        u32::from_le_bytes(glb.stdout[8..12].try_into().unwrap()) as usize,
        glb.stdout.len()
    );
    let report = run("check", &source, &["--resolution", "16"]);
    succeeded(&report);
    let report: Value = serde_json::from_slice(&report.stdout).unwrap();
    assert_eq!(report["metadata"]["dependencies"], json!(["common.ms"]));

    let item = temp.path().join("pipe.item.json");
    fs::write(&item, json!({"format":"itemspec","version":1,"id":"pipe","parameters":{},
        "generator":{"kind":"pipe","length":2,"radius":0.1,"wallThickness":0.01,"segments":8,"capped":true},
        "slots":{"surface":{"entry":"main.ms","parameters":{},"seed":0}},"collision":"mesh"}).to_string()).unwrap();
    let report = run("check", &item, &["--resolution", "16"]);
    succeeded(&report);
    let report: Value = serde_json::from_slice(&report.stdout).unwrap();
    assert_eq!(report["metadata"]["collision"], "mesh");
    assert_eq!(
        report["metadata"]["attachments"].as_array().unwrap().len(),
        2
    );
    let scene = run("scene", &item, &["--resolution", "16"]);
    succeeded(&scene);
    assert!(
        !manifest(&scene.stdout)["colliders"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn portable_bundle_keeps_scene_bindings_and_embedded_inputs_after_relocation() {
    let temp = tempfile::tempdir().unwrap();
    let source =
        "material 1; output base_color = sample_color(\"finish.png\", uv); output roughness = 0.6;";
    let image = include_bytes!("../../../tests/fixtures/aml/asymmetric-heightmap.png");
    let mut project = json!({"schemaVersion":1,"kind":"material","entry":"main.ms","modules":{"main.ms":source},
        "parameters":{},"seed":7,"resolution":16,"inspection":"final","mapping":"object","domain":[1,1],
        "assets":{"finish.png":STANDARD.encode(image)},
        "base":{"kind":"aml","source":"<architecture version=\"1\" id=\"portable\"><box id=\"host\" width=\"1\" depth=\"0.2\" height=\"1\"/></architecture>"},
        "target":{"scope":"mesh","mesh":"host","enabled":true}});
    let file = temp.path().join("material.studio.json");
    fs::write(&file, project.to_string()).unwrap();
    let first = run("scene", &file, &[]);
    succeeded(&first);
    let scene = manifest(&first.stdout);
    assert_eq!(scene["id"], "portable");
    assert!(
        scene["meshes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|mesh| mesh["id"] == "host")
    );
    let relocated = tempfile::tempdir().unwrap();
    let moved = relocated.path().join("moved.studio.json");
    fs::copy(&file, &moved).unwrap();
    let again = run("scene", &moved, &[]);
    succeeded(&again);
    assert_eq!(first.stdout, again.stdout);
    let mut unversioned = project.clone();
    unversioned.as_object_mut().unwrap().remove("schemaVersion");
    fs::write(&moved, unversioned.to_string()).unwrap();
    let invalid = run("scene", &moved, &[]);
    assert_eq!(invalid.status.code(), Some(1));
    assert!(invalid.stdout.is_empty());
    project["assets"] = json!({});
    fs::write(&moved, project.to_string()).unwrap();
    fs::write(relocated.path().join("finish.png"), image).unwrap();
    let missing = run("scene", &moved, &[]);
    assert_eq!(missing.status.code(), Some(1));
    assert!(missing.stdout.is_empty());
    let errors: Value = serde_json::from_slice(&missing.stderr).unwrap();
    assert!(!errors["success"].as_bool().unwrap());
    assert!(String::from_utf8_lossy(&missing.stderr).contains("finish.png"));
}

#[test]
fn source_failure_has_locations_no_partial_output_and_corrected_source_recovers() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("main.ms");
    fs::write(&source, "material 1;\noutput base_color = missing;\n").unwrap();
    let failed = run("glb", &source, &[]);
    assert_eq!(failed.status.code(), Some(1));
    assert!(failed.stdout.is_empty());
    let error: Value = serde_json::from_slice(&failed.stderr).unwrap();
    assert_eq!(error["diagnostics"][0]["sourceName"], "main.ms");
    assert_eq!(error["diagnostics"][0]["line"], 2);
    fs::write(
        &source,
        "material 1; output base_color = vec3(0.4,0.5,0.6);",
    )
    .unwrap();
    succeeded(&run("scene", &source, &["--resolution", "16"]));
    for options in [
        vec!["--unknown"],
        vec!["--resolution", "0"],
        vec!["--seed", "-1"],
    ] {
        let invalid = run("scene", &source, &options);
        assert_eq!(invalid.status.code(), Some(2));
        assert!(invalid.stdout.is_empty());
    }
}

#[test]
fn the_production_profile_threads_bakes_without_changing_their_bytes() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("main.ms");
    fs::write(
        &source,
        "material 1; output base_color = vec3(fbm(surface / 0.05, seed(\"grain\"), 3)); output roughness = 0.6;",
    )
    .unwrap();
    let preview = run("glb", &source, &["--resolution", "64", "--textures", "png"]);
    succeeded(&preview);
    let production = run(
        "glb",
        &source,
        &[
            "--profile",
            "production",
            "--resolution",
            "64",
            "--textures",
            "png",
        ],
    );
    succeeded(&production);
    // Production compresses its PNGs harder: the files differ, the pixels do not.
    assert_eq!(glb_pixels(&preview.stdout), glb_pixels(&production.stdout));
    assert!(production.stdout.len() <= preview.stdout.len());
    // GLB exports store KTX2 textures unless asked for PNG.
    let compressed = run("glb", &source, &["--resolution", "64"]);
    succeeded(&compressed);
    let (document, _) = glb_parts(&compressed.stdout);
    assert_eq!(
        document["extensionsRequired"],
        json!(["KHR_texture_basisu"])
    );
    assert!(
        document["images"]
            .as_array()
            .unwrap()
            .iter()
            .all(|image| image["mimeType"] == "image/ktx2")
    );
    // Large textures need the production profile; unknown profiles are usage errors.
    let large = run("glb", &source, &["--resolution", "1024"]);
    assert!(!large.status.success());
    assert!(String::from_utf8_lossy(&large.stderr).contains("16–512"));
    for options in [
        vec!["--profile", "fast"],
        vec!["--profile", "production", "--resolution", "4096"],
    ] {
        let invalid = run("glb", &source, &options);
        assert_eq!(invalid.status.code(), Some(2));
    }
}

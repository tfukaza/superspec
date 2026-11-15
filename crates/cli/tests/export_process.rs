use serde_json::Value;
use std::process::Command;

fn document(bytes: &[u8]) -> Value {
    let number = |at| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap()) as usize;
    assert_eq!(&bytes[..4], b"glTF");
    assert_eq!(number(4), 2);
    assert_eq!(number(8), bytes.len());
    assert_eq!(&bytes[16..20], b"JSON");
    let json_end = 20 + number(12);
    assert_eq!(&bytes[json_end + 4..json_end + 8], b"BIN\0");
    assert_eq!(json_end + 8 + number(json_end), bytes.len());
    serde_json::from_slice(&bytes[20..json_end]).unwrap()
}

#[test]
fn native_glb_visual_and_collision_commands_need_no_node_and_publish_only_binary() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/aml/rust-architecture.aml");
    let source = std::fs::read_to_string(&path).unwrap();
    let scene = superspec_engine::compile(&source, 0).unwrap();
    for (purpose, no_uv) in [("visual", false), ("visual", true), ("collision", false)] {
        let mut command = Command::new(env!("CARGO_BIN_EXE_superspec"));
        command
            .arg("glb")
            .arg(&path)
            .env("PATH", "")
            .env_remove("NODE_OPTIONS")
            .env_remove("NODE_PATH");
        if purpose == "collision" {
            command.arg("--collision");
        }
        if no_uv {
            command.arg("--no-uv");
        }
        let output = command.output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(output.stderr.is_empty());
        let doc = document(&output.stdout);
        let metadata = &doc["extras"]["superspec"];
        assert_eq!(metadata["id"], "rust-architecture");
        assert_eq!(metadata["purpose"], purpose);
        assert_eq!(metadata["geometryValidation"], "checked");
        assert_eq!(metadata["navigationValidation"], "sampled");
        let expected: Vec<_> = scene
            .manifest
            .meshes
            .iter()
            .filter(|mesh| {
                if purpose == "visual" {
                    mesh.visible
                } else {
                    scene.manifest.colliders.contains(&mesh.id)
                }
            })
            .collect();
        let meshes = doc["meshes"].as_array().unwrap();
        assert_eq!(meshes.len(), expected.len());
        for (mesh, original) in meshes.iter().zip(expected) {
            assert_eq!(mesh["name"], original.id);
            let primitive = &mesh["primitives"][0];
            let accessor = primitive["indices"].as_u64().unwrap() as usize;
            assert_eq!(doc["accessors"][accessor]["count"], original.index_count);
            let attributes = primitive["attributes"].as_object().unwrap();
            assert!(attributes.contains_key("POSITION"));
            assert!(attributes.contains_key("NORMAL"));
            assert_eq!(attributes.contains_key("TEXCOORD_0"), purpose == "visual");
            assert_eq!(
                attributes.contains_key("TEXCOORD_1"),
                purpose == "visual" && !no_uv
            );
            if purpose == "collision" {
                assert!(primitive.get("material").is_none());
            }
        }
    }
}

#[test]
fn invalid_export_publishes_no_partial_bytes_and_next_source_succeeds() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("source.aml");
    std::fs::write(&path,"<architecture version=\"1\" id=\"invalid\"><sphere id=\"bad\" radius=\"-1\"/></architecture>").unwrap();
    let run = || {
        Command::new(env!("CARGO_BIN_EXE_superspec"))
            .arg("glb")
            .arg(&path)
            .env("PATH", "")
            .output()
            .unwrap()
    };
    let failed = run();
    assert_eq!(failed.status.code(), Some(1));
    assert!(failed.stdout.is_empty());
    assert!(!failed.stderr.is_empty());
    std::fs::write(&path,"<architecture version=\"1\" id=\"valid\"><box id=\"box\" width=\"1\" depth=\"1\" height=\"1\"/></architecture>").unwrap();
    let recovered = run();
    assert!(recovered.status.success());
    assert_eq!(&recovered.stdout[..4], b"glTF");
    for arguments in [["scene", "--collision"], ["glb", "--json"]] {
        let output = Command::new(env!("CARGO_BIN_EXE_superspec"))
            .args(arguments)
            .arg(&path)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2));
    }
}

#[test]
fn collision_command_does_not_require_visual_texture_files() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("scene.aml");
    std::fs::write(&path,r##"<architecture version="1" id="collision"><material id="finish" texture="missing.png"/><box id="host" material="finish"/></architecture>"##).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_superspec"))
        .arg("glb")
        .arg(&path)
        .arg("--collision")
        .env("PATH", "")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(&output.stdout[..4], b"glTF");
    let visual = Command::new(env!("CARGO_BIN_EXE_superspec"))
        .arg("glb")
        .arg(&path)
        .env("PATH", "")
        .output()
        .unwrap();
    assert_eq!(visual.status.code(), Some(1));
    assert!(visual.stdout.is_empty());
}

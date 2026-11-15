use std::process::Command;

#[test]
fn scene_compiles_without_node_and_matches_shared_core_bytes() {
    for fixture in ["rust-stage1", "rust-architecture"] {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join(format!("../../tests/fixtures/aml/{fixture}.aml"));
        let source = std::fs::read_to_string(&path).unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_superspec"))
            .arg("scene")
            .arg(path)
            .env("PATH", "")
            .env_remove("NODE_OPTIONS")
            .env_remove("NODE_PATH")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(output.stderr.is_empty());
        assert_eq!(output.stdout, superspec_engine::compile_packet(&source, 0));
    }
}

#[test]
fn unsupported_scene_has_one_diagnostic_packet_and_nonzero_exit() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("invalid.aml");
    let source = "<architecture version=\"1\" id=\"bad\"><sphere id=\"shape\" radius=\"-1\"/></architecture>";
    std::fs::write(&path, source).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_superspec"))
        .arg("scene")
        .arg(&path)
        .env("PATH", "")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(output.stdout, superspec_engine::compile_packet(source, 0));
    let recovered = Command::new(env!("CARGO_BIN_EXE_superspec"))
        .arg("scene")
        .arg(&path)
        .arg("--json")
        .output()
        .unwrap();
    assert_eq!(recovered.status.code(), Some(2));
}

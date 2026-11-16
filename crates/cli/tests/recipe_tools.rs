//! The command line exposes the same recipe tools the browser worker calls, for agents.
use serde_json::{Value, json};
use std::{
    fs,
    path::Path,
    process::{Command, Output},
};

const RECIPE: &str = r#"import { defineMaterial, layer, param, vec3, mix, fbm, seed, surface } from "@superspec/material/v1";

// Size of the pattern in meters; edits must keep this comment.
const size = param.scalar({ default: 0.1, range: [0.01, 1], unit: "m" });
const pattern = fbm(surface, seed("p"), 3);

export default defineMaterial({
  base: { baseColor: vec3(0.4, 0.4, 0.4), roughness: mix(0.9, 0.5, pattern) },
  layers: [layer({ id: "rim", mask: pattern, baseColor: vec3(0.1, 0.1, 0.1) })],
});
"#;

fn studio(directory: &Path, arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_superspec-studio"))
        .args(arguments)
        .current_dir(directory)
        .env("PATH", "")
        .env_remove("SUPERSPEC_TSC")
        .output()
        .unwrap()
}

fn json_of(bytes: &[u8]) -> Value {
    serde_json::from_slice(bytes)
        .unwrap_or_else(|error| panic!("not JSON ({error}): {}", String::from_utf8_lossy(bytes)))
}

fn project() -> (tempfile::TempDir, String) {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("main.material.ts");
    fs::write(&path, RECIPE).unwrap();
    let path = path.to_str().unwrap().to_owned();
    (temp, path)
}

#[test]
fn inspect_reports_the_recipe_structure_as_json() {
    let (temp, path) = project();
    let output = studio(temp.path(), &["inspect", &path]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report = json_of(&output.stdout);
    assert_eq!(report["schemaVersion"], 1);
    assert_eq!(report["success"], true);
    let inspection = &report["inspection"];
    assert_eq!(inspection["sdk"], "@superspec/material/v1");
    assert_eq!(inspection["parameters"][0]["name"], "size");
    assert_eq!(inspection["parameters"][0]["range"], json!([0.01, 1.0]));
    assert_eq!(inspection["layers"][0]["id"], "rim");
    let pattern = inspection["fields"]
        .as_array()
        .unwrap()
        .iter()
        .find(|field| field["name"] == "pattern")
        .unwrap();
    assert_eq!(pattern["references"].as_array().unwrap().len(), 2);
}

#[test]
fn edit_prints_patches_and_writes_only_when_asked() {
    let (temp, path) = project();
    let op = r#"{"op":"setParameterDefault","name":"size","value":0.25}"#;
    let preview = studio(temp.path(), &["edit", &path, "--op", op]);
    assert!(
        preview.status.success(),
        "{}",
        String::from_utf8_lossy(&preview.stderr)
    );
    let report = json_of(&preview.stdout);
    assert_eq!(report["patches"].as_array().unwrap().len(), 1);
    assert!(
        report["source"]
            .as_str()
            .unwrap()
            .contains("default: 0.25, range")
    );
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        RECIPE,
        "a dry run never writes"
    );

    let written = studio(temp.path(), &["edit", &path, "--op", op, "--write"]);
    assert!(written.status.success());
    assert!(json_of(&written.stdout)["written"].is_string());
    let after = fs::read_to_string(&path).unwrap();
    assert_eq!(after, RECIPE.replace("default: 0.1,", "default: 0.25,"));
    assert!(after.contains("// Size of the pattern in meters; edits must keep this comment."));
}

#[test]
fn edit_reads_its_operation_from_a_file_and_applies_layer_changes() {
    let (temp, path) = project();
    let op = temp.path().join("op.json");
    fs::write(
        &op,
        r#"{"op":"addLayer","source":"layer({ id: \"dust\", mask: 0.3, roughness: 0.9 })"}"#,
    )
    .unwrap();
    let output = studio(
        temp.path(),
        &[
            "edit",
            &path,
            "--op",
            &format!("@{}", op.display()),
            "--write",
        ],
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let inspect = studio(temp.path(), &["inspect", &path]);
    let layers: Vec<_> = json_of(&inspect.stdout)["inspection"]["layers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|layer| layer["id"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(layers, ["rim", "dust"]);
}

#[test]
fn edit_publication_preserves_an_existing_temporary_sibling() {
    let (temp, path) = project();
    let occupied = Path::new(&path).with_extension("ts.superspec-edit");
    let sentinel = "An independently authored file, not this edit's temporary output.";
    fs::write(&occupied, sentinel).unwrap();
    let op = r#"{"op":"setParameterDefault","name":"size","value":0.25}"#;
    let edited = studio(temp.path(), &["edit", &path, "--op", op, "--write"]);
    assert!(
        edited.status.success(),
        "{}",
        String::from_utf8_lossy(&edited.stderr)
    );
    assert_eq!(fs::read_to_string(&occupied).unwrap(), sentinel);
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        RECIPE.replace("default: 0.1,", "default: 0.25,")
    );

    let rejected = studio(
        temp.path(),
        &[
            "edit",
            &path,
            "--op",
            r#"{"op":"setParameterDefault","name":"missing","value":0.2}"#,
            "--write",
        ],
    );
    assert!(!rejected.status.success());
    assert_eq!(fs::read_to_string(&occupied).unwrap(), sentinel);
    let restored = studio(
        temp.path(),
        &[
            "edit",
            &path,
            "--op",
            r#"{"op":"setParameterDefault","name":"size","value":0.1}"#,
            "--write",
        ],
    );
    assert!(restored.status.success());
    assert_eq!(fs::read_to_string(&path).unwrap(), RECIPE);
    assert_eq!(fs::read_to_string(&occupied).unwrap(), sentinel);
    assert_eq!(
        fs::read_dir(temp.path()).unwrap().count(),
        2,
        "completed and rejected edits leave no owned temporary files"
    );
}

#[test]
fn a_rejected_edit_exits_nonzero_with_diagnostics_and_leaves_the_file_alone() {
    let (temp, path) = project();
    // A color cannot drive the scalar interpolation weight.
    let inspected = json_of(&studio(temp.path(), &["inspect", &path]).stdout);
    let first = inspected["inspection"]["fields"]
        .as_array()
        .unwrap()
        .iter()
        .find(|field| field["name"] == "pattern")
        .unwrap()["references"][0]
        .clone();
    let op = json!({
        "op": "reconnect", "start": first["start"], "end": first["end"],
        "expression": "vec3(1, 1, 1)",
    })
    .to_string();
    let output = studio(temp.path(), &["edit", &path, "--op", &op, "--write"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    let report = json_of(&output.stderr);
    assert_eq!(report["success"], false);
    assert_eq!(report["diagnostics"][0]["code"], "MS_TYPE");
    assert_eq!(fs::read_to_string(&path).unwrap(), RECIPE);

    let malformed = studio(temp.path(), &["edit", &path, "--op", "{not json"]);
    assert_eq!(malformed.status.code(), Some(1));
    assert!(
        json_of(&malformed.stderr)["diagnostics"][0]["message"]
            .as_str()
            .unwrap()
            .contains("--op")
    );
}

#[test]
fn sdk_and_catalog_commands_publish_the_generated_contract() {
    let temp = tempfile::tempdir().unwrap();
    let out = temp.path().join("sdk");
    let output = studio(temp.path(), &["sdk", "--out", out.to_str().unwrap()]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    for name in [
        "index.d.ts",
        "ports.json",
        "ai-reference.md",
        "tsconfig.json",
    ] {
        assert!(out.join(name).is_file(), "{name}");
    }
    let declarations = fs::read_to_string(out.join("index.d.ts")).unwrap();
    assert!(declarations.contains("export declare function fbm<"));
    assert!(declarations.contains("export declare function tileAndGrout("));

    let catalog = json_of(&studio(temp.path(), &["catalog"]).stdout);
    assert_eq!(
        catalog["catalog"]["operations"].as_array().unwrap().len(),
        52
    );
    assert_eq!(catalog["catalog"]["module"], "@superspec/material/v1");
}

#[test]
fn scene_and_check_build_a_typescript_recipe_and_validate_parameter_overrides() {
    let (temp, path) = project();
    let first = studio(temp.path(), &["scene", &path, "--resolution", "16"]);
    assert!(
        first.status.success(),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );
    assert_eq!(&first.stdout[..4], b"LSP3");
    let values = temp.path().join("values.json");
    fs::write(&values, r#"{"size":0.5}"#).unwrap();
    // `size` is declared but unused by the outputs, so overriding it is accepted and valid.
    let checked = studio(
        temp.path(),
        &["check", &path, "--parameters", values.to_str().unwrap()],
    );
    assert!(
        checked.status.success(),
        "{}",
        String::from_utf8_lossy(&checked.stderr)
    );
    assert_eq!(json_of(&checked.stdout)["success"], true);
    // Overrides are validated against the recipe's declared range.
    fs::write(&values, r#"{"size":50}"#).unwrap();
    let rejected = studio(
        temp.path(),
        &["check", &path, "--parameters", values.to_str().unwrap()],
    );
    assert_eq!(rejected.status.code(), Some(1));
}

#[test]
fn check_lists_every_diagnostic_with_its_module_and_position() {
    let (temp, path) = project();
    fs::write(
        &path,
        RECIPE
            .replace("mix(0.9, 0.5, pattern)", "mix(0.9, 0.5)")
            .replace("vec3(0.1, 0.1, 0.1)", "0.1"),
    )
    .unwrap();
    let output = studio(temp.path(), &["check", &path]);
    assert_eq!(output.status.code(), Some(1));
    let report = json_of(&output.stderr);
    let diagnostics = report["diagnostics"].as_array().unwrap();
    assert!(diagnostics.len() >= 2, "{report}");
    assert!(
        diagnostics
            .iter()
            .all(|d| d["sourceName"] == "main.material.ts")
    );
    assert!(
        diagnostics
            .iter()
            .all(|d| d["line"].as_u64().unwrap() >= 1 && d["offset"].is_u64())
    );
}

#[cfg(unix)]
#[test]
fn check_types_merges_strict_typescript_findings_from_the_configured_compiler() {
    use std::os::unix::fs::PermissionsExt;
    let (temp, path) = project();
    let parent = temp.path().canonicalize().unwrap();
    // A stand-in for tsc that prints findings in its real `--pretty false` format.
    let script = temp.path().join("fake-tsc.sh");
    fs::write(
        &script,
        format!(
            "#!/bin/sh\necho \"{}/main.material.ts(5,15): error TS2322: Type 'Vec3' is not assignable to type 'Scalar'.\"\nexit 2\n",
            parent.display()
        ),
    )
    .unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
    let sdk = temp.path().join("sdk");
    let output = Command::new(env!("CARGO_BIN_EXE_superspec-studio"))
        .args(["check", &path, "--types", "--sdk", sdk.to_str().unwrap()])
        .current_dir(temp.path())
        .env("PATH", "")
        .env("SUPERSPEC_TSC", &script)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    let report = json_of(&output.stderr);
    let finding = &report["diagnostics"][0];
    assert_eq!(finding["code"], "TS2322");
    assert_eq!(finding["sourceName"], "main.material.ts");
    assert_eq!(
        (finding["line"].as_u64(), finding["column"].as_u64()),
        (Some(5), Some(15))
    );
    let expected: usize = RECIPE
        .split_inclusive('\n')
        .take(4)
        .map(|line| line.len())
        .sum::<usize>()
        + 14;
    assert_eq!(finding["offset"].as_u64(), Some(expected as u64));
    // The reusable SDK is published; each checker owns a separate temporary config.
    assert!(sdk.join("index.d.ts").is_file());
    assert!(!sdk.join("recipes.tsconfig.json").exists());

    // Global errors have no file location, and subprocess failures may only use stderr.
    for (body, code, message) in [
        (
            "echo 'error TS6053: File missing.d.ts not found.'\nexit 2\n",
            "TS6053",
            "missing.d.ts",
        ),
        (
            "echo 'compiler startup failed' >&2\nexit 7\n",
            "TSC_PROCESS",
            "compiler startup failed",
        ),
        ("exit 9\n", "TSC_PROCESS", "9"),
        (
            "echo 'error TS6053: File stderr.d.ts not found.' >&2\nexit 2\n",
            "TS6053",
            "stderr.d.ts",
        ),
    ] {
        fs::write(&script, format!("#!/bin/sh\n{body}")).unwrap();
        let failed = Command::new(env!("CARGO_BIN_EXE_superspec-studio"))
            .args([
                "check",
                &path,
                "--resolution",
                "16",
                "--types",
                "--sdk",
                sdk.to_str().unwrap(),
            ])
            .current_dir(temp.path())
            .env("PATH", "")
            .env("SUPERSPEC_TSC", &script)
            .output()
            .unwrap();
        assert_eq!(failed.status.code(), Some(1), "{body}");
        assert!(failed.stdout.is_empty());
        let report = json_of(&failed.stderr);
        assert!(
            report["diagnostics"]
                .as_array()
                .unwrap()
                .iter()
                .any(|diagnostic| diagnostic["code"] == code
                    && diagnostic["message"].as_str().unwrap().contains(message)),
            "{report}"
        );
    }

    // A corrected compiler invocation succeeds after each failed attempt.
    fs::write(&script, "#!/bin/sh\nexit 0\n").unwrap();
    let clean = Command::new(env!("CARGO_BIN_EXE_superspec-studio"))
        .args(["check", &path, "--types", "--sdk", sdk.to_str().unwrap()])
        .current_dir(temp.path())
        .env("PATH", "")
        .env("SUPERSPEC_TSC", &script)
        .output()
        .unwrap();
    assert!(
        clean.status.success(),
        "{}",
        String::from_utf8_lossy(&clean.stderr)
    );
}

#[cfg(unix)]
#[test]
fn concurrent_type_checks_use_separate_source_and_sdk_snapshots() {
    use std::{
        os::unix::fs::PermissionsExt,
        process::Stdio,
        thread,
        time::{Duration, Instant},
    };
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let sdk = root.join("sdk");
    fs::create_dir(&sdk).unwrap();
    let authored_config = sdk.join("recipes.tsconfig.json");
    fs::write(&authored_config, "independently authored configuration").unwrap();
    let gate = root.join("release-checkers");
    let checker = root.join("checker.sh");
    fs::write(&checker, "#!/bin/sh\nprintf '%s' \"$2\" > \"$RECORD\"\ni=0\nwhile [ ! -f \"$GATE\" ]; do\n i=$((i + 1))\n if [ \"$i\" -gt 2000 ]; then exit 7; fi\n /bin/sleep 0.01\ndone\nexit 0\n").unwrap();
    fs::set_permissions(&checker, fs::Permissions::from_mode(0o755)).unwrap();
    let mut children = Vec::new();
    for name in ["first", "second"] {
        let folder = root.join(name);
        fs::create_dir(&folder).unwrap();
        let source = folder.join("main.material.ts");
        let text = format!("// {name}\n{RECIPE}");
        fs::write(&source, &text).unwrap();
        let record = root.join(format!("{name}.config-path"));
        let child = Command::new(env!("CARGO_BIN_EXE_superspec-studio"))
            .args([
                "check",
                source.to_str().unwrap(),
                "--resolution",
                "16",
                "--types",
                "--sdk",
                sdk.to_str().unwrap(),
            ])
            .env("SUPERSPEC_TSC", &checker)
            .env("RECORD", &record)
            .env("GATE", &gate)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        children.push((child, record, source, text));
    }
    let deadline = Instant::now() + Duration::from_secs(25);
    while children
        .iter()
        .any(|(_, record, _, _)| fs::read_to_string(record).unwrap_or_default().is_empty())
    {
        if Instant::now() >= deadline {
            fs::write(&gate, "release").unwrap();
            for (child, _, _, _) in &mut children {
                let _ = child.kill();
                let _ = child.wait();
            }
            panic!("Both checker processes must reach the controlled barrier");
        }
        thread::sleep(Duration::from_millis(10));
    }
    let mut snapshots = Vec::new();
    for (_, record, original, expected) in &children {
        let config = std::path::PathBuf::from(fs::read_to_string(record).unwrap());
        let contents = json_of(&fs::read(&config).unwrap());
        let files = contents["files"].as_array().unwrap();
        let source = std::path::PathBuf::from(
            files
                .iter()
                .find_map(|file| {
                    file.as_str()
                        .filter(|path| path.ends_with("main.material.ts"))
                })
                .unwrap(),
        );
        assert_ne!(&source, original);
        assert_eq!(fs::read_to_string(&source).unwrap(), *expected);
        assert!(
            fs::read_to_string(config.parent().unwrap().join("index.d.ts"))
                .unwrap()
                .contains("export declare function fbm<")
        );
        snapshots.push((config, source));
    }
    assert_ne!(snapshots[0].0, snapshots[1].0);
    // A concurrent publisher/editor cannot change either already-admitted checker input.
    fs::write(sdk.join("index.d.ts"), "changed shared SDK").unwrap();
    for (_, _, original, _) in &children {
        fs::write(original, "changed source").unwrap();
    }
    for ((config, source), (_, _, _, expected)) in snapshots.iter().zip(&children) {
        assert_eq!(fs::read_to_string(source).unwrap(), *expected);
        assert!(
            fs::read_to_string(config.parent().unwrap().join("index.d.ts"))
                .unwrap()
                .contains("export declare function fbm<")
        );
    }
    fs::write(&gate, "release").unwrap();
    for (child, _, _, _) in children {
        let output = child.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    for (config, source) in snapshots {
        assert!(!config.exists());
        assert!(!source.exists());
    }
    assert_eq!(
        fs::read_to_string(authored_config).unwrap(),
        "independently authored configuration"
    );
}

#[test]
fn arguments_are_validated() {
    let temp = tempfile::tempdir().unwrap();
    for arguments in [
        vec!["edit", "x.material.ts"],
        vec!["sdk"],
        vec!["inspect"],
        vec!["inspect", "x.material.ts", "--write"],
        vec!["catalog", "--bogus"],
    ] {
        assert_eq!(
            studio(temp.path(), &arguments).status.code(),
            Some(2),
            "{arguments:?}"
        );
    }
    assert_eq!(
        studio(temp.path(), &["inspect", "missing.material.ts"])
            .status
            .code(),
        Some(1)
    );
}

#[test]
fn scene_bundles_with_several_models_and_materials_build_and_export() {
    let temp = tempfile::tempdir().unwrap();
    let aml = |id: &str, x: f64| {
        format!(
            r##"<architecture version="1" id="{id}"><material id="plain" color="#808080"/><box id="panel" position="{x} 0 0" width="0.6" depth="0.1" height="0.5" material="plain"/></architecture>"##
        )
    };
    let second = RECIPE.replace("vec3(0.4, 0.4, 0.4)", "vec3(0.6, 0.2, 0.1)");
    let bundle = json!({
        "schemaVersion": 2, "kind": "material", "entry": "a.material.ts",
        "modules": { "a.material.ts": RECIPE, "b.material.ts": second },
        "parameters": {}, "seed": 3, "resolution": 32, "inspection": "final", "mapping": "world",
        "domain": [1, 1], "edges": [], "gravity": [0, 0, -1], "assets": {},
        "base": { "kind": "swatch" }, "target": { "scope": "mesh", "enabled": false },
        "models": [
            { "id": "left", "kind": "aml", "source": aml("left", 0.) },
            { "id": "right", "kind": "aml", "source": aml("right", 1.) },
        ],
        "materials": [
            { "id": "grey", "entry": "a.material.ts", "parameters": { "size": 0.2 }, "seed": 3, "meshes": ["left/panel"] },
            { "id": "red", "entry": "b.material.ts", "parameters": {}, "seed": 3, "meshes": ["right/panel"] },
        ],
    });
    let path = temp.path().join("scene.studio.json");
    fs::write(&path, serde_json::to_string(&bundle).unwrap()).unwrap();
    let path = path.to_str().unwrap();
    let scene = studio(temp.path(), &["scene", path]);
    assert!(
        scene.status.success(),
        "{}",
        String::from_utf8_lossy(&scene.stderr)
    );
    assert!(!scene.stdout.is_empty());
    let glb = studio(temp.path(), &["glb", path]);
    assert!(
        glb.status.success(),
        "{}",
        String::from_utf8_lossy(&glb.stderr)
    );
    assert_eq!(&glb.stdout[..4], b"glTF");
    let checked = studio(temp.path(), &["check", path]);
    assert!(
        checked.status.success(),
        "{}",
        String::from_utf8_lossy(&checked.stderr)
    );
    let report = json_of(&checked.stdout);
    assert_eq!(report["success"], true);
    // A mesh named by two materials is reported, not silently resolved.
    let mut twice = bundle.clone();
    twice["materials"][1]["meshes"] = json!(["left/panel"]);
    fs::write(
        temp.path().join("twice.studio.json"),
        serde_json::to_string(&twice).unwrap(),
    )
    .unwrap();
    let rejected = studio(
        temp.path(),
        &[
            "check",
            temp.path().join("twice.studio.json").to_str().unwrap(),
        ],
    );
    assert!(!rejected.status.success());
    assert!(String::from_utf8_lossy(&rejected.stderr).contains("more than one material"));
}

#[test]
fn live_operations_export_as_material_extras_with_texture_arguments() {
    let (temp, _) = project();
    let live = RECIPE
        .replace(
            "import { defineMaterial,",
            "import { modulateDiffuse, defineMaterial,",
        )
        .replace(
            "  layers: [",
            "  live: [modulateDiffuse({ factor: 0.9, mask: pattern })],\n  layers: [",
        );
    let path = temp.path().join("live.material.ts");
    fs::write(&path, live).unwrap();
    let glb = studio(
        temp.path(),
        &["glb", path.to_str().unwrap(), "--resolution", "32"],
    );
    assert!(
        glb.status.success(),
        "{}",
        String::from_utf8_lossy(&glb.stderr)
    );
    // The JSON chunk follows the 12-byte header and its 8-byte chunk header.
    let length = u32::from_le_bytes(glb.stdout[12..16].try_into().unwrap()) as usize;
    let document: Value = serde_json::from_slice(&glb.stdout[20..20 + length]).unwrap();
    let material = document["materials"]
        .as_array()
        .unwrap()
        .iter()
        .find(|material| material["extras"]["materialSpec"].is_object())
        .expect("a material with live operations");
    let operation = &material["extras"]["materialSpec"]["live"][0];
    assert_eq!(material["extras"]["materialSpec"]["version"], 1);
    assert_eq!(operation["id"], "modulateDiffuse");
    assert_eq!(operation["stage"], "lighting-composition");
    assert_eq!(operation["arguments"]["factor"]["value"], 0.9);
    let mask = &operation["arguments"]["mask"];
    let texture = mask["texture"]
        .as_u64()
        .expect("the varying mask is a texture") as usize;
    assert!(texture < document["textures"].as_array().unwrap().len());
    assert!(mask["channel"].as_u64().unwrap() < 4);
    assert!(mask["scale"].as_f64().unwrap() > 0.);
}

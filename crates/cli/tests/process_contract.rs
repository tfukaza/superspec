use serde_json::Value;
use std::{
    fs,
    path::Path,
    process::{Command, Output},
};

fn run(directory: &Path, arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_superspec"))
        .args(arguments)
        .current_dir(directory)
        .env("PATH", "")
        .env_remove("NODE_PATH")
        .env_remove("NODE_OPTIONS")
        .output()
        .unwrap()
}

fn json(output: &Output, code: i32) -> Value {
    assert_eq!(
        output.status.code(),
        Some(code),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout.last(), Some(&b'\n'));
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["schemaVersion"], 1);
    value
}

#[test]
fn native_init_and_inspection_run_outside_the_checkout_with_no_node_path() {
    let directory = tempfile::tempdir().unwrap();
    let created = json(
        &run(directory.path(), &["init", "project 窗 😀", "--json"]),
        0,
    );
    assert_eq!(created["success"], true);
    assert_eq!(created["artifacts"].as_array().unwrap().len(), 5);
    let project = directory.path().join("project 窗 😀");
    assert_eq!(
        fs::read_to_string(project.join("level.aml")).unwrap(),
        include_str!("../assets/starter.aml")
    );
    let brief = fs::read_to_string(project.join("BRIEF.md")).unwrap();
    assert!(brief.contains("superspec inspect"));
    let inspected = json(&run(&project, &["inspect", "level.aml", "--json"]), 0);
    assert_eq!(inspected["inspection"]["id"], "architectural-project");
    assert_eq!(inspected["inspection"]["navigationValidation"], "not-run");
    assert_eq!(inspected["artifacts"], serde_json::json!([]));
}

#[test]
fn init_never_replaces_existing_files_or_directories() {
    let directory = tempfile::tempdir().unwrap();
    fs::create_dir(directory.path().join("empty")).unwrap();
    fs::create_dir(directory.path().join("owned")).unwrap();
    fs::write(directory.path().join("owned/note"), "keep").unwrap();
    fs::write(directory.path().join("file"), "keep file").unwrap();
    for name in ["empty", "owned", "file"] {
        let result = json(&run(directory.path(), &["init", name, "--json"]), 1);
        assert_eq!(result["error"]["stage"], "initialize");
    }
    assert!(
        fs::read_dir(directory.path().join("empty"))
            .unwrap()
            .next()
            .is_none()
    );
    assert_eq!(
        fs::read_to_string(directory.path().join("owned/note")).unwrap(),
        "keep"
    );
    assert_eq!(
        fs::read_to_string(directory.path().join("file")).unwrap(),
        "keep file"
    );
    json(&run(directory.path(), &["init", "new", "--json"]), 0);
}

#[test]
fn simultaneous_initialization_has_exactly_one_winner() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path();
    let outputs = std::thread::scope(|scope| {
        let first = scope.spawn(|| run(path, &["init", "shared", "--json"]));
        let second = scope.spawn(|| run(path, &["init", "shared", "--json"]));
        [first.join().unwrap(), second.join().unwrap()]
    });
    assert_eq!(
        outputs
            .iter()
            .filter(|output| output.status.success())
            .count(),
        1
    );
    for output in &outputs {
        json(output, if output.status.success() { 0 } else { 1 });
    }
    assert_eq!(
        fs::read_to_string(path.join("shared/level.aml")).unwrap(),
        include_str!("../assets/starter.aml")
    );
}

#[test]
fn usage_load_and_parse_failures_are_machine_readable_without_false_validation() {
    let directory = tempfile::tempdir().unwrap();
    for args in [
        vec!["unknown", "--json"],
        vec!["inspect", "--json"],
        vec!["inspect", "a", "--bad", "--json"],
    ] {
        assert_eq!(
            json(&run(directory.path(), &args), 2)["error"]["stage"],
            "arguments"
        );
    }
    assert_eq!(
        json(
            &run(directory.path(), &["inspect", "missing.aml", "--json"]),
            1
        )["error"]["stage"],
        "load"
    );
    fs::write(directory.path().join("bad.aml"), [0xff, 0xfe]).unwrap();
    assert_eq!(
        json(&run(directory.path(), &["inspect", "bad.aml", "--json"]), 1)["error"]["stage"],
        "load"
    );
    fs::write(
        directory.path().join("bad.aml"),
        "<architecture version='1' id='a'>",
    )
    .unwrap();
    let malformed = json(&run(directory.path(), &["inspect", "bad.aml", "--json"]), 1);
    assert_eq!(malformed["error"]["stage"], "parse");
    assert_eq!(malformed["diagnostics"][0]["code"], "XML_SYNTAX");
    let migration = json(
        &run(
            directory.path(),
            &["migrate", "bad.aml", "--out", "converted", "--json"],
        ),
        1,
    );
    assert_eq!(migration["success"], false);
    assert_eq!(migration["error"]["stage"], "workflow");
    assert!(!directory.path().join("converted").exists());
    let missing_output = json(&run(directory.path(), &["build", "bad.aml", "--json"]), 2);
    assert_eq!(missing_output["error"]["stage"], "arguments");
    let plain = run(directory.path(), &["inspect", "missing.aml"]);
    assert!(plain.stdout.is_empty());
    assert!(String::from_utf8_lossy(&plain.stderr).contains("load:"));
}

#[cfg(unix)]
#[test]
fn init_preserves_destination_symlinks() {
    let directory = tempfile::tempdir().unwrap();
    fs::create_dir(directory.path().join("target")).unwrap();
    std::os::unix::fs::symlink(
        directory.path().join("target"),
        directory.path().join("link"),
    )
    .unwrap();
    json(&run(directory.path(), &["init", "link", "--json"]), 1);
    assert!(
        fs::symlink_metadata(directory.path().join("link"))
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert!(
        fs::read_dir(directory.path().join("target"))
            .unwrap()
            .next()
            .is_none()
    );
}

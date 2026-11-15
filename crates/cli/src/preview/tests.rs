use super::watch::{Published, Watcher};
use std::{fs, path::Path, sync::Mutex};

const BOX: &str =
    r#"<architecture version="1" id="watch"><box id="box" width="2"/></architecture>"#;
fn compile(
    source: &str,
    revision: u32,
    assets: &dyn Fn(&str) -> Result<Vec<u8>, String>,
) -> Result<superspec_engine::Scene, superspec_aml::diagnostic::Diagnostic> {
    superspec_engine::compile_with_assets(source, revision, assets)
}
fn texture(name: &str) -> String {
    format!(
        r#"<architecture version="1" id="watch"><material id="paint" texture="{name}"/><box id="box" material="paint"/></architecture>"#
    )
}
fn png() -> Vec<u8> {
    fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/aml/asymmetric-heightmap.png"),
    )
    .unwrap()
}

#[test]
fn content_watch_replaces_dependencies_and_recovers_missing_assets_without_source_edit() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("source.aml");
    let x = temp.path().join("x.png");
    let y = temp.path().join("y.png");
    fs::write(&path, texture("x.png")).unwrap();
    fs::write(&x, png()).unwrap();
    let state = Mutex::new(Published::default());
    let mut watcher = Watcher::new(path.clone());
    watcher.refresh(&state, compile);
    assert_eq!(state.lock().unwrap().status.scene_revision, Some(1));
    fs::write(&path, texture("y.png")).unwrap();
    watcher.refresh(&state, compile);
    assert_eq!(state.lock().unwrap().status.state, "error");
    assert_eq!(state.lock().unwrap().status.scene_revision, Some(1));
    let mut pixels = png();
    pixels.push(1);
    fs::write(&y, &pixels).unwrap();
    watcher.refresh(&state, compile);
    assert_eq!(state.lock().unwrap().status.scene_revision, Some(3));
    fs::write(&x, b"old dependency no longer read").unwrap();
    fs::write(temp.path().join("unrelated.txt"), b"ignored").unwrap();
    watcher.refresh(&state, |_, _, _| {
        panic!("Unrelated file must not recompile")
    });
    let modified = fs::metadata(&y).unwrap().modified().unwrap();
    *pixels.last_mut().unwrap() = 2;
    fs::write(&y, &pixels).unwrap();
    fs::File::options()
        .write(true)
        .open(&y)
        .unwrap()
        .set_times(fs::FileTimes::new().set_modified(modified))
        .unwrap();
    watcher.refresh(&state, compile);
    assert_eq!(state.lock().unwrap().status.scene_revision, Some(4));
    let replacement = temp.path().join("replacement.png");
    fs::write(&replacement, png()).unwrap();
    fs::remove_file(&y).unwrap();
    fs::rename(replacement, &y).unwrap();
    watcher.refresh(&state, compile);
    assert_eq!(state.lock().unwrap().status.scene_revision, Some(5));
    fs::remove_file(&y).unwrap();
    watcher.refresh(&state, compile);
    assert_eq!(state.lock().unwrap().status.state, "error");
    assert_eq!(state.lock().unwrap().status.scene_revision, Some(5));
}

#[test]
fn obsolete_success_and_error_never_replace_the_last_valid_packet_or_diagnostics() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("source.aml");
    fs::write(&path, BOX).unwrap();
    let state = Mutex::new(Published::default());
    let mut watcher = Watcher::new(path.clone());
    watcher.refresh(&state, compile);
    let good = state.lock().unwrap().packet.clone().unwrap();
    for stale in [BOX.replace('2', "3"), "<architecture".into()] {
        fs::write(&path, stale).unwrap();
        watcher.refresh(&state, |source, revision, assets| {
            let result = compile(source, revision, assets);
            // Changes occur after the compile snapshot and before its result is allowed to publish.
            fs::write(&path, BOX).unwrap();
            result
        });
        let result = state.lock().unwrap();
        assert_eq!(result.status.state, "compiling");
        assert!(result.status.diagnostics.is_empty());
        assert_eq!(result.status.scene_revision, Some(1));
        assert_eq!(result.packet.as_ref().unwrap().as_slice(), good.as_slice());
    }
    watcher.refresh(&state, compile);
    assert_eq!(state.lock().unwrap().status.scene_revision, Some(4));
    fs::write(&path, texture("missing.png")).unwrap();
    watcher.refresh(&state, |source, revision, assets| {
        let result = compile(source, revision, assets);
        fs::write(temp.path().join("missing.png"), png()).unwrap();
        result
    });
    assert_eq!(state.lock().unwrap().status.state, "compiling");
    assert!(state.lock().unwrap().status.diagnostics.is_empty());
    watcher.refresh(&state, compile);
    assert_eq!(state.lock().unwrap().status.scene_revision, Some(6));
}

#[test]
fn missing_source_recovers_and_same_timestamp_source_rewrite_is_observed() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("source.aml");
    let state = Mutex::new(Published::default());
    let mut watcher = Watcher::new(path.clone());
    watcher.refresh(&state, compile);
    assert_eq!(state.lock().unwrap().status.state, "error");
    assert!(state.lock().unwrap().packet.is_none());
    fs::write(&path, BOX).unwrap();
    watcher.refresh(&state, compile);
    let modified = fs::metadata(&path).unwrap().modified().unwrap();
    fs::write(&path, BOX.replace('2', "4")).unwrap();
    fs::File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_times(fs::FileTimes::new().set_modified(modified))
        .unwrap();
    watcher.refresh(&state, compile);
    assert_eq!(state.lock().unwrap().status.scene_revision, Some(3));
    assert_eq!(
        state.lock().unwrap().packet.as_ref().unwrap().as_slice(),
        superspec_engine::compile_packet(&BOX.replace('2', "4"), 3)
    );
    fs::remove_file(&path).unwrap();
    watcher.refresh(&state, compile);
    assert_eq!(state.lock().unwrap().status.scene_revision, Some(3));
    let replacement = temp.path().join("next.aml");
    fs::write(&replacement, BOX).unwrap();
    fs::rename(replacement, &path).unwrap();
    watcher.refresh(&state, compile);
    assert_eq!(state.lock().unwrap().status.scene_revision, Some(5));
}

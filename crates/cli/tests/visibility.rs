use std::{fs, process::Command};

#[test]
fn visible_noncolliding_geometry_blocks_the_optical_audit() {
    let root = tempfile::tempdir().unwrap();
    fs::write(
        root.path().join("source.aml"),
        r#"<architecture version="1" id="optics">
        <profile id="floor" points="-4 -4; 4 -4; 4 4; -4 4"/>
        <slab id="ground" profile="floor" thickness="0.2"/>
        <path id="axis" points="0 -3 0; 0 3 0"/>
        <wall id="screen" path="axis" height="3" thickness="0.2" collidable="false"/>
    </architecture>"#,
    )
    .unwrap();
    fs::write(
        root.path().join("queries.json"),
        r#"{
        "settings":{"spacing":4,"maxSamples":100,"maxPairQueries":1000},
        "points":[{"id":"a","feet":[-1,0,0]},{"id":"b","feet":[1,0,0]}],
        "protectedPairs":[{"id":"across-screen","from":"a","to":"b"}]
    }"#,
    )
    .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_superspec"))
        .current_dir(root.path())
        .args([
            "visibility",
            "source.aml",
            "queries.json",
            "--out",
            "audit",
            "--json",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    let result: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["report"]["protectedPairs"][0]["visible"], false);
}

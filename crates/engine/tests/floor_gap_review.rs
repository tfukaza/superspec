use superspec_engine::compile;

#[test]
fn actual_retaining_geometry_seals_an_adjoining_floor_gap() {
    let source = r#"<architecture version="1" id="floor-gap">
        <profile id="a" points="0 0;2 0;2 4;0 4"/>
        <profile id="b" points="2 0;4 0;4 4;2 4"/>
        <slab id="lower" profile="a" thickness="0.2"/>
        <slab id="upper" profile="b" position="0 0 2" thickness="0.2"/>
        REPAIR
    </architecture>"#;
    let open = compile(&source.replace("REPAIR", ""), 0).unwrap();
    let finding = open
        .manifest
        .diagnostics
        .iter()
        .find(|d| d.code == "FLOOR_EDGE_GAP")
        .unwrap();
    assert_eq!(finding.severity, "warning");
    assert!(finding.spatial.is_some());
    let review = open
        .manifest
        .reviews
        .iter()
        .find(|r| r.diagnostic.code == "FLOOR_EDGE_GAP")
        .unwrap();
    let acknowledgment = format!(
        "<review-suppression suggestion=\"{}\" fingerprint=\"{}\" reason=\"Intentional separate storeys\"/>",
        review.id, review.fingerprint
    );
    let acknowledged = compile(&source.replace("REPAIR", &acknowledgment), 0).unwrap();
    assert!(
        !acknowledged
            .manifest
            .diagnostics
            .iter()
            .any(|d| d.code == "FLOOR_EDGE_GAP")
    );
    assert_eq!(acknowledged.manifest.reviews[0].state, "suppressed");
    let changed = compile(
        &source
            .replace("REPAIR", &acknowledgment)
            .replace("0 0 2", "0 0 3"),
        0,
    )
    .unwrap();
    assert!(
        changed
            .manifest
            .reviews
            .iter()
            .any(|r| r.state == "active" && r.suppression.as_ref().is_some_and(|s| s.stale))
    );
    let repair = r#"<path id="join" points="2 0 0;2 4 0"/><wall id="retaining" path="join" thickness="0.2" height="2"/>"#;
    let sealed = compile(&source.replace("REPAIR", repair), 0).unwrap();
    assert!(
        !sealed
            .manifest
            .diagnostics
            .iter()
            .any(|d| d.code == "FLOOR_EDGE_GAP")
    );
}

#[test]
fn exposed_roof_steps_and_wall_overshoots_have_source_linked_warnings() {
    let source = r#"<architecture version="1" id="roof-review">
        <profile id="a" points="0 0;4 0;4 4;0 4"/>
        <profile id="b" points="4 0;8 0;8 4;4 4"/>
        <roof id="lower" profile="a" elevation="2" thickness="0.2"/>
        <roof id="upper" profile="b" elevation="4" thickness="0.2"/>
        REPAIR
    </architecture>"#;
    let open = compile(&source.replace("REPAIR", ""), 0).unwrap();
    let warning = open
        .manifest
        .diagnostics
        .iter()
        .find(|d| d.code == "ROOF_STEP_OPEN")
        .unwrap();
    assert_eq!(warning.objects, vec!["lower", "upper"]);
    assert!(warning.spatial.is_some());
    let repaired=compile(&source.replace("REPAIR",r#"<path id="join" points="4 0 0;4 4 0"/><wall id="seal" path="join" thickness="0.2" height="4"/>"#),0).unwrap();
    assert!(
        !repaired
            .manifest
            .diagnostics
            .iter()
            .any(|d| d.code == "ROOF_STEP_OPEN")
    );
    let overshoot=compile(&source.replace("REPAIR",r#"<path id="axis" points="1 1 0;1 3 0"/><wall id="overshoot" path="axis" thickness="0.2" height="5"/>"#),0).unwrap();
    assert!(
        overshoot
            .manifest
            .diagnostics
            .iter()
            .any(|d| d.code == "WALL_ABOVE_ROOF" && d.objects[0] == "overshoot")
    );
}

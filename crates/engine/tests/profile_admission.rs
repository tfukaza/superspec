use superspec_engine::compile;

#[test]
fn touching_overlapping_and_outside_profile_constraints_are_source_errors() {
    for profile in [
        r#"<profile id="p" points="0 0;4 0;4 4;0 4"><hole points="0 1;1 1;1 2;0 2"/></profile>"#,
        r#"<profile id="p" points="0 0;4 0;4 4;0 4"><hole points="0 2;1 1;1 3"/></profile>"#,
        r#"<profile id="p" points="0 0;4 0;4 4;0 4"><hole points="1 1;2 1;2 2;1 2"/><hole points="2 2;3 2;3 3;2 3"/></profile>"#,
        r#"<profile id="p" points="0 0;4 0;2 0;4 4;0 4"/>"#,
        r#"<profile id="p" points="0 0;4 0;4 4;2 0;0 4"/>"#,
        r#"<profile id="p" points="0 0;4 0;4 4;0 4"><breakline points="-1 1 0;2 1 0"/></profile>"#,
        r#"<profile id="p" points="0 0;4 0;4 4;0 4"><hole points="0.5 1;1 1;1 2;0.5 2"/><breakline points="0 1.5 0;4 1.5 0"/></profile>"#,
        r#"<profile id="p" points="0 0;4 0;4 4;3 4;3 1;1 1;1 4;0 4"><breakline points="0.5 3 0;3.5 3 0"/></profile>"#,
    ] {
        let source = format!(
            r#"<architecture version="1" id="a">{profile}<slab id="s" profile="p"/></architecture>"#
        );
        let error = compile(&source, 0).unwrap_err();
        assert_eq!(error.code, "AML_PROFILE", "{source}");
        assert!(error.objects.iter().any(|id| id == "p"));
    }
    // A straight collinear boundary station and a breakline ending on that edge remain valid.
    let valid = r#"<architecture version="1" id="a"><profile id="p" points="0 0;2 0;4 0;4 4;0 4"><breakline points="2 0 0;2 4 0"/></profile><slab id="s" profile="p"/></architecture>"#;
    assert!(compile(valid, 0).is_ok());
}

#[test]
fn nonadjacent_sweep_overlap_is_rejected_without_rejecting_separated_crossings() {
    let source = |path| {
        format!(
            r#"<architecture version="1" id="a"><profile id="p" points="-0.2 -0.2;0.2 -0.2;0.2 0.2;-0.2 0.2"/><path id="axis" points="{path}"/><sweep id="tube" profile="p" path="axis"/></architecture>"#
        )
    };
    let error = compile(&source("0 0 0;3 3 0;0 3 0;3 0 0"), 0).unwrap_err();
    assert!(error.message.contains("intersects itself"));
    assert!(error.objects.iter().any(|id| id == "tube"));
    assert!(compile(&source("0 0 0;3 3 0;0 3 2;3 0 2"), 0).is_ok());
}

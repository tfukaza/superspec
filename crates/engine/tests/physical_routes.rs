use superspec_engine::{compile_collision_with_assets, validation};

fn report(source: &str) -> validation::Report {
    let mut scene =
        compile_collision_with_assets(source, 0, &|path| Err(format!("Unexpected asset {path}")))
            .unwrap();
    validation::audit(&mut scene, &|| false).unwrap();
    scene.manifest.validation.unwrap()
}

#[test]
fn separate_doorways_count_but_duplicate_declarations_share_capacity() {
    let source = include_str!("../../../tests/fixtures/aml/rust-physical-routes.aml");
    let open = report(source);
    assert_eq!(open.routes[0].verified_routes, 2);
    assert_eq!(open.routes[0].maximum_routes, Some(2));
    let excessive = report(&source.replace("min-routes=\"2\"", "min-routes=\"3\""));
    assert!(
        excessive
            .diagnostics
            .iter()
            .any(|d| d.code == "ROUTE_CAPACITY" && d.severity == "error")
    );
    let closed=source.replace("</architecture>",r#"<box id="close-b" position="3.9 5.25 0" width="0.2" depth="1.5" height="2.5" anchor="corner"/></architecture>"#);
    let closed = report(&closed);
    assert_eq!(closed.routes[0].verified_routes, 1);
    assert_eq!(closed.routes[0].maximum_routes, Some(1));
    assert!(
        closed
            .diagnostics
            .iter()
            .any(|d| d.code == "CONNECTION_BLOCKED" && d.severity == "error")
    );
}

#[test]
fn traversal_requires_its_ability_and_preserves_one_way_direction() {
    let source = include_str!("../../../tests/fixtures/aml/rust-traversal-links.aml");
    let enabled = report(source);
    assert!(enabled.routes[0].reachable);
    assert_eq!(enabled.routes[0].traversals, ["ladder-up"]);
    assert!(!enabled.routes[1].reachable);
    assert_eq!(enabled.routes[0].maximum_routes, Some(1));
    assert_eq!(enabled.routes[1].maximum_routes, Some(0));
    let disabled = report(&source.replace("ladder=\"true\"", "ladder=\"false\""));
    assert!(disabled.routes.iter().all(|route| !route.reachable));
    assert!(
        disabled
            .routes
            .iter()
            .all(|route| route.maximum_routes == Some(0))
    );
}

#[test]
fn route_bounds_retain_uncertain_mouths_and_decline_incomplete_graphs() {
    let source = include_str!("../../../tests/fixtures/aml/rust-physical-routes.aml");
    let remote = report(&source.replace("4 5.25 0; 4 6.75 0", "40 5.25 0; 40 6.75 0"));
    assert_eq!(remote.routes[0].verified_routes, 1);
    assert_eq!(remote.routes[0].maximum_routes, Some(2));
    assert!(
        remote
            .diagnostics
            .iter()
            .any(|d| d.code == "NAV_INDEPENDENCE_UNVERIFIED")
    );
    assert!(
        !remote
            .diagnostics
            .iter()
            .any(|d| d.code == "ROUTE_CAPACITY")
    );
    let incomplete = report(&source.replace(" path=\"door-b\"", ""));
    assert_eq!(incomplete.routes[0].maximum_routes, None);
    assert!(
        !incomplete
            .diagnostics
            .iter()
            .any(|d| d.code == "ROUTE_CAPACITY")
    );
    let same_area = report(&source.replace("to=\"finish\"", "to=\"start\""));
    assert_eq!(same_area.routes[0].maximum_routes, None);
}

#[test]
fn boat_draft_and_headroom_do_not_change_canonical_collision() {
    let source = include_str!("../../../tests/fixtures/aml/rust-water-navigation.aml");
    let mut scene =
        compile_collision_with_assets(source, 0, &|_| Err("unexpected asset".into())).unwrap();
    let original = (scene.positions.clone(), scene.indices.clone());
    validation::audit(&mut scene, &|| false).unwrap();
    assert_eq!((scene.positions, scene.indices), original);
    let r = scene.manifest.validation.unwrap();
    assert!(
        r.routes
            .iter()
            .find(|r| r.id == "boat-cross")
            .unwrap()
            .reachable
    );
    assert!(
        !r.routes
            .iter()
            .find(|r| r.id == "bed-cross")
            .unwrap()
            .reachable
    );
    let shallow = report(&source.replace("elevation=\"-2\"", "elevation=\"-0.2\""));
    assert!(
        shallow
            .diagnostics
            .iter()
            .any(|d| d.code == "WATER_DRAFT" && d.severity == "error")
    );
    let blocked=source.replace("</architecture>",r#"<box id="low-bridge" position="3 0 0.5" width="2" depth="4" height="0.2" anchor="corner"/></architecture>"#);
    assert!(
        !report(&blocked)
            .routes
            .iter()
            .find(|r| r.id == "boat-cross")
            .unwrap()
            .reachable
    );
}

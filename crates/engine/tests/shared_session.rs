use std::{collections::BTreeMap, sync::Arc};
use superspec_engine::{
    Session,
    authoring::{Project, Request},
};
use superspec_geometry::image::{Image, encode};

const AML: &str = r#"<architecture version="1" id="mixed"><material id="finish" texture="finish.png"/><box id="slab" width="2" depth="1" height="0.2" material="finish"/></architecture>"#;
fn bytes() -> Vec<u8> {
    encode(&Image {
        width: 1,
        height: 1,
        rgba: vec![80, 120, 160, 255],
        mime: "image/png",
    })
    .unwrap()
}
fn recipe() -> Request {
    Request {
        source: "material 1; output base_color=vec3(0.2,0.4,0.7);".into(),
        resolution: 16,
        ..Default::default()
    }
}
fn material_project() -> Project {
    let mut project = Project {
        modules: BTreeMap::from([("main.ms".into(), recipe().source)]),
        resolution: 16,
        ..Default::default()
    };
    project.base.kind = "aml".into();
    project.base.source = Some(AML.into());
    project.target.enabled = true;
    project
}

#[test]
fn aml_and_authored_work_share_one_clear_and_keep_public_revision_contracts() {
    let mut session = Session::default();
    let image = bytes();
    let first = session
        .compile(AML, 900, &|_| Ok(image.clone()))
        .unwrap()
        .packet();
    let request = recipe();
    // The compatibility import is the same Session, not a containing adapter.
    let authored: &mut superspec_engine::authoring::Session = &mut session;
    let a = authored
        .build(&request, 2, &|_| panic!("no recipe images"), &|| false)
        .unwrap();
    session.compile(AML, 0, &|_| Ok(image.clone())).unwrap();
    assert_eq!(session.stats().hits, 1);
    let b = session
        .build(&request, 3, &|_| panic!("no recipe images"), &|| false)
        .unwrap();
    let a_mesh = a.model.meshes.values().next().unwrap();
    let b_mesh = b.model.meshes.values().next().unwrap();
    assert!(
        Arc::ptr_eq(a_mesh, b_mesh),
        "AML work must not discard another producer's completed artifacts"
    );
    assert!(
        session
            .build(&request, 1, &|_| panic!("obsolete request"), &|| false)
            .err()
            .unwrap()[0]
            .message
            .contains("obsolete")
    );
    session.clear();
    assert_eq!(session.stats().entries, 0);
    assert_eq!(session.bake_stats().entries, 0);
    let c = session
        .build(&request, 0, &|_| panic!("no recipe images"), &|| false)
        .unwrap();
    assert!(!Arc::ptr_eq(
        a_mesh,
        c.model.meshes.values().next().unwrap()
    ));
    assert_eq!(a.scene(0).unwrap().packet(), c.scene(0).unwrap().packet());
    assert_eq!(
        first,
        session
            .compile(AML, 900, &|_| Ok(image.clone()))
            .unwrap()
            .packet()
    );
    assert_eq!(session.stats().misses, 1);
}

#[test]
fn nested_aml_project_uses_shared_admission_and_failure_cannot_mutate_accepted_model() {
    let mut session = Session::default();
    let image: Arc<[u8]> = bytes().into();
    let assets = |name: &str| {
        assert_eq!(name, "finish.png");
        Ok(image.clone())
    };
    let project = material_project();
    session.compile(AML, 100, &|_| Ok(image.to_vec())).unwrap();
    let accepted = session
        .build_project(&project, 2, &assets, &|| false)
        .unwrap();
    assert_eq!(
        session.stats().hits,
        1,
        "project AML lowering uses the same admission owner"
    );
    let packet = accepted.scene(2).unwrap().packet();
    let mut invalid = project.clone();
    invalid.modules.insert(
        "main.ms".into(),
        "material 1; output base_color=missing;".into(),
    );
    let errors = session
        .build_project(&invalid, 3, &assets, &|| false)
        .err()
        .unwrap();
    assert_eq!(errors[0].source_name, "main.ms");
    assert_eq!(accepted.scene(2).unwrap().packet(), packet);
    assert!(
        session
            .compile_cancellable(AML, 0, &|_| Ok(image.to_vec()), true, &|| true)
            .is_err()
    );
    let recovered = session
        .build_project(&project, 4, &assets, &|| false)
        .unwrap();
    let fresh = Session::default()
        .build_project(&project, 4, &assets, &|| false)
        .unwrap();
    assert_eq!(
        recovered.scene(4).unwrap().packet(),
        fresh.scene(4).unwrap().packet()
    );
    assert_eq!(accepted.scene(2).unwrap().packet(), packet);
}

#[test]
fn a_world_request_can_lower_aml_then_items_without_superseding_itself() {
    let project:Project=serde_json::from_value(serde_json::json!({
        "kind":"world","entry":"scene.aml","resolution":16,
        "modules":{"scene.aml":"<architecture version=\"1\" id=\"world\"><box id=\"slab\" width=\"3\" depth=\"3\" height=\"0.2\"/></architecture>",
        "pipe.item.json":"{\"format\":\"itemspec\",\"version\":1,\"id\":\"pipe\",\"generator\":{\"kind\":\"pipe\",\"length\":1,\"radius\":0.1},\"collision\":\"mesh\"}"},
        "world":{"items":[{"id":"pipe","entry":"pipe.item.json"}]}
    })).unwrap();
    let mut session = Session::default();
    let output = session
        .build_project(&project, 12, &|_| panic!("no source images"), &|| false)
        .unwrap();
    let scene = output.scene(12).unwrap();
    assert!(
        scene
            .manifest
            .colliders
            .iter()
            .any(|id| id.starts_with("item/pipe/"))
    );
    assert!(
        session
            .build_project(&project, 11, &|_| panic!("obsolete request"), &|| false)
            .is_err()
    );
    let again = session
        .build_project(&project, 13, &|_| panic!("no source images"), &|| false)
        .unwrap();
    assert_eq!(scene.packet(), again.scene(12).unwrap().packet());
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn a_session_moves_between_threads_and_observers_can_read_or_clear_producer_state() {
    fn assert_send<T: Send>() {}
    assert_send::<Session>();
    let session = Session::default();
    let session = std::thread::spawn(move || {
        let image = bytes();
        let visited = std::cell::Cell::new(false);
        let error = session
            .compile_observed(AML, 1, &|_| Ok(image.clone()), &|| false, &|phase| {
                // A host may inspect progress without acquiring a compile-wide producer lock.
                let _stats = session.bake_stats();
                if phase == "physical-solids" {
                    visited.set(true);
                    session.clear();
                    assert_eq!(session.bake_stats().entries, 0);
                }
            })
            .unwrap_err();
        assert!(visited.get());
        assert_eq!(error.code, "CANCELLED");
        let recovered = session.compile(AML, 2, &|_| Ok(image.clone())).unwrap();
        let fresh = Session::default()
            .compile(AML, 2, &|_| Ok(image.clone()))
            .unwrap();
        assert_eq!(recovered.packet(), fresh.packet());
        session
    })
    .join()
    .unwrap();
    assert!(session.compile(AML, 3, &|_| Ok(bytes())).is_ok());
}

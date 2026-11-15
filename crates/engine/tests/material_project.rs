//! Multi-model, multi-material projects: models compose under their own identities, and each
//! material bakes onto its own meshes through the same `build_project` preview and export use.
use std::{collections::BTreeMap, path::PathBuf, sync::Arc};
use superspec_engine::authoring::{MaterialAssignment, ModelSource, Project, Session};

fn examples() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/materialspec/program/examples")
}

fn asset(name: &str) -> Result<Arc<[u8]>, String> {
    let path = match name.strip_prefix("superspec-material:") {
        Some(catalog) => PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../assets/materials")
            .join(catalog),
        None => examples().join(name),
    };
    std::fs::read(&path)
        .map(Arc::from)
        .map_err(|e| format!("{name}: {e}"))
}

/// Two small AML scenes that both name an element `wall`.
fn models() -> Vec<ModelSource> {
    let scene = |id: &str, boxes: &str| ModelSource {
        id: id.into(),
        kind: "aml".into(),
        source: Some(format!(
            r##"<architecture version="1" id="{id}"><material id="plain" color="#888888"/>{boxes}</architecture>"##
        )),
        name: None,
    };
    vec![
        scene(
            "yard",
            r#"<box id="wall" width="0.8" depth="0.2" height="0.6" material="plain"/><box id="slab" position="0 0.6 0" width="0.8" depth="0.6" height="0.1" material="plain"/>"#,
        ),
        scene(
            "steel",
            r#"<box id="wall" position="1.2 0 0" width="0.3" depth="0.2" height="0.25" material="plain"/>"#,
        ),
    ]
}

fn project(materials: Vec<MaterialAssignment>) -> Project {
    let modules: BTreeMap<String, String> = [
        "kowloon-noise.material.ts",
        "kowloon-concrete.material.ts",
        "kowloon-rusted-steel.material.ts",
        "painted-metal.material.ts",
        "common.material.ts",
    ]
    .into_iter()
    .map(|name| {
        (
            name.to_owned(),
            std::fs::read_to_string(examples().join(name)).unwrap(),
        )
    })
    .collect();
    Project {
        schema_version: 2,
        kind: "material".into(),
        entry: "kowloon-concrete.material.ts".into(),
        modules,
        resolution: 64,
        mapping: superspec_engine::authoring::Mapping::World,
        models: models(),
        materials,
        ..Project::default()
    }
}

fn assign(id: &str, entry: &str, meshes: &[&str]) -> MaterialAssignment {
    MaterialAssignment {
        id: id.into(),
        entry: entry.into(),
        meshes: meshes.iter().map(|mesh| (*mesh).to_owned()).collect(),
        ..MaterialAssignment::default()
    }
}

fn build(project: &Project) -> Result<superspec_engine::authoring::ProjectOutput, String> {
    Session::default()
        .build_project(project, 1, &asset, &|| false)
        .map_err(|errors| {
            errors
                .iter()
                .map(|e| e.message.clone())
                .collect::<Vec<_>>()
                .join("; ")
        })
}

/// The diagnostics of a project expected to fail.
fn failure(project: &Project) -> String {
    match build(project) {
        Ok(_) => panic!("the project should be rejected"),
        Err(message) => message,
    }
}

fn mesh_ids(project: &Project) -> Vec<String> {
    let output = build(&Project {
        materials: vec![],
        ..project.clone()
    })
    .unwrap();
    output.metadata["meshes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_owned())
        .collect()
}

#[test]
fn models_compose_under_their_own_identities() {
    let ids = mesh_ids(&project(vec![]));
    assert!(ids.iter().any(|id| id.starts_with("yard/")), "{ids:?}");
    assert!(ids.iter().any(|id| id.starts_with("steel/")), "{ids:?}");
    // Both scenes name an element `wall`; the prefixes keep their meshes apart.
    let walls: Vec<_> = ids.iter().filter(|id| id.ends_with("/wall")).collect();
    assert_eq!(walls.len(), 2, "{ids:?}");
}

#[test]
fn each_material_bakes_onto_its_own_meshes() {
    let base = project(vec![]);
    let ids = mesh_ids(&base);
    let yard: Vec<&str> = ids
        .iter()
        .filter(|id| id.starts_with("yard/"))
        .map(String::as_str)
        .collect();
    let steel: Vec<&str> = ids
        .iter()
        .filter(|id| id.starts_with("steel/"))
        .map(String::as_str)
        .collect();
    let project = project(vec![
        assign("concrete", "kowloon-concrete.material.ts", &yard),
        assign("rust", "kowloon-rusted-steel.material.ts", &steel),
        assign("spare", "painted-metal.material.ts", &[]),
    ]);
    let output = build(&project).unwrap();
    let scene = output.scene(1).unwrap();
    let material_of = |id: &str| {
        scene
            .manifest
            .meshes
            .iter()
            .find(|mesh| mesh.id == id)
            .unwrap()
            .material
            .clone()
            .unwrap()
    };
    let concrete: std::collections::BTreeSet<_> = yard.iter().map(|id| material_of(id)).collect();
    let rust: std::collections::BTreeSet<_> = steel.iter().map(|id| material_of(id)).collect();
    assert!(
        concrete
            .iter()
            .chain(&rust)
            .all(|m| m.starts_with("studio/material/")),
        "{concrete:?} {rust:?}"
    );
    assert!(
        concrete.is_disjoint(&rust),
        "different materials bake different maps"
    );
    // Every material reports its parameters, including one not yet applied to a mesh.
    let materials = output.metadata["materials"].as_object().unwrap();
    assert_eq!(materials.len(), 3);
    for (id, parameter) in [
        ("concrete", "chipAmount"),
        ("rust", "edgeRust"),
        ("spare", "damage"),
    ] {
        let names: Vec<_> = materials[id]["parameters"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p["name"].as_str().unwrap())
            .collect();
        assert!(names.contains(&parameter), "{id}: {names:?}");
    }
    // The same inputs build the same scene.
    let again = build(&project).unwrap().scene(1).unwrap();
    assert_eq!(again.packet(), scene.packet());
}

#[test]
fn invalid_assignments_and_models_are_reported() {
    let ids = mesh_ids(&project(vec![]));
    let first = ids[0].as_str();
    let twice = failure(&project(vec![
        assign("a", "kowloon-concrete.material.ts", &[first]),
        assign("b", "kowloon-rusted-steel.material.ts", &[first]),
    ]));
    assert!(twice.contains("more than one material"), "{twice}");
    let unknown = failure(&project(vec![assign(
        "a",
        "kowloon-concrete.material.ts",
        &["yard/missing"],
    )]));
    assert!(unknown.contains("not a visible mesh"), "{unknown}");
    let missing = failure(&project(vec![assign("a", "nowhere.material.ts", &[])]));
    assert!(missing.contains("missing module"), "{missing}");
    let mut duplicate = project(vec![]);
    duplicate.models[1].id = "yard".into();
    assert!(failure(&duplicate).contains("unique"));
    let mut slashed = project(vec![]);
    slashed.models[0].id = "a/b".into();
    assert!(failure(&slashed).contains("unique"));
}

/// A slab with a block on it, or `gap` meters above it, as one AML model.
fn pile(gap: f64) -> Project {
    let compose = |z: f64| Project {
        models: vec![ModelSource {
            id: "pile".into(),
            kind: "aml".into(),
            source: Some(format!(
                r##"<architecture version="1" id="pile"><material id="plain" color="#808080"/><box id="slab" width="1" depth="1" height="0.2" material="plain"/><box id="block" position="0 0 {z}" width="0.4" depth="0.4" height="0.3" material="plain"/></architecture>"##
            )),
            name: None,
        }],
        ..project(vec![])
    };
    // Measure where AML places the boxes rather than assume center or base positioning.
    let scene = build(&compose(0.2)).unwrap().scene(1).unwrap();
    let bounds = |id: &str| {
        scene
            .manifest
            .meshes
            .iter()
            .find(|mesh| mesh.id == id)
            .unwrap()
            .bounds
    };
    let lift = bounds("pile/slab")[1][2] - bounds("pile/block")[0][2] + gap;
    let mut project = compose(0.2 + lift);
    project.modules.insert(
        "contact.ms".into(),
        "material 1; let d = crease_distance(\"contact\", 0.0, 0.05); output base_color = vec3(clamp(d * 20.0, 0.0, 1.0));".into(),
    );
    project.materials = vec![assign("contact", "contact.ms", &["pile/block"])];
    project
}

/// The darkest and lightest baked base color bytes of a mesh.
fn color_range(output: &superspec_engine::authoring::ProjectOutput, mesh: &str) -> (u8, u8) {
    let scene = output.scene(1).unwrap();
    let material = scene
        .manifest
        .meshes
        .iter()
        .find(|m| m.id == mesh)
        .unwrap()
        .material
        .clone()
        .unwrap();
    let texture = scene
        .manifest
        .materials
        .iter()
        .find(|m| m.id == material)
        .unwrap()
        .texture
        .clone()
        .unwrap();
    let asset = scene
        .manifest
        .assets
        .iter()
        .find(|a| a.id == texture)
        .unwrap();
    let image = superspec_geometry::image::decode(
        &scene.asset_bytes[asset.byte_start..asset.byte_start + asset.byte_length],
    )
    .unwrap();
    let reds: Vec<u8> = image
        .rgba
        .chunks_exact(4)
        .filter(|t| t[3] > 0)
        .map(|t| t[0])
        .collect();
    (*reds.iter().min().unwrap(), *reds.iter().max().unwrap())
}

#[test]
fn edges_resting_on_another_surface_are_contacts() {
    // Resting: the block's base edges touch the slab, so texels there are near zero distance.
    let (darkest, lightest) = color_range(&build(&pile(0.)).unwrap(), "pile/block");
    assert!(
        darkest < 40,
        "contact distance reaches zero at the base ({darkest})"
    );
    assert_eq!(lightest, 255, "faces away from the base stay at the limit");
    // Lifted 10 cm: no edge touches another surface.
    let (darkest, _) = color_range(&build(&pile(0.1)).unwrap(), "pile/block");
    assert_eq!(darkest, 255, "no contact without touching");
}

/// The same pile as a version 1 project: one material on one target mesh of an AML base scene.
fn single_material_pile(gap: f64) -> Project {
    let scene = pile(gap);
    Project {
        schema_version: 1,
        entry: "contact.ms".into(),
        base: superspec_engine::authoring::Base {
            kind: "aml".into(),
            source: scene.models[0].source.clone(),
            name: None,
        },
        target: superspec_engine::authoring::Target {
            mesh: Some("block".into()),
            enabled: true,
            ..superspec_engine::authoring::Target::default()
        },
        models: vec![],
        materials: vec![],
        ..scene
    }
}

#[test]
fn single_material_projects_see_contacts_too() {
    let (darkest, lightest) = color_range(&build(&single_material_pile(0.)).unwrap(), "block");
    assert!(darkest < 40, "contact at the base ({darkest})");
    assert_eq!(lightest, 255);
    let (darkest, _) = color_range(&build(&single_material_pile(0.1)).unwrap(), "block");
    assert_eq!(darkest, 255, "no contact without touching");
}

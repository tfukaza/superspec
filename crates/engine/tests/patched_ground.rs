use superspec_aml::material_catalog;
use superspec_engine::{Scene, compile, compile_collision_with_assets, compile_with_assets};

use std::path::Path;
const SOURCE: &str = include_str!("../../../tests/fixtures/aml/rust-patched-ground.aml");
fn assets(id: &str) -> Result<Vec<u8>, String> {
    let name = material_catalog::image_name(id).ok_or_else(|| format!("Unexpected asset {id}"))?;
    std::fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../assets/materials")
            .join(name),
    )
    .map_err(|e| e.to_string())
}
fn physical(scene: &Scene) -> serde_json::Value {
    serde_json::json!(scene.manifest.meshes.iter().filter(|m| scene.manifest.colliders.contains(&m.id)).map(|m| serde_json::json!({
        "mesh":m, "positions":scene.positions[m.vertex_start*3..(m.vertex_start+m.vertex_count)*3],
        "normals":scene.normals[m.vertex_start*3..(m.vertex_start+m.vertex_count)*3],
        "indices":scene.indices[m.index_start..m.index_start+m.index_count]
    })).collect::<Vec<_>>())
}
#[test]
fn patched_ground_builds_visual_geometry_without_changing_collision_or_inlays() {
    let scene = compile_with_assets(SOURCE, 0, &assets).unwrap();
    let baseline_source = format!(
        "{}</architecture>",
        SOURCE.split("  <dressing").next().unwrap()
    );
    let baseline = compile(&baseline_source, 0).unwrap();
    assert_eq!(physical(&scene), physical(&baseline));
    assert!(
        scene
            .manifest
            .meshes
            .iter()
            .any(|m| m.visible && m.object_id.starts_with("floor/$dressing/"))
    );
    let inlay = |scene: &Scene| {
        scene
            .manifest
            .meshes
            .iter()
            .filter(|m| m.visible && m.material.as_deref() == Some("inlay"))
            .map(|m| {
                (
                    scene.positions[m.vertex_start * 3..(m.vertex_start + m.vertex_count) * 3]
                        .to_vec(),
                    scene.indices[m.index_start..m.index_start + m.index_count].to_vec(),
                )
            })
            .collect::<Vec<_>>()
    };
    assert!(!inlay(&scene).is_empty());
    assert_eq!(inlay(&scene), inlay(&baseline));
    assert!(!scene.manifest.assets.is_empty());
    assert!(scene.manifest.diagnostics.is_empty());
    assert_eq!(scene.manifest.ground_plans.len(), 1);
    assert!(!scene.manifest.ground_plans[0].regions.is_empty());
    assert!(scene.manifest.ground_plans[0].anchors.iter().any(|a| a.kind
        == superspec_engine::worldspec::themes::kowloon::pavement::layout::Kind::Connection));
}

#[test]
fn invalid_ground_controls_and_tilt_fail_before_visual_image_requests() {
    for (from, to) in [
        ("ground=\"patched\"", "ground=\"unknown\""),
        ("ground=\"patched\"", ""),
        ("patch-size=\"2.3\"", "patch-size=\".1\""),
        ("material-size=\"7\"", "material-size=\"25\""),
        ("relief=\".032\"", "relief=\".05\""),
        ("tile-share=\".1\"", "tile-share=\"1\""),
        (
            "asphalt-material=\"builtin:asphalt\"",
            "asphalt-material=\"missing\"",
        ),
    ] {
        let error = compile_with_assets(&SOURCE.replace(from, to), 0, &|_| {
            panic!("Invalid recipe requested image")
        })
        .unwrap_err();
        assert!(
            matches!(error.code, "AML_DRESSING" | "AML_REFERENCE"),
            "{from}: {error:?}"
        );
    }
    let simple = "<architecture version=\"1\" id=\"tilt\"><profile id=\"p\" points=\"0 0;3 0;3 3;0 3\"/><slab id=\"floor\" profile=\"p\" rotation=\"10 0 0\"/><dressing seed=\"x\"><floor host=\"floor\" ground=\"patched\"/></dressing></architecture>";
    let error = compile_with_assets(simple, 0, &|_| panic!("Tilt requested image")).unwrap_err();
    assert!(error.message.contains("horizontal"), "{error:?}");
}

#[test]
fn hidden_and_collision_only_ground_do_not_load_visual_assets() {
    compile_collision_with_assets(SOURCE, 0, &|_| panic!("Collision requested image")).unwrap();
    let hidden = SOURCE.replace("<slab id=\"floor\"", "<slab visible=\"false\" id=\"floor\"");
    let scene =
        compile_with_assets(&hidden, 0, &|_| panic!("Hidden ground requested image")).unwrap();
    assert!(
        !scene
            .manifest
            .meshes
            .iter()
            .any(|m| m.object_id.starts_with("floor/$dressing/"))
    );
    assert!(scene.manifest.ground_plans.is_empty());
}

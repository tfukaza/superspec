use superspec_engine::{Scene, compile_collision_with_assets, compile_with_assets};

const WALL: &str = r#"<architecture version="1" id="windows">
    <path id="axis" points="0 0 0; 6 0 0"/>
    <wall id="host" path="axis" thickness="0.14" height="2" position="2 3 4" rotation="0 0 90"/>
    <opening id="cut" host="host" center="2 1" width="1.1" height="0.9"/>
    <window id="casement" opening="cut"/>
</architecture>"#;

fn physical(aml: &str) -> Result<Scene, superspec_types::diagnostic::Diagnostic> {
    compile_collision_with_assets(aml, 1, &|p| {
        panic!("Unexpected image read in collision build: {p}")
    })
}

fn render(aml: &str) -> Scene {
    let known: Vec<String> = serde_json::from_str(&superspec_engine::library_assets(aml)).unwrap();
    compile_with_assets(aml, 1, &|p| {
        assert!(known.iter().any(|path| path == p), "Unadvertised image {p}");
        let file = superspec_catalog::image_name(p).unwrap();
        std::fs::read(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../assets/materials")
                .join(file),
        )
        .map_err(|e| e.to_string())
    })
    .unwrap()
}

#[test]
fn hosted_leaf_turns_at_its_hinge_in_the_rotated_wall_frame() {
    let mut previous = None;
    for (attribute, hinge_open) in [("", false), (" open=\"true\"", true)] {
        let aml = WALL.replace(
            "opening=\"cut\"/>",
            &format!("opening=\"cut\"{attribute}/>"),
        );
        let visual = render(&aml);
        let only = physical(&aml).unwrap();
        assert_eq!(visual.manifest.colliders, only.manifest.colliders);
        let collider = only
            .manifest
            .meshes
            .iter()
            .find(|m| m.id == "$collision/casement")
            .unwrap();
        let glass = visual
            .manifest
            .meshes
            .iter()
            .find(|m| {
                m.object_id == "casement" && m.material.as_deref() == Some("builtin:display-glass")
            })
            .unwrap();
        // Host +90deg maps outward local -Y to world +X. The left jamb is at world Y=4.45.
        if hinge_open {
            assert!((collider.bounds[1][0] - 3.079).abs() < 1e-4);
            assert!(glass.bounds[0][1] > 4.49 && glass.bounds[1][1] < 4.54);
            assert!(glass.bounds[1][0] > 3.0);
        } else {
            assert!((collider.bounds[1][0] - 2.075).abs() < 1e-4);
            assert!(glass.bounds[0][1] < 4.54 && glass.bounds[1][1] > 5.45);
            assert!(glass.bounds[1][0] < 2.08);
        }
        assert_eq!(
            visual
                .manifest
                .architecture
                .as_ref()
                .unwrap()
                .objects
                .iter()
                .filter(|o| o.id == "casement")
                .count(),
            1
        );
        let host = only
            .manifest
            .meshes
            .iter()
            .find(|m| m.id == "$collision/host")
            .unwrap();
        let cut_wall = only.positions
            [host.vertex_start * 3..(host.vertex_start + host.vertex_count) * 3]
            .to_vec();
        if let Some(first) = previous {
            assert_eq!(cut_wall, first);
        }
        previous = Some(cut_wall);
    }
}

#[test]
fn hosted_visual_visibility_and_canonical_collision_are_independent() {
    for (attr, draw, collide) in [
        ("visible=\"false\"", false, true),
        ("collidable=\"false\"", true, false),
    ] {
        let aml = WALL.replace("opening=\"cut\"/>", &format!("opening=\"cut\" {attr}/>"));
        let scene = render(&aml);
        assert_eq!(
            scene
                .manifest
                .meshes
                .iter()
                .any(|m| m.object_id == "casement" && m.visible),
            draw
        );
        assert_eq!(
            scene
                .manifest
                .meshes
                .iter()
                .any(|m| m.id == "$collision/casement"),
            collide
        );
    }
}

#[test]
fn hosted_box_apertures_remain_instance_scoped() {
    let scene = render(
        r#"<architecture version="1" id="rep">
      <definition id="bay">
        <box id="panel" width="3" depth=".2" height="2"/>
        <opening id="cut" host="panel" center="1.5 1" width="1.2" height="1.1"/>
        <window id="leaf" opening="cut"/>
      </definition>
      <instance id="left" ref="bay"/><instance id="right" ref="bay" position="6 0 0"/>
    </architecture>"#,
    );
    for id in ["left/leaf", "right/leaf"] {
        assert!(
            scene
                .manifest
                .meshes
                .iter()
                .any(|m| m.visible && m.object_id == id)
        );
        assert!(
            scene
                .manifest
                .meshes
                .iter()
                .any(|m| m.id == format!("$collision/{id}"))
        );
    }
}

#[test]
fn unsupported_apertures_and_unseated_frames_report_source_errors() {
    for aml in [
        WALL.replace("opening=\"cut\"", "opening=\"missing\""),
        WALL.replace("opening=\"cut\"", "opening=\"host\""),
        WALL.replace(
            "center=\"2 1\"",
            "mode=\"recess\" depth=\".05\" center=\"2 1\"",
        ),
        WALL.replace("center=\"2 1\"", "shape=\"circle\" center=\"2 1\""),
        WALL.replace("center=\"2 1\"", "center=\"2 1.9\""),
        WALL.replace("6 0 0", "2 0 0; 2 4 0"),
        WALL.replace(
            "</architecture>",
            "<window id=\"other\" opening=\"cut\"/></architecture>",
        ),
    ] {
        let e = physical(&aml).err().expect("invalid fitting accepted");
        assert!(matches!(e.code, "AML_WINDOW" | "AML_REFERENCE"), "{e}");
        assert!(e.source.line >= 4);
    }
}

#[test]
fn same_size_casements_in_one_scene_retain_their_individual_visual_poses() {
    for first_open in [false, true] {
        let aml = format!(
            r#"<architecture version="1" id="pair">
          <box id="wall" width="5" height="2" depth=".18"/>
          <opening id="a" host="wall" center="1 1" width="1.13" height="1.28"/>
          <opening id="b" host="wall" center="3 1" width="1.13" height="1.28"/>
          <window id="first" opening="a" open="{first_open}"/>
          <window id="second" opening="b" open="{}"/>
        </architecture>"#,
            !first_open
        );
        let scene = render(&aml);
        for (id, open) in [("first", first_open), ("second", !first_open)] {
            let glass = scene
                .manifest
                .meshes
                .iter()
                .find(|m| {
                    m.object_id == id
                        && m.material.as_deref() == Some("builtin:display-glass")
                        && m.id.ends_with("/side")
                })
                .unwrap();
            let extent = [0, 1].map(|axis| glass.bounds[1][axis] - glass.bounds[0][axis]);
            let (long, short) = if open { (1, 0) } else { (0, 1) };
            assert!(
                (extent[long] - 0.964).abs() < 1e-4 && (extent[short] - 0.004).abs() < 1e-4,
                "{id}, open={open}: {extent:?}"
            );
        }
    }
}

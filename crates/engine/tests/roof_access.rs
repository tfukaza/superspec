use superspec_engine::worldspec::themes::kowloon::{
    fixture::Fixture,
    mesh::{Mesh, Mode},
    pen::Pen,
    roof_access::{self, Plot, Settings},
    weathered,
};
use superspec_geometry::Matrix;
#[path = "support/roof_access_scene.rs"]
mod scene_fixture;
struct Case {
    settings: Settings,
    plot: Plot,
    height: f64,
    reflected: bool,
}
fn case(seed: usize, masonry: bool, reflected: bool) -> Case {
    let mut settings = serde_json::json!({"weathering":{"seed":format!("roof/{seed}"),"grime":0.6,"wear":0.25,"rust":0.4,"textureDensity":32,"metal":{"material":"metal","repeat":1.5},"paint":{"material":"paint","repeat":1},"sheet":{"material":"sheet","repeat":1.5}},"wall":{"material":"wall","repeat":1.3},"concrete":{"material":"concrete","repeat":2.7},"wall_tint":[0.67,0.78,0.66],"lot_key":"lot"});
    if masonry {
        settings["masonry"] = serde_json::json!({"grime": 0, "wear": 0, "density": 32});
    }
    Case {
        settings: serde_json::from_value(settings).unwrap(),
        plot: serde_json::from_value(serde_json::json!({"x0": 1, "x1": 3.4, "y0": 2, "y1": 4.8}))
            .unwrap(),
        height: 2.3,
        reflected,
    }
}
fn repeat(slot: &str) -> f64 {
    match slot {
        "wall" => 1.3,
        "concrete" => 2.7,
        "metal" | "sheet" => 1.5,
        _ => 1.,
    }
}
fn draw(mesh: &mut Mesh<'_>, case: &Case) -> Result<(), String> {
    roof_access::stair_hut(
        &mut Fixture {
            pen: Pen::new(
                if case.reflected {
                    Matrix([[-1., 0., 0.], [0., -1., 0.], [0., 0., -1.], [4., 3., 5.]])
                } else {
                    Matrix::default()
                },
                "roof",
            )
            .unwrap(),
            mesh,
            repeats: &|s| repeat(s),
        },
        case.plot,
        case.height,
        &case.settings,
    )
}
#[test]
fn roof_access_receivers_partition_triangles_and_surveys_match_render() {
    for (seed, masonry, reflected) in [
        (0, false, false),
        (1, false, false),
        (2, false, false),
        (3, false, false),
        (0, true, false),
        (1, false, true),
    ] {
        let case = case(seed, masonry, reflected);
        let label = format!("roof {seed} masonry={masonry} reflected={reflected}");
        let mut mesh = Mesh::new(Mode::Render, 100000, &|| false).unwrap();
        draw(&mut mesh, &case).unwrap();
        let triangles = mesh.triangle_count();
        assert!(triangles > 0, "{label}");
        let mut survey = Mesh::new(Mode::Survey, 100000, &|| false).unwrap();
        draw(&mut survey, &case).unwrap();
        assert_eq!(survey.triangle_count(), triangles, "{label}");
        let plan = weathered::finish(&mut mesh, &|| false).unwrap();
        let mut received = vec![false; triangles];
        for receiver in plan.receivers() {
            for seen in &mut received
                [receiver.first_triangle..receiver.first_triangle + receiver.triangle_count]
            {
                assert!(!*seen, "{label}: receivers overlap");
                *seen = true;
            }
        }
        assert!(received.iter().any(|v| *v), "{label}");
    }
}
#[test]
fn complete_roof_access_bakes_appearance_without_changing_host_collision() {
    let scene = scene_fixture::scene();
    assert_eq!(scene.manifest.colliders.len(), 1);
    assert!(!scene.manifest.assets.is_empty());
    assert!(scene.manifest.meshes.iter().filter(|m| m.visible).count() > 4);
}
#[test]
fn malformed_roof_access_inputs_do_not_emit_and_next_build_succeeds() {
    let case = &mut case(0, false, false);
    for error in 0..4 {
        let s = case.settings.clone();
        let h = case.height;
        let p = case.plot;
        match error {
            0 => case.height = 2.,
            1 => case.plot.x1 = case.plot.x0 + 1.,
            2 => case.settings.weathering.grime = f64::NAN,
            _ => {
                case.settings.masonry = Some(roof_access::Masonry {
                    grime: 0.5,
                    wear: 0.5,
                    density: 0.,
                })
            }
        }
        let mut mesh = Mesh::new(Mode::Render, 100000, &|| false).unwrap();
        assert!(draw(&mut mesh, case).is_err());
        assert!(mesh.indices.is_empty());
        case.settings = s;
        case.height = h;
        case.plot = p;
    }
    let mut mesh = Mesh::new(Mode::Render, 100000, &|| false).unwrap();
    draw(&mut mesh, case).unwrap();
    assert!(!mesh.indices.is_empty());
}

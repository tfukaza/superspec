use serde_json::json;
use superspec_engine::worldspec::themes::kowloon::{
    fixture::Fixture,
    mesh::{Mesh, Mode},
    pen::Pen,
    tarps::{self, Envelope, Pattern},
    weathered,
};
use superspec_geometry::{Matrix, V3};
fn pen(reflected: bool) -> Pen {
    Pen::new(
        if reflected {
            Matrix([[-1., 0., 0.], [0., -1., 0.], [0., 0., -1.], [4., 3., 5.]])
        } else {
            Matrix::default()
        },
        "awning",
    )
    .unwrap()
}
fn repeats(slot: &str) -> f64 {
    match slot {
        "tarp" => 2.3,
        "metal" | "sheet" => 1.5,
        _ => 1.,
    }
}
fn envelope(x1: f64, depth: f64, drop: f64, valance: f64) -> Envelope {
    Envelope {
        x0: -0.4,
        x1,
        z: 2.5,
        depth,
        drop,
        valance,
    }
}
const TINT: V3 = [0.05, 0.21, 0.11];
const HEM: V3 = [0.81, 0.74, 0.53];
struct Case {
    envelope: Envelope,
    pattern: Option<Pattern>,
    settings: Option<weathered::Settings>,
    reflected: bool,
}
fn draw(mesh: &mut Mesh<'_>, c: &Case, key: &str) -> Result<(), String> {
    tarps::canvas(
        &mut Fixture {
            pen: pen(c.reflected),
            mesh,
            repeats: &repeats,
        },
        c.envelope,
        TINT,
        HEM,
        key,
        c.settings.as_ref(),
        c.pattern,
    )
}
fn triangles(c: &Case, mode: Mode) -> usize {
    let mut mesh = Mesh::new(mode, 150000, &|| false).unwrap();
    draw(&mut mesh, c, "canvas/0").unwrap();
    mesh.triangle_count()
}
#[test]
fn canvas_surveys_like_render_mirrors_without_changing_topology_and_valance_adds_cloth() {
    let settings: weathered::Settings = serde_json::from_value(json!({
        "seed": "awning/1", "grime": 0.6, "wear": 0.25, "rust": 0.4, "textureDensity": 32,
        "metal": {"material": "metal", "repeat": 1.5},
        "paint": {"material": "paint", "repeat": 1},
        "sheet": {"material": "sheet", "repeat": 1.5}
    }))
    .unwrap();
    let pattern: Pattern =
        serde_json::from_value(json!({"color": [0.69, 0.35, 0.03], "width": 0.24})).unwrap();
    for (x1, depth) in [(2.4, 1.3), (4.8, 0.65)] {
        for pattern in [None, Some(pattern)] {
            for settings in [None, Some(settings.clone())] {
                let case = |valance, reflected| Case {
                    envelope: envelope(x1, depth, 0.4, valance),
                    pattern,
                    settings: settings.clone(),
                    reflected,
                };
                let plain = case(0., false);
                let render = triangles(&plain, Mode::Render);
                assert!(render > 0);
                assert_eq!(triangles(&plain, Mode::Survey), render);
                assert_eq!(triangles(&case(0., true), Mode::Render), render);
                assert!(triangles(&case(0.28, false), Mode::Render) > render);
            }
        }
    }
}
#[test]
fn invalid_canvas_does_not_emit_and_next_build_recovers() {
    let mut c = Case {
        envelope: envelope(2.4, f64::NAN, 0.4, 0.),
        pattern: None,
        settings: None,
        reflected: false,
    };
    let mut mesh = Mesh::new(Mode::Render, 150000, &|| false).unwrap();
    assert!(draw(&mut mesh, &c, "canvas/0").is_err());
    assert!(mesh.indices.is_empty());
    c.envelope = envelope(2.4, 1.3, 0.4, 0.);
    draw(&mut mesh, &c, "canvas/0").unwrap();
    assert!(!mesh.indices.is_empty());
    let mut cancelled = Mesh::new(Mode::Render, 150000, &|| true).unwrap();
    assert!(draw(&mut cancelled, &c, "canvas/0").is_err());
}

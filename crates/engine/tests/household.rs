use serde_json::json;
use superspec_aml::rng::SeededRng;
use superspec_engine::worldspec::themes::kowloon::{
    fixture::Fixture,
    household,
    mesh::{Mesh, Mode},
    metal::Metal,
    palette::srgb,
    pen::Pen,
    weathered,
};
use superspec_geometry::Matrix;
const KINDS: [&str; 6] = ["rack", "pole", "plants", "cage", "lamp", "meters"];
fn pen(reflected: bool) -> Pen {
    Pen::new(
        if reflected {
            Matrix([[-1., 0., 0.], [0., -1., 0.], [0., 0., -1.], [4., 3., 5.]])
        } else {
            Matrix::default()
        },
        "household",
    )
    .unwrap()
}
fn repeats(slot: &str) -> f64 {
    match slot {
        "wall" => 1.3,
        "concrete" => 2.7,
        "metal" | "sheet" => 1.5,
        _ => 1.,
    }
}
fn settings() -> weathered::Settings {
    serde_json::from_value(json!({
        "seed": "household/1", "grime": 0.6, "wear": 0.25, "rust": 0.4, "textureDensity": 32,
        "metal": {"material": "metal", "repeat": 1.5},
        "paint": {"material": "paint", "repeat": 1},
        "sheet": {"material": "sheet", "repeat": 1.5}
    }))
    .unwrap()
}
struct Case<'a> {
    kind: &'a str,
    depth: f64,
    reflected: bool,
    settings: Option<weathered::Settings>,
}
fn draw(mesh: &mut Mesh<'_>, c: &Case<'_>) -> Result<u32, String> {
    let mut rng = SeededRng::from_text("fixture/1");
    let mut d = Fixture {
        pen: pen(c.reflected),
        mesh,
        repeats: &repeats,
    };
    let s = c.settings.as_ref();
    match c.kind {
        "rack" => household::drying_rack(&mut d, [-0.4, 1.5], 2., c.depth, &mut rng, s, true)?,
        "pole" => household::drying_pole(&mut d, 0.3, 2., 1.2, &mut rng, s, true)?,
        "plants" => household::plants(&mut d, [-0.4, 1.5], 0.3, 0.1, &mut rng, true)?,
        "cage" => household::cage(
            &mut d,
            &household::Cage {
                span: [-0.4, 1.5],
                heights: [0.2, 1.8],
                depth: c.depth,
                color: srgb("#7fb0c8"),
                stocked: true,
            },
            &mut rng,
            s,
            true,
        )?,
        "lamp" => household::wall_lamp(&mut d, 0.3, 0.7, s.unwrap())?,
        "meters" => household::meter_bank(&mut d, -0.4, 0.2, 3, &mut rng, s.unwrap())?,
        _ => panic!("Unknown fixture"),
    }
    Ok(rng.next_u32())
}
#[test]
fn household_survey_matches_render_and_weathered_receivers_are_disjoint_metal() {
    for kind in KINDS {
        for reflected in [false, true] {
            let c = Case {
                kind,
                depth: 0.4,
                reflected,
                settings: Some(settings()),
            };
            let mut mesh = Mesh::new(Mode::Render, 100000, &|| false).unwrap();
            let next = draw(&mut mesh, &c).unwrap();
            let triangles = mesh.triangle_count();
            assert!(triangles > 0, "{kind}");
            let mut survey = Mesh::new(Mode::Survey, 100000, &|| false).unwrap();
            assert_eq!(draw(&mut survey, &c).unwrap(), next, "{kind} survey RNG");
            assert_eq!(survey.triangle_count(), triangles, "{kind} survey");
            let plan = weathered::finish(&mut mesh, &|| false).unwrap();
            let mut covered = vec![false; triangles];
            for receiver in plan.receivers() {
                for seen in &mut covered
                    [receiver.first_triangle..receiver.first_triangle + receiver.triangle_count]
                {
                    assert!(!*seen, "{kind} receivers overlap");
                    *seen = true;
                }
                let job = &plan.jobs()[receiver.job];
                let runoff = job.runoff_recipe().map(|r| r.build(&|| false).unwrap());
                let sample_runoff = |u, v| runoff.as_ref().unwrap().sample(u, v);
                Metal::new(
                    job.metal_settings().unwrap(),
                    runoff
                        .as_ref()
                        .map(|_| &sample_runoff as &dyn Fn(f64, f64) -> Result<f64, String>),
                )
                .unwrap()
                .sample(0.5, 0.5)
                .unwrap();
            }
        }
    }
}
#[test]
fn invalid_household_admission_preserves_seed_and_rebuild_recovers() {
    let mut c = Case {
        kind: "rack",
        depth: f64::NAN,
        reflected: false,
        settings: None,
    };
    let mut mesh = Mesh::new(Mode::Render, 100000, &|| false).unwrap();
    assert!(draw(&mut mesh, &c).is_err());
    assert!(mesh.indices.is_empty());
    c.depth = 0.65;
    let next = draw(&mut mesh, &c).unwrap();
    assert!(!mesh.indices.is_empty());
    let mut fresh = Mesh::new(Mode::Render, 100000, &|| false).unwrap();
    assert_eq!(
        draw(&mut fresh, &c).unwrap(),
        next,
        "rebuild repeats the seeded draw"
    );
    let mut cancelled = Mesh::new(Mode::Render, 100000, &|| true).unwrap();
    assert!(draw(&mut cancelled, &c).is_err());
}

use serde_json::json;
use superspec_aml::rng::SeededRng;
use superspec_engine::worldspec::themes::kowloon::{
    fixture::Fixture,
    ground::{self, Kind, Style, Unit},
    kit::Kit,
    mesh::{Mesh, Mode},
    pen::Pen,
    signs::panel::Settings,
    weathered,
};
use superspec_geometry::Matrix;
struct Case {
    kind: Kind,
    unit: Unit,
    style: Style,
    seed: String,
    detail: u8,
    textured: bool,
    settings: Settings,
}
fn case(kind: Kind, detail: u8) -> Case {
    Case {
        kind,
        unit: serde_json::from_value(json!({"key": "unit", "u0": 0.1, "u1": 3.5, "height": 3.4}))
            .unwrap(),
        style: serde_json::from_value(json!({
            "frame": [0.05, 0.21, 0.19],
            "trim": [0.87, 0.84, 0.76],
            "shutter": [0.07, 0.16, 0.3],
            "accent": [0.81, 0.74, 0.53]
        }))
        .unwrap(),
        seed: format!("ground-caller/{detail}"),
        detail,
        textured: true,
        settings: serde_json::from_value(json!({"weathering":{"seed":"ground/1","wear":0.25,"grime":0.6,"rust":0.4,"textureDensity":32,"metal":{"material":"metal","repeat":1},"paint":{"material":"paint","repeat":1},"sheet":{"material":"sheet","repeat":1}},"lettering":true,"timber":{"textured":true,"timberRepeat":1,"atlasRepeat":1}})).unwrap(),
    }
}
fn pen(reflected: bool) -> Pen {
    Pen::new(
        if reflected {
            Matrix([[-1., 0., 0.], [0., -1., 0.], [0., 0., -1.], [4., 3., 5.]])
        } else {
            Matrix::default()
        },
        "ground",
    )
    .unwrap()
}
#[test]
fn ground_bay_surveys_match_render_geometry_signs_and_random_stream() {
    for kind in [
        Kind::Shutter,
        Kind::Gate,
        Kind::Door,
        Kind::Shop,
        Kind::Service,
    ] {
        for detail in 0..3 {
            for reflected in [false, true] {
                let c = case(kind, detail);
                let kit = Kit::new(c.settings.clone(), c.detail).unwrap();
                let s = ground::Settings {
                    kit: &kit,
                    style: &c.style,
                    posters: c.textured,
                };
                let label = format!("{} detail {detail} reflected {reflected}", c.seed);
                // Planning an opening is pure: it repeats for the same stream.
                let opening =
                    ground::opening(kind, &c.unit, &mut SeededRng::from_text(&c.seed)).unwrap();
                assert_eq!(
                    json!(opening),
                    json!(
                        ground::opening(kind, &c.unit, &mut SeededRng::from_text(&c.seed)).unwrap()
                    )
                );
                let mut draws = vec![];
                for mode in [Mode::Render, Mode::Survey] {
                    let mut rng = SeededRng::from_text(&c.seed);
                    let mut mesh = Mesh::new(mode, 150000, &|| false).unwrap();
                    let proposals = ground::draw(
                        &mut Fixture {
                            pen: pen(reflected),
                            mesh: &mut mesh,
                            repeats: &|_| 1.,
                        },
                        c.kind,
                        &c.unit,
                        &s,
                        &mut rng,
                    )
                    .unwrap();
                    draws.push((
                        mesh.triangle_count(),
                        rng.next_f64(),
                        proposals.iter().map(|p| p.key.clone()).collect::<Vec<_>>(),
                    ));
                }
                assert!(draws[0].0 > 0, "{label}");
                assert_eq!(draws[0], draws[1], "{label}");
            }
        }
    }
}
#[test]
fn invalid_ground_jobs_do_not_advance_the_seed_and_cancelled_jobs_recover() {
    let mut c = case(Kind::Gate, 1);
    let kit = Kit::new(c.settings.clone(), c.detail).unwrap();
    let s = ground::Settings {
        kit: &kit,
        style: &c.style,
        posters: c.textured,
    };
    let original = c.unit.clone();
    for error in 0..3 {
        c.unit = original.clone();
        match error {
            0 => c.unit.u1 = f64::INFINITY,
            1 => c.unit.height = 0.,
            _ => c.unit.key = "x".repeat(2049),
        }
        let mut rng = SeededRng::from_text(&c.seed);
        let next = rng.clone().next_f64();
        let mut mesh = Mesh::new(Mode::Render, 150000, &|| false).unwrap();
        assert!(
            ground::draw(
                &mut Fixture {
                    pen: pen(false),
                    mesh: &mut mesh,
                    repeats: &|_| 1.
                },
                c.kind,
                &c.unit,
                &s,
                &mut rng
            )
            .is_err()
        );
        assert!(mesh.indices.is_empty());
        assert_eq!(rng.next_f64(), next);
    }
    c.unit = original;
    let mut mesh = Mesh::new(Mode::Render, 150000, &|| true).unwrap();
    let mut rng = SeededRng::from_text(&c.seed);
    assert!(
        ground::draw(
            &mut Fixture {
                pen: pen(false),
                mesh: &mut mesh,
                repeats: &|_| 1.
            },
            c.kind,
            &c.unit,
            &s,
            &mut rng
        )
        .is_err()
    );
    assert!(mesh.indices.is_empty());
    let mut mesh = Mesh::new(Mode::Render, 150000, &|| false).unwrap();
    ground::draw(
        &mut Fixture {
            pen: pen(false),
            mesh: &mut mesh,
            repeats: &|_| 1.,
        },
        c.kind,
        &c.unit,
        &s,
        &mut rng,
    )
    .unwrap();
    assert!(
        !weathered::finish(&mut mesh, &|| false)
            .unwrap()
            .jobs()
            .is_empty()
    );
}

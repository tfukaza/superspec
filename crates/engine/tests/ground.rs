use serde::Deserialize;
use serde_json::{Value, json};
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
#[path = "support/assembly_reference.rs"]
mod assembly;
#[path = "support/sign_reference.rs"]
mod reference;
use assembly::{Chart, Geometry};
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Case {
    kind: Kind,
    unit: Unit,
    style: Style,
    opening: Option<Value>,
    opening_next: f64,
    seed: String,
    next: f64,
    reflected: bool,
    detail: u8,
    textured: bool,
    settings: Settings,
    proposals: Vec<Value>,
    geometry: Geometry,
    charts: Vec<Chart>,
}
#[derive(Deserialize)]
struct Golden {
    cases: Vec<Case>,
}
fn fixture() -> Golden {
    serde_json::from_str(include_str!("../../../tests/fixtures/rust/ground.json")).unwrap()
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
pub fn check_reference_ground() {
    for (i, c) in fixture().cases.into_iter().enumerate() {
        let kit = Kit::new(c.settings, c.detail).unwrap();
        let s = ground::Settings {
            kit: &kit,
            style: &c.style,
            posters: c.textured,
        };
        let mut rng = SeededRng::from_text(&c.seed);
        let opening = ground::opening(c.kind, &c.unit, &mut rng).unwrap();
        reference::close(&json!(opening), &json!(c.opening));
        assert_eq!(rng.next_f64(), c.opening_next);
        let mut rng = SeededRng::from_text(&c.seed);
        let mut mesh = Mesh::new(Mode::Render, 150000, &|| false).unwrap();
        let proposals = ground::draw(
            &mut Fixture {
                pen: pen(c.reflected),
                mesh: &mut mesh,
                repeats: &|_| 1.,
            },
            c.kind,
            &c.unit,
            &s,
            &mut rng,
        )
        .unwrap();
        assert_eq!(rng.next_f64(), c.next, "ground {i} continuation");
        assert_eq!(proposals.len(), c.proposals.len());
        for (p, e) in proposals.iter().zip(&c.proposals) {
            reference::close(&reference::metadata(p), e);
        }
        assembly::check(&mut mesh, &c.geometry, &c.charts, &format!("ground {i}"));
        let mut mesh = Mesh::new(Mode::Survey, 150000, &|| false).unwrap();
        let mut rng = SeededRng::from_text(&c.seed);
        ground::draw(
            &mut Fixture {
                pen: pen(c.reflected),
                mesh: &mut mesh,
                repeats: &|_| 1.,
            },
            c.kind,
            &c.unit,
            &s,
            &mut rng,
        )
        .unwrap();
        assert_eq!(
            mesh.triangle_count() * 3,
            c.geometry.indices.len(),
            "ground {i} survey"
        );
        assert_eq!(rng.next_f64(), c.next);
    }
}
#[test]
fn ground_bays_match_reference_geometry_finishes_signs_and_random_stream() {
    check_reference_ground()
}
#[test]
fn invalid_ground_jobs_do_not_advance_the_seed_and_cancelled_jobs_recover() {
    let mut c = fixture().cases.remove(1);
    let kit = Kit::new(c.settings, c.detail).unwrap();
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

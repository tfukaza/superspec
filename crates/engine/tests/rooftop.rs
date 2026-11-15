use serde::Deserialize;
use serde_json::Value;
use superspec_engine::worldspec::themes::kowloon::{
    fixture::Fixture,
    kit::Kit,
    mesh::{Mesh, Mode},
    metal::Binding,
    pen::Pen,
    rooftop::{self, Request},
    signs::panel::Settings,
};
use superspec_geometry::Matrix;
#[path = "support/assembly_reference.rs"]
mod assembly;
#[path = "support/sign_reference.rs"]
mod reference;
#[derive(Deserialize)]
struct Case {
    request: Request,
    reflected: bool,
    detail: u8,
    settings: Settings,
    proposals: Vec<Value>,
    geometry: assembly::Geometry,
    charts: Vec<assembly::Chart>,
}
#[derive(Deserialize)]
struct Golden {
    cases: Vec<Case>,
}
fn fixture() -> Golden {
    serde_json::from_str(include_str!("../../../tests/fixtures/rust/rooftop.json")).unwrap()
}
fn binding(slot: &str) -> Binding {
    Binding {
        material: slot.into(),
        repeat: 1.,
    }
}
fn draw(
    mesh: &mut Mesh<'_>,
    c: &Case,
) -> Result<Vec<superspec_engine::worldspec::themes::kowloon::signs::mounts::Proposal>, String> {
    let kit = Kit::new(c.settings.clone(), c.detail)?;
    rooftop::draw(
        &mut Fixture {
            pen: Pen::new(
                if c.reflected {
                    Matrix([[-1., 0., 0.], [0., -1., 0.], [0., 0., -1.], [4., 3., 5.]])
                } else {
                    Matrix::default()
                },
                "roof",
            )?,
            mesh,
            repeats: &|_| 1.,
        },
        &c.request,
        &rooftop::Settings {
            kit: &kit,
            wall: binding("wall"),
            concrete: binding("concrete"),
            brushed: binding("brushed-metal"),
            plants: true,
            cloth: true,
        },
    )
}
pub fn check_reference_rooftop() {
    for (i, c) in fixture().cases.into_iter().enumerate() {
        let mut mesh = Mesh::new(Mode::Render, 150000, &|| false).unwrap();
        let proposals = draw(&mut mesh, &c).unwrap();
        assert_eq!(proposals.len(), c.proposals.len(), "rooftop {i} signs");
        for (p, e) in proposals.iter().zip(&c.proposals) {
            reference::close(&reference::metadata(p), e);
        }
        assembly::check(&mut mesh, &c.geometry, &c.charts, &format!("rooftop {i}"));
        let mut survey = Mesh::new(Mode::Survey, 150000, &|| false).unwrap();
        draw(&mut survey, &c).unwrap();
        assert_eq!(
            survey.triangle_count() * 3,
            c.geometry.indices.len(),
            "rooftop {i} survey"
        );
    }
}
#[test]
fn complete_rooftops_preserve_reservations_geometry_and_finishes_at_every_detail() {
    check_reference_rooftop()
}
#[test]
fn invalid_and_cancelled_rooftops_fail_without_poisoning_the_next_job() {
    let mut c = fixture().cases.remove(4);
    let request = c.request.clone();
    for fault in 0..4 {
        c.request = request.clone();
        match fault {
            0 => c.request.width = f64::INFINITY,
            1 => c.request.depth = 0.,
            2 => c.request.wall_tint[1] = f64::NAN,
            _ => c.request.lot_key = "x".repeat(1025),
        }
        let mut mesh = Mesh::new(Mode::Render, 150000, &|| false).unwrap();
        assert!(draw(&mut mesh, &c).is_err());
        assert!(mesh.indices.is_empty());
    }
    c.request = request;
    let mut mesh = Mesh::new(Mode::Render, 150000, &|| true).unwrap();
    assert!(draw(&mut mesh, &c).is_err());
    assert!(mesh.indices.is_empty());
    let mut mesh = Mesh::new(Mode::Render, 150000, &|| false).unwrap();
    draw(&mut mesh, &c).unwrap();
    assert!(!mesh.indices.is_empty());
}

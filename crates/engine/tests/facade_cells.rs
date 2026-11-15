use serde::Deserialize;
use serde_json::Value;
use superspec_engine::worldspec::themes::kowloon::{
    facade_cells::{self, Cell, Kind, Role},
    fixture::Fixture,
    kit::Kit,
    mesh::{Mesh, Mode},
    palette,
    pen::Pen,
    signs::panel::Settings,
};
use superspec_geometry::Matrix;
#[path = "support/assembly_reference.rs"]
mod assembly;
#[path = "support/sign_reference.rs"]
mod reference;
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Case {
    style_seed: String,
    flat_seed: String,
    home: Value,
    opening: Value,
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
    serde_json::from_str(include_str!(
        "../../../tests/fixtures/rust/facade-cells.json"
    ))
    .unwrap()
}
#[derive(Clone, Deserialize)]
struct Request {
    kind: Kind,
    role: Role,
    story: usize,
    width: f64,
    height: f64,
    reach: f64,
    balcony: f64,
    ground: f64,
    story_height: f64,
    top: bool,
    lot_key: String,
    key: String,
    wash: [f64; 3],
}
fn draw(
    mesh: &mut Mesh<'_>,
    c: &Case,
) -> Result<Vec<superspec_engine::worldspec::themes::kowloon::signs::mounts::Proposal>, String> {
    let kit = Kit::new(c.settings.clone(), c.detail)?;
    let style = palette::lot_style(&mut superspec_aml::rng::SeededRng::from_text(&c.style_seed));
    let flat = palette::flat(
        &style,
        &mut superspec_aml::rng::SeededRng::from_text(&c.flat_seed),
    );
    reference::close(&serde_json::json!(flat), &c.home);
    let r = &c.request;
    let cell = Cell {
        kind: r.kind,
        role: r.role,
        story: r.story,
        width: r.width,
        height: r.height,
        reach: r.reach,
        balcony: r.balcony,
        ground: r.ground,
        story_height: r.story_height,
        top: r.top,
        lot_key: &r.lot_key,
        key: &r.key,
        wash: r.wash,
        style: &style,
        flat: &flat,
        kit: &kit,
        plants: true,
        cloth: true,
    };
    if let Ok(opening) = cell.opening() {
        reference::close(&serde_json::json!(opening), &c.opening);
    }
    facade_cells::draw(
        &mut Fixture {
            pen: Pen::new(
                if c.reflected {
                    Matrix([[-1., 0., 0.], [0., -1., 0.], [0., 0., -1.], [4., 3., 5.]])
                } else {
                    Matrix::default()
                },
                "envelope",
            )?,
            mesh,
            repeats: &|_| 1.,
        },
        &cell,
    )
}
pub fn check_reference_facade_cells() {
    let mut enclosures = std::collections::BTreeSet::new();
    for (i, c) in fixture().cases.into_iter().enumerate() {
        if c.request.kind == Kind::Balcony {
            enclosures.insert(if c.home["vacant"] == true {
                "vacant".to_owned()
            } else {
                c.home["enclosure"].as_str().unwrap().to_owned()
            });
        }
        let mut mesh = Mesh::new(Mode::Render, 150000, &|| false).unwrap();
        let proposals = draw(&mut mesh, &c).unwrap();
        assert_eq!(proposals.len(), c.proposals.len(), "facade_cells {i} signs");
        for (p, e) in proposals.iter().zip(&c.proposals) {
            reference::close(&reference::metadata(p), e);
        }
        assembly::check(
            &mut mesh,
            &c.geometry,
            &c.charts,
            &format!("facade_cells {i}"),
        );
        let mut survey = Mesh::new(Mode::Survey, 150000, &|| false).unwrap();
        draw(&mut survey, &c).unwrap();
        assert_eq!(
            survey.triangle_count() * 3,
            c.geometry.indices.len(),
            "facade_cells {i} survey"
        );
    }
    assert_eq!(
        enclosures,
        ["caged", "glazed", "open", "vacant"]
            .map(str::to_owned)
            .into_iter()
            .collect()
    );
}
#[test]
fn complete_facade_cells_preserve_household_geometry_and_finishes_at_every_detail() {
    check_reference_facade_cells()
}
#[test]
fn invalid_and_cancelled_facade_cells_fail_without_poisoning_the_next_job() {
    let mut c = fixture().cases.remove(4);
    let request = c.request.clone();
    for fault in 0..4 {
        c.request = request.clone();
        match fault {
            0 => c.request.width = f64::INFINITY,
            1 => c.request.height = 0.,
            2 => c.request.wash[1] = f64::NAN,
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

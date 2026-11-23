use superspec_aml::rng::SeededRng;
use superspec_engine::worldspec::themes::kowloon::{
    facade_cells::{self, Cell, Kind, Role},
    fixture::Fixture,
    kit::Kit,
    mesh::{Mesh, Mode},
    palette,
    pen::Pen,
    signs::mounts::Proposal,
};
use superspec_geometry::Matrix;
#[derive(Clone)]
struct Request {
    kind: Kind,
    width: f64,
    height: f64,
    lot_key: String,
    wash: [f64; 3],
    detail: u8,
    reflected: bool,
    /// Selects the household flat: `None` takes the plain seed, otherwise the first match.
    enclosure: Option<&'static str>,
}
fn request(kind: Kind) -> Request {
    Request {
        kind,
        width: 3.6,
        height: 3.,
        lot_key: "lot-0".into(),
        wash: [0.67, 0.78, 0.66],
        detail: 1,
        reflected: false,
        enclosure: None,
    }
}
fn draw(mesh: &mut Mesh<'_>, r: &Request) -> Result<Vec<Proposal>, String> {
    let settings = serde_json::from_value(serde_json::json!({"weathering":{"seed":"cell/1","wear":0.25,"grime":0.6,"rust":0.4,"textureDensity":32,"metal":{"material":"metal","repeat":1},"paint":{"material":"paint","repeat":1},"sheet":{"material":"sheet","repeat":1}},"lettering":true,"timber":{"textured":true,"timberRepeat":1,"atlasRepeat":1}})).unwrap();
    let kit = Kit::new(settings, r.detail)?;
    let style = palette::lot_style(&mut SeededRng::from_text("cell-style/0"));
    let mut flat = palette::flat(&style, &mut SeededRng::from_text("cell-flat/0"));
    if let Some(enclosure) = r.enclosure {
        for n in 0..1000 {
            flat = palette::flat(
                &style,
                &mut SeededRng::from_text(&format!("cell-flat-extra/{n}")),
            );
            if if enclosure == "vacant" {
                flat.vacant
            } else {
                !flat.vacant && flat.enclosure == enclosure
            } {
                break;
            }
        }
    }
    let cell = Cell {
        kind: r.kind,
        role: Role::Front,
        story: 0,
        width: r.width,
        height: r.height,
        reach: 1.3,
        balcony: 0.85,
        ground: 3.4,
        story_height: 3.,
        top: true,
        lot_key: &r.lot_key,
        key: "cell/1",
        wash: r.wash,
        style: &style,
        flat: &flat,
        kit: &kit,
        plants: true,
        cloth: true,
    };
    facade_cells::draw(
        &mut Fixture {
            pen: Pen::new(
                if r.reflected {
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
#[test]
fn facade_cell_surveys_agree_with_render_for_every_household_kind() {
    let mut cases = vec![];
    for kind in [Kind::Window, Kind::Band, Kind::Stair] {
        cases.push(request(kind));
    }
    for enclosure in ["caged", "glazed", "open", "vacant"] {
        for detail in [0, 2] {
            cases.push(Request {
                enclosure: Some(enclosure),
                detail,
                ..request(Kind::Balcony)
            });
        }
    }
    cases.push(Request {
        reflected: true,
        enclosure: Some("caged"),
        ..request(Kind::Balcony)
    });
    for (i, r) in cases.iter().enumerate() {
        let mut mesh = Mesh::new(Mode::Render, 150000, &|| false).unwrap();
        let rendered = draw(&mut mesh, r).unwrap();
        assert!(mesh.triangle_count() > 0, "cell {i}");
        let mut survey = Mesh::new(Mode::Survey, 150000, &|| false).unwrap();
        let surveyed = draw(&mut survey, r).unwrap();
        assert_eq!(survey.triangle_count(), mesh.triangle_count(), "cell {i}");
        assert_eq!(
            rendered.iter().map(|p| &p.key).collect::<Vec<_>>(),
            surveyed.iter().map(|p| &p.key).collect::<Vec<_>>(),
            "cell {i} signs"
        );
        for (corner, (actual, &index)) in survey.survey.iter().zip(&mesh.indices).enumerate() {
            for axis in 0..3 {
                assert!(
                    (actual[axis] - mesh.vertices[index as usize].position[axis]).abs() <= 2e-10,
                    "cell {i} corner {corner}"
                );
            }
        }
    }
}
#[test]
fn invalid_and_cancelled_facade_cells_fail_without_poisoning_the_next_job() {
    let request = request(Kind::Band);
    for fault in 0..4 {
        let mut r = request.clone();
        match fault {
            0 => r.width = f64::INFINITY,
            1 => r.height = 0.,
            2 => r.wash[1] = f64::NAN,
            _ => r.lot_key = "x".repeat(1025),
        }
        let mut mesh = Mesh::new(Mode::Render, 150000, &|| false).unwrap();
        assert!(draw(&mut mesh, &r).is_err());
        assert!(mesh.indices.is_empty());
    }
    let mut mesh = Mesh::new(Mode::Render, 150000, &|| true).unwrap();
    assert!(draw(&mut mesh, &request).is_err());
    assert!(mesh.indices.is_empty());
    let mut mesh = Mesh::new(Mode::Render, 150000, &|| false).unwrap();
    draw(&mut mesh, &request).unwrap();
    assert!(!mesh.indices.is_empty());
}

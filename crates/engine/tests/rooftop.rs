use superspec_engine::worldspec::themes::kowloon::{
    fixture::Fixture,
    kit::Kit,
    mesh::{Mesh, Mode},
    metal::Binding,
    pen::Pen,
    rooftop::{self, Request},
    signs::mounts::Proposal,
    signs::panel::Settings,
};
use superspec_geometry::Matrix;
struct Case {
    request: Request,
    reflected: bool,
    detail: u8,
    settings: Settings,
}
fn case(tenement: bool, detail: u8, reflected: bool) -> Case {
    Case {
        request: serde_json::from_value(serde_json::json!({"lot_key":"lot","tenement":tenement,"wall_tint":[0.89,0.82,0.69],"width":10,"depth":11,"parapet":0.65,"ceiling":4.8,"perimeter":true})).unwrap(),
        reflected,
        detail,
        settings: serde_json::from_value(serde_json::json!({"weathering":{"seed":"rooftop/1","wear":0.25,"grime":0.6,"rust":0.4,"textureDensity":32,"metal":{"material":"metal","repeat":1},"paint":{"material":"paint","repeat":1},"sheet":{"material":"sheet","repeat":1}},"lettering":true,"timber":{"textured":true,"timberRepeat":1,"atlasRepeat":1}})).unwrap(),
    }
}
fn binding(slot: &str) -> Binding {
    Binding {
        material: slot.into(),
        repeat: 1.,
    }
}
fn draw(mesh: &mut Mesh<'_>, c: &Case) -> Result<Vec<Proposal>, String> {
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
#[test]
fn rooftop_surveys_match_render_geometry_and_reservations_at_every_detail() {
    for tenement in [true, false] {
        for detail in 0..3 {
            for (reflected, perimeter, small) in [
                (false, true, false),
                (true, true, false),
                (false, false, false),
                (false, true, true),
            ] {
                let mut c = case(tenement, detail, reflected);
                c.request.perimeter = perimeter;
                if small {
                    c.request.width = 3.5;
                    c.request.depth = 3.5;
                }
                let label = format!(
                    "tenement={tenement} detail={detail} reflected={reflected} perimeter={perimeter} small={small}"
                );
                let mut mesh = Mesh::new(Mode::Render, 150000, &|| false).unwrap();
                let rendered = draw(&mut mesh, &c).unwrap();
                if tenement {
                    assert!(mesh.triangle_count() > 0, "{label}");
                }
                let mut survey = Mesh::new(Mode::Survey, 150000, &|| false).unwrap();
                let surveyed = draw(&mut survey, &c).unwrap();
                assert_eq!(survey.triangle_count(), mesh.triangle_count(), "{label}");
                assert_eq!(
                    rendered.iter().map(|p| &p.key).collect::<Vec<_>>(),
                    surveyed.iter().map(|p| &p.key).collect::<Vec<_>>(),
                    "{label} signs"
                );
            }
        }
    }
}
#[test]
fn invalid_and_cancelled_rooftops_fail_without_poisoning_the_next_job() {
    let mut c = case(true, 1, true);
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

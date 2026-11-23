use superspec_aml::rng::SeededRng;
use superspec_engine::worldspec::themes::kowloon::{
    facade_cells::Role,
    facade_envelope,
    facade_envelope::Finish,
    facade_plan,
    facade_plan::{Queries, Request},
    fixture::Fixture,
    kit::Kit,
    massing::{Lot, Rect},
    mesh::{Mesh, Mode},
    metal::Binding,
    palette,
    pen::Pen,
};
use superspec_geometry::Matrix;
fn lot() -> Lot {
    Lot {
        index: 0,
        kind: "tenement",
        x0: 0.,
        x1: 9.8,
        ground: 3.4,
        story_height: 3.,
        stories: 3,
        roof: 9.4,
        top: 10.05,
        front: 0.,
        back: 0.,
        style: palette::lot_style(&mut SeededRng::from_text("facade-style/0")),
        key: "lot-2".into(),
    }
}
fn never() -> bool {
    false
}
fn binding(id: &str) -> Binding {
    Binding {
        material: id.into(),
        repeat: 1.,
    }
}
/// Draws one planned envelope; `group` selects blocked, shallow or clear balcony reach.
fn draw(mode: Mode, group: usize) -> Mesh<'static> {
    let lot = lot();
    let ground = [Rect {
        u0: 0.,
        u1: 9.8,
        v0: 0.,
        v1: 3.4,
    }];
    let body = [Rect {
        u0: -0.1,
        u1: 10.,
        v0: 3.4,
        v1: 10.05,
    }];
    let request = Request {
        lot: &lot,
        role: Role::Front,
        key: "front-2",
        width: 9.8,
        shift: -0.2,
        body_span: [-0.1, 10.],
        ground: &ground,
        body: &body,
        shops: 0.,
    };
    let finish = Finish {
        coat: binding("wall"),
        bare: binding("concrete"),
        concrete: false,
        seed: 45,
        density: 32.,
        tiles: None,
    };
    let kit=Kit::new(serde_json::from_value(serde_json::json!({"weathering":{"seed":"facade/2","wear":0.25,"grime":0.6,"rust":0.4,"textureDensity":32,"metal":{"material":"metal","repeat":1},"sheet":{"material":"sheet","repeat":1},"paint":{"material":"paint","repeat":1}},"lettering":true,"timber":{"textured":true,"timberRepeat":1,"atlasRepeat":1}})).unwrap(),2).unwrap();
    let queries = Queries {
        reach: &|_, r, limit| {
            Ok(if group % 3 == 0 && r.u0 > 3. {
                0.
            } else if group % 3 == 1 {
                limit.min(0.4)
            } else {
                limit
            })
        },
        hollow: &|_, _, _, _| Ok(if group % 4 == 0 { 0. } else { 3.1 }),
    };
    let plan = facade_plan::plan(&request, &kit, &queries, &|| false).unwrap();
    let mut mesh = Mesh::new(mode, 400000, &never).unwrap();
    facade_envelope::draw(
        &mut Fixture {
            pen: Pen::new(Matrix::default(), "envelope").unwrap(),
            mesh: &mut mesh,
            repeats: &|_| 1.,
        },
        &request,
        &plan,
        &finish,
    )
    .unwrap();
    mesh
}
#[test]
fn facade_envelope_surveys_agree_with_rendered_walls_openings_and_recesses() {
    for group in [0, 1, 2, 5] {
        let render = draw(Mode::Render, group);
        let survey = draw(Mode::Survey, group);
        assert!(render.triangle_count() > 0);
        assert_eq!(render.triangle_count(), survey.triangle_count(), "{group}");
        assert_eq!(survey.survey.len(), render.indices.len());
        for (corner, (actual, &index)) in survey.survey.iter().zip(&render.indices).enumerate() {
            for axis in 0..3 {
                assert!(
                    (actual[axis] - render.vertices[index as usize].position[axis]).abs() <= 2e-10,
                    "group {group} corner {corner}"
                );
            }
        }
    }
}

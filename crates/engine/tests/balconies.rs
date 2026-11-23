use superspec_engine::worldspec::themes::kowloon::{
    balconies::{self, Outline, Settings},
    fixture::Fixture,
    mesh::{Mesh, Mode},
    pen::Pen,
    weathered,
};
use superspec_geometry::{Matrix, V3};
const TINT: V3 = [0.67, 0.78, 0.66];
const TRIM: V3 = [0.79, 0.76, 0.67];
struct Case {
    outline: Outline,
    settings: Settings,
    reflected: bool,
}
fn case(outline: Outline, seed: &str, reflected: bool) -> Case {
    Case {
        outline,
        settings: serde_json::from_value(serde_json::json!({"weathering":{"seed":seed,"grime":0.6,"wear":0.25,"rust":0.4,"textureDensity":32,"metal":{"material":"metal","repeat":1.5},"paint":{"material":"paint","repeat":1},"sheet":{"material":"sheet","repeat":1.5}},"wall":{"material":"wall","repeat":1.3},"concrete":{"material":"concrete","repeat":2.7}})).unwrap(),
        reflected,
    }
}
/// Square, splayed and tapered balcony outlines, plain and mirrored.
fn cases() -> Vec<Case> {
    let square = [[0., 0., 0.], [2.4, 0., 0.], [2.4, 1.1, 0.], [0., 1.1, 0.]];
    let splayed = [[0., 0., 0.], [2.4, 0., 0.], [2.7, 1.1, 0.], [-0.2, 1.1, 0.]];
    let tapered = [
        [0., 0., 0.],
        [2.4, 0., 0.],
        [2.15, 1.1, 0.],
        [0.15, 1.1, 0.],
    ];
    let mut cases = vec![];
    for reflected in [false, true] {
        for (i, outline) in [square, splayed, tapered].into_iter().enumerate() {
            cases.push(case(
                serde_json::from_value(serde_json::json!(outline)).unwrap(),
                &format!("balcony/{i}"),
                reflected,
            ));
        }
    }
    cases
}
fn repeat(slot: &str) -> f64 {
    match slot {
        "wall" => 1.3,
        "concrete" => 2.7,
        "metal" | "sheet" => 1.5,
        _ => 1.,
    }
}
fn pen(reflected: bool) -> Pen {
    Pen::new(
        if reflected {
            Matrix([[-1., 0., 0.], [0., -1., 0.], [0., 0., -1.], [4., 3., 5.]])
        } else {
            Matrix::default()
        },
        "balcony",
    )
    .unwrap()
}
fn draw(mesh: &mut Mesh<'_>, c: &Case) -> Result<(), String> {
    let mut d = Fixture {
        pen: pen(c.reflected),
        mesh,
        repeats: &|s| repeat(s),
    };
    balconies::slab(&mut d, &c.outline, -0.12, 0., TINT, "concrete")?;
    balconies::parapet(&mut d, &c.outline, 0.85, TINT, TRIM, &c.settings, "balcony")?;
    balconies::anti_climb(&mut d, 0., 0.8, 0.5, TRIM, &c.settings.weathering)
}
#[test]
fn balcony_receivers_partition_triangles_and_surveys_match_render() {
    for (i, c) in cases().iter().enumerate() {
        let mut mesh = Mesh::new(Mode::Render, 150000, &|| false).unwrap();
        draw(&mut mesh, c).unwrap();
        let triangles = mesh.triangle_count();
        assert!(triangles > 0);
        let mut survey = Mesh::new(Mode::Survey, 150000, &|| false).unwrap();
        draw(&mut survey, c).unwrap();
        assert_eq!(survey.triangle_count(), triangles, "survey {i}");
        for (corner, (actual, &index)) in survey.survey.iter().zip(&mesh.indices).enumerate() {
            for axis in 0..3 {
                assert!(
                    (actual[axis] - mesh.vertices[index as usize].position[axis]).abs() <= 2e-10,
                    "balcony {i} corner {corner}"
                );
            }
        }
        let plan = weathered::finish(&mut mesh, &|| false).unwrap();
        let mut covered = vec![false; triangles];
        for receiver in plan.receivers() {
            assert!(receiver.first_triangle + receiver.triangle_count <= triangles);
            for seen in covered
                .iter_mut()
                .skip(receiver.first_triangle)
                .take(receiver.triangle_count)
            {
                assert!(!*seen, "balcony {i}: receivers overlap");
                *seen = true;
            }
        }
        assert!(covered.iter().any(|c| *c), "balcony {i}");
    }
}
#[test]
fn invalid_balcony_outline_does_not_emit_and_next_draw_recovers() {
    let c = cases().remove(0);
    let mut mesh = Mesh::new(Mode::Render, 150000, &|| false).unwrap();
    let mut bad = c.outline;
    bad.swap(1, 2);
    let mut d = Fixture {
        pen: pen(false),
        mesh: &mut mesh,
        repeats: &|_| 1.,
    };
    assert!(balconies::slab(&mut d, &bad, -0.12, 0., TINT, "concrete").is_err());
    assert!(d.mesh.indices.is_empty());
    balconies::slab(&mut d, &c.outline, -0.12, 0., TINT, "concrete").unwrap();
    assert!(!mesh.indices.is_empty());
}

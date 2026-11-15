use superspec_engine::itemspec::{
    mesh::{Mesh, Mode},
    primitives::{PatchVertex, face, patch},
};
use superspec_geometry::Matrix;
use superspec_model::visual::VisualInput;

#[test]
fn unthemed_item_geometry_exposes_complete_model_input() {
    let mut mesh = Mesh::new(Mode::Render, 8, &|| false).unwrap();
    face(
        &mut mesh,
        Matrix::default(),
        "panel",
        "paint",
        &[[0., 0., 0.], [2., 0., 0.], [2., 0., 1.], [0., 0., 1.]],
        [0., 1., 0.],
        2.,
        None,
        None,
        &|_, _, normal| {
            assert_eq!(normal, [0., 1., 0.]);
            Ok([0.2, 0.4, 0.6])
        },
    )
    .unwrap();
    let input: &dyn VisualInput = &mesh;
    assert_eq!(input.indices().len(), 6);
    assert_eq!(input.vertices().len(), 4);
    assert_eq!(input.surfaces()[0].part, "panel");
    assert_eq!(input.surfaces()[0].slot, "paint");
    assert_eq!(input.triangle_surfaces(), [0, 0]);
    assert!(input.vertices().iter().any(|v| v.uv == [4., 2.]));
    assert!(input.vertices().iter().all(|v| v.color == [0.2, 0.4, 0.6]));
    assert!(!input.surveying());
}

#[test]
fn item_patch_survey_omits_appearance_and_render_rejects_invalid_attributes() {
    let point = |x, y| PatchVertex {
        at: [x, y, 0.],
        uv: [f64::NAN; 2],
        tint: [f64::INFINITY; 3],
    };
    let grid = [
        vec![point(0., 0.), point(1., 0.)],
        vec![point(0., 1.), point(1., 1.)],
    ];
    let mut survey = Mesh::new(Mode::Survey, 10, &|| false).unwrap();
    patch(
        &mut survey,
        Matrix::default(),
        "patch",
        "unresolved-material",
        &grid,
        false,
    )
    .unwrap();
    assert_eq!(survey.triangle_count(), 2);
    let mut render = Mesh::new(Mode::Render, 10, &|| false).unwrap();
    assert!(
        patch(
            &mut render,
            Matrix::default(),
            "patch",
            "paint",
            &grid,
            false
        )
        .is_err()
    );
    assert!(render.vertices.is_empty());
}

#[test]
fn item_primitives_reject_scaled_or_sheared_frames_before_emission() {
    let points = [[0., 0., 0.], [1., 0., 0.], [1., 1., 0.], [0., 1., 0.]];
    let point = |at| PatchVertex {
        at,
        uv: [0.; 2],
        tint: [1.; 3],
    };
    let grid = [
        vec![point(points[0]), point(points[1])],
        vec![point(points[3]), point(points[2])],
    ];
    for matrix in [
        Matrix([[2., 0., 0.], [0., 1., 0.], [0., 0., 1.], [0.; 3]]),
        Matrix([[1., 0., 0.], [0.6, 0.8, 0.], [0., 0., 1.], [0.; 3]]),
    ] {
        let mut mesh = Mesh::new(Mode::Render, 8, &|| false).unwrap();
        assert!(
            face(
                &mut mesh,
                matrix,
                "panel",
                "paint",
                &points,
                [0., 0., 1.],
                1.,
                None,
                None,
                &|_, _, _| panic!("invalid frame must fail before appearance evaluation")
            )
            .is_err()
        );
        assert!(patch(&mut mesh, matrix, "panel", "paint", &grid, false).is_err());
        assert!(mesh.vertices.is_empty());
        assert!(mesh.indices.is_empty());
        face(
            &mut mesh,
            Matrix::default(),
            "panel",
            "paint",
            &points,
            [0., 0., 1.],
            1.,
            None,
            None,
            &|_, _, _| Ok([1.; 3]),
        )
        .unwrap();
        assert_eq!(mesh.triangle_count(), 2);
    }
}

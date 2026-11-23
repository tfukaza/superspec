use superspec_engine::worldspec::themes::kowloon::{
    fixture::Fixture,
    mesh::{Mesh, Mode},
    pen::Pen,
    shop::restaurant_work_details as details,
};
use superspec_geometry::Matrix;
#[test]
fn invalid_and_cancelled_detail_work_emits_nothing() {
    let mut mesh = Mesh::new(Mode::Render, 10000, &|| false).unwrap();
    let d = &mut Fixture {
        pen: Pen::new(Matrix::default(), "details").unwrap(),
        mesh: &mut mesh,
        repeats: &|_| 1.,
    };
    assert!(details::display_cabinet(d, -1., 0.8, 1.8, 1.).is_err());
    assert!(details::counter_towel(d, f64::NAN, 0.8).is_err());
    assert_eq!(mesh.triangle_count(), 0);
    let mut mesh = Mesh::new(Mode::Render, 10000, &|| true).unwrap();
    assert!(
        details::counter_towel(
            &mut Fixture {
                pen: Pen::new(Matrix::default(), "details").unwrap(),
                mesh: &mut mesh,
                repeats: &|_| 1.
            },
            0.23,
            0.8
        )
        .is_err()
    );
}

use superspec_engine::worldspec::themes::kowloon::{
    fixture::Fixture,
    mesh::{Mesh, Mode},
    pen::Pen,
    shop::restaurant_fittings as kitchen,
};
use superspec_geometry::Matrix;
#[test]
fn invalid_and_cancelled_kitchen_work_is_bounded() {
    let mut mesh = Mesh::new(Mode::Render, 10000, &|| false).unwrap();
    let d = &mut Fixture {
        pen: Pen::new(Matrix::default(), "kitchen").unwrap(),
        mesh: &mut mesh,
        repeats: &|_| 1.,
    };
    assert!(kitchen::workbench(d, [0.1, 0.4, 0.8], 1.).is_err());
    assert!(kitchen::steel_top(d, [0.8, 0.4, 0.8], 1., 1.).is_err());
    assert!(kitchen::bowl_stack(d, [0.; 3], usize::MAX, 0.07, [1.; 3]).is_err());
    assert_eq!(mesh.triangle_count(), 0);
    let mut mesh = Mesh::new(Mode::Render, 10000, &|| true).unwrap();
    assert!(
        kitchen::sauce_tray(
            &mut Fixture {
                pen: Pen::new(Matrix::default(), "kitchen").unwrap(),
                mesh: &mut mesh,
                repeats: &|_| 1.
            },
            [0.; 3],
            1.
        )
        .is_err()
    );
}

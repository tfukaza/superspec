use superspec_engine::worldspec::themes::kowloon::{
    fixture::Fixture,
    mesh::{Mesh, Mode},
    pen::Pen,
    shop::restaurant_surfaces::{self, Settings},
};
use superspec_geometry::Matrix;
#[test]
fn kitchen_charts_reject_invalid_input_cancel_and_recover() {
    assert!(restaurant_surfaces::uv(4, 0., 0.).is_err());
    assert!(restaurant_surfaces::uv(0, f64::NAN, 0.).is_err());
    let settings = Settings {
        fields: true,
        tiles: false,
        metal_repeat: 1.,
    };
    let mut mesh = Mesh::new(Mode::Render, 10000, &|| true).unwrap();
    assert!(
        restaurant_surfaces::tiles(
            &mut Fixture {
                pen: Pen::new(Matrix::default(), "tiles").unwrap(),
                mesh: &mut mesh,
                repeats: &|_| 1.
            },
            [1., 1.],
            0.,
            317,
            false,
            settings
        )
        .is_err()
    );
    assert_eq!(mesh.triangle_count(), 0);
    let mut mesh = Mesh::new(Mode::Render, 10000, &|| false).unwrap();
    let d = &mut Fixture {
        pen: Pen::new(Matrix::default(), "tiles").unwrap(),
        mesh: &mut mesh,
        repeats: &|_| 1.,
    };
    assert!(restaurant_surfaces::tiles(d, [-1., 1.], 0., 317, false, settings).is_err());
    restaurant_surfaces::tiles(d, [1., 1.], 0., 317, false, settings).unwrap();
    assert!(mesh.triangle_count() > 0);
}

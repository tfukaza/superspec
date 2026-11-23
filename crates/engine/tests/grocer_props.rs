use superspec_engine::worldspec::themes::kowloon::{
    fixture::Fixture,
    mesh::{Mesh, Mode},
    pen::Pen,
    shop::grocer_props::{self, Group, Options},
};
use superspec_geometry::Matrix;
#[test]
fn unknown_invalid_and_cancelled_stock_rejects_then_recovers() {
    let mut mesh = Mesh::new(Mode::Render, 30000, &|| false).unwrap();
    let d = &mut Fixture {
        pen: Pen::new(Matrix::default(), "props").unwrap(),
        mesh: &mut mesh,
        repeats: &|_| 1.,
    };
    let o = Options::default();
    assert!(grocer_props::draw(d, "missing", [0.; 3], [0.2; 3], 0., &o).is_err());
    assert!(grocer_props::draw(d, "rice-sack", [0.; 3], [0.2, 0., 0.3], 0., &o).is_err());
    assert!(
        grocer_props::draw(
            d,
            "rice-sack",
            [0.; 3],
            [0.2; 3],
            0.,
            &Options { variant: 100, ..o }
        )
        .is_err()
    );
    assert!(
        grocer_props::bag_cluster(
            d,
            "rice-sack",
            [0.; 3],
            [0.5; 3],
            &Group {
                yaw: 0.,
                variant: 1,
                arrangement: 4,
                textured: true
            }
        )
        .is_err()
    );
    assert_eq!(d.mesh.triangle_count(), 0);
    let mut cancelled = Mesh::new(Mode::Render, 30000, &|| true).unwrap();
    assert!(
        grocer_props::draw(
            &mut Fixture {
                pen: d.pen.clone(),
                mesh: &mut cancelled,
                repeats: d.repeats
            },
            "rice-sack",
            [0.; 3],
            [0.2; 3],
            0.,
            &o
        )
        .is_err()
    );
    assert_eq!(cancelled.triangle_count(), 0);
    grocer_props::draw(d, "rice-sack", [0.; 3], [0.2; 3], 0., &o).unwrap();
    assert!(d.mesh.triangle_count() > 0);
}

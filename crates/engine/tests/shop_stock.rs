use superspec_aml::rng::SeededRng;
use superspec_engine::worldspec::themes::kowloon::{
    fixture::Fixture,
    mesh::{Mesh, Mode},
    pen::Pen,
    shop::{
        furniture::Shelf,
        stock::{self, Grocery, Settings},
    },
};
use superspec_geometry::Matrix;
#[test]
fn invalid_and_cancelled_stocking_preserves_rng_and_next_shelf_recovers() {
    let shelf = Shelf {
        x0: 0.,
        x1: 0.7,
        y: 0.,
        depth: 0.3,
        z: 0.5,
        clear: 0.3,
    };
    let settings = Settings {
        fill: 0.9,
        maximum_rows: 2,
        packed: true,
    };
    let mut mesh = Mesh::new(Mode::Render, 10000, &|| false).unwrap();
    let d = &mut Fixture {
        pen: Pen::new(Matrix::default(), "stock").unwrap(),
        mesh: &mut mesh,
        repeats: &|_| 1.,
    };
    let mut rng = SeededRng::from_text("stock");
    let mut expected = rng.clone();
    assert!(stock::groceries(d, &shelf, &[], &mut rng, settings, true).is_err());
    assert!(
        stock::groceries(
            d,
            &shelf,
            &[Grocery::Bottles],
            &mut rng,
            Settings {
                maximum_rows: 0,
                ..settings
            },
            true
        )
        .is_err()
    );
    assert_eq!(rng.next_f64(), expected.next_f64());
    assert_eq!(d.mesh.triangle_count(), 0);
    let mut cancelled = Mesh::new(Mode::Render, 10000, &|| true).unwrap();
    assert!(
        stock::groceries(
            &mut Fixture {
                pen: d.pen.clone(),
                mesh: &mut cancelled,
                repeats: d.repeats
            },
            &shelf,
            &[Grocery::Bottles],
            &mut rng,
            settings,
            true
        )
        .is_err()
    );
    assert_eq!(rng.next_f64(), expected.next_f64());
    stock::groceries(d, &shelf, &[Grocery::Bottles], &mut rng, settings, true).unwrap();
    assert!(d.mesh.triangle_count() > 0);
}

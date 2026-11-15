use serde::Deserialize;
use superspec_engine::worldspec::themes::kowloon::{
    fixture::Fixture,
    mesh::{Mesh, Mode},
    pen::Pen,
    shop::restaurant_surfaces::{self, Settings},
};
use superspec_geometry::{Matrix, V3};
#[path = "support/assembly_reference.rs"]
mod assembly;
#[allow(dead_code)]
#[path = "support/sign_reference.rs"]
mod reference;
#[derive(Deserialize)]
struct Case {
    size: [f64; 2],
    y: f64,
    seed: u32,
    backsplash: bool,
    textured: String,
    matrix: [V3; 4],
    geometry: assembly::Geometry,
}
#[derive(Deserialize)]
struct Golden {
    cases: Vec<Case>,
}
pub fn check_reference_restaurant_surfaces() {
    let g: Golden = serde_json::from_str(include_str!(
        "../../../tests/fixtures/rust/restaurant-surfaces.json"
    ))
    .unwrap();
    for c in g.cases {
        for mode in [Mode::Render, Mode::Survey] {
            let mut mesh = Mesh::new(mode, 100000, &|| false).unwrap();
            restaurant_surfaces::tiles(
                &mut Fixture {
                    pen: Pen::new(Matrix(c.matrix), "furniture").unwrap(),
                    mesh: &mut mesh,
                    repeats: &|_| 1.7,
                },
                c.size,
                c.y,
                c.seed,
                c.backsplash,
                Settings {
                    fields: c.textured == "fields",
                    tiles: c.textured == "tiles",
                    metal_repeat: 1.7,
                },
            )
            .unwrap();
            assert_eq!(mesh.triangle_count() * 3, c.geometry.indices.len());
            if !mesh.surveying() {
                assembly::check(
                    &mut mesh,
                    &c.geometry,
                    &[],
                    &format!("{} {}", c.seed, c.textured),
                );
            }
        }
    }
}
#[test]
fn kitchen_fields_preserve_atlas_padding_tile_wear_and_authored_overrides() {
    check_reference_restaurant_surfaces();
}
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

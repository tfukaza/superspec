use serde::Deserialize;
use superspec_engine::worldspec::themes::kowloon::{
    fixture::Fixture,
    mesh::{Mesh, Mode},
    pen::Pen,
    shop::stock_mesh,
};
use superspec_geometry::{Matrix, V3};
#[path = "support/assembly_reference.rs"]
mod assembly;
#[allow(dead_code)]
#[path = "support/sign_reference.rs"]
mod reference;
#[derive(Deserialize)]
struct Case {
    kind: String,
    index: usize,
    at: V3,
    size: V3,
    tint: V3,
    textured: bool,
    matrix: [V3; 4],
    geometry: assembly::Geometry,
}
#[derive(Deserialize)]
struct Golden {
    cases: Vec<Case>,
}
pub fn check_reference_stock_mesh() {
    let g: Golden = serde_json::from_str(include_str!(
        "../../../tests/fixtures/rust/shop-stock-mesh.json"
    ))
    .unwrap();
    for c in g.cases {
        for mode in [Mode::Render, Mode::Survey] {
            let mut mesh = Mesh::new(mode, 30000, &|| false).unwrap();
            let d = &mut Fixture {
                pen: Pen::new(Matrix(c.matrix), "furniture").unwrap(),
                mesh: &mut mesh,
                repeats: &|_| 1.7,
            };
            if c.kind == "apple" {
                stock_mesh::apple_pile(d, c.at, c.size, c.tint, c.textured)
            } else {
                stock_mesh::snack_packet(
                    d,
                    c.at,
                    [c.size[0], c.size[2]],
                    c.tint,
                    c.textured,
                    c.index,
                )
            }
            .unwrap();
            assert_eq!(mesh.triangle_count() * 3, c.geometry.indices.len());
            if !mesh.surveying() {
                assembly::check(
                    &mut mesh,
                    &c.geometry,
                    &[],
                    &format!("{} {} textured={}", c.kind, c.index, c.textured),
                );
            }
        }
    }
}
#[test]
fn baked_stock_preserves_reflected_labels_normals_atlas_variants_and_survey() {
    check_reference_stock_mesh();
}
#[test]
fn invalid_and_cancelled_stock_rejects_before_drawing_then_recovers() {
    let mut mesh = Mesh::new(Mode::Render, 30000, &|| false).unwrap();
    let d = &mut Fixture {
        pen: Pen::new(Matrix::default(), "stock").unwrap(),
        mesh: &mut mesh,
        repeats: &|_| 1.,
    };
    assert!(stock_mesh::snack_packet(d, [0.; 3], [0.1, 0.2], [1.; 3], true, 4).is_err());
    assert!(stock_mesh::apple_pile(d, [0.; 3], [0.4, 0., 0.1], [1.; 3], true).is_err());
    assert_eq!(d.mesh.triangle_count(), 0);
    let mut cancelled = Mesh::new(Mode::Render, 30000, &|| true).unwrap();
    assert!(
        stock_mesh::apple_pile(
            &mut Fixture {
                pen: d.pen.clone(),
                mesh: &mut cancelled,
                repeats: d.repeats
            },
            [0.; 3],
            [0.4, 0.3, 0.1],
            [1.; 3],
            true
        )
        .is_err()
    );
    assert_eq!(cancelled.triangle_count(), 0);
    stock_mesh::apple_pile(d, [0.; 3], [0.4, 0.3, 0.1], [1.; 3], true).unwrap();
    assert!(d.mesh.triangle_count() > 0);
}

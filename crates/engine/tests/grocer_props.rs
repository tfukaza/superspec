use serde::Deserialize;
use superspec_engine::worldspec::themes::kowloon::{
    fixture::Fixture,
    mesh::{Mesh, Mode},
    pen::Pen,
    shop::grocer_props::{self, Group, Options},
};
use superspec_geometry::{Matrix, V3};
#[path = "support/assembly_reference.rs"]
mod assembly;
#[allow(dead_code)]
#[path = "support/sign_reference.rs"]
mod reference;
#[derive(Deserialize)]
struct Case {
    name: String,
    reflected: bool,
    bag: bool,
    arrangement: usize,
    variant: usize,
    at: V3,
    requested: V3,
    yaw: f64,
    tint: V3,
    textured: bool,
    result: V3,
    matrix: [V3; 4],
    geometry: assembly::Geometry,
}
#[derive(Deserialize)]
struct Golden {
    cases: Vec<Case>,
}
pub fn check_reference_grocer_props() {
    let g: Golden = serde_json::from_str(include_str!(
        "../../../tests/fixtures/rust/grocer-props.json"
    ))
    .unwrap();
    for c in g.cases {
        for mode in [Mode::Render, Mode::Survey] {
            let mut mesh = Mesh::new(mode, 100000, &|| false).unwrap();
            let d = &mut Fixture {
                pen: Pen::new(Matrix(c.matrix), "props").unwrap(),
                mesh: &mut mesh,
                repeats: &|_| 1.7,
            };
            let size = if c.bag {
                grocer_props::bag_cluster(
                    d,
                    &c.name,
                    c.at,
                    c.requested,
                    &Group {
                        yaw: c.yaw,
                        variant: c.variant,
                        arrangement: c.arrangement,
                        textured: c.textured,
                    },
                )
                .unwrap();
                grocer_props::bag_size(&c.name, c.requested, c.reflected, c.arrangement).unwrap()
            } else {
                grocer_props::draw(
                    d,
                    &c.name,
                    c.at,
                    c.requested,
                    c.yaw,
                    &Options {
                        tint: c.tint,
                        supported: c.reflected,
                        variant: c.variant,
                        textured: c.textured,
                    },
                )
                .unwrap()
            };
            reference::close(&serde_json::json!(size), &serde_json::json!(c.result));
            assert_eq!(mesh.triangle_count() * 3, c.geometry.indices.len());
            if !mesh.surveying() {
                assembly::check(
                    &mut mesh,
                    &c.geometry,
                    &[],
                    &format!(
                        "{} bag={} arrangement={} variant={} reflected={} textured={}",
                        c.name, c.bag, c.arrangement, c.variant, c.reflected, c.textured
                    ),
                );
            }
        }
    }
}
#[test]
fn grocer_props_keep_scaled_normals_labels_supported_bases_and_touching_bag_groups() {
    check_reference_grocer_props();
}
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

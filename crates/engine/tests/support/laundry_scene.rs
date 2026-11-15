//! Three support types share the shipped cloth atlas and keep physical geometry separate.
use super::balcony_props_scene as props;
use superspec_aml::rng::SeededRng;
use superspec_engine::{
    Scene,
    worldspec::themes::kowloon::{
        fixture::Fixture,
        laundry::{self, Support},
        mesh::{Mesh, Mode},
        pen::Pen,
    },
};
use superspec_geometry::Matrix;
pub fn scene() -> Scene {
    let (mut scene, materials) = props::atlas_scene();
    let mut mesh = Mesh::new(Mode::Render, 100000, &|| false).unwrap();
    for (i, support) in [Support::Line, Support::Pole, Support::Hanger]
        .into_iter()
        .enumerate()
    {
        let mut matrix = Matrix::default();
        matrix.0[3] = [0., i as f64 * 1.7, 0.];
        let mut draw = Fixture {
            pen: Pen::new(matrix, &format!("washing-{i}")).unwrap(),
            mesh: &mut mesh,
            repeats: &|_| 1.,
        };
        let mut rng = SeededRng::from_text(&format!("row/{support:?}"));
        if support == Support::Hanger {
            laundry::hangers(
                &mut draw,
                &laundry::HangerRow {
                    span: [0., 3.],
                    z: 2.5,
                    slope: 0.03,
                    reach: 0.6,
                    drop: 1.5,
                },
                &mut rng,
                true,
            )
            .unwrap();
        } else {
            laundry::washing(
                &mut draw,
                [0., 3.],
                2.5,
                &mut rng,
                &laundry::WashingOptions {
                    fill: 1.,
                    drop: 1.5,
                    support,
                },
                true,
            )
            .unwrap();
        }
        draw.tube(
            "paint",
            &[
                [0., 0., 2.5],
                [
                    3.,
                    0.,
                    if support == Support::Hanger {
                        2.59
                    } else {
                        2.5
                    },
                ],
            ],
            if support == Support::Line {
                0.006
            } else {
                0.016
            },
            [0.4, 0.4, 0.4],
            8,
        )
        .unwrap();
    }
    let bindings = materials
        .keys()
        .map(|id| (id.clone(), id.clone()))
        .collect();
    scene
        .append_baked_visual(
            "host",
            &mesh,
            &bindings,
            &[] as &[superspec_engine::materialspec::bake::BakedMaterial],
        )
        .unwrap();
    scene
}

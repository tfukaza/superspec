//! Shipped timber and poster atlases on shared shop furniture and equipment.
use super::cells_scene;
use superspec_aml::rng::SeededRng;
use superspec_engine::{
    Asset, Scene,
    worldspec::themes::kowloon::{
        fixture::Fixture,
        mesh::{Mesh, Mode},
        palette::srgb,
        pen::Pen,
        shop::{
            fittings,
            furniture::{self, Finish},
        },
        timber,
    },
};
use superspec_geometry::Matrix;
pub fn scene() -> Scene {
    let (mut scene, mut materials) = cells_scene::assets();
    let mut poster = superspec_aml::material_catalog::material("builtin:wall-posters").unwrap();
    poster.id = "posters".into();
    materials.insert("posters".into(), poster);
    let bytes = include_bytes!("../../../../assets/materials/posters-atlas.jpg");
    scene.manifest.assets.push(Asset {
        id: "superspec-material:posters-atlas.jpg".into(),
        mime: "image/jpeg",
        byte_start: scene.asset_bytes.len(),
        byte_length: bytes.len(),
    });
    scene.asset_bytes.extend_from_slice(bytes);
    scene.manifest.asset_byte_count = scene.asset_bytes.len();
    scene.manifest.materials = materials.values().cloned().collect();
    let mut mesh = Mesh::new(Mode::Render, 100000, &|| false).unwrap();
    let mut rng = SeededRng::from_text("equipment");
    let wood = timber::Settings::default();
    for i in 0..3 {
        let mut matrix = Matrix::default();
        matrix.0[3] = [i as f64 * 2.5, 0., 0.];
        let d = &mut Fixture {
            pen: Pen::new(matrix, &format!("equipment-{i}")).unwrap(),
            mesh: &mut mesh,
            repeats: &|_| 1.,
        };
        furniture::shelving(
            d,
            [2., 0.35, 1.9],
            4,
            if i == 2 { "metal" } else { "timber" },
            srgb("#8e6747"),
            0.35,
            &wood,
        )
        .unwrap();
        let pen = d.pen.moved([0., -0.9, 0.]).unwrap();
        let bench = &mut Fixture {
            pen,
            mesh: d.mesh,
            repeats: d.repeats,
        };
        furniture::counter(
            bench,
            [1.4, 0.5, 0.9],
            [Finish::Timber, Finish::Tiled, Finish::Steel][i],
            srgb("#7a6851"),
            &mut rng,
            &wood,
        )
        .unwrap();
        fittings::counter_scale(bench, [0.35, 0.25, 0.9], srgb("#759b8a")).unwrap();
        fittings::price_card(bench, [1.05, 0.15, 0.9], &mut rng).unwrap();
        let pen = bench.pen.moved([1.55, 0., 0.]).unwrap();
        let crate_scope = &mut Fixture {
            pen,
            mesh: bench.mesh,
            repeats: bench.repeats,
        };
        if i == 1 {
            furniture::crate_box(
                crate_scope,
                [0.5, 0.4, 0.3],
                srgb("#a68454"),
                &mut rng,
                &wood,
            )
            .unwrap();
        } else {
            fittings::bottle_crate(
                crate_scope,
                srgb("#a42c29"),
                srgb("#3a6a3c"),
                srgb("#cfa343"),
                true,
            )
            .unwrap();
        }
        fittings::pendant(d, [0.4, -0.45, 2.7], 0.5, Some(srgb("#487b64"))).unwrap();
        fittings::ceiling_fan(d, [1.3, 0., 2.7], 0.28, 0.9, &mut rng).unwrap();
        fittings::tube_light(d, [0.2, 0.2, 2.4], [1.8, 0.2, 2.4]).unwrap();
        fittings::calendar(d, [0.35, -0.01, 1.7], &mut rng, true).unwrap();
        fittings::wall_clock(d, [1.6, -0.01, 1.65], &mut rng).unwrap();
        fittings::hanging_scale(d, [1.8, -0.8, 2.7], 0.7, srgb("#4a7899")).unwrap();
    }
    let bindings = materials
        .keys()
        .map(|id| (id.clone(), id.clone()))
        .collect();
    scene.append_visual("host", &mesh, &bindings).unwrap();
    scene
}

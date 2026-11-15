//! Complete hardware, mixed household content and baked aging in one delivered scene.
use super::balcony_props_scene as props;
use std::collections::BTreeMap;
use superspec_aml::{material::Material, rng::SeededRng};
use superspec_engine::{
    Scene,
    materialspec::surface::Inputs,
    worldspec::themes::kowloon::{
        fixture::Fixture,
        household,
        mesh::{Mesh, Mode},
        palette::srgb,
        pen::Pen,
        weathered,
    },
};
use superspec_geometry::Matrix;
pub fn scene() -> Scene {
    let (mut scene, mut materials) = props::atlas_scene();
    for id in ["metal", "sheet"] {
        materials.insert(
            id.into(),
            Material {
                id: id.into(),
                color: "#ffffff".into(),
                repeat: 1.5,
                roughness: 0.6,
                metalness: 0.3,
                ..Material::default()
            },
        );
    }
    scene.manifest.materials = materials.values().cloned().collect();
    let settings:weathered::Settings=serde_json::from_value(serde_json::json!({"seed":"household/7","wear":0.25,"grime":0.6,"rust":0.4,"textureDensity":64,"metal":{"material":"metal","repeat":1.5},"sheet":{"material":"sheet","repeat":1.5},"paint":{"material":"paint","repeat":1}})).unwrap();
    let mut mesh = Mesh::new(Mode::Render, 100000, &|| false).unwrap();
    for i in 0..3 {
        let mut matrix = Matrix::default();
        matrix.0[3] = [i as f64 * 2.5, 0., 0.];
        household::cage(
            &mut Fixture {
                pen: Pen::new(matrix, &format!("cage-{i}")).unwrap(),
                mesh: &mut mesh,
                repeats: &|s| materials[s].repeat,
            },
            &household::Cage {
                span: [0., 1.9],
                heights: [0., 1.6],
                depth: 0.65,
                color: srgb("#7fb0c8"),
                stocked: true,
            },
            &mut SeededRng::from_text(&format!("fixture/{}", i + 5)),
            Some(&settings),
            true,
        )
        .unwrap();
    }
    let mut matrix = Matrix::default();
    matrix.0[3] = [0., -2., 0.];
    household::drying_lines(
        &mut Fixture {
            pen: Pen::new(matrix, "drying").unwrap(),
            mesh: &mut mesh,
            repeats: &|s| materials[s].repeat,
        },
        [0., 3.5],
        0.,
        &mut SeededRng::from_text("lines"),
        Some(&settings),
        true,
    )
    .unwrap();
    let plan = weathered::finish(&mut mesh, &|| false).unwrap();
    let inputs = Inputs {
        materials: &materials,
        images: &BTreeMap::new(),
    };
    #[cfg(not(target_arch = "wasm32"))]
    let bakes = plan
        .bake_parallel(
            &inputs,
            "household",
            &weathered::ParallelBaking::default(),
            &|| false,
        )
        .unwrap();
    #[cfg(target_arch = "wasm32")]
    let bakes = plan.bake(&inputs, "household", &|| false).unwrap();
    let mut bindings: BTreeMap<_, _> = materials
        .keys()
        .map(|id| (id.clone(), id.clone()))
        .collect();
    bindings.extend(
        bakes
            .iter()
            .map(|b| (b.material().id.clone(), b.material().id.clone())),
    );
    scene
        .append_baked_visual("host", &mesh, &bindings, &bakes)
        .unwrap();
    scene
}

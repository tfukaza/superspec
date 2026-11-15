//! Actual Rust tank geometry/material baking with the shipped stainless texture channels.
use std::collections::BTreeMap;
use superspec_aml::{material::Material, rng::SeededRng};
use superspec_engine::{
    Scene, compile,
    materialspec::surface::Inputs,
    worldspec::themes::kowloon::{
        fixture::Fixture,
        mesh::{Mesh, Mode},
        pen::Pen,
        water_tanks::{self, Settings},
        weathered,
    },
};
use superspec_geometry::{Matrix, image};
#[path = "pipe_scene.rs"]
#[allow(dead_code)]
mod pipe;

pub fn settings() -> Settings {
    Settings {
        weathering: pipe::options().weathering.unwrap(),
        brushed: superspec_engine::worldspec::themes::kowloon::metal::Binding {
            material: "brushed-metal".into(),
            repeat: 1.5,
        },
    }
}
pub fn scene(round: bool) -> Scene {
    let mut scene=compile(r#"<architecture version="1" id="rust-tank"><box id="host" width="2" depth="2" height="2" visible="false"/></architecture>"#,0).unwrap();
    let mut materials = pipe::materials();
    materials.insert(
        "concrete".into(),
        Material {
            id: "concrete".into(),
            color: "#ffffff".into(),
            ..Material::default()
        },
    );
    let mut brushed = superspec_aml::material_catalog::material("builtin:brushed-metal").unwrap();
    brushed.id = "brushed-metal".into();
    materials.insert(brushed.id.clone(), brushed);
    let mut images = BTreeMap::new();
    for (name, bytes) in [
        (
            "brushed-metal.png",
            include_bytes!("../../../../assets/materials/brushed-metal.png").as_slice(),
        ),
        (
            "brushed-metal-normal.png",
            include_bytes!("../../../../assets/materials/brushed-metal-normal.png").as_slice(),
        ),
        (
            "brushed-metal-properties.png",
            include_bytes!("../../../../assets/materials/brushed-metal-properties.png").as_slice(),
        ),
    ] {
        let id = format!("superspec-material:{name}");
        images.insert(id.clone(), image::decode(bytes).unwrap());
        scene.manifest.assets.push(superspec_engine::Asset {
            id,
            mime: "image/png",
            byte_start: scene.asset_bytes.len(),
            byte_length: bytes.len(),
        });
        scene.asset_bytes.extend_from_slice(bytes);
    }
    scene.manifest.asset_byte_count = scene.asset_bytes.len();
    scene.manifest.materials.extend(materials.values().cloned());
    let mut mesh = Mesh::new(Mode::Render, 50000, &|| false).unwrap();
    let pen = Pen::new(Matrix::default(), "tank").unwrap();
    let mut drawing = Fixture {
        pen,
        mesh: &mut mesh,
        repeats: &|slot| if slot == "brushed-metal" { 1.5 } else { 1. },
    };
    let mut rng = SeededRng::from_text("hardware/7");
    let settings = settings();
    if round {
        water_tanks::round_tank(&mut drawing, [0.; 3], &mut rng, &settings)
    } else {
        water_tanks::box_tank(&mut drawing, [0.; 3], &mut rng, &settings)
    }
    .unwrap();
    let plan = weathered::finish(&mut mesh, &|| false).unwrap();
    let inputs = Inputs {
        materials: &materials,
        images: &images,
    };
    #[cfg(not(target_arch = "wasm32"))]
    let bakes = plan
        .bake_parallel(
            &inputs,
            "tank",
            &weathered::ParallelBaking::default(),
            &|| false,
        )
        .unwrap();
    #[cfg(target_arch = "wasm32")]
    let bakes = plan.bake(&inputs, "tank", &|| false).unwrap();
    let mut bindings: BTreeMap<String, String> = materials
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

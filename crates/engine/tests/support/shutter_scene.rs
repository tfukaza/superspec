//! Complete procedural metal fixture generated and baked in Rust for real renderer inspection.
use std::collections::BTreeMap;
use superspec_aml::material::Material;
use superspec_engine::{
    Scene, compile,
    materialspec::surface::Inputs,
    worldspec::themes::kowloon::{
        mesh::{Mesh, Mode},
        metal::Binding,
        pen::Pen,
        shutters::roller_shutter,
        weathered::{self, Settings},
    },
};
use superspec_geometry::Matrix;

pub fn scene() -> Scene {
    let mut scene=compile(r#"<architecture version="1" id="rust-shutter"><box id="host" width="1.2" depth=".1" height="1.87" visible="false"/></architecture>"#,0).unwrap();
    let materials = BTreeMap::from([
        (
            "steel".into(),
            Material {
                id: "steel".into(),
                color: "#a4a9aa".into(),
                roughness: 0.55,
                metalness: 0.9,
                ..Material::default()
            },
        ),
        (
            "paint".into(),
            Material {
                id: "paint".into(),
                color: "#ffffff".into(),
                roughness: 0.85,
                ..Material::default()
            },
        ),
    ]);
    let settings = Settings {
        seed: "rust/shutter".into(),
        wear: 0.6,
        grime: 0.7,
        rust: 0.4,
        texture_density: 128.,
        metal: Binding {
            material: "steel".into(),
            repeat: 3.,
        },
        sheet: Binding {
            material: "steel".into(),
            repeat: 3.,
        },
        paint: Binding {
            material: "paint".into(),
            repeat: 5.,
        },
    };
    let mut drawing = Mesh::new(Mode::Render, 10000, &|| false).unwrap();
    let pen = Pen::new(
        Matrix([[1., 0., 0.], [0., -1., 0.], [0., 0., 1.], [0.; 3]]),
        "shutter",
    )
    .unwrap();
    roller_shutter(
        &pen,
        &mut drawing,
        [0., 1.2, 0.3, 1.65],
        [0.3, 0.5, 0.7],
        &settings,
    )
    .unwrap();
    let plan = weathered::finish(&mut drawing, &|| false).unwrap();
    let bakes = plan
        .bake(
            &Inputs {
                materials: &materials,
                images: &BTreeMap::new(),
            },
            "shutter",
            &|| false,
        )
        .unwrap();
    let mut bindings: BTreeMap<String, String> = bakes
        .iter()
        .map(|b| (b.material().id.clone(), b.material().id.clone()))
        .collect();
    bindings.insert("paint".into(), "paint".into());
    scene.manifest.materials.push(materials["paint"].clone());
    scene
        .append_baked_visual("host", &drawing, &bindings, &bakes)
        .unwrap();
    scene
}

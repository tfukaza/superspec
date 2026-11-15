//! Real atlas and baked support maps for rendered HVAC fixture acceptance.
use std::collections::BTreeMap;
use superspec_aml::{material::Material, rng::SeededRng};
use superspec_engine::{
    Scene, compile,
    materialspec::surface::Inputs,
    worldspec::themes::kowloon::{
        aircon,
        fixture::Fixture,
        mesh::{Mesh, Mode},
        pen::Pen,
        weathered,
    },
};
use superspec_geometry::Matrix;
#[path = "pipe_scene.rs"]
#[allow(dead_code)]
mod pipe;
pub fn scene() -> Scene {
    let mut scene=compile(r#"<architecture version="1" id="rust-aircon"><box id="host" width="3" depth=".1" height="3" visible="false"/></architecture>"#,0).unwrap();
    let mut materials = pipe::materials();
    materials.insert(
        "sheet".into(),
        Material {
            id: "sheet".into(),
            color: "#ffffff".into(),
            roughness: 0.7,
            metalness: 0.5,
            ..Material::default()
        },
    );
    let mut atlas = superspec_aml::material_catalog::material("builtin:aircon").unwrap();
    atlas.id = "aircon".into();
    materials.insert(atlas.id.clone(), atlas);
    let bytes = include_bytes!("../../../../assets/materials/aircon.png");
    scene.manifest.assets.push(superspec_engine::Asset {
        id: "superspec-material:aircon.png".into(),
        mime: "image/png",
        byte_start: 0,
        byte_length: bytes.len(),
    });
    scene.asset_bytes.extend_from_slice(bytes);
    scene.manifest.asset_byte_count = scene.asset_bytes.len();
    scene.manifest.materials.extend(materials.values().cloned());
    let mut mesh = Mesh::new(Mode::Render, 50000, &|| false).unwrap();
    let pen = Pen::new(
        Matrix([[1., 0., 0.], [0., -1., 0.], [0., 0., 1.], [0.; 3]]),
        "window-unit",
    )
    .unwrap();
    let mut settings = pipe::options().weathering.unwrap();
    settings.sheet.material = "sheet".into();
    aircon::air_conditioner(
        &mut Fixture {
            pen: pen.clone(),
            mesh: &mut mesh,
            repeats: &|_| 1.,
        },
        0.5,
        1.4,
        0.7,
        &mut SeededRng::from_text("aircon/2"),
        Some(&settings),
    )
    .unwrap();
    aircon::condenser(
        &mut Fixture {
            pen: pen.into_part("condenser").unwrap(),
            mesh: &mut mesh,
            repeats: &|_| 1.,
        },
        [1.1, 2.5, 0.8, 1.6],
        &mut SeededRng::from_text("aircon/0"),
        Some(&settings),
    )
    .unwrap();
    let plan = weathered::finish(&mut mesh, &|| false).unwrap();
    let bakes = plan
        .bake(
            &Inputs {
                materials: &materials,
                images: &BTreeMap::new(),
            },
            "aircon",
            &|| false,
        )
        .unwrap();
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

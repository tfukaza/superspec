//! Shared canvas PBR maps on fitted cloth, with independently baked supporting metal.
use std::collections::BTreeMap;
use superspec_aml::material::Material;
use superspec_engine::{
    Asset, Scene, compile,
    materialspec::surface::Inputs,
    worldspec::themes::kowloon::{
        fixture::Fixture,
        mesh::{Mesh, Mode},
        palette::srgb,
        pen::Pen,
        tarps::{self, Envelope, Pattern},
        weathered,
    },
};
use superspec_geometry::Matrix;
pub fn scene() -> Scene {
    let mut scene=compile(r#"<architecture version="1" id="rust-tarps"><box id="host" width="10" depth=".1" height="3" visible="false"/></architecture>"#,0).unwrap();
    let mut materials: BTreeMap<String, Material> = ["metal", "sheet", "paint"]
        .into_iter()
        .map(|id| {
            (
                id.into(),
                Material {
                    id: id.into(),
                    color: "#ffffff".into(),
                    repeat: 1.5,
                    roughness: 0.6,
                    metalness: 0.3,
                    ..Material::default()
                },
            )
        })
        .collect();
    let mut tarp = superspec_aml::material_catalog::material("builtin:tarp").unwrap();
    tarp.id = "tarp".into();
    materials.insert("tarp".into(), tarp);
    for (name, bytes) in [
        (
            "tarp.png",
            include_bytes!("../../../../assets/materials/tarp.png").as_slice(),
        ),
        (
            "tarp-normal.png",
            include_bytes!("../../../../assets/materials/tarp-normal.png").as_slice(),
        ),
        (
            "tarp-properties.png",
            include_bytes!("../../../../assets/materials/tarp-properties.png").as_slice(),
        ),
    ] {
        scene.manifest.assets.push(Asset {
            id: format!("superspec-material:{name}"),
            mime: "image/png",
            byte_start: scene.asset_bytes.len(),
            byte_length: bytes.len(),
        });
        scene.asset_bytes.extend_from_slice(bytes);
    }
    scene.manifest.asset_byte_count = scene.asset_bytes.len();
    scene.manifest.materials = materials.values().cloned().collect();
    let settings:weathered::Settings=serde_json::from_value(serde_json::json!({"seed":"awning/1","wear":0.25,"grime":0.6,"rust":0.4,"textureDensity":64,"metal":{"material":"metal","repeat":1.5},"sheet":{"material":"sheet","repeat":1.5},"paint":{"material":"paint","repeat":1.5}})).unwrap();
    let mut mesh = Mesh::new(Mode::Render, 150000, &|| false).unwrap();
    for i in 0..3 {
        let mut matrix = Matrix::default();
        matrix.0[3] = [i as f64 * 3.6, 0., 0.];
        tarps::canvas(
            &mut Fixture {
                pen: Pen::new(matrix, &format!("canvas-{i}")).unwrap(),
                mesh: &mut mesh,
                repeats: &|s| materials[s].repeat,
            },
            Envelope {
                x0: 0.,
                x1: 3.2,
                z: 2.5,
                depth: if i == 2 { 0.65 } else { 1.3 },
                drop: 0.35,
                valance: if i == 0 { 0. } else { 0.28 },
            },
            srgb("#3f7f5f"),
            srgb("#e8dfc0"),
            &format!("canvas/{i}"),
            Some(&settings),
            if i == 0 {
                None
            } else {
                Some(Pattern {
                    color: srgb("#d8a030"),
                    width: 0.24,
                })
            },
        )
        .unwrap();
    }
    let plan = weathered::finish(&mut mesh, &|| false).unwrap();
    let inputs = Inputs {
        materials: &materials,
        images: &BTreeMap::new(),
    };
    #[cfg(not(target_arch = "wasm32"))]
    let bakes = plan
        .bake_parallel(
            &inputs,
            "tarps",
            &weathered::ParallelBaking::default(),
            &|| false,
        )
        .unwrap();
    #[cfg(target_arch = "wasm32")]
    let bakes = plan.bake(&inputs, "tarps", &|| false).unwrap();
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

//! Complete roof access through real procedural receivers, shared baking and LSP delivery.
use std::collections::BTreeMap;
use superspec_aml::material::Material;
use superspec_engine::{
    Scene, compile,
    materialspec::surface::Inputs,
    worldspec::themes::kowloon::{
        fixture::Fixture,
        mesh::{Mesh, Mode},
        pen::Pen,
        roof_access, weathered,
    },
};
use superspec_geometry::Matrix;
pub fn scene() -> Scene {
    let settings:roof_access::Settings=serde_json::from_value(serde_json::json!({
        "weathering":{"seed":"roof/2","wear":0.25,"grime":0.6,"rust":0.4,"textureDensity":128,
            "metal":{"material":"metal","repeat":1.5},"sheet":{"material":"sheet","repeat":1.5},"paint":{"material":"paint","repeat":1}},
        "wall":{"material":"wall","repeat":1.3},"concrete":{"material":"concrete","repeat":2.7},
        "wall_tint":[0.67,0.77,0.66],"lot_key":"lot"
    })).unwrap();
    let materials: BTreeMap<String, Material> = [
        ("wall", 1.3, 0.9, 0.),
        ("concrete", 2.7, 0.95, 0.),
        ("paint", 1., 0.75, 0.),
        ("metal", 1.5, 0.65, 0.3),
        ("sheet", 1.5, 0.65, 0.3),
        ("pane", 1., 0.85, 0.),
    ]
    .into_iter()
    .map(|(id, repeat, roughness, metalness)| {
        (
            id.into(),
            Material {
                id: id.into(),
                color: "#ffffff".into(),
                repeat,
                roughness,
                metalness,
                ..Material::default()
            },
        )
    })
    .collect();
    let mut mesh = Mesh::new(Mode::Render, 100000, &|| false).unwrap();
    roof_access::stair_hut(
        &mut Fixture {
            pen: Pen::new(Matrix::default(), "roof-access").unwrap(),
            mesh: &mut mesh,
            repeats: &|slot| materials[slot].repeat,
        },
        roof_access::Plot {
            x0: 0.,
            x1: 2.4,
            y0: 0.,
            y1: 2.8,
        },
        2.3,
        &settings,
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
            "roof-access",
            &weathered::ParallelBaking::default(),
            &|| false,
        )
        .unwrap();
    #[cfg(target_arch = "wasm32")]
    let bakes = plan.bake(&inputs, "roof-access", &|| false).unwrap();
    let mut scene=compile(r#"<architecture version="1" id="rust-roof-access"><box id="host" width="2.4" depth="2.8" height="2.445" visible="false"/></architecture>"#,0).unwrap();
    for material in materials.values() {
        scene.manifest.materials.push(material.clone());
    }
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

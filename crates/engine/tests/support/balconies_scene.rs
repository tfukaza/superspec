//! Mitered balcony construction and wall deposits delivered through the shared scene packet.
use std::collections::BTreeMap;
use superspec_aml::material::Material;
use superspec_engine::{
    Scene, compile,
    materialspec::surface::Inputs,
    worldspec::themes::kowloon::{
        balconies::{self, Settings},
        fixture::Fixture,
        mesh::{Mesh, Mode},
        palette::srgb,
        pen::Pen,
        weathered,
    },
};
use superspec_geometry::Matrix;
pub fn scene() -> Scene {
    let mut scene=compile(r#"<architecture version="1" id="rust-balconies"><box id="host" width="6" depth="1.1" height="1" visible="false"/></architecture>"#,0).unwrap();
    let materials: BTreeMap<String, Material> = [
        ("wall", 1.3),
        ("concrete", 2.7),
        ("metal", 1.5),
        ("sheet", 1.5),
        ("paint", 1.),
    ]
    .into_iter()
    .map(|(id, repeat)| {
        (
            id.into(),
            Material {
                id: id.into(),
                color: "#ffffff".into(),
                repeat,
                roughness: 0.8,
                metalness: if id == "metal" { 0.3 } else { 0. },
                ..Material::default()
            },
        )
    })
    .collect();
    let settings:Settings=serde_json::from_value(serde_json::json!({"weathering":{"seed":"balcony/1","wear":0.25,"grime":0.6,"rust":0.4,"textureDensity":64,"metal":{"material":"metal","repeat":1.5},"sheet":{"material":"sheet","repeat":1.5},"paint":{"material":"paint","repeat":1}},"wall":{"material":"wall","repeat":1.3},"concrete":{"material":"concrete","repeat":2.7}})).unwrap();
    let mut mesh = Mesh::new(Mode::Render, 150000, &|| false).unwrap();
    for i in 0..2 {
        let mut matrix = Matrix::default();
        matrix.0[3] = [i as f64 * 3.2, 0., 0.];
        let mut d = Fixture {
            pen: Pen::new(matrix, &format!("balcony-{i}")).unwrap(),
            mesh: &mut mesh,
            repeats: &|s| materials[s].repeat,
        };
        let outline = if i == 0 {
            [[0., 0., 0.], [2.4, 0., 0.], [2.4, 1.1, 0.], [0., 1.1, 0.]]
        } else {
            [[0., 0., 0.], [2.4, 0., 0.], [2.7, 1.1, 0.], [-0.2, 1.1, 0.]]
        };
        let color = srgb("#d6e4d4");
        let trim = srgb("#e6e2d6");
        balconies::slab(&mut d, &outline, -0.12, 0., color, "concrete").unwrap();
        balconies::parapet(
            &mut d,
            &outline,
            0.85,
            color,
            trim,
            &settings,
            &format!("balcony-{i}"),
        )
        .unwrap();
        balconies::anti_climb(&mut d, 0., 0.8, 0.5, trim, &settings.weathering).unwrap();
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
            "balconies",
            &weathered::ParallelBaking::default(),
            &|| false,
        )
        .unwrap();
    #[cfg(target_arch = "wasm32")]
    let bakes = plan.bake(&inputs, "balconies", &|| false).unwrap();
    scene.manifest.materials = materials.values().cloned().collect();
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

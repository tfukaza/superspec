//! Three ceramic panels: new, worn, and staggered; each remains one flat quad.
use std::collections::BTreeMap;
use superspec_aml::material::Material;
use superspec_engine::{
    Scene, compile,
    materialspec::surface::Inputs,
    worldspec::themes::kowloon::{
        fixture::Fixture,
        mesh::{Mesh, Mode},
        pen::Pen,
        tile, weathered,
    },
};
use superspec_geometry::Matrix;
pub fn scene() -> Scene {
    let materials = BTreeMap::from([
        (
            "ceramic".into(),
            Material {
                id: "ceramic".into(),
                color: "#b4c4a2".into(),
                roughness: 0.65,
                ..Material::default()
            },
        ),
        (
            "bed".into(),
            Material {
                id: "bed".into(),
                color: "#737373".into(),
                roughness: 0.9,
                ..Material::default()
            },
        ),
        (
            "grime".into(),
            Material {
                id: "grime".into(),
                color: "#725139".into(),
                roughness: 1.,
                ..Material::default()
            },
        ),
    ]);
    let mut mesh = Mesh::new(Mode::Render, 100, &|| false).unwrap();
    for i in 0..3 {
        let recipe = tile::Recipe {
            bounds: [0., 0., 1.5, 2.4],
            density: 128.,
            bed_tint: [1.; 3],
            tiles: tile::Settings {
                seed: if i == 2 { 1 } else { 22 },
                share: 1.,
                wear: if i == 0 { 0. } else { 0.8 },
                grime: if i == 0 { 0. } else { 0.6 },
                size: Some(0.18),
                aspect: Some(if i == 2 { 2. } else { 1. }),
            },
            materials: tile::Materials {
                ceramic: "ceramic".into(),
                bed: "bed".into(),
                grime: "grime".into(),
                repeat: 1.,
                bed_repeat: 2.4,
                bed_roughness: 0.9,
                roughness: 0.65,
            },
        };
        tile::panel(
            &mut Fixture {
                pen: Pen::new(
                    Matrix([
                        [1., 0., 0.],
                        [0., -1., 0.],
                        [0., 0., 1.],
                        [i as f64 * 1.6, 0., 0.],
                    ]),
                    &format!("tile-{i}"),
                )
                .unwrap(),
                mesh: &mut mesh,
                repeats: &|_| 1.,
            },
            "ceramic",
            &recipe,
            "tiles",
        )
        .unwrap();
    }
    assert_eq!(mesh.triangle_count(), 6);
    let plan = weathered::finish(&mut mesh, &|| false).unwrap();
    assert_eq!(plan.jobs().len(), 3);
    let bakes = plan
        .bake(
            &Inputs {
                materials: &materials,
                images: &BTreeMap::new(),
            },
            "tiles",
            &|| false,
        )
        .unwrap();
    let mut scene=compile(r#"<architecture version="1" id="rust-tiles"><box id="host" width="5" depth=".1" height="2.4" visible="false"/></architecture>"#,0).unwrap();
    let bindings = bakes
        .iter()
        .map(|b| (b.material().id.clone(), b.material().id.clone()))
        .collect();
    scene
        .append_baked_visual("host", &mesh, &bindings, &bakes)
        .unwrap();
    scene
}

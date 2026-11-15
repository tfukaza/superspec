//! Two adjacent receivers sharing one seamless intrinsic wall material.
use std::collections::BTreeMap;
use superspec_aml::material::Material;
use superspec_engine::{
    Scene, compile,
    materialspec::surface::Inputs,
    worldspec::themes::kowloon::{
        facade_material::{self, Coat, PERIOD, Settings},
        fixture::Fixture,
        mesh::{Mesh, Mode},
        metal::Binding,
        pen::Pen,
        weathered,
    },
};
use superspec_geometry::Matrix;
pub fn scene(concrete: bool) -> Scene {
    let materials = BTreeMap::from([
        (
            "coat".into(),
            Material {
                id: "coat".into(),
                color: "#caaf92".into(),
                roughness: 0.8,
                ..Material::default()
            },
        ),
        (
            "backing".into(),
            Material {
                id: "backing".into(),
                color: "#807a70".into(),
                roughness: 0.95,
                metalness: 0.05,
                ..Material::default()
            },
        ),
    ]);
    let settings = Settings {
        concrete,
        coat: Coat {
            material: "coat".into(),
            repeat: 1.3,
            tint: [0.7, 0.9, 0.8],
        },
        backing: Binding {
            material: "backing".into(),
            repeat: 2.7,
        },
        wear: 0.8,
        density: 128.,
        variant: 0,
    };
    let mut mesh = Mesh::new(Mode::Render, 100, &|| false).unwrap();
    let root = Pen::new(
        Matrix([[1., 0., 0.], [0., -1., 0.], [0., 0., 1.], [0.; 3]]),
        "wall",
    )
    .unwrap();
    for i in 0..2 {
        facade_material::panel(
            &mut Fixture {
                pen: root.into_part(&format!("wall-{i}")).unwrap(),
                mesh: &mut mesh,
                repeats: &|_| 1.,
            },
            "wall",
            [i as f64 * PERIOD, 0., (i + 1) as f64 * PERIOD, PERIOD],
            0.,
            &settings,
            "shared",
        )
        .unwrap();
    }
    let plan = weathered::finish(&mut mesh, &|| false).unwrap();
    assert_eq!(plan.jobs().len(), 1);
    assert_eq!(plan.receivers().len(), 2);
    let bakes = plan
        .bake(
            &Inputs {
                materials: &materials,
                images: &BTreeMap::new(),
            },
            "facade",
            &|| false,
        )
        .unwrap();
    let mut scene=compile(r#"<architecture version="1" id="rust-facade"><box id="host" width="10" depth=".1" height="5" visible="false"/></architecture>"#,0).unwrap();
    let bindings = bakes
        .iter()
        .map(|b| (b.material().id.clone(), b.material().id.clone()))
        .collect();
    scene
        .append_baked_visual("host", &mesh, &bindings, &bakes)
        .unwrap();
    scene
}

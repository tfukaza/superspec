//! Rust wall skin with window cut-outs, local runoff and a changed ground-floor wash.
use std::collections::BTreeMap;
use superspec_aml::material::Material;
use superspec_engine::{
    Scene, compile,
    materialspec::surface::Inputs,
    worldspec::themes::kowloon::{
        fixture::Fixture,
        mesh::{Mesh, Mode},
        pen::Pen,
        skin, weathered,
    },
};
use superspec_geometry::Matrix;
pub fn scene(tiled: bool) -> Scene {
    build(tiled, false)
}
pub fn stained_scene(tiled: bool) -> Scene {
    build(tiled, true)
}
fn build(tiled: bool, stained: bool) -> Scene {
    let mut settings: skin::Settings = serde_json::from_value(serde_json::json!({
        "rects":[{"u0":-0.4,"u1":5.1,"v0":0,"v1":3.4}],
        "openings":[{"u0":1,"u1":2,"v0":1.25,"v1":2.65},{"u0":3,"u1":4,"v0":1.25,"v1":2.65}],
        "u_marks":[1,2,3,4,2.8],"z_marks":[1.25,2.65],
        "finish":{"concrete":false,"coat":{"material":"wall","repeat":1.3,"tint":[0.7,0.9,0.8]},
            "backing":{"material":"concrete","repeat":2.7},"wear":0.65,"density":128,"variant":1},
        "condition":{"base":0,"ledges":[{"u0":-0.4,"u1":5.1,"z":3.4,"flow":0.65},
            {"u0":1,"u1":2,"z":1.25,"flow":1},{"u0":3,"u1":4,"z":1.25,"flow":0.8}],
            "openings":[{"u0":1,"u1":2,"z0":1.25,"z1":2.65},{"u0":3,"u1":4,"z0":1.25,"z1":2.65}],
            "grime":0.7,"wear":0.65,"seed":123},
        "washes":[{"bounds":[2.8,0,5.1,3.4],"tint":[0.9,0.6,0.5]}]
    }))
    .unwrap();
    let mut materials = BTreeMap::from([
        (
            "wall".into(),
            Material {
                id: "wall".into(),
                color: "#caaf92".into(),
                roughness: 0.8,
                ..Material::default()
            },
        ),
        (
            "concrete".into(),
            Material {
                id: "concrete".into(),
                color: "#807a70".into(),
                roughness: 0.95,
                ..Material::default()
            },
        ),
    ]);
    if tiled {
        settings.finish.density = 96.;
        settings.tiles=Some(serde_json::from_value(serde_json::json!({"seed":123,
            "field":{"seed":22,"share":1,"size":0.18,"aspect":1,"wear":0.7,"grime":0.6},
            "materials":{"ceramic":"ceramic","bed":"bed","grime":"grime","repeat":1,"bedRepeat":2.4,"bedRoughness":0.9,"roughness":0.65},
            "repair":{"material":"repair","repeat":1}
        })).unwrap());
        for (id, color, roughness) in [
            ("ceramic", "#b4c4a2", 0.65),
            ("bed", "#737373", 0.9),
            ("grime", "#725139", 1.),
            ("repair", "#aaaaaa", 0.82),
        ] {
            materials.insert(
                id.into(),
                Material {
                    id: id.into(),
                    color: color.into(),
                    roughness,
                    ..Material::default()
                },
            );
        }
    }
    let mut mesh = Mesh::new(Mode::Render, 100000, &|| false).unwrap();
    skin::draw(
        &mut Fixture {
            pen: Pen::new(
                Matrix([[1., 0., 0.], [0., -1., 0.], [0., 0., 1.], [0.; 3]]),
                "skin",
            )
            .unwrap(),
            mesh: &mut mesh,
            repeats: &|_| 1.,
        },
        "wall",
        &settings,
        "skin",
    )
    .unwrap();
    if stained {
        let pen = Pen::new(
            Matrix([[1., 0., 0.], [0., -1., 0.], [0., 0., 1.], [0.; 3]]),
            "skin",
        )
        .unwrap();
        for bounds in [[0.1, 0.2, 0.9, 3.3], [2.2, 0.1, 2.8, 3.3]] {
            superspec_engine::worldspec::themes::kowloon::stains::stain(
                &mut mesh,
                &pen,
                bounds,
                [0.7, 0.4, 0.2],
                0.9,
            )
            .unwrap();
        }
    }
    let plan = weathered::finish(&mut mesh, &|| false).unwrap();
    assert_eq!(plan.jobs().len(), if tiled { 1 } else { 4 });
    let bakes = plan
        .bake(
            &Inputs {
                materials: &materials,
                images: &BTreeMap::new(),
            },
            "skin",
            &|| false,
        )
        .unwrap();
    let mut scene=compile(r#"<architecture version="1" id="rust-skin"><box id="host" width="6" depth=".1" height="3.4" visible="false"/></architecture>"#,0).unwrap();
    let bindings = bakes
        .iter()
        .map(|b| (b.material().id.clone(), b.material().id.clone()))
        .collect();
    scene
        .append_baked_visual("host", &mesh, &bindings, &bakes)
        .unwrap();
    scene
}

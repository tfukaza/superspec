//! Full Rust raster-to-scene delivery fixture; not the courtyard acceptance scene.
use std::collections::BTreeMap;
use superspec_aml::material::Material;
use superspec_engine::{
    Scene, compile,
    materialspec::{
        bake::BakedMaterial,
        surface::{Inputs, RasterSettings, rasterize},
    },
    worldspec::themes::kowloon::{
        mesh::{Mesh, Mode},
        metal::{Binding, Kind, Metal, Settings},
        pen::{Pen, Tint},
    },
};
use superspec_geometry::Matrix;

pub fn scene() -> Scene {
    let mut scene=compile(r#"<architecture version="1" id="baked-metal"><box id="host" width="1" depth="0.1" height="0.6" visible="false"/></architecture>"#,0).unwrap();
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
        kind: Kind::Frame,
        bounds: [0., 0., 1., 0.6],
        paint: [0.3, 0.5, 0.7],
        seed: 37.,
        wear: 0.6,
        grime: 0.7,
        rust: 0.4,
        paint_coverage: None,
        edge_axes: None,
        contacts: None,
        metric: None,
        folds: vec![],
        wrap_u: None,
        substrate: Binding {
            material: "steel".into(),
            repeat: 3.,
        },
        coating: Binding {
            material: "paint".into(),
            repeat: 5.,
        },
    };
    let metal = Metal::new(&settings, None).unwrap();
    let raster = rasterize(
        RasterSettings {
            bounds: settings.bounds,
            texels_per_meter: 256.,
            preserve: true,
            repeat: false,
            uv_coordinates: true,
            bump: true,
        },
        &|u, v| metal.sample(u, v),
        &Inputs {
            materials: &materials,
            images: &BTreeMap::new(),
        },
        16 * 1024 * 1024,
        &|| false,
    )
    .unwrap();
    let bake =
        BakedMaterial::encode("fixture-metal", raster, &materials["steel"], &|| false).unwrap();
    let mut drawing = Mesh::new(Mode::Render, 100, &|| false).unwrap();
    let pen = Pen::new(
        Matrix([[1., 0., 0.], [0., -1., 0.], [0., 0., 1.], [0., 0., 0.]]),
        "panel",
    )
    .unwrap();
    pen.face(
        &mut drawing,
        "metal",
        &[[0., 0., 0.], [1., 0., 0.], [1., 0., 0.6], [0., 0., 0.6]],
        [0., 1., 0.],
        &Tint::Color([1.; 3]),
        1.,
        Some(&[[0., 0.], [1., 0.], [1., 0.6], [0., 0.6]]),
        None,
    )
    .unwrap();
    bake.map_uvs(&mut drawing, "metal").unwrap();
    let bindings = BTreeMap::from([("metal".into(), bake.material().id.clone())]);
    scene
        .append_baked_visual("host", &drawing, &bindings, &[bake])
        .unwrap();
    scene
}

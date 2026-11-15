//! Roof equipment, real atlas companions and baked finish receivers on a deck.
use super::balcony_props_scene as props;
use std::collections::BTreeMap;
use superspec_aml::material::Material;
use superspec_engine::{
    Scene,
    materialspec::surface::Inputs,
    worldspec::themes::kowloon::{
        fixture::Fixture,
        kit::Kit,
        mesh::{Mesh, Mode},
        metal::Binding,
        pen::Pen,
        rooftop, weathered,
    },
};
use superspec_geometry::Matrix;
pub fn scene() -> Scene {
    let (mut scene, mut materials) = props::atlas_scene();
    for id in [
        "wall",
        "concrete",
        "paint",
        "metal",
        "sheet",
        "pane",
        "brushed-metal",
    ] {
        materials.insert(
            id.into(),
            Material {
                id: id.into(),
                color: "#ffffff".into(),
                repeat: 1.,
                roughness: 0.75,
                metalness: if ["metal", "sheet", "brushed-metal"].contains(&id) {
                    0.3
                } else {
                    0.
                },
                ..Material::default()
            },
        );
    }
    scene.manifest.materials = materials.values().cloned().collect();
    let settings=serde_json::from_value(serde_json::json!({"weathering":{"seed":"rooftop/1","wear":0.25,"grime":0.6,"rust":0.4,"textureDensity":64,"metal":{"material":"metal","repeat":1},"sheet":{"material":"sheet","repeat":1},"paint":{"material":"paint","repeat":1}},"lettering":true,"timber":{"textured":true,"timberRepeat":1,"atlasRepeat":1}})).unwrap();
    let kit = Kit::new(settings, 2).unwrap();
    let mut mesh = Mesh::new(Mode::Render, 150000, &|| false).unwrap();
    let d = &mut Fixture {
        pen: Pen::new(Matrix::default(), "roof").unwrap(),
        mesh: &mut mesh,
        repeats: &|s| materials[s].repeat,
    };
    d.cuboid(
        "concrete",
        [0., 0., -0.2],
        [10., 11., 0.],
        [0.34, 0.36, 0.32],
        "b",
    )
    .unwrap();
    for (a, b) in [
        ([0., 0., 0.], [10., 0.16, 0.65]),
        ([0., 10.84, 0.], [10., 11., 0.65]),
        ([0., 0.16, 0.], [0.16, 10.84, 0.65]),
        ([9.84, 0.16, 0.], [10., 10.84, 0.65]),
    ] {
        d.cuboid("concrete", a, b, [0.5, 0.52, 0.48], "b").unwrap();
    }
    let binding = |slot: &str| Binding {
        material: slot.into(),
        repeat: 1.,
    };
    rooftop::draw(
        d,
        &rooftop::Request {
            lot_key: "lot".into(),
            tenement: true,
            wall_tint: [0.67, 0.77, 0.66],
            width: 10.,
            depth: 11.,
            parapet: 0.65,
            ceiling: 4.8,
            perimeter: true,
        },
        &rooftop::Settings {
            kit: &kit,
            wall: binding("wall"),
            concrete: binding("concrete"),
            brushed: binding("brushed-metal"),
            plants: true,
            cloth: true,
        },
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
            "rooftop",
            &weathered::ParallelBaking::default(),
            &|| false,
        )
        .unwrap();
    #[cfg(target_arch = "wasm32")]
    let bakes = plan.bake(&inputs, "rooftop", &|| false).unwrap();
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

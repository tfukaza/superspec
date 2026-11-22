//! Complete closed-front facade with baked receivers and real fixture atlases.
use super::{cells_scene, grocer_scene};
use std::collections::BTreeMap;
use superspec_aml::{material::Material, rng::SeededRng};
use superspec_engine::{
    Scene,
    materialspec::surface::Inputs,
    worldspec::themes::kowloon::{
        facade,
        facade_cells::Role,
        facade_envelope::Finish,
        facade_plan::{Queries, Request},
        fixture::Fixture,
        kit::Kit,
        massing::{Lot, Rect},
        mesh::{Mesh, Mode},
        metal::Binding,
        palette,
        pen::Pen,
        weathered,
    },
};
use superspec_geometry::Matrix;
pub fn scene() -> Scene {
    build(false)
}
pub fn open_scene() -> Scene {
    build(true)
}
fn build(open: bool) -> Scene {
    let (mut scene, mut materials) = if open {
        grocer_scene::assets(true)
    } else {
        cells_scene::assets()
    };
    for id in [
        "board",
        "rust",
        "lettering",
        "posters",
        "render-repair",
        "brushed-metal",
    ] {
        materials.entry(id.into()).or_insert_with(|| Material {
            id: id.into(),
            color: "#ffffff".into(),
            repeat: 1.,
            roughness: 0.75,
            ..Material::default()
        });
    }
    scene.manifest.materials = materials.values().cloned().collect();
    let mut mesh = Mesh::new(Mode::Render, 400000, &|| false).unwrap();
    let lot = Lot {
        index: 0,
        kind: "tenement",
        x0: 0.,
        x1: 9.8,
        ground: 3.4,
        story_height: 3.,
        stories: 3,
        roof: 9.4,
        top: 10.05,
        front: 0.,
        back: 0.,
        style: palette::lot_style(&mut SeededRng::from_text(if open {
            "facade-style/6"
        } else {
            "facade-style/0"
        })),
        key: if open { "lot-31" } else { "lot-2" }.into(),
    };
    let ground = [Rect {
        u0: 0.,
        u1: 9.8,
        v0: 0.,
        v1: 3.4,
    }];
    let body = [Rect {
        u0: -0.1,
        u1: 10.,
        v0: 3.4,
        v1: 10.05,
    }];
    let request = Request {
        lot: &lot,
        role: Role::Front,
        key: if open { "front-31" } else { "front-2" },
        width: 9.8,
        shift: if open { 0.3 } else { -0.2 },
        body_span: [-0.1, 10.],
        ground: &ground,
        body: &body,
        shops: if open { 1. } else { 0. },
    };
    let binding = |id: &str| Binding {
        material: id.into(),
        repeat: 1.,
    };
    let finish = Finish {
        coat: binding("wall"),
        bare: binding("concrete"),
        concrete: false,
        seed: 45,
        density: 32.,
        tiles: None,
    };
    let kit=Kit::new(serde_json::from_value(serde_json::json!({"weathering":{"seed":if open {"facade/31"}else{"facade/2"},"wear":0.25,"grime":0.6,"rust":0.4,"textureDensity":32,"metal":{"material":"metal","repeat":1},"sheet":{"material":"sheet","repeat":1},"paint":{"material":"paint","repeat":1}},"lettering":true,"timber":{"textured":true,"timberRepeat":1,"atlasRepeat":1}})).unwrap(),2).unwrap();
    facade::draw(
        &mut Fixture {
            pen: Pen::new(Matrix::default(), "envelope").unwrap(),
            mesh: &mut mesh,
            repeats: &|_| 1.,
        },
        &request,
        &facade::Settings {
            kit: &kit,
            finish: &finish,
            concrete: binding("concrete"),
            plants: true,
            cloth: true,
            posters: open,
            shops: open.then_some(
                superspec_engine::worldspec::themes::kowloon::shop::shell::Assets {
                    room: superspec_engine::worldspec::themes::kowloon::shop::room::Assets {
                        posters: true,
                        grocer_props: true,
                        snack_packets: true,
                        market_apple: true,
                        shop_register: false,
                        market_trays: false,
                    },
                    restaurant: superspec_engine::worldspec::themes::kowloon::shop::roast::Assets {
                        hero_props: false,
                        food: true,
                        metal: true,
                        surfaces: true,
                        tiles: true,
                    },
                },
            ),
        },
        &facade::Queries {
            geometry: Queries {
                reach: &|_, _, limit| Ok(limit),
                hollow: &|_, _, _, _| Ok(3.1),
            },
            miters: &|_, _| Ok([None; 2]),
            shop_near: &|_, _, _| Ok(None),
            frontage: open.then_some(&|_, _, _, _| Ok(true)),
        },
        &|| false,
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
            "facade",
            &weathered::ParallelBaking::default(),
            &|| false,
        )
        .unwrap();
    #[cfg(target_arch = "wasm32")]
    let bakes = plan.bake(&inputs, "facade", &|| false).unwrap();
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

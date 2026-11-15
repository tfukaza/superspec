//! Complete entrances, actual timber/poster atlases and procedural paint/glass/metal maps.
use std::collections::BTreeMap;
use superspec_aml::{material::Material, rng::SeededRng};
use superspec_engine::{
    Scene, compile,
    materialspec::surface::Inputs,
    worldspec::themes::kowloon::{
        doors::{self, Unit},
        fixture::Fixture,
        mesh::{Mesh, Mode},
        pen::Pen,
        timber, weathered,
    },
};
use superspec_geometry::Matrix;
#[path = "pipe_scene.rs"]
#[allow(dead_code)]
mod pipe;
pub fn scene() -> Scene {
    build(false)
}
pub fn ground_scene() -> Scene {
    build(true)
}
fn build(ground_bays: bool) -> Scene {
    let mut scene=compile(r#"<architecture version="1" id="rust-doors"><box id="host" width="11" depth=".1" height="3.4" visible="false"/></architecture>"#,0).unwrap();
    let mut materials = pipe::materials();
    for slot in ["wall", "pane", "timber"] {
        materials.insert(
            slot.into(),
            Material {
                id: slot.into(),
                color: "#ffffff".into(),
                ..Material::default()
            },
        );
    }
    if ground_bays {
        for slot in ["glass", "interior-lit", "concrete"] {
            materials.insert(
                slot.into(),
                Material {
                    id: slot.into(),
                    color: "#ffffff".into(),
                    ..Material::default()
                },
            );
        }
    }
    for (slot, builtin) in [
        ("shop-timber", "builtin:shop-timber"),
        ("posters", "builtin:wall-posters"),
    ] {
        let mut m = superspec_aml::material_catalog::material(builtin).unwrap();
        m.id = slot.into();
        materials.insert(slot.into(), m);
    }
    for (name, bytes, mime) in [
        (
            "shop-timber.png",
            include_bytes!("../../../../assets/materials/shop-timber.png").as_slice(),
            "image/png",
        ),
        (
            "shop-timber-normal.png",
            include_bytes!("../../../../assets/materials/shop-timber-normal.png").as_slice(),
            "image/png",
        ),
        (
            "shop-timber-properties.png",
            include_bytes!("../../../../assets/materials/shop-timber-properties.png").as_slice(),
            "image/png",
        ),
        (
            "posters-atlas.jpg",
            include_bytes!("../../../../assets/materials/posters-atlas.jpg").as_slice(),
            "image/jpeg",
        ),
    ] {
        scene.manifest.assets.push(superspec_engine::Asset {
            id: format!("superspec-material:{name}"),
            mime,
            byte_start: scene.asset_bytes.len(),
            byte_length: bytes.len(),
        });
        scene.asset_bytes.extend_from_slice(bytes);
    }
    scene.manifest.asset_byte_count = scene.asset_bytes.len();
    scene.manifest.materials.extend(materials.values().cloned());
    let mut weathering = pipe::options().weathering.unwrap();
    weathering.seed = "doors/finish".into();
    weathering.texture_density = 64.;
    weathering.wear = 0.25;
    weathering.grime = 0.6;
    let settings = doors::Settings {
        weathering,
        timber: timber::Settings::default(),
        trim: [0.8, 0.78, 0.72],
        detail: 1,
        posters: true,
    };
    let mut mesh = Mesh::new(Mode::Render, 100000, &|| false).unwrap();
    if ground_bays {
        use superspec_engine::worldspec::themes::kowloon::{ground, kit::Kit, signs::panel};
        let kit = Kit::new(
            panel::Settings {
                weathering: settings.weathering.clone(),
                timber: settings.timber.clone(),
                lettering: false,
            },
            1,
        )
        .unwrap();
        let style = ground::Style {
            frame: [0.1, 0.2, 0.15],
            trim: settings.trim,
            shutter: [0.2, 0.35, 0.3],
            accent: [0.3, 0.5, 0.4],
        };
        for (i, kind) in [
            ground::Kind::Shutter,
            ground::Kind::Gate,
            ground::Kind::Shop,
            ground::Kind::Service,
        ]
        .into_iter()
        .enumerate()
        {
            let pen = Pen::new(
                Matrix([
                    [1., 0., 0.],
                    [0., -1., 0.],
                    [0., 0., 1.],
                    [i as f64 * 3.8, 0., 0.],
                ]),
                &format!("ground-{i}"),
            )
            .unwrap();
            // The global sign survey owns these proposals; this fixture isolates the bay geometry.
            let _proposals = ground::draw(
                &mut Fixture {
                    pen,
                    mesh: &mut mesh,
                    repeats: &|_| 1.,
                },
                kind,
                &Unit {
                    u0: 0.,
                    u1: 3.6,
                    height: 3.4,
                    key: format!("unit-{i}"),
                },
                &ground::Settings {
                    kit: &kit,
                    style: &style,
                    posters: true,
                },
                &mut SeededRng::from_text(&format!("ground-caller/{i}")),
            )
            .unwrap();
        }
    } else {
        for (i, index) in [0, 2, 4].into_iter().enumerate() {
            let pen = Pen::new(
                Matrix([
                    [1., 0., 0.],
                    [0., -1., 0.],
                    [0., 0., 1.],
                    [i as f64 * 3.8, 0., 0.],
                ]),
                &format!("door-{index}"),
            )
            .unwrap();
            doors::entrance(
                &mut Fixture {
                    pen,
                    mesh: &mut mesh,
                    repeats: &|_| 1.,
                },
                &Unit {
                    u0: 0.,
                    u1: 3.6,
                    height: 3.4,
                    key: "unit".into(),
                },
                &settings,
                &mut SeededRng::from_text(&format!("door/{index}")),
            )
            .unwrap();
        }
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
            "doors",
            &weathered::ParallelBaking::default(),
            &|| false,
        )
        .unwrap();
    #[cfg(target_arch = "wasm32")]
    let bakes = plan.bake(&inputs, "doors", &|| false).unwrap();
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

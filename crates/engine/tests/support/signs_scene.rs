//! Complete aged signs with shipped glyph artwork baked into opaque panel materials.
use std::collections::BTreeMap;
use superspec_aml::material::Material;
use superspec_engine::{
    Asset, Scene, compile,
    materialspec::surface::Inputs,
    worldspec::themes::kowloon::{
        fixture::Fixture,
        mesh::{Mesh, Mode},
        pen::Pen,
        signs::{
            Construction, Outline,
            lettering::{Direction, Face},
            panel::{self, Block, Design, Hardware, Settings, Sides, Stack},
        },
        timber, weathered,
    },
};
use superspec_geometry::{Matrix, image};
pub fn scene() -> Scene {
    build(false)
}
pub fn projecting_scene() -> Scene {
    build(true)
}
fn build(projecting: bool) -> Scene {
    let mut scene=compile(r#"<architecture version="1" id="rust-signs"><box id="host" width="7" depth=".1" height="4" visible="false"/></architecture>"#,0).unwrap();
    let mut materials: BTreeMap<String, Material> =
        ["paint", "metal", "sheet", "timber", "wall", "shop-timber"]
            .into_iter()
            .map(|id| {
                (
                    id.into(),
                    Material {
                        id: id.into(),
                        color: "#ffffff".into(),
                        repeat: 1.,
                        roughness: 0.7,
                        ..Material::default()
                    },
                )
            })
            .collect();
    for (slot, builtin) in [
        ("board", "signboard"),
        ("rust", "rust"),
        ("lettering", "lettering"),
    ] {
        let mut material =
            superspec_aml::material_catalog::material(&format!("builtin:{builtin}")).unwrap();
        material.id = slot.into();
        materials.insert(slot.into(), material);
    }
    let mut images = BTreeMap::new();
    for (name, bytes) in [
        (
            "lettering.png",
            include_bytes!("../../../../assets/materials/lettering.png").as_slice(),
        ),
        (
            "signboard.png",
            include_bytes!("../../../../assets/materials/signboard.png").as_slice(),
        ),
        (
            "rust.png",
            include_bytes!("../../../../assets/materials/rust.png").as_slice(),
        ),
    ] {
        let id = format!("superspec-material:{name}");
        images.insert(id.clone(), image::decode(bytes).unwrap());
        scene.manifest.assets.push(Asset {
            id,
            mime: "image/png",
            byte_start: scene.asset_bytes.len(),
            byte_length: bytes.len(),
        });
        scene.asset_bytes.extend_from_slice(bytes);
    }
    scene.manifest.asset_byte_count = scene.asset_bytes.len();
    scene.manifest.materials = materials.values().cloned().collect();
    let settings=Settings{weathering:serde_json::from_value(serde_json::json!({"seed":"sign-scene","wear":0.7,"grime":0.6,"rust":0.6,"textureDensity":128,"metal":{"material":"metal","repeat":1},"paint":{"material":"paint","repeat":1},"sheet":{"material":"sheet","repeat":1}})).unwrap(),lettering:true,timber:timber::Settings{textured:false,..Default::default()}};
    let mut mesh = Mesh::new(Mode::Render, 150000, &|| false).unwrap();
    if projecting {
        use superspec_aml::rng::SeededRng;
        use superspec_engine::worldspec::themes::kowloon::signs::{copy, mounts};
        for (position, index) in [1, 3, 49].into_iter().enumerate() {
            let mut matrix = Matrix::default();
            matrix.0[3] = [position as f64 * 2.2, 0., 0.];
            let site = mounts::Site {
                pen: Pen::new(matrix, "sign").unwrap(),
                key: format!("sign/site/{position}"),
                shown: true,
                elevation: 0.,
                detail: 1,
                wash: None,
            };
            let room = mounts::PierRoom {
                u: 0.7,
                foot: 2.2,
                ceiling: 6.8,
                reach: if index == 3 { 0.9 } else { 1.5 },
            };
            let business = copy::invent(
                &mut SeededRng::from_text(&format!("mount-business/{index}")),
                &Default::default(),
            )
            .unwrap();
            let mut rng = SeededRng::from_text(&format!("mount/{index}"));
            let make = if index == 49 {
                mounts::hung_sign
            } else {
                mounts::blade_sign
            };
            make(&site, room, &business, &settings, &mut rng)
                .unwrap()
                .unwrap()
                .proposal
                .draw(&mut mesh, &|s| materials[s].repeat, &settings)
                .unwrap();
        }
    } else {
        for i in 0..4 {
            let vertical = i == 2;
            let width = if vertical { 0.8 } else { 2.8 };
            let height = if vertical { 2.8 } else { 1.1 };
            let design = Design {
                width,
                height,
                depth: 0.08,
                construction: [
                    Construction::Sheet,
                    Construction::Timber,
                    Construction::Lightbox,
                    Construction::Enamel,
                ][i],
                outline: Outline::Rectangle,
                ground: if vertical {
                    [0.32, 0.025, 0.01]
                } else {
                    [0.025, 0.17, 0.12]
                },
                border: Some([0.85, 0.7, 0.5]),
                stack: Stack::Down,
                blocks: if vertical {
                    vec![Block {
                        text: "永發茶餐廳".into(),
                        face: Face::Kai,
                        direction: Direction::Ttb,
                        ink: [0.9, 0.8, 0.6],
                        share: 1.,
                        shadow: Some([0.04, 0.08, 0.1]),
                    }]
                } else {
                    vec![
                        Block {
                            text: "永發茶餐廳".into(),
                            face: Face::Kai,
                            direction: if i == 3 {
                                Direction::Ltr
                            } else {
                                Direction::Rtl
                            },
                            ink: [0.9, 0.8, 0.6],
                            share: 2.,
                            shadow: Some([0.04, 0.08, 0.1]),
                        },
                        Block {
                            text: "PEARL 98".into(),
                            face: Face::Hei,
                            direction: Direction::Ltr,
                            ink: [0.85, 0.7, 0.5],
                            share: 1.,
                            shadow: None,
                        },
                    ]
                },
                age: if i == 3 { 0.15 } else { 0.85 },
                phase: i as f64 + 0.125,
            };
            let mut matrix = Matrix::default();
            matrix.0[3] = [[0., 0., 2.8], [0., 0., 1.3], [3.2, 0., 1.1], [4.5, 0., 2.8]][i];
            if i == 3 {
                matrix.0[0] = [-1., 0., 0.];
                matrix.0[3][0] += width;
            }
            panel::draw(
                &mut Fixture {
                    pen: Pen::new(matrix, &format!("panel-{i}")).unwrap(),
                    mesh: &mut mesh,
                    repeats: &|s| materials[s].repeat,
                },
                &design,
                if i == 3 { Sides::Both } else { Sides::Front },
                &Hardware {
                    slot: "metal".into(),
                    tint: [0.25, 0.3, 0.28],
                    weathering: Some(settings.weathering.clone()),
                },
                &settings,
            )
            .unwrap();
        }
    }
    let plan = weathered::finish(&mut mesh, &|| false).unwrap();
    let inputs = Inputs {
        materials: &materials,
        images: &images,
    };
    #[cfg(not(target_arch = "wasm32"))]
    let bakes = plan
        .bake_parallel(
            &inputs,
            "signs",
            &weathered::ParallelBaking::default(),
            &|| false,
        )
        .unwrap();
    #[cfg(target_arch = "wasm32")]
    let bakes = plan.bake(&inputs, "signs", &|| false).unwrap();
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

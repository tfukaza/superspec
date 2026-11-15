//! Shipped plant/cloth atlases with their normal and property channels, emitted by Rust.
use std::collections::BTreeMap;
use superspec_aml::{material::Material, rng::SeededRng};
use superspec_engine::{
    Asset, Scene, compile,
    worldspec::themes::kowloon::{
        balcony_props::{self, GarmentName, Plant, Room, Support},
        fixture::Fixture,
        mesh::{Mesh, Mode},
        pen::Pen,
    },
};
use superspec_geometry::Matrix;
pub fn atlas_scene() -> (Scene, BTreeMap<String, Material>) {
    let mut scene=compile(r#"<architecture version="1" id="rust-balcony-props"><box id="host" width="7" depth=".1" height="3" visible="false"/></architecture>"#,0).unwrap();
    let mut materials: BTreeMap<String, Material> = BTreeMap::from([(
        "paint".into(),
        Material {
            id: "paint".into(),
            color: "#ffffff".into(),
            ..Material::default()
        },
    )]);
    for slot in ["balcony-plants", "balcony-cloth"] {
        let mut material =
            superspec_aml::material_catalog::material(&format!("builtin:{slot}")).unwrap();
        material.id = slot.into();
        materials.insert(slot.into(), material);
    }
    for (name, bytes) in [
        (
            "balcony-plants.png",
            include_bytes!("../../../../assets/materials/balcony-plants.png").as_slice(),
        ),
        (
            "balcony-plants-normal.png",
            include_bytes!("../../../../assets/materials/balcony-plants-normal.png").as_slice(),
        ),
        (
            "balcony-plants-properties.png",
            include_bytes!("../../../../assets/materials/balcony-plants-properties.png").as_slice(),
        ),
        (
            "balcony-cloth.png",
            include_bytes!("../../../../assets/materials/balcony-cloth.png").as_slice(),
        ),
        (
            "balcony-cloth-normal.png",
            include_bytes!("../../../../assets/materials/balcony-cloth-normal.png").as_slice(),
        ),
        (
            "balcony-cloth-properties.png",
            include_bytes!("../../../../assets/materials/balcony-cloth-properties.png").as_slice(),
        ),
    ] {
        scene.manifest.assets.push(Asset {
            id: format!("superspec-material:{name}"),
            mime: "image/png",
            byte_start: scene.asset_bytes.len(),
            byte_length: bytes.len(),
        });
        scene.asset_bytes.extend_from_slice(bytes);
    }
    scene.manifest.asset_byte_count = scene.asset_bytes.len();
    scene.manifest.materials.extend(materials.values().cloned());
    (scene, materials)
}
pub fn scene() -> Scene {
    let (mut scene, materials) = atlas_scene();
    let mut mesh = Mesh::new(Mode::Render, 100000, &|| false).unwrap();
    for (i, name) in [Plant::RubberPlant, Plant::TrailingPothos, Plant::SnakePlant]
        .into_iter()
        .enumerate()
    {
        balcony_props::plant(
            &mut Fixture {
                pen: Pen::new(Matrix::default(), &format!("plant-{i}")).unwrap(),
                mesh: &mut mesh,
                repeats: &|_| 1.,
            },
            name,
            [i as f64 * 0.9, 0., 0.],
            1.3,
            0.3,
            true,
        )
        .unwrap();
    }
    for (i, (name, support)) in [
        (GarmentName::HangingShirt, Support::Hanger),
        (GarmentName::HangingTrousers, Support::Pole),
        (GarmentName::HangingBedsheet, Support::Line),
    ]
    .into_iter()
    .enumerate()
    {
        let g = (0..128)
            .find_map(|n| {
                balcony_props::choose(
                    &mut SeededRng::from_text(&format!("display/{n}")),
                    Room {
                        width: 1.5,
                        drop: 1.8,
                    },
                    support,
                )
                .unwrap()
                .filter(|g| g.asset == name)
            })
            .unwrap();
        let x = 3. + i as f64 * 1.4;
        balcony_props::hang(
            &mut Fixture {
                pen: Pen::new(Matrix::default(), &format!("garment-{i}")).unwrap(),
                mesh: &mut mesh,
                repeats: &|_| 1.,
            },
            &g,
            [x - g.width / 2., 0., 2.2],
            [x + g.width / 2., 0., 2.2],
            0.03,
            true,
        )
        .unwrap();
    }
    let bindings = materials
        .keys()
        .map(|id| (id.clone(), id.clone()))
        .collect();
    scene
        .append_baked_visual(
            "host",
            &mesh,
            &bindings,
            &[] as &[superspec_engine::materialspec::bake::BakedMaterial],
        )
        .unwrap();
    scene
}

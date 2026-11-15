//! Baked stock and deformed bag groups with their owned image companions.
use std::collections::BTreeMap;
use superspec_aml::material::Material;
use superspec_engine::{
    Asset, Scene, compile,
    worldspec::themes::kowloon::{
        fixture::Fixture,
        mesh::{Mesh, Mode},
        pen::Pen,
        shop::{
            grocer_props::{self, Group, Options},
            stock_mesh,
        },
    },
};
use superspec_geometry::Matrix;
pub fn assets() -> (Scene, BTreeMap<String, Material>) {
    let mut scene=compile(r#"<architecture version="1" id="rust-stock"><box id="host" width="8" depth="4" height="1" visible="false"/></architecture>"#,0).unwrap();
    let mut materials = BTreeMap::from([(
        "paint".to_string(),
        Material {
            id: "paint".into(),
            color: "#ffffff".into(),
            repeat: 1.,
            roughness: 0.8,
            ..Default::default()
        },
    )]);
    for id in ["grocer-props", "market-apple", "snack-packet"] {
        let mut m = superspec_aml::material_catalog::material(&format!("builtin:{id}")).unwrap();
        m.id = id.into();
        materials.insert(id.into(), m);
    }
    for (name, bytes) in [
        (
            "grocer-props.png",
            include_bytes!("../../../../assets/materials/grocer-props.png").as_slice(),
        ),
        (
            "grocer-props-normal.png",
            include_bytes!("../../../../assets/materials/grocer-props-normal.png").as_slice(),
        ),
        (
            "grocer-props-properties.png",
            include_bytes!("../../../../assets/materials/grocer-props-properties.png").as_slice(),
        ),
        (
            "market-apple.png",
            include_bytes!("../../../../assets/materials/market-apple.png").as_slice(),
        ),
        (
            "snack-packet.png",
            include_bytes!("../../../../assets/materials/snack-packet.png").as_slice(),
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
    scene.manifest.materials = materials.values().cloned().collect();
    scene.manifest.asset_byte_count = scene.asset_bytes.len();
    (scene, materials)
}
pub fn scene() -> Scene {
    let (mut scene, materials) = assets();
    let mut mesh = Mesh::new(Mode::Render, 100000, &|| false).unwrap();
    for (i, name) in [
        "rice-sack",
        "egg-basket",
        "garlic-braid",
        "counter-scale",
        "bottle-crate",
        "drink-bottle",
        "food-tin",
        "tea-carton",
        "tissue-roll",
        "shop-stool",
        "shop-counter",
        "drinks-cooler",
        "seasoning-jar",
        "dry-goods-bag",
        "bottle-load",
        "produce-crate",
        "stock-box",
    ]
    .into_iter()
    .enumerate()
    {
        let mut m = Matrix::default();
        if i % 2 == 1 {
            m.0[0][0] = -1.;
        }
        m.0[3] = [(i % 6) as f64 * 1.1, (i / 6) as f64 * 1.1, 0.];
        grocer_props::draw(
            &mut Fixture {
                pen: Pen::new(m, name).unwrap(),
                mesh: &mut mesh,
                repeats: &|_| 1.,
            },
            name,
            [0.; 3],
            [0.75, 0.65, 0.8],
            0.,
            &Options {
                variant: 1,
                ..Default::default()
            },
        )
        .unwrap();
    }
    for i in 0..4 {
        let mut m = Matrix::default();
        if i % 2 == 1 {
            m.0[0][0] = -1.;
        }
        m.0[3] = [i as f64 * 1.3, -1.2, 0.];
        let d = &mut Fixture {
            pen: Pen::new(m, &format!("bag-group-{i}")).unwrap(),
            mesh: &mut mesh,
            repeats: &|_| 1.,
        };
        grocer_props::bag_cluster(
            d,
            "dry-goods-bag",
            [0.; 3],
            [1., 0.55, 0.5],
            &Group {
                yaw: 0.,
                variant: i + 1,
                arrangement: i,
                textured: true,
            },
        )
        .unwrap();
        stock_mesh::snack_packet(d, [0., -0.55, 0.], [0.28, 0.36], [1.; 3], true, i).unwrap();
    }
    stock_mesh::apple_pile(
        &mut Fixture {
            pen: Pen::new(Matrix::default(), "apple-pile").unwrap(),
            mesh: &mut mesh,
            repeats: &|_| 1.,
        },
        [5.2, -1.2, 0.],
        [0.8, 0.6, 0.25],
        [1.; 3],
        true,
    )
    .unwrap();
    let bindings = materials
        .keys()
        .map(|id| (id.clone(), id.clone()))
        .collect();
    scene.append_visual("host", &mesh, &bindings).unwrap();
    scene
}

//! Complete stocked room with owned atlas companions and an open inspection shell.
use super::{cells_scene, shop_stock_scene};
use superspec_aml::rng::SeededRng;
use superspec_engine::{
    Asset, Scene,
    worldspec::themes::kowloon::{
        fixture::Fixture,
        kit::Kit,
        mesh::{Mesh, Mode},
        pen::Pen,
        shop::{
            greengrocer, grocer, hardware, herbalist, roast,
            room::{Assets, Room},
        },
    },
};
use superspec_geometry::Matrix;
pub fn scene() -> Scene {
    build(Trade::Grocer)
}
pub fn greengrocer_scene() -> Scene {
    build(Trade::Greengrocer)
}
pub fn hardware_scene() -> Scene {
    build(Trade::Hardware)
}
pub fn herbalist_scene() -> Scene {
    build(Trade::Herbalist)
}
pub fn roast_scene() -> Scene {
    build(Trade::Roast)
}
enum Trade {
    Grocer,
    Greengrocer,
    Hardware,
    Herbalist,
    Roast,
}
pub fn assets(
    restaurant: bool,
) -> (
    Scene,
    std::collections::BTreeMap<String, superspec_aml::material::Material>,
) {
    let (mut scene, mut materials) = cells_scene::assets();
    let (stock_scene, stock_materials) = shop_stock_scene::assets();
    for mut asset in stock_scene.manifest.assets {
        let bytes =
            &stock_scene.asset_bytes[asset.byte_start..asset.byte_start + asset.byte_length];
        asset.byte_start = scene.asset_bytes.len();
        scene.asset_bytes.extend_from_slice(bytes);
        scene.manifest.assets.push(asset);
    }
    materials.extend(stock_materials);
    let mut poster = superspec_aml::material_catalog::material("builtin:wall-posters").unwrap();
    poster.id = "posters".into();
    materials.insert("posters".into(), poster);
    let bytes = include_bytes!("../../../../assets/materials/posters-atlas.jpg");
    scene.manifest.assets.push(Asset {
        id: "superspec-material:posters-atlas.jpg".into(),
        mime: "image/jpeg",
        byte_start: scene.asset_bytes.len(),
        byte_length: bytes.len(),
    });
    scene.asset_bytes.extend_from_slice(bytes);
    if restaurant {
        kitchen_assets(&mut scene, &mut materials);
    }
    scene.manifest.materials = materials.values().cloned().collect();
    scene.manifest.asset_byte_count = scene.asset_bytes.len();
    (scene, materials)
}
fn build(trade: Trade) -> Scene {
    let (mut scene, materials) = assets(matches!(trade, Trade::Roast));
    let kit = Kit::new(serde_json::from_value(serde_json::json!({"weathering":{"seed":"room/3","wear":0.25,"grime":0.6,"rust":0.4,"textureDensity":32,"metal":{"material":"metal","repeat":1.7},"sheet":{"material":"sheet","repeat":1.7},"paint":{"material":"paint","repeat":1.7}},"lettering":true,"timber":{"textured":true,"timberRepeat":1.7,"atlasRepeat":1.7}})).unwrap(),2).unwrap();
    let room = Room {
        width: 3.2,
        depth: 3.1,
        height: 2.6,
        kit: &kit,
        key: "shop/3",
        assets: Assets {
            posters: true,
            grocer_props: true,
            snack_packets: true,
            market_apple: true,
            shop_register: false,
            market_trays: false,
        },
    };
    let mut mesh = Mesh::new(Mode::Render, 400000, &|| false).unwrap();
    let d = &mut Fixture {
        pen: Pen::new(Matrix::default(), "grocer").unwrap(),
        mesh: &mut mesh,
        repeats: &|_| 1.7,
    };
    match trade {
        Trade::Grocer => grocer::furnish(d, &room),
        Trade::Greengrocer => greengrocer::furnish(d, &room),
        Trade::Hardware => hardware::furnish(d, &room),
        Trade::Herbalist => herbalist::furnish(d, &room),
        Trade::Roast => roast::furnish(
            d,
            &room,
            roast::Assets {
                hero_props: false,
                food: true,
                metal: true,
                surfaces: true,
                tiles: true,
            },
        ),
    }
    .unwrap();
    d.pen = Pen::new(Matrix::default(), "shell").unwrap();
    let mut rng = SeededRng::from_text("room-interior");
    let interior = match trade {
        Trade::Grocer => grocer::interior(&mut rng),
        Trade::Greengrocer => greengrocer::interior(&mut rng),
        Trade::Hardware => hardware::interior(&mut rng),
        Trade::Herbalist => herbalist::interior(&mut rng),
        Trade::Roast => roast::interior(&mut rng),
    };
    d.cuboid(
        "paint",
        [0., 0., -0.08],
        [room.width, room.depth, 0.],
        interior.floor,
        "",
    )
    .unwrap();
    d.cuboid(
        "paint",
        [0., room.depth, 0.],
        [room.width, room.depth + 0.06, room.height],
        interior.wall,
        "",
    )
    .unwrap();
    d.cuboid(
        "paint",
        [0., room.depth - 0.002, 0.],
        [room.width, room.depth, interior.dado_height],
        interior.dado,
        "",
    )
    .unwrap();
    let mut bindings: std::collections::BTreeMap<_, _> =
        materials.keys().map(|s| (s.clone(), s.clone())).collect();
    if matches!(trade, Trade::Roast) {
        use superspec_engine::{
            materialspec::surface::Inputs, worldspec::themes::kowloon::weathered,
        };
        let plan = weathered::finish(&mut mesh, &|| false).unwrap();
        let inputs = Inputs {
            materials: &materials,
            images: &std::collections::BTreeMap::new(),
        };
        let bakes = plan.bake(&inputs, "roast", &|| false).unwrap();
        bindings.extend(
            bakes
                .iter()
                .map(|b| (b.material().id.clone(), b.material().id.clone())),
        );
        scene
            .append_baked_visual("host", &mesh, &bindings, &bakes)
            .unwrap();
    } else {
        scene.append_visual("host", &mesh, &bindings).unwrap();
    }
    scene
}

fn kitchen_assets(
    scene: &mut Scene,
    materials: &mut std::collections::BTreeMap<String, superspec_aml::material::Material>,
) {
    use superspec_aml::material::Material;
    for id in [
        "restaurant-food",
        "restaurant-surfaces",
        "restaurant-tiles",
        "brushed-metal",
        "kitchen-metal",
    ] {
        let mut m = superspec_aml::material_catalog::material(&format!("builtin:{id}")).unwrap();
        m.id = id.into();
        materials.insert(id.into(), m);
    }
    materials.insert(
        "display-glass".into(),
        Material {
            id: "display-glass".into(),
            color: "#ffffff".into(),
            roughness: 0.08,
            opacity: Some(0.055),
            ..Default::default()
        },
    );
    for (name, bytes) in [
        (
            "restaurant-food.png",
            include_bytes!("../../../../assets/materials/restaurant-food.png").as_slice(),
        ),
        (
            "restaurant-food-normal.png",
            include_bytes!("../../../../assets/materials/restaurant-food-normal.png").as_slice(),
        ),
        (
            "restaurant-food-properties.png",
            include_bytes!("../../../../assets/materials/restaurant-food-properties.png")
                .as_slice(),
        ),
        (
            "restaurant-surfaces.png",
            include_bytes!("../../../../assets/materials/restaurant-surfaces.png").as_slice(),
        ),
        (
            "restaurant-surfaces-properties.png",
            include_bytes!("../../../../assets/materials/restaurant-surfaces-properties.png")
                .as_slice(),
        ),
        (
            "restaurant-tiles.png",
            include_bytes!("../../../../assets/materials/restaurant-tiles.png").as_slice(),
        ),
        (
            "restaurant-tiles-normal.png",
            include_bytes!("../../../../assets/materials/restaurant-tiles-normal.png").as_slice(),
        ),
        (
            "restaurant-tiles-properties.png",
            include_bytes!("../../../../assets/materials/restaurant-tiles-properties.png")
                .as_slice(),
        ),
        (
            "kitchen-metal.png",
            include_bytes!("../../../../assets/materials/kitchen-metal.png").as_slice(),
        ),
        (
            "kitchen-metal-normal.png",
            include_bytes!("../../../../assets/materials/kitchen-metal-normal.png").as_slice(),
        ),
        (
            "kitchen-metal-properties.png",
            include_bytes!("../../../../assets/materials/kitchen-metal-properties.png").as_slice(),
        ),
        (
            "brushed-metal.png",
            include_bytes!("../../../../assets/materials/brushed-metal.png").as_slice(),
        ),
        (
            "brushed-metal-normal.png",
            include_bytes!("../../../../assets/materials/brushed-metal-normal.png").as_slice(),
        ),
        (
            "brushed-metal-properties.png",
            include_bytes!("../../../../assets/materials/brushed-metal-properties.png").as_slice(),
        ),
    ] {
        scene.manifest.assets.push(Asset {
            id: format!("superspec-material:{name}"),
            mime: if name.ends_with(".jpg") {
                "image/jpeg"
            } else {
                "image/png"
            },
            byte_start: scene.asset_bytes.len(),
            byte_length: bytes.len(),
        });
        scene.asset_bytes.extend_from_slice(bytes);
    }
}

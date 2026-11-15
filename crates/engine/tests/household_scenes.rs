//! Native output boundary: real bakes and atlases bind without altering host collision.
#[path = "support/balconies_scene.rs"]
mod balconies;
#[allow(dead_code)]
#[path = "support/balcony_props_scene.rs"]
mod balcony_props_scene;
#[path = "support/cells_scene.rs"]
mod cells_scene;
#[path = "support/complete_facade_scene.rs"]
mod complete_facade;
#[path = "support/facade_details_scene.rs"]
mod facade_details;
#[path = "support/grocer_scene.rs"]
mod grocer_scene;
#[path = "support/household_scene.rs"]
mod household;
#[allow(dead_code)]
#[path = "support/laundry_scene.rs"]
mod laundry;
#[path = "support/pavement_scene.rs"]
mod pavement_scene;
#[path = "support/rooftop_scene.rs"]
mod rooftop;
#[path = "support/shop_equipment_scene.rs"]
mod shop_equipment;
#[path = "support/shop_stock_scene.rs"]
mod shop_stock_scene;
#[path = "support/signs_scene.rs"]
mod signs;
#[path = "support/tarps_scene.rs"]
mod tarps;
#[test]
fn household_and_balcony_scenes_deliver_complete_materials_with_unchanged_collision() {
    for build in [
        laundry::scene,
        household::scene,
        tarps::scene,
        balconies::scene,
        facade_details::scene,
        signs::scene,
        signs::projecting_scene,
        rooftop::scene,
        cells_scene::scene,
        complete_facade::scene,
        complete_facade::open_scene,
        pavement_scene::scene,
        shop_equipment::scene,
        shop_stock_scene::scene,
        grocer_scene::scene,
        grocer_scene::greengrocer_scene,
        grocer_scene::hardware_scene,
        grocer_scene::herbalist_scene,
        grocer_scene::roast_scene,
    ] {
        let scene = build();
        assert_eq!(scene.manifest.colliders.len(), 1);
        assert!(!scene.indices.is_empty());
        assert!(!scene.manifest.assets.is_empty());
        for mesh in scene.manifest.meshes.iter().filter(|m| m.visible) {
            assert!(
                scene
                    .manifest
                    .materials
                    .iter()
                    .any(|m| Some(&m.id) == mesh.material.as_ref())
            );
        }
        for material in &scene.manifest.materials {
            for companion in material.companions() {
                assert!(
                    scene
                        .manifest
                        .assets
                        .iter()
                        .any(|asset| asset.id == companion),
                    "unbound {companion}"
                );
            }
        }
        assert!(!scene.packet().is_empty());
    }
}

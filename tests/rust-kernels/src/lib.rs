//! Browser acceptance artifact only; never linked into the production compiler or editor.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = console, js_name = error)]
    fn report_panic(message: &str);
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen(start)]
pub fn install_panic_hook() {
    std::panic::set_hook(Box::new(|panic| report_panic(&panic.to_string())));
}

#[cfg(target_arch = "wasm32")]
#[allow(dead_code, unused_imports)]
#[path = "../../../crates/engine/tests/material_program.rs"]
mod material_program;

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn material_program_result() -> Vec<u8> {
    material_program::reference_program_result()
}

#[cfg(target_arch = "wasm32")]
#[allow(dead_code, unused_imports)]
#[path = "../../../crates/engine/tests/material_ts.rs"]
mod material_ts;

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn material_ts_result() -> Vec<u8> {
    material_ts::typescript_program_result()
}

#[cfg(target_arch = "wasm32")]
#[allow(dead_code, unused_imports)]
#[path = "../../../crates/engine/tests/material_live.rs"]
mod material_live;

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn material_live_result() -> Vec<u8> {
    material_live::live_program_result()
}

#[cfg(target_arch = "wasm32")]
#[allow(dead_code, unused_imports)]
#[path = "../../../crates/engine/tests/material_primitives.rs"]
mod material_primitives;

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn material_kowloon_result() -> Vec<u8> {
    material_primitives::kowloon_program_result()
}

#[cfg(target_arch = "wasm32")]
#[allow(dead_code, unused_imports)]
#[path = "../../../crates/engine/tests/material_features.rs"]
mod material_features;

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn check_material_features() -> bool {
    material_features::check_large_feature_indices();
    true
}

// The ordinary test bodies are omitted in Wasm; these shared behavioural checks execute on both targets.
#[cfg(target_arch = "wasm32")]
#[allow(dead_code, unused_imports)]
#[path = "../../../crates/engine/tests/shop_display.rs"]
mod shop_display;

#[cfg(target_arch = "wasm32")]
#[allow(dead_code, unused_imports)]
#[path = "../../../crates/engine/tests/shop_shell.rs"]
mod shop_shell;

#[cfg(target_arch = "wasm32")]
#[allow(dead_code, unused_imports)]
#[path = "../../../crates/engine/tests/open_facade.rs"]
mod open_facade;

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn check_procedural_meshes() -> bool {
    shop_display::check_frontage_plans();
    shop_shell::check_shop_shell_rooms();
    open_facade::check_open_facades();
    true
}

#[cfg(target_arch = "wasm32")]
#[path = "../../../crates/engine/tests/support/cells_scene.rs"]
mod cells_scene;
#[cfg(target_arch = "wasm32")]
#[path = "../../../crates/engine/tests/support/complete_facade_scene.rs"]
mod complete_facade_scene;
#[cfg(target_arch = "wasm32")]
#[path = "../../../crates/engine/tests/support/grocer_scene.rs"]
mod grocer_scene;
#[cfg(target_arch = "wasm32")]
#[path = "../../../crates/engine/tests/support/shop_equipment_scene.rs"]
mod shop_equipment_scene;
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn grocer_scene_packet() -> Vec<u8> {
    grocer_scene::scene().packet()
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn greengrocer_scene_packet() -> Vec<u8> {
    grocer_scene::greengrocer_scene().packet()
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn hardware_scene_packet() -> Vec<u8> {
    grocer_scene::hardware_scene().packet()
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn herbalist_scene_packet() -> Vec<u8> {
    grocer_scene::herbalist_scene().packet()
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn roast_scene_packet() -> Vec<u8> {
    grocer_scene::roast_scene().packet()
}
#[cfg(target_arch = "wasm32")]
#[path = "../../../crates/engine/tests/support/shop_stock_scene.rs"]
mod shop_stock_scene;
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn shop_stock_scene_packet() -> Vec<u8> {
    shop_stock_scene::scene().packet()
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn shop_equipment_scene_packet() -> Vec<u8> {
    shop_equipment_scene::scene().packet()
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn complete_facade_scene_packet() -> Vec<u8> {
    complete_facade_scene::scene().packet()
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn open_facade_scene_packet() -> Vec<u8> {
    complete_facade_scene::open_scene().packet()
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn cells_scene_packet() -> Vec<u8> {
    cells_scene::scene().packet()
}
#[cfg(target_arch = "wasm32")]
#[path = "../../../crates/engine/tests/support/rooftop_scene.rs"]
mod rooftop_scene;
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn rooftop_scene_packet() -> Vec<u8> {
    rooftop_scene::scene().packet()
}
#[cfg(target_arch = "wasm32")]
#[path = "../../../crates/engine/tests/support/signs_scene.rs"]
mod signs_scene;
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn signs_scene_packet() -> Vec<u8> {
    signs_scene::scene().packet()
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn projecting_signs_scene_packet() -> Vec<u8> {
    signs_scene::projecting_scene().packet()
}
#[cfg(target_arch = "wasm32")]
#[path = "../../../crates/engine/tests/support/facade_details_scene.rs"]
mod facade_details_scene;
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn facade_details_scene_packet() -> Vec<u8> {
    facade_details_scene::scene().packet()
}
#[cfg(target_arch = "wasm32")]
#[path = "../../../crates/engine/tests/support/balconies_scene.rs"]
mod balconies_scene;
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn balconies_scene_packet() -> Vec<u8> {
    balconies_scene::scene().packet()
}
#[cfg(target_arch = "wasm32")]
#[path = "../../../crates/engine/tests/support/tarps_scene.rs"]
mod tarps_scene;
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn tarps_scene_packet() -> Vec<u8> {
    tarps_scene::scene().packet()
}
#[cfg(target_arch = "wasm32")]
#[allow(dead_code)]
#[path = "../../../crates/engine/tests/support/household_scene.rs"]
mod household_scene;
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn household_scene_packet() -> Vec<u8> {
    household_scene::scene().packet()
}
#[cfg(target_arch = "wasm32")]
#[allow(dead_code)]
#[path = "../../../crates/engine/tests/support/laundry_scene.rs"]
mod laundry_scene;
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn laundry_scene_packet() -> Vec<u8> {
    laundry_scene::scene().packet()
}
#[cfg(target_arch = "wasm32")]
#[path = "../../../crates/engine/tests/support/balcony_props_scene.rs"]
mod balcony_props_scene;
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn balcony_props_scene_packet() -> Vec<u8> {
    balcony_props_scene::scene().packet()
}
#[cfg(target_arch = "wasm32")]
#[path = "../../../crates/engine/tests/support/roof_access_scene.rs"]
mod roof_access_scene;
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn roof_access_scene_packet() -> Vec<u8> {
    roof_access_scene::scene().packet()
}

#[cfg(target_arch = "wasm32")]
#[path = "../../../crates/engine/tests/support/tile_scene.rs"]
mod tile_scene;
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn tile_scene_packet() -> Vec<u8> {
    tile_scene::scene().packet()
}

#[cfg(target_arch = "wasm32")]
#[path = "../../../crates/engine/tests/support/skin_scene.rs"]
mod skin_scene;
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn skin_scene_packet() -> Vec<u8> {
    skin_scene::scene(false).packet()
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn tiled_skin_scene_packet() -> Vec<u8> {
    skin_scene::scene(true).packet()
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn stained_skin_scene_packet(tiled: bool) -> Vec<u8> {
    skin_scene::stained_scene(tiled).packet()
}

#[cfg(target_arch = "wasm32")]
#[path = "../../../crates/engine/tests/support/facade_scene.rs"]
mod facade_scene;

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn facade_scene_packet(concrete: bool) -> Vec<u8> {
    facade_scene::scene(concrete).packet()
}

#[cfg(target_arch = "wasm32")]
#[path = "../../../crates/engine/tests/support/door_scene.rs"]
mod door_scene;

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn door_scene_packet() -> Vec<u8> {
    door_scene::scene().packet()
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn ground_scene_packet() -> Vec<u8> {
    door_scene::ground_scene().packet()
}

#[cfg(target_arch = "wasm32")]
#[path = "../../../crates/engine/tests/support/paint_scene.rs"]
mod paint_scene;

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn painted_scene_packet() -> Vec<u8> {
    paint_scene::scene(false).packet()
}

#[cfg(target_arch = "wasm32")]
#[path = "../../../crates/engine/tests/support/aircon_scene.rs"]
mod aircon_scene;

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn aircon_scene_packet() -> Vec<u8> {
    aircon_scene::scene().packet()
}

#[cfg(target_arch = "wasm32")]
#[allow(dead_code)]
#[path = "../../../crates/engine/tests/support/tank_scene.rs"]
mod tank_scene;

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn tank_scene_packet(round: bool) -> Vec<u8> {
    tank_scene::scene(round).packet()
}

#[cfg(target_arch = "wasm32")]
#[path = "../../../crates/engine/tests/support/procedural_scene.rs"]
mod procedural;

#[cfg(target_arch = "wasm32")]
#[path = "../../../crates/engine/tests/support/window_scene.rs"]
mod window_scene;

#[cfg(target_arch = "wasm32")]
#[path = "../../../crates/engine/tests/support/baked_scene.rs"]
mod baked_scene;

#[cfg(target_arch = "wasm32")]
#[path = "../../../crates/engine/tests/support/shutter_scene.rs"]
mod shutter_scene;

#[cfg(target_arch = "wasm32")]
#[path = "../../../crates/engine/tests/support/pipe_scene.rs"]
mod pipe_scene;

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn pipe_scene_packet() -> Vec<u8> {
    pipe_scene::scene().packet()
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn shutter_scene_packet() -> Vec<u8> {
    shutter_scene::scene().packet()
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn weathered_window_packet() -> Vec<u8> {
    window_scene::weathered_scene().packet()
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn baked_scene_packet() -> Vec<u8> {
    baked_scene::scene().packet()
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn window_scene_packet() -> Vec<u8> {
    window_scene::scene().packet()
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn timber_scene_packet() -> Vec<u8> {
    window_scene::timber_scene().packet()
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn procedural_scene_packet() -> Vec<u8> {
    procedural::scene().packet()
}

#[cfg(target_arch = "wasm32")]
#[path = "../../../crates/engine/tests/support/pavement_scene.rs"]
mod pavement_scene;
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn pavement_scene_packet() -> Vec<u8> {
    pavement_scene::scene().packet()
}

#[cfg(target_arch = "wasm32")]
#[allow(dead_code, unused_imports)]
#[path = "../../../crates/engine/tests/span_draw.rs"]
mod span_draw;
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn span_scene_packet() -> Vec<u8> {
    span_draw::scene().packet()
}

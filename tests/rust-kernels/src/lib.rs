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

// The ordinary test bodies are omitted in Wasm; the shared golden assertions execute on both targets.
#[cfg(target_arch = "wasm32")]
#[allow(dead_code, unused_imports)]
#[path = "../../../crates/engine/tests/surface_raster.rs"]
mod surface;

#[cfg(target_arch = "wasm32")]
#[allow(dead_code, unused_imports)]
#[path = "../../../crates/engine/tests/pen_emission.rs"]
mod pen;

#[cfg(target_arch = "wasm32")]
#[allow(dead_code, unused_imports)]
#[path = "../../../crates/engine/tests/metal_surfaces.rs"]
mod metal;

#[cfg(target_arch = "wasm32")]
#[allow(dead_code, unused_imports)]
#[path = "../../../crates/engine/tests/wall_condition.rs"]
mod wall;

#[cfg(target_arch = "wasm32")]
#[allow(dead_code, unused_imports)]
#[path = "../../../crates/engine/tests/assembly_exposure.rs"]
mod exposure;

#[cfg(target_arch = "wasm32")]
#[allow(dead_code, unused_imports)]
#[path = "../../../crates/engine/tests/window_fixtures.rs"]
mod windows;

#[cfg(target_arch = "wasm32")]
#[allow(dead_code, unused_imports)]
#[path = "../../../crates/engine/tests/timber_mapping.rs"]
mod timber;

#[cfg(target_arch = "wasm32")]
#[allow(dead_code, unused_imports)]
#[path = "../../../crates/engine/tests/floor_dressing.rs"]
mod floor;

#[cfg(target_arch = "wasm32")]
#[allow(dead_code, unused_imports)]
#[path = "../../../crates/engine/tests/weathered_receivers.rs"]
mod weathered;

#[cfg(target_arch = "wasm32")]
#[allow(dead_code, unused_imports)]
#[path = "../../../crates/engine/tests/pipe_runoff.rs"]
mod pipe_runoff;

#[cfg(target_arch = "wasm32")]
#[allow(dead_code, unused_imports)]
#[path = "../../../crates/engine/tests/pipe_fixtures.rs"]
mod pipes;

#[cfg(target_arch = "wasm32")]
#[allow(dead_code, unused_imports)]
#[path = "../../../crates/engine/tests/hardware_fixtures.rs"]
mod hardware;

#[cfg(target_arch = "wasm32")]
#[allow(dead_code, unused_imports)]
#[path = "../../../crates/engine/tests/aircon_fixtures.rs"]
mod aircon;

#[cfg(target_arch = "wasm32")]
#[allow(dead_code, unused_imports)]
#[path = "../../../crates/engine/tests/painted_surfaces.rs"]
mod painted;

#[cfg(target_arch = "wasm32")]
#[allow(dead_code, unused_imports)]
#[path = "../../../crates/engine/tests/door_fixtures.rs"]
mod doors;

#[cfg(target_arch = "wasm32")]
#[allow(dead_code, unused_imports)]
#[path = "../../../crates/engine/tests/facade_material.rs"]
mod facade;

#[cfg(target_arch = "wasm32")]
#[allow(dead_code, unused_imports)]
#[path = "../../../crates/engine/tests/skin.rs"]
mod skin;

#[cfg(target_arch = "wasm32")]
#[allow(dead_code, unused_imports)]
#[path = "../../../crates/engine/tests/tile.rs"]
mod tile;

#[cfg(target_arch = "wasm32")]
#[allow(dead_code, unused_imports)]
#[path = "../../../crates/engine/tests/roof_access.rs"]
mod roof_access;

#[cfg(target_arch = "wasm32")]
#[allow(dead_code, unused_imports)]
#[path = "../../../crates/engine/tests/balconies.rs"]
mod balconies;
#[cfg(target_arch = "wasm32")]
#[path = "../../../crates/engine/tests/support/cells_scene.rs"]
mod cells_scene;
#[cfg(target_arch = "wasm32")]
#[allow(dead_code, unused_imports)]
#[path = "../../../crates/engine/tests/facade.rs"]
mod complete_facade;
#[cfg(target_arch = "wasm32")]
#[path = "../../../crates/engine/tests/support/complete_facade_scene.rs"]
mod complete_facade_scene;
#[cfg(target_arch = "wasm32")]
#[allow(dead_code, unused_imports)]
#[path = "../../../crates/engine/tests/facade_envelope.rs"]
mod facade_envelope;
#[cfg(target_arch = "wasm32")]
#[allow(dead_code, unused_imports)]
#[path = "../../../crates/engine/tests/greengrocer.rs"]
mod greengrocer;
#[cfg(target_arch = "wasm32")]
#[allow(dead_code, unused_imports)]
#[path = "../../../crates/engine/tests/grocer.rs"]
mod grocer;
#[cfg(target_arch = "wasm32")]
#[allow(dead_code, unused_imports)]
#[path = "../../../crates/engine/tests/grocer_props.rs"]
mod grocer_props;
#[cfg(target_arch = "wasm32")]
#[path = "../../../crates/engine/tests/support/grocer_scene.rs"]
mod grocer_scene;
#[cfg(target_arch = "wasm32")]
#[allow(dead_code, unused_imports)]
#[path = "../../../crates/engine/tests/hardware_shop.rs"]
mod hardware_shop;
#[cfg(target_arch = "wasm32")]
#[allow(dead_code, unused_imports)]
#[path = "../../../crates/engine/tests/herbalist_shop.rs"]
mod herbalist_shop;
#[cfg(target_arch = "wasm32")]
#[path = "../../../crates/engine/tests/support/shop_equipment_scene.rs"]
mod shop_equipment_scene;
#[cfg(target_arch = "wasm32")]
#[allow(dead_code, unused_imports)]
#[path = "../../../crates/engine/tests/shop_fittings.rs"]
mod shop_fittings;
#[cfg(target_arch = "wasm32")]
#[allow(dead_code, unused_imports)]
#[path = "../../../crates/engine/tests/shop_goods.rs"]
mod shop_goods;
#[cfg(target_arch = "wasm32")]
#[allow(dead_code, unused_imports)]
#[path = "../../../crates/engine/tests/shop_produce.rs"]
mod shop_produce;
#[cfg(target_arch = "wasm32")]
#[allow(dead_code, unused_imports)]
#[path = "../../../crates/engine/tests/shop_rooms.rs"]
mod shop_rooms;
#[cfg(target_arch = "wasm32")]
#[allow(dead_code, unused_imports)]
#[path = "../../../crates/engine/tests/shop_stock.rs"]
mod shop_stock;
#[cfg(target_arch = "wasm32")]
#[allow(dead_code, unused_imports)]
#[path = "../../../crates/engine/tests/shop_wares.rs"]
mod shop_wares;
#[cfg(target_arch = "wasm32")]
#[allow(dead_code, unused_imports)]
#[path = "../../../crates/engine/tests/snack_string.rs"]
mod snack_string;
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
#[allow(dead_code, unused_imports)]
#[path = "../../../crates/engine/tests/shop_stock_mesh.rs"]
mod shop_stock_mesh;
#[cfg(target_arch = "wasm32")]
#[path = "../../../crates/engine/tests/support/shop_stock_scene.rs"]
mod shop_stock_scene;
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn shop_stock_scene_packet() -> Vec<u8> {
    shop_stock_scene::scene().packet()
}
#[cfg(target_arch = "wasm32")]
#[allow(dead_code, unused_imports)]
#[path = "../../../crates/engine/tests/shop_forms.rs"]
mod shop_forms;
#[cfg(target_arch = "wasm32")]
#[allow(dead_code, unused_imports)]
#[path = "../../../crates/engine/tests/shop_furniture.rs"]
mod shop_furniture;
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
#[allow(dead_code, unused_imports)]
#[path = "../../../crates/engine/tests/facade_cells.rs"]
mod facade_cells;
#[cfg(target_arch = "wasm32")]
#[allow(dead_code, unused_imports)]
#[path = "../../../crates/engine/tests/facade_details.rs"]
mod facade_details;
#[cfg(target_arch = "wasm32")]
#[allow(dead_code, unused_imports)]
#[path = "../../../crates/engine/tests/facade_plan.rs"]
mod facade_plan;
#[cfg(target_arch = "wasm32")]
#[allow(dead_code, unused_imports)]
#[path = "../../../crates/engine/tests/ground.rs"]
mod ground;
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn cells_scene_packet() -> Vec<u8> {
    cells_scene::scene().packet()
}
#[cfg(target_arch = "wasm32")]
#[allow(dead_code, unused_imports)]
#[path = "../../../crates/engine/tests/rooftop.rs"]
mod rooftop;
#[cfg(target_arch = "wasm32")]
#[path = "../../../crates/engine/tests/support/rooftop_scene.rs"]
mod rooftop_scene;
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn rooftop_scene_packet() -> Vec<u8> {
    rooftop_scene::scene().packet()
}
#[cfg(target_arch = "wasm32")]
#[allow(dead_code, unused_imports)]
#[path = "../../../crates/engine/tests/sign_designs.rs"]
mod sign_designs;
#[cfg(target_arch = "wasm32")]
#[allow(dead_code, unused_imports)]
#[path = "../../../crates/engine/tests/sign_frontage.rs"]
mod sign_frontage;
#[cfg(target_arch = "wasm32")]
#[allow(dead_code, unused_imports)]
#[path = "../../../crates/engine/tests/sign_street.rs"]
mod sign_street;
#[cfg(target_arch = "wasm32")]
#[allow(dead_code, unused_imports)]
#[path = "../../../crates/engine/tests/sign_survey.rs"]
mod sign_survey;
#[cfg(target_arch = "wasm32")]
#[allow(dead_code, unused_imports)]
#[path = "../../../crates/engine/tests/signs.rs"]
mod signs;
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
#[allow(dead_code, unused_imports)]
#[path = "../../../crates/engine/tests/balcony_props.rs"]
mod balcony_props;
#[cfg(target_arch = "wasm32")]
#[allow(dead_code, unused_imports)]
#[path = "../../../crates/engine/tests/household.rs"]
mod household;
#[cfg(target_arch = "wasm32")]
#[allow(dead_code, unused_imports)]
#[path = "../../../crates/engine/tests/tarps.rs"]
mod tarps;
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
#[allow(dead_code, unused_imports)]
#[path = "../../../crates/engine/tests/laundry.rs"]
mod laundry;
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
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn check_surface_kernels() -> bool {
    surface::check_reference_rasters();
    true
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn check_procedural_meshes() -> bool {
    pen::check_reference_pen();
    windows::check_reference_windows();
    timber::check_reference_timber();
    floor::check_reference_floor();
    weathered::check_reference_weathered();
    pipes::check_reference_pipes();
    hardware::check_reference_hardware();
    aircon::check_reference_aircon();
    painted::check_reference_paint();
    doors::check_reference_doors();
    facade::check_reference_facade();
    skin::check_reference_skin();
    skin::check_reference_stains();
    skin::check_household_wash_boundaries();
    tile::check_reference_tiles();
    roof_access::check_reference_roof_access();
    balcony_props::check_reference_balcony_props();
    laundry::check_reference_laundry();
    household::check_reference_household();
    tarps::check_reference_tarps();
    balconies::check_reference_balconies();
    facade_details::check_reference_facade_details();
    signs::check_reference_signs();
    signs::check_reference_panels();
    signs::check_reference_mounts();
    signs::check_reference_projecting();
    sign_street::check_reference_streets();
    sign_frontage::check_reference_frontages();
    ground::check_reference_ground();
    facade_cells::check_reference_facade_cells();
    facade_plan::check_reference_facade_plans();
    facade_envelope::check_reference_facade_envelopes();
    complete_facade::check_reference_facades();
    shop_forms::check_reference_shop_forms();
    shop_furniture::check_reference_shop_furniture();
    shop_fittings::check_reference_shop_fittings();
    shop_stock_mesh::check_reference_stock_mesh();
    grocer_props::check_reference_grocer_props();
    shop_rooms::check_reference_shop_rooms();
    shop_stock::check_reference_shop_stock();
    snack_string::check_reference_snack_strings();
    shop_wares::check_reference_shop_wares();
    shop_produce::check_reference_shop_produce();
    grocer::check_reference_grocer();
    greengrocer::check_reference_greengrocer();
    hardware_shop::check_reference_hardware_shop();
    herbalist_shop::check_reference_herbalist_shop();
    restaurant_food::check_reference_restaurant_food();
    restaurant_surfaces::check_reference_restaurant_surfaces();
    restaurant_fittings::check_reference_restaurant_fittings();
    restaurant_details::check_reference_restaurant_details();
    roast::check_reference_roast();
    shop_display::check_reference_shop_display();
    shop_shell::check_reference_shop_shell();
    open_facade::check_reference_open_facades();
    clearance::check_reference_clearance();
    pavement_layout::check_reference_pavement_layout();
    pavement_surface::check_reference_pavement_surfaces();
    pavement_plan::check_reference_pavement_plans();
    pavement_render::check_reference_pavement_render();
    pavement_materials::check_reference_pavement_materials();
    deck::check_reference_decks();
    building_placement::check_reference_building_placement();
    building_shell::check_reference_building_shells();
    building::check_reference_buildings();
    span_foundations::check_reference_span_foundations();
    span_anchors::check_reference_span_anchors();
    span_classification::check_reference_span_classification();
    span_plan::check_reference_span_plans();
    span_draw::check_reference_span_drawings();
    receiver_edges::check_reference_receiver_edges();
    kit_materials::check_reference_kit_materials();
    shop_goods::check_reference_shop_goods();
    rooftop::check_reference_rooftop();
    sign_designs::check_reference_copy();
    sign_designs::check_reference_designs();
    sign_survey::check_reference_survey();
    sign_survey::check_reference_book();
    true
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn check_metal_recipes() -> bool {
    metal::check_reference_metal();
    exposure::check_reference_exposure();
    pipe_runoff::check_reference_pipe_runoff();
    true
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn check_wall_recipes() -> bool {
    wall::check_reference_walls();
    true
}

#[cfg(target_arch = "wasm32")]
#[allow(dead_code, unused_imports)]
#[path = "../../../crates/engine/tests/restaurant_food.rs"]
mod restaurant_food;

#[cfg(target_arch = "wasm32")]
#[allow(dead_code, unused_imports)]
#[path = "../../../crates/engine/tests/restaurant_surfaces.rs"]
mod restaurant_surfaces;

#[cfg(target_arch = "wasm32")]
#[allow(dead_code, unused_imports)]
#[path = "../../../crates/engine/tests/restaurant_fittings.rs"]
mod restaurant_fittings;

#[cfg(target_arch = "wasm32")]
#[allow(dead_code, unused_imports)]
#[path = "../../../crates/engine/tests/restaurant_details.rs"]
mod restaurant_details;

#[cfg(target_arch = "wasm32")]
#[allow(dead_code, unused_imports)]
#[path = "../../../crates/engine/tests/roast.rs"]
mod roast;

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn roast_scene_packet() -> Vec<u8> {
    grocer_scene::roast_scene().packet()
}

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
#[path = "../../../crates/engine/tests/building.rs"]
mod building;
#[cfg(target_arch = "wasm32")]
#[allow(dead_code, unused_imports)]
#[path = "../../../crates/engine/tests/building_placement.rs"]
mod building_placement;
#[cfg(target_arch = "wasm32")]
#[allow(dead_code, unused_imports)]
#[path = "../../../crates/engine/tests/building_shell.rs"]
mod building_shell;
#[path = "../../../crates/engine/tests/clearance.rs"]
#[cfg(target_arch = "wasm32")]
mod clearance;
#[cfg(target_arch = "wasm32")]
#[allow(dead_code, unused_imports)]
#[path = "../../../crates/engine/tests/deck.rs"]
mod deck;
#[cfg(target_arch = "wasm32")]
#[allow(dead_code, unused_imports)]
#[path = "../../../crates/engine/tests/kit_materials.rs"]
mod kit_materials;
#[cfg(target_arch = "wasm32")]
#[allow(dead_code, unused_imports)]
#[path = "../../../crates/engine/tests/open_facade.rs"]
mod open_facade;
#[cfg(target_arch = "wasm32")]
#[allow(dead_code, unused_imports)]
#[path = "../../../crates/engine/tests/pavement_layout.rs"]
mod pavement_layout;
#[cfg(target_arch = "wasm32")]
#[allow(dead_code, unused_imports)]
#[path = "../../../crates/engine/tests/pavement_materials.rs"]
mod pavement_materials;
#[cfg(target_arch = "wasm32")]
#[allow(dead_code, unused_imports)]
#[path = "../../../crates/engine/tests/pavement_plan.rs"]
mod pavement_plan;
#[cfg(target_arch = "wasm32")]
#[allow(dead_code, unused_imports)]
#[path = "../../../crates/engine/tests/pavement_render.rs"]
mod pavement_render;
#[cfg(target_arch = "wasm32")]
#[path = "../../../crates/engine/tests/support/pavement_scene.rs"]
mod pavement_scene;
#[cfg(target_arch = "wasm32")]
#[allow(dead_code, unused_imports)]
#[path = "../../../crates/engine/tests/pavement_surface.rs"]
mod pavement_surface;
#[cfg(target_arch = "wasm32")]
#[allow(dead_code, unused_imports)]
#[path = "../../../crates/engine/tests/receiver_edges.rs"]
mod receiver_edges;
#[cfg(target_arch = "wasm32")]
#[allow(dead_code, unused_imports)]
#[path = "../../../crates/engine/tests/span_anchors.rs"]
mod span_anchors;
#[cfg(target_arch = "wasm32")]
#[allow(dead_code, unused_imports)]
#[path = "../../../crates/engine/tests/span_classification.rs"]
mod span_classification;
#[cfg(target_arch = "wasm32")]
#[allow(dead_code, unused_imports)]
#[path = "../../../crates/engine/tests/span_draw.rs"]
mod span_draw;
#[cfg(target_arch = "wasm32")]
#[allow(dead_code, unused_imports)]
#[path = "../../../crates/engine/tests/span_foundations.rs"]
mod span_foundations;
#[cfg(target_arch = "wasm32")]
#[allow(dead_code, unused_imports)]
#[path = "../../../crates/engine/tests/span_plan.rs"]
mod span_plan;
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn pavement_scene_packet() -> Vec<u8> {
    pavement_scene::scene().packet()
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn span_scene_packet() -> Vec<u8> {
    span_draw::scene().packet()
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn open_facade_scene_packet() -> Vec<u8> {
    complete_facade_scene::open_scene().packet()
}

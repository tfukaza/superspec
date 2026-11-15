//! Authored wash and HVAC atlas recipes for kowloon-street@1.

use super::{Image, image};
use superspec_types::rng::SeededRng;

#[path = "prop_textures/aircon.rs"]
mod aircon;
#[path = "prop_textures/cloth.rs"]
mod cloth;
#[path = "prop_textures/raster.rs"]
mod raster;

const CELL: usize = 256;
const COLUMNS: usize = 8;
const ROWS: usize = 4;

fn atlas(
    scale: usize,
    seed: u32,
    count: usize,
    paint: fn(usize, &mut SeededRng) -> raster::Canvas,
) -> Result<Vec<u8>, String> {
    let size = CELL * scale;
    let width = COLUMNS * size;
    let height = ROWS * size;
    let mut atlas = vec![[128, 128, 128, 255]; width * height];
    let mut rng = SeededRng::from_u32(seed);
    for index in 0..count {
        let tile = paint(index, &mut rng);
        let x = index % COLUMNS * size;
        let y = index / COLUMNS * size;
        for (row, pixels) in tile.pixels.chunks_exact(size).enumerate() {
            let start = (y + row) * width + x;
            atlas[start..start + size].copy_from_slice(pixels);
        }
    }
    let bytes = image::encode(&Image {
        width,
        height,
        rgba: atlas.into_flattened(),
        mime: "image/png",
    })?;
    Ok(bytes)
}

pub fn cloth() -> Result<Vec<u8>, String> {
    atlas(1, 41, cloth::COUNT, cloth::tile)
}
pub fn aircon() -> Result<Vec<u8>, String> {
    atlas(2, 43, aircon::COUNT, aircon::tile)
}

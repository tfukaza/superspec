//! Native image/catalog inputs and ordered publication.
use super::{decode, fs, image};
use std::path::Path;
use superspec_engine::{
    materialspec::authoring::image_ops, worldspec::themes::kowloon::authoring::stock_meshes,
};

pub(super) fn resize(source: &Path, edge: usize, output: &Path) -> Result<(), String> {
    if !(1..=4096).contains(&edge) {
        return Err("Texture edge must be between 1 and 4096 pixels.".into());
    }
    let resized = image_ops::resize_max(&decode(source)?, edge)?;
    fs::write(output, image::encode(&resized)?).map_err(|e| e.to_string())
}
pub(super) fn apples(source: &Path, output: &Path) -> Result<(), String> {
    let bytes = stock_meshes::apples(&fs::read(source).map_err(|e| e.to_string())?)?;
    fs::write(output, bytes).map_err(|e| e.to_string())
}
pub(super) fn packet(source: &Path, aspect: f64, output: &Path) -> Result<(), String> {
    if !aspect.is_finite() || aspect <= 0. {
        return Err("Packet aspect ratio must be positive and finite.".into());
    }
    let extended = stock_meshes::packet_image(decode(source)?)?;
    fs::create_dir(output).map_err(|e| e.to_string())?;
    fs::write(
        output.join("snack-packet-base.png"),
        image::encode(&extended)?,
    )
    .map_err(|e| e.to_string())?;
    fs::write(
        output.join("snack-packet.json"),
        stock_meshes::encode_faces(&mut stock_meshes::packet_faces(aspect)?)?,
    )
    .map_err(|e| e.to_string())
}

//! Embedded source-image extraction from the supported single-material GLB form.
use serde_json::Value;

pub fn image_name(stem: &str, channel: &str) -> Result<String, String> {
    if stem.is_empty()
        || !stem
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        return Err(format!("Invalid atlas input name {stem}."));
    }
    Ok(format!("{stem}-{channel}"))
}

pub fn extract(
    bytes: &[u8],
    stem: &str,
    emit: &mut dyn FnMut(&str, Option<&[u8]>) -> Result<(), String>,
) -> Result<(), String> {
    let number = |at: usize| {
        bytes
            .get(at..at + 4)
            .map(|slice| u32::from_le_bytes(slice.try_into().unwrap()) as usize)
    };
    if bytes.get(..4) != Some(b"glTF")
        || number(4) != Some(2)
        || number(8) != Some(bytes.len())
        || bytes.get(16..20) != Some(b"JSON")
    {
        return Err("Expected a glTF 2 binary model.".into());
    }
    let json_length = number(12).ok_or("Missing GLB JSON length.")?;
    let json_end = 20usize
        .checked_add(json_length)
        .ok_or("Invalid GLB JSON length.")?;
    let document: Value =
        serde_json::from_slice(bytes.get(20..json_end).ok_or("Truncated GLB JSON.")?)
            .map_err(|e| e.to_string())?;
    let bin_length = number(json_end).ok_or("Missing GLB binary chunk.")?;
    if bytes.get(json_end + 4..json_end + 8) != Some(b"BIN\0") {
        return Err("Expected an embedded GLB binary chunk.".into());
    }
    let binary = bytes
        .get(json_end + 8..json_end + 8 + bin_length)
        .ok_or("Truncated GLB binary chunk.")?;
    let material = document["materials"]
        .as_array()
        .and_then(|values| match values.as_slice() {
            [material] => Some(material),
            _ => None,
        })
        .ok_or("Expected one source material.")?;
    let index = |value: &Value| {
        value
            .as_u64()
            .and_then(|v| usize::try_from(v).ok())
            .ok_or("Invalid GLB map index.")
    };
    let pbr = &material["pbrMetallicRoughness"];
    for (channel, reference) in [
        ("color", &pbr["baseColorTexture"]),
        ("normal", &material["normalTexture"]),
        ("properties", &pbr["metallicRoughnessTexture"]),
    ] {
        let name = image_name(stem, channel)?;
        if reference.is_null() {
            emit(&name, None)?;
            continue;
        }
        let texture = &document["textures"][index(&reference["index"])?];
        let image = &document["images"][index(&texture["source"])?];
        let view = &document["bufferViews"][index(&image["bufferView"])?];
        if view["buffer"].as_u64() != Some(0) {
            return Err("Expected an embedded image in GLB buffer zero.".into());
        }
        let start = index(&view["byteOffset"]).unwrap_or(0);
        let length = index(&view["byteLength"])?;
        let end = start
            .checked_add(length)
            .ok_or("Invalid GLB image length.")?;
        emit(
            &name,
            Some(
                binary
                    .get(start..end)
                    .ok_or("GLB image falls outside its buffer.")?,
            ),
        )?;
    }
    Ok(())
}

mod accessor;
mod materials;
mod scene;
use super::{AssetReader, ItemDiagnostic, ItemImage, check_cancelled, fail};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, sync::Arc};

use superspec_types::limits::{MAX_FILE_BYTES as MAX_BYTES, MAX_IMAGE_BYTES};
/// Bytes one import may hold: the model, its external files and copied images.
const MAX_HELD_BYTES: usize = superspec_types::limits::MAX_COMPANION_BYTES;
/// Decoded (PNG and JPEG) pixels; KTX2 images pass through without decoding.
const MAX_PIXELS: usize = superspec_types::limits::MAX_SCENE_IMAGE_PIXELS;

pub(super) struct Package {
    pub(super) hashes: BTreeMap<String, String>,
    source: String,
    document: Value,
    buffers: Vec<Arc<[u8]>>,
    images: Vec<ItemImage>,
    scene: Option<usize>,
}
impl Package {
    fn error(&self, pointer: &str, message: impl Into<String>) -> ItemDiagnostic {
        fail(&self.source, pointer, "ITEM_GLB", message)
    }
    fn array(&self, key: &str, maximum: usize) -> Result<&[Value], ItemDiagnostic> {
        match self.document.get(key) {
            None => Ok(&[]),
            Some(value) => value
                .as_array()
                .filter(|array| array.len() <= maximum)
                .map(Vec::as_slice)
                .ok_or_else(|| {
                    self.error(
                        &format!("/{key}"),
                        format!("{key} must be an array with at most {maximum} entries."),
                    )
                }),
        }
    }
    fn index(&self, value: &Value, pointer: &str) -> Result<usize, ItemDiagnostic> {
        value
            .as_u64()
            .and_then(|value| usize::try_from(value).ok())
            .ok_or_else(|| self.error(pointer, "Expected a nonnegative bounded integer."))
    }
    fn number(&self, value: &Value, pointer: &str) -> Result<f64, ItemDiagnostic> {
        value
            .as_f64()
            .filter(|v| v.is_finite())
            .ok_or_else(|| self.error(pointer, "Expected a finite number."))
    }
    fn vector<const N: usize>(
        &self,
        value: Option<&Value>,
        default: [f64; N],
        pointer: &str,
    ) -> Result<[f64; N], ItemDiagnostic> {
        let Some(value) = value else {
            return Ok(default);
        };
        let array = value
            .as_array()
            .filter(|a| a.len() == N)
            .ok_or_else(|| self.error(pointer, format!("Expected {N} numeric components.")))?;
        let mut result = [0.; N];
        for (index, value) in array.iter().enumerate() {
            result[index] = self.number(value, &format!("{pointer}/{index}"))?;
        }
        Ok(result)
    }
    fn view(&self, index: usize, pointer: &str) -> Result<&[u8], ItemDiagnostic> {
        let views = self.array("bufferViews", 16384)?;
        let view = views
            .get(index)
            .ok_or_else(|| self.error(pointer, "Missing buffer view."))?;
        let buffer = self.index(&view["buffer"], &format!("/bufferViews/{index}/buffer"))?;
        let bytes = self
            .buffers
            .get(buffer)
            .ok_or_else(|| self.error(pointer, "Missing buffer."))?;
        let start = view
            .get("byteOffset")
            .map(|v| self.index(v, pointer))
            .transpose()?
            .unwrap_or(0);
        let length = self.index(&view["byteLength"], pointer)?;
        let end = start
            .checked_add(length)
            .ok_or_else(|| self.error(pointer, "Buffer view range overflow."))?;
        bytes
            .get(start..end)
            .ok_or_else(|| self.error(pointer, "Buffer view exceeds its declared buffer."))
    }
}

pub(super) fn prepare(
    source: &str,
    bytes: Arc<[u8]>,
    scene: Option<usize>,
    reader: &AssetReader<'_>,
    cancelled: &dyn Fn() -> bool,
) -> Result<Package, ItemDiagnostic> {
    check_cancelled(source, cancelled)?;
    let error = |pointer: &str, message: &str| fail(source, pointer, "ITEM_GLB", message);
    if bytes.len() > MAX_BYTES || bytes.len() < 20 {
        return Err(error("/", "GLB must be between 20 bytes and 512 MiB."));
    }
    let number = |offset: usize| {
        bytes
            .get(offset..offset + 4)
            .map(|v| u32::from_le_bytes(v.try_into().unwrap()) as usize)
    };
    if bytes.get(..4) != Some(b"glTF") || number(4) != Some(2) || number(8) != Some(bytes.len()) {
        return Err(error("/", "Expected a complete glTF 2 binary container."));
    }
    let mut offset = 12usize;
    let mut json = None;
    let mut binary = None;
    while offset < bytes.len() {
        let length = number(offset).ok_or_else(|| error("/", "Truncated GLB chunk header."))?;
        let kind = bytes
            .get(offset + 4..offset + 8)
            .ok_or_else(|| error("/", "Truncated GLB chunk type."))?;
        let start = offset
            .checked_add(8)
            .ok_or_else(|| error("/", "GLB chunk overflow."))?;
        let end = start
            .checked_add(length)
            .filter(|end| *end <= bytes.len())
            .ok_or_else(|| error("/", "Truncated GLB chunk."))?;
        if !length.is_multiple_of(4) {
            return Err(error("/", "GLB chunks must be aligned to four bytes."));
        }
        if offset == 12 && kind != b"JSON" {
            return Err(error("/", "The first GLB chunk must be JSON."));
        }
        if kind == b"JSON" {
            if json.replace(&bytes[start..end]).is_some() {
                return Err(error("/", "Duplicate GLB JSON chunk."));
            }
        } else if kind == b"BIN\0" {
            if binary.replace(&bytes[start..end]).is_some() {
                return Err(error("/", "Duplicate GLB binary chunk."));
            }
        }
        offset = end;
    }
    let json = json.ok_or_else(|| error("/", "Missing GLB JSON chunk."))?;
    if json.len() > 8 * 1024 * 1024 {
        return Err(error("/", "GLB JSON exceeds 8 MiB."));
    }
    let document: Value = serde_json::from_slice(json)
        .map_err(|e| fail(source, "/", "ITEM_GLB_JSON", e.to_string()))?;
    if document["asset"]["version"].as_str() != Some("2.0") {
        return Err(error("/asset/version", "Expected glTF asset version 2.0."));
    }
    let mut result = Package {
        hashes: BTreeMap::from([(source.into(), format!("{:x}", Sha256::digest(&bytes)))]),
        source: source.into(),
        document,
        buffers: vec![],
        images: vec![],
        scene,
    };
    for extension in result.array("extensionsRequired", 32)? {
        if !matches!(
            extension.as_str(),
            Some(
                "KHR_texture_transform"
                    | "KHR_materials_unlit"
                    | "KHR_materials_emissive_strength"
                    | "KHR_texture_basisu"
            )
        ) {
            return Err(result.error(
                "/extensionsRequired",
                "This static item importer does not support a required extension.",
            ));
        }
    }
    if !result.array("skins", 256)?.is_empty() || !result.array("animations", 256)?.is_empty() {
        return Err(result.error(
            "/",
            "Skinned and animated assets require an animated item importer.",
        ));
    }
    result.array("nodes", 4096)?;
    result.array("meshes", 1024)?;
    result.array("materials", 1024)?;
    result.array("textures", 2048)?;
    result.array("samplers", 1024)?;
    let buffers = result.array("buffers", 64)?.to_vec();
    let mut total = bytes.len();
    for (index, buffer) in buffers.iter().enumerate() {
        check_cancelled(source, cancelled)?;
        let pointer = format!("/buffers/{index}");
        let length = result.index(&buffer["byteLength"], &pointer)?;
        let data = if let Some(uri) = buffer.get("uri") {
            let uri = uri
                .as_str()
                .ok_or_else(|| result.error(&pointer, "Buffer URI must be a string."))?;
            load(&mut result, uri, reader, &mut total)?
        } else if index == 0 {
            Arc::from(binary.unwrap_or_default())
        } else {
            return Err(result.error(&pointer, "Only buffer zero may use the GLB binary chunk."));
        };
        if length > data.len()
            || length > MAX_BYTES
            || (buffer.get("uri").is_none() && data.len() - length > 3)
        {
            return Err(result.error(&pointer, "Buffer length does not match its bytes."));
        }
        result.buffers.push(if length == data.len() {
            data
        } else {
            Arc::from(&data[..length])
        });
    }
    let images = result.array("images", 1024)?.to_vec();
    let mut pixels = 0usize;
    for (index, image) in images.iter().enumerate() {
        check_cancelled(source, cancelled)?;
        let pointer = format!("/images/{index}");
        if image.get("uri").is_some() && image.get("bufferView").is_some() {
            return Err(result.error(&pointer, "Image must use either a URI or a buffer view."));
        }
        let data = if let Some(uri) = image.get("uri") {
            load(
                &mut result,
                uri.as_str()
                    .ok_or_else(|| error(&pointer, "Image URI must be a string."))?,
                reader,
                &mut total,
            )?
        } else {
            let view = result.index(&image["bufferView"], &pointer)?;
            let data: Arc<[u8]> = Arc::from(result.view(view, &pointer)?);
            // `total` bounds the bytes this import holds: the GLB plus each copied image.
            total = total
                .checked_add(data.len())
                .filter(|n| *n <= MAX_HELD_BYTES)
                .ok_or_else(|| {
                    result.error(&pointer, "Imported model and image copies exceed 1 GiB.")
                })?;
            data
        };
        if data.len() > MAX_IMAGE_BYTES {
            return Err(result.error(&pointer, "Imported image exceeds 64 MiB."));
        }
        let decoded = if data.starts_with(&superspec_geometry::ktx2::MAGIC) {
            // KTX2 is checked structurally and passed through for the renderer to upload.
            let (width, height) =
                superspec_geometry::ktx2::inspect(&data).map_err(|e| result.error(&pointer, e))?;
            superspec_geometry::image::Image {
                width,
                height,
                rgba: vec![],
                mime: superspec_geometry::ktx2::MIME,
            }
        } else {
            superspec_geometry::image::decode(&data).map_err(|e| result.error(&pointer, e))?
        };
        let decoded_pixels = if decoded.mime == superspec_geometry::ktx2::MIME {
            0
        } else {
            decoded
                .width
                .checked_mul(decoded.height)
                .ok_or_else(|| result.error(&pointer, "Image dimensions overflow."))?
        };
        pixels = pixels
            .checked_add(decoded_pixels)
            .filter(|n| *n <= MAX_PIXELS)
            .ok_or_else(|| {
                result.error(
                    &pointer,
                    "Imported images exceed the total pixel allowance.",
                )
            })?;
        if let Some(mime) = image.get("mimeType")
            && mime.as_str() != Some(decoded.mime)
        {
            return Err(result.error(&pointer, "Image MIME type does not match its bytes."));
        }
        result.images.push(ItemImage {
            id: format!("image/{index}"),
            mime: decoded.mime.into(),
            bytes: data,
        });
    }
    check_cancelled(source, cancelled)?;
    Ok(result)
}

fn load(
    package: &mut Package,
    uri: &str,
    reader: &AssetReader<'_>,
    total: &mut usize,
) -> Result<Arc<[u8]>, ItemDiagnostic> {
    if uri.len() > 24 * 1024 * 1024 {
        return Err(package.error("/", "Asset URI exceeds its allowance."));
    }
    let (name, bytes) = if uri.starts_with("data:") {
        let (header, data) = uri
            .split_once(',')
            .ok_or_else(|| package.error("/", "Invalid data URI."))?;
        if !header.ends_with(";base64") {
            return Err(package.error("/", "Only base64 data URIs are supported."));
        }
        (
            format!("data:{:x}", Sha256::digest(uri.as_bytes())),
            Arc::from(base64(data).map_err(|message| package.error("/", message))?),
        )
    } else {
        let name =
            reference(&package.source, uri).map_err(|message| package.error("/", message))?;
        let bytes =
            reader(&name).map_err(|message| package.error("/", format!("{name}: {message}")))?;
        (name, bytes)
    };
    *total = total
        .checked_add(bytes.len())
        .filter(|n| *n <= MAX_HELD_BYTES)
        .ok_or_else(|| package.error("/", "Imported model and its files exceed 1 GiB."))?;
    let hash = format!("{:x}", Sha256::digest(&bytes));
    if package
        .hashes
        .get(&name)
        .is_some_and(|previous| previous != &hash)
    {
        return Err(package.error(
            "/",
            "An asset resolver returned different bytes for the same reference during one import.",
        ));
    }
    package.hashes.insert(name, hash);
    Ok(bytes)
}
fn reference(source: &str, uri: &str) -> Result<String, String> {
    if uri.is_empty() || uri.starts_with('/') || uri.contains(['\\', ':', '?', '#', '%']) {
        return Err("External GLB references must be plain relative asset paths.".into());
    }
    let mut parts = source
        .rsplit_once('/')
        .map_or(Vec::new(), |(parent, _)| parent.split('/').collect());
    for part in uri.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                if parts.pop().is_none() {
                    return Err("GLB reference escapes its asset root.".into());
                }
            }
            _ => parts.push(part),
        }
    }
    if parts.is_empty() {
        return Err("Invalid GLB reference.".into());
    }
    Ok(parts.join("/"))
}
fn base64(text: &str) -> Result<Vec<u8>, String> {
    if !text.len().is_multiple_of(4) {
        return Err("Invalid base64 data length.".into());
    }
    let value = |byte: u8| match byte {
        b'A'..=b'Z' => Some(byte - b'A'),
        b'a'..=b'z' => Some(byte - b'a' + 26),
        b'0'..=b'9' => Some(byte - b'0' + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    };
    let mut output = Vec::with_capacity(text.len() / 4 * 3);
    for (index, group) in text.as_bytes().chunks_exact(4).enumerate() {
        let final_group = index + 1 == text.len() / 4;
        let a = value(group[0]).ok_or("Invalid base64 character.")?;
        let b = value(group[1]).ok_or("Invalid base64 character.")?;
        output.push((a << 2) | (b >> 4));
        if group[2] == b'=' {
            if !final_group || group[3] != b'=' || b & 15 != 0 {
                return Err("Invalid base64 padding.".into());
            }
            continue;
        }
        let c = value(group[2]).ok_or("Invalid base64 character.")?;
        output.push((b << 4) | (c >> 2));
        if group[3] == b'=' {
            if !final_group || c & 3 != 0 {
                return Err("Invalid base64 padding.".into());
            }
            continue;
        }
        output.push((c << 6) | value(group[3]).ok_or("Invalid base64 character.")?);
    }
    Ok(output)
}

//! Studio project archives (`.studio.zip`), as Material Studio saves large projects: `project.json`
//! is the JSON project document except that each `assets` value names its archive entry instead of
//! holding base64. Stored and deflated entries are read; ZIP64 is not needed below 4 GiB.
use std::{collections::BTreeMap, io::Read, sync::Arc};
use superspec_types::limits::{MAX_COMPANION_BYTES, MAX_COMPANIONS, MAX_FILE_BYTES};

pub const PROJECT_ENTRY: &str = "project.json";
const ASSET_DIRECTORY: &str = "assets/";
const LOCAL: u32 = 0x0403_4b50;
const CENTRAL: u32 = 0x0201_4b50;
const END: u32 = 0x0605_4b50;
const MAX_DOCUMENT_BYTES: usize = 64 * 1024 * 1024;

pub fn is_archive(bytes: &[u8]) -> bool {
    bytes.len() >= 4 && u32_at(bytes, 0) == Some(LOCAL)
}

fn u16_at(bytes: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(bytes.get(at..at + 2)?.try_into().ok()?))
}

fn u32_at(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
}

/// Reads every file entry. `limit(name, size)` sees each declared size before its bytes are read.
fn entries(
    bytes: &[u8],
    limit: &mut dyn FnMut(&str, usize) -> Result<(), String>,
) -> Result<BTreeMap<String, Vec<u8>>, String> {
    let corrupt = |what: &str| format!("Corrupt project archive {what}.");
    let lowest = bytes.len().saturating_sub(22 + 0xffff);
    let end = (lowest..=bytes.len().saturating_sub(22))
        .rev()
        .find(|&at| u32_at(bytes, at) == Some(END))
        .ok_or("This is not a ZIP project archive.")?;
    let count = u16_at(bytes, end + 10).ok_or_else(|| corrupt("directory"))?;
    let mut at = u32_at(bytes, end + 16).ok_or_else(|| corrupt("directory"))? as usize;
    let mut result = BTreeMap::new();
    for _ in 0..count {
        let field16 = |offset| u16_at(bytes, at + offset).ok_or_else(|| corrupt("directory"));
        let field32 = |offset| u32_at(bytes, at + offset).ok_or_else(|| corrupt("directory"));
        if field32(0)? != CENTRAL {
            return Err(corrupt("directory"));
        }
        let (flags, method) = (field16(8)?, field16(10)?);
        let (crc, compressed, size) = (field32(16)?, field32(20)? as usize, field32(24)? as usize);
        let name_length = field16(28)? as usize;
        let skip = name_length + field16(30)? as usize + field16(32)? as usize;
        let local = field32(42)? as usize;
        let name = std::str::from_utf8(
            bytes
                .get(at + 46..at + 46 + name_length)
                .ok_or_else(|| corrupt("directory"))?,
        )
        .map_err(|_| "Project archive entry names must be UTF-8.")?
        .to_owned();
        at += 46 + skip;
        if name.ends_with('/') {
            continue;
        }
        if flags & 1 != 0 {
            return Err(format!("Project archive entry {name} is encrypted."));
        }
        if method != 0 && method != 8 {
            return Err(format!(
                "Project archive entry {name} uses an unsupported compression method."
            ));
        }
        if result.contains_key(&name) {
            return Err(format!("Project archive contains {name} twice."));
        }
        limit(&name, size)?;
        if u32_at(bytes, local) != Some(LOCAL) {
            return Err(corrupt(&format!("entry {name}")));
        }
        let start = local
            + 30
            + u16_at(bytes, local + 26).ok_or_else(|| corrupt("entry"))? as usize
            + u16_at(bytes, local + 28).ok_or_else(|| corrupt("entry"))? as usize;
        let data = bytes
            .get(start..start + compressed)
            .ok_or_else(|| format!("Truncated project archive entry {name}."))?;
        let content = if method == 0 {
            data.to_vec()
        } else {
            let mut content = Vec::with_capacity(size);
            flate2::read::DeflateDecoder::new(data)
                .take(size as u64 + 1)
                .read_to_end(&mut content)
                .map_err(|error| format!("Project archive entry {name}: {error}"))?;
            content
        };
        if content.len() != size || crc32fast::hash(&content) != crc {
            return Err(format!(
                "Project archive entry {name} is damaged (checksum mismatch)."
            ));
        }
        result.insert(name, content);
    }
    Ok(result)
}

/// A project document without its `assets` field, and the companions it names.
pub type Contents = (serde_json::Value, BTreeMap<String, Arc<[u8]>>);

/// The project document and its companions, under the same allowances as JSON projects.
pub fn read(bytes: &[u8]) -> Result<Contents, String> {
    let mut total = 0usize;
    let mut files = entries(bytes, &mut |name, size| {
        if size
            > if name == PROJECT_ENTRY {
                MAX_DOCUMENT_BYTES
            } else {
                MAX_FILE_BYTES
            }
        {
            return Err(format!(
                "Project archive entry {name} exceeds its byte allowance."
            ));
        }
        total += size;
        if total > MAX_COMPANION_BYTES + MAX_DOCUMENT_BYTES {
            return Err("Project archive exceeds its companion byte allowance.".into());
        }
        Ok(())
    })?;
    let document = files
        .remove(PROJECT_ENTRY)
        .ok_or_else(|| format!("The project archive has no {PROJECT_ENTRY}."))?;
    let mut value: serde_json::Value = serde_json::from_slice(&document)
        .map_err(|error| format!("Invalid studio project JSON: {error}"))?;
    let paths = value
        .as_object_mut()
        .ok_or("Studio project must be an object.")?
        .remove("assets")
        .ok_or("A studio bundle must contain an assets object.")?;
    let paths = paths
        .as_object()
        .ok_or("A studio bundle must contain an assets object.")?;
    if paths.len() > MAX_COMPANIONS {
        return Err("Bundle contains too many companions.".into());
    }
    let mut assets = BTreeMap::new();
    let mut total = 0usize;
    for (name, path) in paths {
        let bytes = path
            .as_str()
            .filter(|path| path.starts_with(ASSET_DIRECTORY))
            .and_then(|path| files.get(path))
            .ok_or_else(|| format!("The project archive is missing companion {name}."))?;
        total += bytes.len();
        if total > MAX_COMPANION_BYTES {
            return Err("Bundle companions exceed their byte allowance.".into());
        }
        assets.insert(name.clone(), Arc::from(bytes.as_slice()));
    }
    Ok((value, assets))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A minimal ZIP writer, mirroring the browser's; `deflate` compresses every entry.
    pub fn archive(entries: &[(&str, &[u8])], deflate: bool) -> Vec<u8> {
        use std::io::Write;
        let (mut output, mut directory) = (vec![], vec![]);
        for (name, bytes) in entries {
            let data = if deflate {
                let mut encoder =
                    flate2::write::DeflateEncoder::new(vec![], flate2::Compression::default());
                encoder.write_all(bytes).unwrap();
                encoder.finish().unwrap()
            } else {
                bytes.to_vec()
            };
            let method: u16 = if deflate { 8 } else { 0 };
            let header = |signature: u32| {
                let mut header = signature.to_le_bytes().to_vec();
                if signature == CENTRAL {
                    header.extend(20u16.to_le_bytes());
                }
                for value in [20u16, 0x0800, method, 0, 33] {
                    header.extend(value.to_le_bytes());
                }
                for value in [
                    crc32fast::hash(bytes),
                    data.len() as u32,
                    bytes.len() as u32,
                ] {
                    header.extend(value.to_le_bytes());
                }
                header.extend((name.len() as u16).to_le_bytes());
                header.extend(0u16.to_le_bytes());
                header
            };
            let mut central = header(CENTRAL);
            // Comment length, disk number, internal and external attributes.
            central.extend([0u8; 10]);
            central.extend((output.len() as u32).to_le_bytes());
            central.extend(name.as_bytes());
            directory.extend(central);
            output.extend(header(LOCAL));
            output.extend(name.as_bytes());
            output.extend(data);
        }
        let offset = output.len() as u32;
        let size = directory.len() as u32;
        output.extend(directory);
        output.extend(END.to_le_bytes());
        output.extend([0u8; 4]);
        output.extend((entries.len() as u16).to_le_bytes());
        output.extend((entries.len() as u16).to_le_bytes());
        output.extend(size.to_le_bytes());
        output.extend(offset.to_le_bytes());
        output.extend([0u8; 2]);
        output
    }

    #[test]
    fn archives_map_companions_to_entries_and_reject_damage() {
        let document = br#"{"kind":"material","assets":{"a.png":"assets/a.png"}}"#;
        for deflate in [false, true] {
            let bytes = archive(
                &[(PROJECT_ENTRY, document), ("assets/a.png", b"image bytes")],
                deflate,
            );
            assert!(is_archive(&bytes));
            let (value, assets) = read(&bytes).unwrap();
            assert_eq!(value["kind"], "material");
            assert!(value.get("assets").is_none());
            assert_eq!(&*assets["a.png"], b"image bytes");
        }
        let missing = archive(&[(PROJECT_ENTRY, document)], false);
        assert!(
            read(&missing)
                .unwrap_err()
                .contains("missing companion a.png")
        );
        let mut damaged = archive(
            &[(PROJECT_ENTRY, document), ("assets/a.png", b"image bytes")],
            false,
        );
        let at = damaged
            .windows(11)
            .position(|w| w == b"image bytes")
            .unwrap();
        damaged[at] ^= 1;
        assert!(read(&damaged).unwrap_err().contains("checksum"));
        assert!(read(b"PK\x03\x04 not an archive").is_err());
    }
}

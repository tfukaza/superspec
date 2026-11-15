use std::io::Cursor;

pub struct Raster {
    pub width: usize,
    pub height: usize,
    pub maximum: f64,
    pub samples: Vec<u16>,
}
/// Decode authored grayscale samples without gamma conversion or 16-bit truncation.
pub fn decode(bytes: &[u8]) -> Result<Raster, String> {
    if bytes.len() > 16 * 1024 * 1024 {
        return Err("Heightmap encoded byte budget exceeded.".into());
    }
    // Check framing independently: the streaming decoder deliberately stops at IEND.
    let mut offset = 8usize;
    let mut ended = false;
    let mut data_seen = false;
    let mut data_ended = false;
    let mut chunks = 0;
    while offset.checked_add(12).is_some_and(|end| end <= bytes.len()) {
        let length = u32::from_be_bytes(bytes[offset..offset + 4].try_into().unwrap()) as usize;
        chunks += 1;
        if chunks > 65_536 {
            return Err("PNG chunk budget exceeded.".into());
        }
        if &bytes[offset + 4..offset + 8] == b"IDAT" {
            if data_ended {
                return Err("PNG image data chunks must be consecutive.".into());
            }
            data_seen = true;
        } else if data_seen {
            data_ended = true;
        }
        let end = offset
            .checked_add(12)
            .and_then(|n| n.checked_add(length))
            .filter(|end| *end <= bytes.len())
            .ok_or("Truncated PNG chunk.")?;
        if &bytes[offset + 4..offset + 8] == b"IEND" {
            if length != 0 || end != bytes.len() {
                return Err("Invalid IEND or trailing PNG bytes.".into());
            }
            ended = true;
            break;
        }
        offset = end;
    }
    if !ended {
        return Err("Missing PNG end chunk.".into());
    }
    let mut decoder = png::Decoder::new(Cursor::new(bytes));
    decoder.ignore_checksums(false);
    decoder.set_limits(png::Limits {
        bytes: 16 * 1024 * 1024,
    });
    let mut reader = decoder.read_info().map_err(|e| e.to_string())?;
    let info = reader.info();
    let width = info.width as usize;
    let height = info.height as usize;
    if width < 2 || height < 2 || width.checked_mul(height).is_none_or(|n| n > 1_048_576) {
        return Err("Heightmap sample budget exceeded or grid too small.".into());
    }
    if info.interlaced
        || info.color_type != png::ColorType::Grayscale
        || ![png::BitDepth::Eight, png::BitDepth::Sixteen].contains(&info.bit_depth)
        || info.animation_control.is_some()
    {
        return Err("Heightmaps require non-interlaced 8/16-bit grayscale PNG.".into());
    }
    let sixteen = info.bit_depth == png::BitDepth::Sixteen;
    let size = reader
        .output_buffer_size()
        .ok_or("Heightmap decoded byte budget exceeded.")?;
    if size > 2 * 1_048_576 {
        return Err("Heightmap decoded byte budget exceeded.".into());
    }
    let mut pixels = vec![0; size];
    reader.next_frame(&mut pixels).map_err(|e| e.to_string())?;
    reader.finish().map_err(|e| e.to_string())?;
    let samples = if sixteen {
        pixels
            .as_chunks::<2>()
            .0
            .iter()
            .map(|p| u16::from_be_bytes(*p))
            .collect()
    } else {
        pixels.iter().map(|p| *p as u16).collect()
    };
    Ok(Raster {
        width,
        height,
        maximum: if sixteen { 65535. } else { 255. },
        samples,
    })
}

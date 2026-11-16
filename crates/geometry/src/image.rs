use std::io::{Cursor, Write};

pub struct Image {
    pub width: usize,
    pub height: usize,
    pub rgba: Vec<u8>,
    pub mime: &'static str,
}

/// Encodes owned raster pixels losslessly; publication adapters own output paths and transactions.
pub fn encode(image: &Image) -> Result<Vec<u8>, String> {
    encode_cancellable(image, &|| false)
}

fn encoding_capacity(width: usize, height: usize) -> Result<usize, String> {
    let pixels = dimensions(width, height)?;
    // Pinned fdeflate 0.3.7 literals use at most 12 bits per filtered byte. Include chunk/header slack.
    Ok(((pixels * 4 + height) * 2 + 4096).min(16 * 1024 * 1024))
}
/// Encoder-only owned buffers, excluding caller-owned RGBA and completed sibling images.
/// Streaming uses three scanlines, one 64 KiB chunk and constant-size fdeflate state.
pub fn encoding_working_bytes(width: usize, height: usize) -> Result<usize, String> {
    let bytes = 2 * encoding_capacity(width, height)? + 12 * width + 65536 + 4096;
    if bytes > 64 * 1024 * 1024 {
        return Err("PNG encoder exceeds its working buffer budget.".into());
    }
    Ok(bytes)
}

struct PngOutput {
    bytes: Vec<u8>,
    limit: usize,
    overflow: bool,
}
impl Write for PngOutput {
    fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
        if self.overflow || data.len() > self.limit - self.bytes.len() {
            self.overflow = true;
        } else {
            self.bytes.extend_from_slice(data);
        }
        // The pinned fdeflate finalizer unwraps sink writes. Record overflow and discard until finished.
        Ok(data.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
/// Streams lossless PNG rows without an image-sized compressed temporary or fallback buffer.
pub fn encode_cancellable(image: &Image, cancelled: &dyn Fn() -> bool) -> Result<Vec<u8>, String> {
    encode_with(image, cancelled, Strength::Fast)
}

/// Like [`encode_cancellable`] but smaller and slower: DEFLATE level 3 with per-row adaptive
/// filters, for delivered output (production bakes). The pixels are identical and the bytes are
/// deterministic on every target. Level 3 was chosen by measurement on 2048 px material maps:
/// within 6-9 % of the size of the level 6 used before, at about a quarter of its time.
pub fn encode_small_cancellable(
    image: &Image,
    cancelled: &dyn Fn() -> bool,
) -> Result<Vec<u8>, String> {
    encode_with(image, cancelled, Strength::Small)
}

/// How hard an encoding works for size; each is one fixed, target-independent setting.
#[derive(Clone, Copy)]
enum Strength {
    /// The fast DEFLATE with the Sub filter.
    Fast,
    /// DEFLATE level 3 with per-row adaptive filters.
    Small,
}

fn encode_with(
    image: &Image,
    cancelled: &dyn Fn() -> bool,
    strength: Strength,
) -> Result<Vec<u8>, String> {
    let count = dimensions(image.width, image.height)?;
    if image.rgba.len() != count * 4
        || image.width > u32::MAX as usize
        || image.height > u32::MAX as usize
    {
        return Err("PNG output requires matching dimensions and complete RGBA8 pixels.".into());
    }
    encoding_working_bytes(image.width, image.height)?;
    if cancelled() {
        return Err("PNG encoding cancelled.".into());
    }
    let limit = encoding_capacity(image.width, image.height)?;
    let mut bytes = Vec::new();
    bytes.try_reserve_exact(limit).map_err(|e| e.to_string())?;
    let mut output = PngOutput {
        bytes,
        limit,
        overflow: false,
    };
    let mut encoder = png::Encoder::new(&mut output, image.width as u32, image.height as u32);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    match strength {
        Strength::Fast => {
            encoder.set_compression(png::Compression::Fast);
            encoder.set_filter(png::Filter::Sub);
        }
        Strength::Small => {
            encoder.set_deflate_compression(png::DeflateCompression::Level(3));
            encoder.set_filter(png::Filter::Adaptive);
        }
    }
    let mut writer = encoder.write_header().map_err(|e| e.to_string())?;
    {
        let mut stream = writer
            .stream_writer_with_size(65536)
            .map_err(|e| e.to_string())?;
        for row in image.rgba.chunks_exact(image.width * 4) {
            if cancelled() {
                return Err("PNG encoding cancelled.".into());
            }
            stream.write_all(row).map_err(|e| e.to_string())?;
        }
        stream.finish().map_err(|e| e.to_string())?;
    }
    writer.finish().map_err(|e| e.to_string())?;
    if output.overflow {
        return Err("Encoded image byte budget exceeded.".into());
    }
    // A completed image owns exactly its retained byte count, not its worst-case reserved capacity.
    Ok(output.bytes.into_boxed_slice().into_vec())
}

fn dimensions(width: usize, height: usize) -> Result<usize, String> {
    width
        .checked_mul(height)
        .filter(|count| *count > 0 && *count <= 16_777_216)
        .ok_or_else(|| "Image dimensions exceed the decoded pixel budget.".into())
}

/// Largest encoded image accepted for decoding or inspection; matches
/// `superspec_types::limits::MAX_IMAGE_BYTES` (checked by a test there).
pub const MAX_IMAGE_BYTES: usize = 64 * 1024 * 1024;

/// An image's base level size and encoding, read from its header.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Info {
    pub width: usize,
    pub height: usize,
    pub mime: &'static str,
}

/// Reads dimensions and encoding from the header alone (PNG IHDR, JPEG frame header, KTX2 header),
/// without decoding pixels. Use it to account for or route images; [`decode`] validates them.
pub fn info(bytes: &[u8]) -> Result<Info, String> {
    if bytes.len() > MAX_IMAGE_BYTES {
        return Err("Encoded image byte budget exceeded.".into());
    }
    let (width, height, mime) = if bytes.starts_with(&crate::ktx2::MAGIC) {
        let (width, height) = crate::ktx2::dimensions(bytes)?;
        (width, height, crate::ktx2::MIME)
    } else if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        if bytes.len() < 24 || &bytes[12..16] != b"IHDR" {
            return Err("PNG has no header.".into());
        }
        let word = |at: usize| u32::from_be_bytes(bytes[at..at + 4].try_into().unwrap()) as usize;
        (word(16), word(20), "image/png")
    } else if bytes.starts_with(&[255, 216, 255]) {
        let mut decoder = jpeg_decoder::Decoder::new(Cursor::new(bytes));
        decoder.read_info().map_err(|e| e.to_string())?;
        let info = decoder.info().ok_or("JPEG has no image frame.")?;
        (info.width as usize, info.height as usize, "image/jpeg")
    } else {
        return Err("Material images require PNG, JPEG or KTX2 data.".into());
    };
    if width == 0 || height == 0 {
        return Err("Image has no pixels.".into());
    }
    Ok(Info {
        width,
        height,
        mime,
    })
}

/// Bounded, portable source decoding; retains channel values without gamma conversion.
pub fn decode(bytes: &[u8]) -> Result<Image, String> {
    if bytes.len() > MAX_IMAGE_BYTES {
        return Err("Encoded image byte budget exceeded.".into());
    }
    if bytes.starts_with(&crate::ktx2::MAGIC) {
        let image = crate::ktx2::decode(bytes)?;
        dimensions(image.width, image.height)?;
        return Ok(image);
    }
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        let mut decoder = png::Decoder::new(Cursor::new(bytes));
        decoder.ignore_checksums(false);
        decoder.set_limits(png::Limits {
            bytes: 128 * 1024 * 1024,
        });
        decoder.set_transformations(png::Transformations::EXPAND);
        let mut reader = decoder.read_info().map_err(|e| e.to_string())?;
        let (width, height) = (reader.info().width as usize, reader.info().height as usize);
        let count = dimensions(width, height)?;
        let keyed_transparency = reader.info().trns.is_some()
            && matches!(
                reader.info().color_type,
                png::ColorType::Rgb | png::ColorType::Grayscale
            );
        if reader.info().animation_control.is_some() {
            return Err("Animated material images are not supported.".into());
        }
        let size = reader
            .output_buffer_size()
            .filter(|n| *n <= 128 * 1024 * 1024)
            .ok_or("Decoded image byte budget exceeded.")?;
        let mut data = vec![0; size];
        let output = reader.next_frame(&mut data).map_err(|e| e.to_string())?;
        reader.finish().map_err(|e| e.to_string())?;
        let mut rgba = Vec::with_capacity(count * 4);
        let channels = match output.color_type {
            png::ColorType::Rgba => 4,
            png::ColorType::Rgb => 3,
            png::ColorType::Grayscale => 1,
            png::ColorType::GrayscaleAlpha => 2,
            _ => return Err("PNG expansion did not produce color samples.".into()),
        };
        let stride = if output.bit_depth == png::BitDepth::Sixteen {
            2
        } else {
            1
        };
        for pixel in data[..output.buffer_size()].chunks_exact(channels * stride) {
            let sample = |i| {
                if stride == 1 {
                    pixel[i]
                } else {
                    let value = u16::from_be_bytes([pixel[i * 2], pixel[i * 2 + 1]]) as u32;
                    ((value * 255 + 32767) / 65535) as u8
                }
            };
            let mut p = match channels {
                1 => [sample(0), sample(0), sample(0), 255],
                2 => [sample(0), sample(0), sample(0), sample(1)],
                3 => [sample(0), sample(1), sample(2), 255],
                _ => [sample(0), sample(1), sample(2), sample(3)],
            };
            if keyed_transparency && p[3] == 0 {
                p[..3].fill(0);
            }
            rgba.extend(p);
        }
        Ok(Image {
            width,
            height,
            rgba,
            mime: "image/png",
        })
    } else if bytes.starts_with(&[255, 216, 255]) {
        let mut decoder = jpeg_decoder::Decoder::new(Cursor::new(bytes));
        decoder.set_max_decoding_buffer_size(64 * 1024 * 1024);
        decoder.read_info().map_err(|e| e.to_string())?;
        let info = decoder.info().ok_or("JPEG has no image frame.")?;
        let (width, height) = (info.width as usize, info.height as usize);
        let count = dimensions(width, height)?;
        let data = decoder.decode().map_err(|e| e.to_string())?;
        let mut rgba = Vec::with_capacity(count * 4);
        match info.pixel_format {
            jpeg_decoder::PixelFormat::RGB24 => {
                for p in data.as_chunks::<3>().0 {
                    rgba.extend([p[0], p[1], p[2], 255]);
                }
            }
            jpeg_decoder::PixelFormat::L8 => {
                for &p in &data {
                    rgba.extend([p, p, p, 255]);
                }
            }
            _ => {
                return Err("JPEG material images require 8-bit grayscale or RGB channels.".into());
            }
        }
        Ok(Image {
            width,
            height,
            rgba,
            mime: "image/jpeg",
        })
    } else {
        Err("Material images require PNG or JPEG data.".into())
    }
}

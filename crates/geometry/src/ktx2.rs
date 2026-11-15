//! KTX2 textures with Basis Universal payloads, as glTF `KHR_texture_basisu` stores them.
//!
//! Color maps use ETC1S (small, sRGB); data maps (normals, metallic-roughness, occlusion and
//! live-operation channels) use UASTC LDR 4x4 with Zstandard supercompression, which keeps
//! per-channel values close enough for normals and quantized data. Every texture carries a full
//! mip chain with clamped edges, since renderers cannot generate mips for compressed textures.
//! Encoding is single-threaded and deterministic: equal pixels give equal bytes.
//!
//! Native targets compile the vendored Basis Universal C++ and initialize on first use. On
//! `wasm32-unknown-unknown` the codec is a separate embedded module that must be instantiated
//! once with [`prepare`] before [`encode`] or [`decode`].
use crate::image::Image;
use basisu_c_sys::{common, safe};
use std::sync::atomic::{AtomicBool, Ordering};

/// Media type of KTX2 images in glTF.
pub const MIME: &str = "image/ktx2";
/// File identifier at the start of every KTX2 file.
pub const MAGIC: [u8; 12] = [
    0xAB, b'K', b'T', b'X', b' ', b'2', b'0', 0xBB, b'\r', b'\n', 0x1A, b'\n',
];
/// Largest texture edge accepted for encoding and decoding.
pub const MAX_EDGE: usize = 16384;
/// Unified Basis Universal quality (1-100) and effort (0-10) for every texture.
const QUALITY: i32 = 75;
const EFFORT: i32 = 2;

/// How a texture's channels are used, which selects its codec and color space.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// sRGB color (base color, emissive): ETC1S.
    Color,
    /// Linear data (normals, metallic-roughness, occlusion, live channels): UASTC + Zstandard.
    Data,
}

static READY: AtomicBool = AtomicBool::new(false);

fn initialize() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        safe::init();
        READY.store(true, Ordering::Release);
    });
}

const WEB: bool = cfg!(all(
    target_arch = "wasm32",
    target_vendor = "unknown",
    target_os = "unknown"
));

/// Loads the codec. Required once on the web before encoding or decoding; on native targets the
/// codec initializes on first use and this completes immediately.
pub async fn prepare() {
    basisu_c_sys::instantiate_embedded_basisu_wasm().await;
    initialize();
}

/// Whether [`encode`] and [`decode`] can run now.
pub fn ready() -> bool {
    !WEB || READY.load(Ordering::Acquire)
}

fn ensure() -> Result<(), String> {
    if !WEB {
        initialize();
    }
    if READY.load(Ordering::Acquire) {
        Ok(())
    } else {
        Err("The KTX2 texture codec is not loaded yet.".into())
    }
}

fn check_edges(width: usize, height: usize) -> Result<(), String> {
    if width == 0 || height == 0 || width > MAX_EDGE || height > MAX_EDGE {
        return Err(format!(
            "KTX2 textures must be 1-{MAX_EDGE} pixels on each side, not {width} x {height}."
        ));
    }
    Ok(())
}

/// Encodes RGBA8 pixels (rows top to bottom, as glTF stores images) as a KTX2 file.
pub fn encode(image: &Image, kind: Kind) -> Result<Vec<u8>, String> {
    check_edges(image.width, image.height)?;
    if image.rgba.len() != image.width * image.height * 4 {
        return Err("KTX2 input requires complete RGBA8 pixels.".into());
    }
    ensure()?;
    let (format, flags) = match kind {
        Kind::Color => (common::BTF_ETC1S, common::BU_COMP_FLAGS_SRGB),
        Kind::Data => (
            common::BTF_UASTC_LDR_4X4,
            common::BU_COMP_FLAGS_KTX2_UASTC_ZSTD,
        ),
    };
    let flags = flags | common::BU_COMP_FLAGS_KTX2_OUTPUT | common::BU_COMP_FLAGS_GEN_MIPS_CLAMP;
    let bytes = safe::compress(
        &image.rgba,
        image.width as u32,
        image.height as u32,
        format,
        flags,
        QUALITY,
        EFFORT,
    )?;
    if !bytes.starts_with(&MAGIC) {
        return Err("KTX2 compression produced an invalid file.".into());
    }
    Ok(bytes)
}

/// Reads a KTX2 file's base level dimensions from its header, without transcoding.
pub fn dimensions(bytes: &[u8]) -> Result<(usize, usize), String> {
    if bytes.len() < 28 || !bytes.starts_with(&MAGIC) {
        return Err("Expected a KTX2 file.".into());
    }
    let word = |at: usize| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap()) as usize;
    let (width, height) = (word(20), word(24).max(1));
    check_edges(width, height)?;
    Ok((width, height))
}

/// Validates a KTX2 file's structure (header, levels and Basis Universal global data) without
/// transcoding pixels, returning its base level width and height.
pub fn inspect(bytes: &[u8]) -> Result<(usize, usize), String> {
    let (width, height) = dimensions(bytes)?;
    ensure()?;
    let (w, h, levels) = safe::inspect_ktx2(bytes)?;
    if (w as usize, h as usize) != (width, height) || levels == 0 {
        return Err("KTX2 header and payload dimensions differ.".into());
    }
    Ok((width, height))
}

/// Transcodes a KTX2 file's base level to RGBA8, keeping stored channel values (an sRGB color
/// texture decodes to its sRGB bytes, like a PNG).
pub fn decode(bytes: &[u8]) -> Result<Image, String> {
    let (width, height) = dimensions(bytes)?;
    ensure()?;
    let (w, h, rgba) = safe::transcode_rgba32(bytes)?;
    if (w as usize, h as usize) != (width, height) {
        return Err("KTX2 header and payload dimensions differ.".into());
    }
    Ok(Image {
        width,
        height,
        rgba,
        mime: MIME,
    })
}

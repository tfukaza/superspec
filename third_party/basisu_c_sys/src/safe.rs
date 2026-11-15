//! SuperSpec addition: a small safe API over the C functions it uses, so callers in a workspace
//! that forbids `unsafe` never touch raw codec memory. Every allocation, parameter set, KTX2
//! handle and transcode state is released on all paths.
use crate::{common, copy_basisu_memory_to_host, copy_host_memory_to_basisu};
use crate::{encoder as enc, transcoder as tr};
use alloc::vec::Vec;

/// Initializes the encoder and transcoder tables. Call once, after
/// [`crate::instantiate_embedded_basisu_wasm`] on the web, before the functions below.
pub fn init() {
    // SAFETY: global table setup with no arguments; callers serialize it (a `Once`).
    unsafe {
        enc::bu_init();
        tr::bt_init();
    }
}

struct Allocation(u64);
impl Allocation {
    fn new(size: u64) -> Result<Self, &'static str> {
        // SAFETY: plain allocation in the codec heap.
        let at = unsafe { tr::bt_alloc(size) };
        if at == 0 {
            Err("Basis Universal allocation failed.")
        } else {
            Ok(Self(at))
        }
    }
    fn copy(data: &[u8]) -> Result<Self, &'static str> {
        let allocation = Self::new(data.len() as u64)?;
        // SAFETY: the allocation holds exactly `data.len()` bytes.
        unsafe { copy_host_memory_to_basisu(data, allocation.0) };
        Ok(allocation)
    }
}
impl Drop for Allocation {
    fn drop(&mut self) {
        // SAFETY: allocated by `bt_alloc`, freed once.
        unsafe { tr::bt_free(self.0) };
    }
}

/// Compresses one RGBA8 image (`width * height * 4` bytes, rows top to bottom) with the given
/// `BTF_*` format, `BU_COMP_FLAGS_*` flags, unified quality (1-100) and effort (0-10).
pub fn compress(
    rgba: &[u8],
    width: u32,
    height: u32,
    format: u32,
    flags: u64,
    quality: i32,
    effort: i32,
) -> Result<Vec<u8>, &'static str> {
    if width == 0
        || height == 0
        || rgba.len() as u64 != u64::from(width) * u64::from(height) * 4
        || width.checked_mul(4).is_none()
    {
        return Err("Basis Universal input requires complete RGBA8 pixels.");
    }
    struct Params(u64);
    impl Drop for Params {
        fn drop(&mut self) {
            // SAFETY: created by `bu_new_comp_params`, deleted once.
            let _ = unsafe { enc::bu_delete_comp_params(self.0) };
        }
    }
    // SAFETY: `params` and the pixel copy are live codec objects for the calls that use them; the
    // encoder copies the image when it is set, so the pixels may be freed before compressing.
    unsafe {
        let params = Params(enc::bu_new_comp_params());
        if params.0 == 0 {
            return Err("Basis Universal parameters could not be created.");
        }
        {
            let pixels = Allocation::copy(rgba)?;
            if enc::bu_comp_params_set_image_rgba32(params.0, 0, pixels.0, width, height, width * 4)
                .is_err()
            {
                return Err("Basis Universal rejected the image.");
            }
        }
        if enc::bu_compress_texture(params.0, format, quality, effort, flags, 0.).is_err() {
            return Err("Basis Universal compression failed.");
        }
        let size = enc::bu_comp_params_get_comp_data_size(params.0);
        let at = enc::bu_comp_params_get_comp_data_ofs(params.0);
        if size == 0 || at == 0 {
            return Err("Basis Universal compression produced no data.");
        }
        Ok(copy_basisu_memory_to_host(at, size))
    }
}

/// Opens a 2D LDR KTX2 file and parses its levels and global data without transcoding pixels,
/// returning its width, height and level count.
pub fn inspect_ktx2(ktx2: &[u8]) -> Result<(u32, u32, u32), &'static str> {
    let length = u32::try_from(ktx2.len()).map_err(|_| "KTX2 file is too large.")?;
    struct Handle(u64);
    impl Drop for Handle {
        fn drop(&mut self) {
            // SAFETY: opened by `bt_ktx2_open`, closed once.
            unsafe { tr::bt_ktx2_close(self.0) };
        }
    }
    let file = Allocation::copy(ktx2)?;
    // SAFETY: the handle reads the file copy, which outlives it.
    unsafe {
        let handle = Handle(tr::bt_ktx2_open(file.0, length));
        if handle.0 == 0 || tr::bt_ktx2_start_transcoding(handle.0).is_err() {
            return Err("KTX2 file could not be opened.");
        }
        if tr::bt_ktx2_is_ldr(handle.0).is_err()
            || tr::bt_ktx2_get_faces(handle.0) > 1
            || tr::bt_ktx2_get_layers(handle.0) > 1
        {
            return Err("Only 2D LDR KTX2 textures are supported.");
        }
        Ok((
            tr::bt_ktx2_get_width(handle.0),
            tr::bt_ktx2_get_height(handle.0),
            tr::bt_ktx2_get_levels(handle.0),
        ))
    }
}

/// Transcodes the base level of a 2D LDR KTX2 file to RGBA8, returning its width, height and
/// pixels.
pub fn transcode_rgba32(ktx2: &[u8]) -> Result<(u32, u32, Vec<u8>), &'static str> {
    let length = u32::try_from(ktx2.len()).map_err(|_| "KTX2 file is too large.")?;
    struct Handle(u64);
    impl Drop for Handle {
        fn drop(&mut self) {
            // SAFETY: opened by `bt_ktx2_open`, closed once.
            unsafe { tr::bt_ktx2_close(self.0) };
        }
    }
    // Drop order runs in reverse: output and handle are released before the file copy.
    let file = Allocation::copy(ktx2)?;
    // SAFETY: the handle reads the file copy, which outlives it; the output holds the size the
    // codec reports for RGBA32 at the base level.
    unsafe {
        let handle = Handle(tr::bt_ktx2_open(file.0, length));
        if handle.0 == 0 || tr::bt_ktx2_start_transcoding(handle.0).is_err() {
            return Err("KTX2 file could not be opened.");
        }
        if tr::bt_ktx2_is_ldr(handle.0).is_err()
            || tr::bt_ktx2_get_faces(handle.0) > 1
            || tr::bt_ktx2_get_layers(handle.0) > 1
        {
            return Err("Only 2D LDR KTX2 textures are supported.");
        }
        let (width, height) = (tr::bt_ktx2_get_width(handle.0), tr::bt_ktx2_get_height(handle.0));
        let format = common::TF_RGBA32;
        let size = tr::bt_basis_compute_transcoded_image_size_in_bytes(format, width, height);
        if width == 0 || height == 0 || u64::from(size) != u64::from(width) * u64::from(height) * 4 {
            return Err("KTX2 transcode size mismatch.");
        }
        let output = Allocation::new(u64::from(size))?;
        if tr::bt_ktx2_transcode_image_level(
            handle.0,
            0,
            0,
            0,
            output.0,
            width * height,
            format,
            0,
            0,
            0,
            -1,
            -1,
            // The transcoder's own state. A state from t_ktx2_create_transcode_state starts with
            // its decompressed-level index zeroed instead of -1, so level 0 of a Zstandard file
            // would be treated as already decompressed (upstream bug in 0.7.1).
            0,
        )
        .is_err()
        {
            return Err("KTX2 transcoding failed.");
        }
        Ok((width, height, copy_basisu_memory_to_host(output.0, u64::from(size))))
    }
}

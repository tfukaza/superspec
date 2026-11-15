pub unsafe fn bt_get_version() -> u32 {
    BASISU_INSTANCE
        .with(|inst| {
            let inst = inst.get().unwrap();
            inst.bt_get_version()
        })
}
pub unsafe fn bt_enable_debug_printf(flag: u32) {
    BASISU_INSTANCE
        .with(|inst| {
            let inst = inst.get().unwrap();
            inst.bt_enable_debug_printf(flag)
        })
}
pub unsafe fn bt_init() {
    BASISU_INSTANCE
        .with(|inst| {
            let inst = inst.get().unwrap();
            inst.bt_init()
        })
}
pub unsafe fn bt_alloc(size: u64) -> u64 {
    BASISU_INSTANCE
        .with(|inst| {
            let inst = inst.get().unwrap();
            inst.bt_alloc(size)
        })
}
pub unsafe fn bt_free(ofs: u64) {
    BASISU_INSTANCE
        .with(|inst| {
            let inst = inst.get().unwrap();
            inst.bt_free(ofs)
        })
}
#[must_use]
pub unsafe fn bt_basis_tex_format_is_xuastc_ldr(basis_tex_fmt_u32: u32) -> Bool32 {
    BASISU_INSTANCE
        .with(|inst| {
            let inst = inst.get().unwrap();
            Bool32(inst.bt_basis_tex_format_is_xuastc_ldr(basis_tex_fmt_u32))
        })
}
#[must_use]
pub unsafe fn bt_basis_tex_format_is_astc_ldr(basis_tex_fmt_u32: u32) -> Bool32 {
    BASISU_INSTANCE
        .with(|inst| {
            let inst = inst.get().unwrap();
            Bool32(inst.bt_basis_tex_format_is_astc_ldr(basis_tex_fmt_u32))
        })
}
pub unsafe fn bt_basis_tex_format_get_block_width(basis_tex_fmt_u32: u32) -> u32 {
    BASISU_INSTANCE
        .with(|inst| {
            let inst = inst.get().unwrap();
            inst.bt_basis_tex_format_get_block_width(basis_tex_fmt_u32)
        })
}
pub unsafe fn bt_basis_tex_format_get_block_height(basis_tex_fmt_u32: u32) -> u32 {
    BASISU_INSTANCE
        .with(|inst| {
            let inst = inst.get().unwrap();
            inst.bt_basis_tex_format_get_block_height(basis_tex_fmt_u32)
        })
}
#[must_use]
pub unsafe fn bt_basis_tex_format_is_hdr(basis_tex_format_u32: u32) -> Bool32 {
    BASISU_INSTANCE
        .with(|inst| {
            let inst = inst.get().unwrap();
            Bool32(inst.bt_basis_tex_format_is_hdr(basis_tex_format_u32))
        })
}
#[must_use]
pub unsafe fn bt_basis_tex_format_is_ldr(basis_tex_format_u32: u32) -> Bool32 {
    BASISU_INSTANCE
        .with(|inst| {
            let inst = inst.get().unwrap();
            Bool32(inst.bt_basis_tex_format_is_ldr(basis_tex_format_u32))
        })
}
pub unsafe fn bt_basis_get_bytes_per_block_or_pixel(
    transcoder_texture_format_u32: u32,
) -> u32 {
    BASISU_INSTANCE
        .with(|inst| {
            let inst = inst.get().unwrap();
            inst.bt_basis_get_bytes_per_block_or_pixel(transcoder_texture_format_u32)
        })
}
#[must_use]
pub unsafe fn bt_basis_transcoder_format_has_alpha(
    transcoder_texture_format_u32: u32,
) -> Bool32 {
    BASISU_INSTANCE
        .with(|inst| {
            let inst = inst.get().unwrap();
            Bool32(
                inst.bt_basis_transcoder_format_has_alpha(transcoder_texture_format_u32),
            )
        })
}
#[must_use]
pub unsafe fn bt_basis_transcoder_format_is_hdr(
    transcoder_texture_format_u32: u32,
) -> Bool32 {
    BASISU_INSTANCE
        .with(|inst| {
            let inst = inst.get().unwrap();
            Bool32(inst.bt_basis_transcoder_format_is_hdr(transcoder_texture_format_u32))
        })
}
#[must_use]
pub unsafe fn bt_basis_transcoder_format_is_ldr(
    transcoder_texture_format_u32: u32,
) -> Bool32 {
    BASISU_INSTANCE
        .with(|inst| {
            let inst = inst.get().unwrap();
            Bool32(inst.bt_basis_transcoder_format_is_ldr(transcoder_texture_format_u32))
        })
}
#[must_use]
pub unsafe fn bt_basis_transcoder_texture_format_is_astc(
    transcoder_texture_format_u32: u32,
) -> Bool32 {
    BASISU_INSTANCE
        .with(|inst| {
            let inst = inst.get().unwrap();
            Bool32(
                inst
                    .bt_basis_transcoder_texture_format_is_astc(
                        transcoder_texture_format_u32,
                    ),
            )
        })
}
#[must_use]
pub unsafe fn bt_basis_transcoder_format_is_uncompressed(
    transcoder_texture_format_u32: u32,
) -> Bool32 {
    BASISU_INSTANCE
        .with(|inst| {
            let inst = inst.get().unwrap();
            Bool32(
                inst
                    .bt_basis_transcoder_format_is_uncompressed(
                        transcoder_texture_format_u32,
                    ),
            )
        })
}
pub unsafe fn bt_basis_get_uncompressed_bytes_per_pixel(
    transcoder_texture_format_u32: u32,
) -> u32 {
    BASISU_INSTANCE
        .with(|inst| {
            let inst = inst.get().unwrap();
            inst.bt_basis_get_uncompressed_bytes_per_pixel(transcoder_texture_format_u32)
        })
}
pub unsafe fn bt_basis_get_block_width(transcoder_texture_format_u32: u32) -> u32 {
    BASISU_INSTANCE
        .with(|inst| {
            let inst = inst.get().unwrap();
            inst.bt_basis_get_block_width(transcoder_texture_format_u32)
        })
}
pub unsafe fn bt_basis_get_block_height(transcoder_texture_format_u32: u32) -> u32 {
    BASISU_INSTANCE
        .with(|inst| {
            let inst = inst.get().unwrap();
            inst.bt_basis_get_block_height(transcoder_texture_format_u32)
        })
}
pub unsafe fn bt_basis_get_transcoder_texture_format_from_basis_tex_format(
    basis_tex_format_u32: u32,
) -> u32 {
    BASISU_INSTANCE
        .with(|inst| {
            let inst = inst.get().unwrap();
            inst.bt_basis_get_transcoder_texture_format_from_basis_tex_format(
                basis_tex_format_u32,
            )
        })
}
#[must_use]
pub unsafe fn bt_basis_is_format_supported(
    transcoder_texture_format_u32: u32,
    basis_tex_format_u32: u32,
) -> Bool32 {
    BASISU_INSTANCE
        .with(|inst| {
            let inst = inst.get().unwrap();
            Bool32(
                inst
                    .bt_basis_is_format_supported(
                        transcoder_texture_format_u32,
                        basis_tex_format_u32,
                    ),
            )
        })
}
pub unsafe fn bt_basis_compute_transcoded_image_size_in_bytes(
    transcoder_texture_format_u32: u32,
    orig_width: u32,
    orig_height: u32,
) -> u32 {
    BASISU_INSTANCE
        .with(|inst| {
            let inst = inst.get().unwrap();
            inst.bt_basis_compute_transcoded_image_size_in_bytes(
                transcoder_texture_format_u32,
                orig_width,
                orig_height,
            )
        })
}
pub unsafe fn bt_ktx2_open(data_mem_ofs: u64, data_len: u32) -> u64 {
    BASISU_INSTANCE
        .with(|inst| {
            let inst = inst.get().unwrap();
            inst.bt_ktx2_open(data_mem_ofs, data_len)
        })
}
pub unsafe fn bt_ktx2_close(handle: u64) {
    BASISU_INSTANCE
        .with(|inst| {
            let inst = inst.get().unwrap();
            inst.bt_ktx2_close(handle)
        })
}
pub unsafe fn bt_ktx2_get_width(handle: u64) -> u32 {
    BASISU_INSTANCE
        .with(|inst| {
            let inst = inst.get().unwrap();
            inst.bt_ktx2_get_width(handle)
        })
}
pub unsafe fn bt_ktx2_get_height(handle: u64) -> u32 {
    BASISU_INSTANCE
        .with(|inst| {
            let inst = inst.get().unwrap();
            inst.bt_ktx2_get_height(handle)
        })
}
pub unsafe fn bt_ktx2_get_levels(handle: u64) -> u32 {
    BASISU_INSTANCE
        .with(|inst| {
            let inst = inst.get().unwrap();
            inst.bt_ktx2_get_levels(handle)
        })
}
pub unsafe fn bt_ktx2_get_faces(handle: u64) -> u32 {
    BASISU_INSTANCE
        .with(|inst| {
            let inst = inst.get().unwrap();
            inst.bt_ktx2_get_faces(handle)
        })
}
pub unsafe fn bt_ktx2_get_layers(handle: u64) -> u32 {
    BASISU_INSTANCE
        .with(|inst| {
            let inst = inst.get().unwrap();
            inst.bt_ktx2_get_layers(handle)
        })
}
pub unsafe fn bt_ktx2_get_basis_tex_format(handle: u64) -> u32 {
    BASISU_INSTANCE
        .with(|inst| {
            let inst = inst.get().unwrap();
            inst.bt_ktx2_get_basis_tex_format(handle)
        })
}
#[must_use]
pub unsafe fn bt_ktx2_is_etc1s(handle: u64) -> Bool32 {
    BASISU_INSTANCE
        .with(|inst| {
            let inst = inst.get().unwrap();
            Bool32(inst.bt_ktx2_is_etc1s(handle))
        })
}
#[must_use]
pub unsafe fn bt_ktx2_is_uastc_ldr_4x4(handle: u64) -> Bool32 {
    BASISU_INSTANCE
        .with(|inst| {
            let inst = inst.get().unwrap();
            Bool32(inst.bt_ktx2_is_uastc_ldr_4x4(handle))
        })
}
#[must_use]
pub unsafe fn bt_ktx2_is_hdr(handle: u64) -> Bool32 {
    BASISU_INSTANCE
        .with(|inst| {
            let inst = inst.get().unwrap();
            Bool32(inst.bt_ktx2_is_hdr(handle))
        })
}
#[must_use]
pub unsafe fn bt_ktx2_is_hdr_4x4(handle: u64) -> Bool32 {
    BASISU_INSTANCE
        .with(|inst| {
            let inst = inst.get().unwrap();
            Bool32(inst.bt_ktx2_is_hdr_4x4(handle))
        })
}
#[must_use]
pub unsafe fn bt_ktx2_is_hdr_6x6(handle: u64) -> Bool32 {
    BASISU_INSTANCE
        .with(|inst| {
            let inst = inst.get().unwrap();
            Bool32(inst.bt_ktx2_is_hdr_6x6(handle))
        })
}
#[must_use]
pub unsafe fn bt_ktx2_is_ldr(handle: u64) -> Bool32 {
    BASISU_INSTANCE
        .with(|inst| {
            let inst = inst.get().unwrap();
            Bool32(inst.bt_ktx2_is_ldr(handle))
        })
}
#[must_use]
pub unsafe fn bt_ktx2_is_astc_ldr(handle: u64) -> Bool32 {
    BASISU_INSTANCE
        .with(|inst| {
            let inst = inst.get().unwrap();
            Bool32(inst.bt_ktx2_is_astc_ldr(handle))
        })
}
#[must_use]
pub unsafe fn bt_ktx2_is_xuastc_ldr(handle: u64) -> Bool32 {
    BASISU_INSTANCE
        .with(|inst| {
            let inst = inst.get().unwrap();
            Bool32(inst.bt_ktx2_is_xuastc_ldr(handle))
        })
}
pub unsafe fn bt_ktx2_get_block_width(handle: u64) -> u32 {
    BASISU_INSTANCE
        .with(|inst| {
            let inst = inst.get().unwrap();
            inst.bt_ktx2_get_block_width(handle)
        })
}
pub unsafe fn bt_ktx2_get_block_height(handle: u64) -> u32 {
    BASISU_INSTANCE
        .with(|inst| {
            let inst = inst.get().unwrap();
            inst.bt_ktx2_get_block_height(handle)
        })
}
#[must_use]
pub unsafe fn bt_ktx2_has_alpha(handle: u64) -> Bool32 {
    BASISU_INSTANCE
        .with(|inst| {
            let inst = inst.get().unwrap();
            Bool32(inst.bt_ktx2_has_alpha(handle))
        })
}
pub unsafe fn bt_ktx2_get_dfd_color_model(handle: u64) -> u32 {
    BASISU_INSTANCE
        .with(|inst| {
            let inst = inst.get().unwrap();
            inst.bt_ktx2_get_dfd_color_model(handle)
        })
}
pub unsafe fn bt_ktx2_get_dfd_color_primaries(handle: u64) -> u32 {
    BASISU_INSTANCE
        .with(|inst| {
            let inst = inst.get().unwrap();
            inst.bt_ktx2_get_dfd_color_primaries(handle)
        })
}
pub unsafe fn bt_ktx2_get_dfd_transfer_func(handle: u64) -> u32 {
    BASISU_INSTANCE
        .with(|inst| {
            let inst = inst.get().unwrap();
            inst.bt_ktx2_get_dfd_transfer_func(handle)
        })
}
#[must_use]
pub unsafe fn bt_ktx2_is_srgb(handle: u64) -> Bool32 {
    BASISU_INSTANCE
        .with(|inst| {
            let inst = inst.get().unwrap();
            Bool32(inst.bt_ktx2_is_srgb(handle))
        })
}
pub unsafe fn bt_ktx2_get_dfd_flags(handle: u64) -> u32 {
    BASISU_INSTANCE
        .with(|inst| {
            let inst = inst.get().unwrap();
            inst.bt_ktx2_get_dfd_flags(handle)
        })
}
pub unsafe fn bt_ktx2_get_dfd_total_samples(handle: u64) -> u32 {
    BASISU_INSTANCE
        .with(|inst| {
            let inst = inst.get().unwrap();
            inst.bt_ktx2_get_dfd_total_samples(handle)
        })
}
pub unsafe fn bt_ktx2_get_dfd_channel_id0(handle: u64) -> u32 {
    BASISU_INSTANCE
        .with(|inst| {
            let inst = inst.get().unwrap();
            inst.bt_ktx2_get_dfd_channel_id0(handle)
        })
}
pub unsafe fn bt_ktx2_get_dfd_channel_id1(handle: u64) -> u32 {
    BASISU_INSTANCE
        .with(|inst| {
            let inst = inst.get().unwrap();
            inst.bt_ktx2_get_dfd_channel_id1(handle)
        })
}
#[must_use]
pub unsafe fn bt_ktx2_is_video(handle: u64) -> Bool32 {
    BASISU_INSTANCE
        .with(|inst| {
            let inst = inst.get().unwrap();
            Bool32(inst.bt_ktx2_is_video(handle))
        })
}
pub unsafe fn bt_ktx2_get_ldr_hdr_upconversion_nit_multiplier(handle: u64) -> f32 {
    BASISU_INSTANCE
        .with(|inst| {
            let inst = inst.get().unwrap();
            inst.bt_ktx2_get_ldr_hdr_upconversion_nit_multiplier(handle)
        })
}
pub unsafe fn bt_ktx2_get_level_orig_width(
    handle: u64,
    level_index: u32,
    layer_index: u32,
    face_index: u32,
) -> u32 {
    BASISU_INSTANCE
        .with(|inst| {
            let inst = inst.get().unwrap();
            inst.bt_ktx2_get_level_orig_width(
                handle,
                level_index,
                layer_index,
                face_index,
            )
        })
}
pub unsafe fn bt_ktx2_get_level_orig_height(
    handle: u64,
    level_index: u32,
    layer_index: u32,
    face_index: u32,
) -> u32 {
    BASISU_INSTANCE
        .with(|inst| {
            let inst = inst.get().unwrap();
            inst.bt_ktx2_get_level_orig_height(
                handle,
                level_index,
                layer_index,
                face_index,
            )
        })
}
pub unsafe fn bt_ktx2_get_level_actual_width(
    handle: u64,
    level_index: u32,
    layer_index: u32,
    face_index: u32,
) -> u32 {
    BASISU_INSTANCE
        .with(|inst| {
            let inst = inst.get().unwrap();
            inst.bt_ktx2_get_level_actual_width(
                handle,
                level_index,
                layer_index,
                face_index,
            )
        })
}
pub unsafe fn bt_ktx2_get_level_actual_height(
    handle: u64,
    level_index: u32,
    layer_index: u32,
    face_index: u32,
) -> u32 {
    BASISU_INSTANCE
        .with(|inst| {
            let inst = inst.get().unwrap();
            inst.bt_ktx2_get_level_actual_height(
                handle,
                level_index,
                layer_index,
                face_index,
            )
        })
}
pub unsafe fn bt_ktx2_get_level_num_blocks_x(
    handle: u64,
    level_index: u32,
    layer_index: u32,
    face_index: u32,
) -> u32 {
    BASISU_INSTANCE
        .with(|inst| {
            let inst = inst.get().unwrap();
            inst.bt_ktx2_get_level_num_blocks_x(
                handle,
                level_index,
                layer_index,
                face_index,
            )
        })
}
pub unsafe fn bt_ktx2_get_level_num_blocks_y(
    handle: u64,
    level_index: u32,
    layer_index: u32,
    face_index: u32,
) -> u32 {
    BASISU_INSTANCE
        .with(|inst| {
            let inst = inst.get().unwrap();
            inst.bt_ktx2_get_level_num_blocks_y(
                handle,
                level_index,
                layer_index,
                face_index,
            )
        })
}
pub unsafe fn bt_ktx2_get_level_total_blocks(
    handle: u64,
    level_index: u32,
    layer_index: u32,
    face_index: u32,
) -> u32 {
    BASISU_INSTANCE
        .with(|inst| {
            let inst = inst.get().unwrap();
            inst.bt_ktx2_get_level_total_blocks(
                handle,
                level_index,
                layer_index,
                face_index,
            )
        })
}
#[must_use]
pub unsafe fn bt_ktx2_get_level_alpha_flag(
    handle: u64,
    level_index: u32,
    layer_index: u32,
    face_index: u32,
) -> Bool32 {
    BASISU_INSTANCE
        .with(|inst| {
            let inst = inst.get().unwrap();
            Bool32(
                inst
                    .bt_ktx2_get_level_alpha_flag(
                        handle,
                        level_index,
                        layer_index,
                        face_index,
                    ),
            )
        })
}
#[must_use]
pub unsafe fn bt_ktx2_get_level_iframe_flag(
    handle: u64,
    level_index: u32,
    layer_index: u32,
    face_index: u32,
) -> Bool32 {
    BASISU_INSTANCE
        .with(|inst| {
            let inst = inst.get().unwrap();
            Bool32(
                inst
                    .bt_ktx2_get_level_iframe_flag(
                        handle,
                        level_index,
                        layer_index,
                        face_index,
                    ),
            )
        })
}
#[must_use]
pub unsafe fn bt_ktx2_start_transcoding(handle: u64) -> Bool32 {
    BASISU_INSTANCE
        .with(|inst| {
            let inst = inst.get().unwrap();
            Bool32(inst.bt_ktx2_start_transcoding(handle))
        })
}
pub unsafe fn bt_ktx2_create_transcode_state() -> u64 {
    BASISU_INSTANCE
        .with(|inst| {
            let inst = inst.get().unwrap();
            inst.bt_ktx2_create_transcode_state()
        })
}
pub unsafe fn bt_ktx2_destroy_transcode_state(handle: u64) {
    BASISU_INSTANCE
        .with(|inst| {
            let inst = inst.get().unwrap();
            inst.bt_ktx2_destroy_transcode_state(handle)
        })
}
#[must_use]
pub unsafe fn bt_ktx2_transcode_image_level(
    ktx2_handle: u64,
    level_index: u32,
    layer_index: u32,
    face_index: u32,
    output_block_mem_ofs: u64,
    output_blocks_buf_size_in_blocks_or_pixels: u32,
    transcoder_texture_format_u32: u32,
    decode_flags: u32,
    output_row_pitch_in_blocks_or_pixels: u32,
    output_rows_in_pixels: u32,
    channel0: ::core::ffi::c_int,
    channel1: ::core::ffi::c_int,
    state_handle: u64,
) -> Bool32 {
    BASISU_INSTANCE
        .with(|inst| {
            let inst = inst.get().unwrap();
            Bool32(
                inst
                    .bt_ktx2_transcode_image_level(
                        ktx2_handle,
                        level_index,
                        layer_index,
                        face_index,
                        output_block_mem_ofs,
                        output_blocks_buf_size_in_blocks_or_pixels,
                        transcoder_texture_format_u32,
                        decode_flags,
                        output_row_pitch_in_blocks_or_pixels,
                        output_rows_in_pixels,
                        channel0,
                        channel1,
                        state_handle,
                    ),
            )
        })
}

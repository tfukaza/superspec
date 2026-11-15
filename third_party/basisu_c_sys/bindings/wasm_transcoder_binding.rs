#[wasm_bindgen]
extern "C" {
    #[derive(Debug)]
    pub type Basisu;
    #[wasm_bindgen(method, getter, js_name = HEAPU8)]
    pub(crate) fn wasm_heap_memory(this: &Basisu) -> Uint8Array;
    #[wasm_bindgen(method, js_name = "_bt_get_version")]
    pub fn bt_get_version(this: &Basisu) -> u32;
    #[wasm_bindgen(method, js_name = "_bt_enable_debug_printf")]
    pub fn bt_enable_debug_printf(this: &Basisu, flag: u32);
    #[wasm_bindgen(method, js_name = "_bt_init")]
    pub fn bt_init(this: &Basisu);
    #[wasm_bindgen(method, js_name = "_bt_alloc")]
    pub fn bt_alloc(this: &Basisu, size: u64) -> u64;
    #[wasm_bindgen(method, js_name = "_bt_free")]
    pub fn bt_free(this: &Basisu, ofs: u64);
    #[wasm_bindgen(method, js_name = "_bt_basis_tex_format_is_xuastc_ldr")]
    pub fn bt_basis_tex_format_is_xuastc_ldr(
        this: &Basisu,
        basis_tex_fmt_u32: u32,
    ) -> u32;
    #[wasm_bindgen(method, js_name = "_bt_basis_tex_format_is_astc_ldr")]
    pub fn bt_basis_tex_format_is_astc_ldr(this: &Basisu, basis_tex_fmt_u32: u32) -> u32;
    #[wasm_bindgen(method, js_name = "_bt_basis_tex_format_get_block_width")]
    pub fn bt_basis_tex_format_get_block_width(
        this: &Basisu,
        basis_tex_fmt_u32: u32,
    ) -> u32;
    #[wasm_bindgen(method, js_name = "_bt_basis_tex_format_get_block_height")]
    pub fn bt_basis_tex_format_get_block_height(
        this: &Basisu,
        basis_tex_fmt_u32: u32,
    ) -> u32;
    #[wasm_bindgen(method, js_name = "_bt_basis_tex_format_is_hdr")]
    pub fn bt_basis_tex_format_is_hdr(this: &Basisu, basis_tex_format_u32: u32) -> u32;
    #[wasm_bindgen(method, js_name = "_bt_basis_tex_format_is_ldr")]
    pub fn bt_basis_tex_format_is_ldr(this: &Basisu, basis_tex_format_u32: u32) -> u32;
    #[wasm_bindgen(method, js_name = "_bt_basis_get_bytes_per_block_or_pixel")]
    pub fn bt_basis_get_bytes_per_block_or_pixel(
        this: &Basisu,
        transcoder_texture_format_u32: u32,
    ) -> u32;
    #[wasm_bindgen(method, js_name = "_bt_basis_transcoder_format_has_alpha")]
    pub fn bt_basis_transcoder_format_has_alpha(
        this: &Basisu,
        transcoder_texture_format_u32: u32,
    ) -> u32;
    #[wasm_bindgen(method, js_name = "_bt_basis_transcoder_format_is_hdr")]
    pub fn bt_basis_transcoder_format_is_hdr(
        this: &Basisu,
        transcoder_texture_format_u32: u32,
    ) -> u32;
    #[wasm_bindgen(method, js_name = "_bt_basis_transcoder_format_is_ldr")]
    pub fn bt_basis_transcoder_format_is_ldr(
        this: &Basisu,
        transcoder_texture_format_u32: u32,
    ) -> u32;
    #[wasm_bindgen(method, js_name = "_bt_basis_transcoder_texture_format_is_astc")]
    pub fn bt_basis_transcoder_texture_format_is_astc(
        this: &Basisu,
        transcoder_texture_format_u32: u32,
    ) -> u32;
    #[wasm_bindgen(method, js_name = "_bt_basis_transcoder_format_is_uncompressed")]
    pub fn bt_basis_transcoder_format_is_uncompressed(
        this: &Basisu,
        transcoder_texture_format_u32: u32,
    ) -> u32;
    #[wasm_bindgen(method, js_name = "_bt_basis_get_uncompressed_bytes_per_pixel")]
    pub fn bt_basis_get_uncompressed_bytes_per_pixel(
        this: &Basisu,
        transcoder_texture_format_u32: u32,
    ) -> u32;
    #[wasm_bindgen(method, js_name = "_bt_basis_get_block_width")]
    pub fn bt_basis_get_block_width(
        this: &Basisu,
        transcoder_texture_format_u32: u32,
    ) -> u32;
    #[wasm_bindgen(method, js_name = "_bt_basis_get_block_height")]
    pub fn bt_basis_get_block_height(
        this: &Basisu,
        transcoder_texture_format_u32: u32,
    ) -> u32;
    #[wasm_bindgen(
        method,
        js_name = "_bt_basis_get_transcoder_texture_format_from_basis_tex_format"
    )]
    pub fn bt_basis_get_transcoder_texture_format_from_basis_tex_format(
        this: &Basisu,
        basis_tex_format_u32: u32,
    ) -> u32;
    #[wasm_bindgen(method, js_name = "_bt_basis_is_format_supported")]
    pub fn bt_basis_is_format_supported(
        this: &Basisu,
        transcoder_texture_format_u32: u32,
        basis_tex_format_u32: u32,
    ) -> u32;
    #[wasm_bindgen(method, js_name = "_bt_basis_compute_transcoded_image_size_in_bytes")]
    pub fn bt_basis_compute_transcoded_image_size_in_bytes(
        this: &Basisu,
        transcoder_texture_format_u32: u32,
        orig_width: u32,
        orig_height: u32,
    ) -> u32;
    #[wasm_bindgen(method, js_name = "_bt_ktx2_open")]
    pub fn bt_ktx2_open(this: &Basisu, data_mem_ofs: u64, data_len: u32) -> u64;
    #[wasm_bindgen(method, js_name = "_bt_ktx2_close")]
    pub fn bt_ktx2_close(this: &Basisu, handle: u64);
    #[wasm_bindgen(method, js_name = "_bt_ktx2_get_width")]
    pub fn bt_ktx2_get_width(this: &Basisu, handle: u64) -> u32;
    #[wasm_bindgen(method, js_name = "_bt_ktx2_get_height")]
    pub fn bt_ktx2_get_height(this: &Basisu, handle: u64) -> u32;
    #[wasm_bindgen(method, js_name = "_bt_ktx2_get_levels")]
    pub fn bt_ktx2_get_levels(this: &Basisu, handle: u64) -> u32;
    #[wasm_bindgen(method, js_name = "_bt_ktx2_get_faces")]
    pub fn bt_ktx2_get_faces(this: &Basisu, handle: u64) -> u32;
    #[wasm_bindgen(method, js_name = "_bt_ktx2_get_layers")]
    pub fn bt_ktx2_get_layers(this: &Basisu, handle: u64) -> u32;
    #[wasm_bindgen(method, js_name = "_bt_ktx2_get_basis_tex_format")]
    pub fn bt_ktx2_get_basis_tex_format(this: &Basisu, handle: u64) -> u32;
    #[wasm_bindgen(method, js_name = "_bt_ktx2_is_etc1s")]
    pub fn bt_ktx2_is_etc1s(this: &Basisu, handle: u64) -> u32;
    #[wasm_bindgen(method, js_name = "_bt_ktx2_is_uastc_ldr_4x4")]
    pub fn bt_ktx2_is_uastc_ldr_4x4(this: &Basisu, handle: u64) -> u32;
    #[wasm_bindgen(method, js_name = "_bt_ktx2_is_hdr")]
    pub fn bt_ktx2_is_hdr(this: &Basisu, handle: u64) -> u32;
    #[wasm_bindgen(method, js_name = "_bt_ktx2_is_hdr_4x4")]
    pub fn bt_ktx2_is_hdr_4x4(this: &Basisu, handle: u64) -> u32;
    #[wasm_bindgen(method, js_name = "_bt_ktx2_is_hdr_6x6")]
    pub fn bt_ktx2_is_hdr_6x6(this: &Basisu, handle: u64) -> u32;
    #[wasm_bindgen(method, js_name = "_bt_ktx2_is_ldr")]
    pub fn bt_ktx2_is_ldr(this: &Basisu, handle: u64) -> u32;
    #[wasm_bindgen(method, js_name = "_bt_ktx2_is_astc_ldr")]
    pub fn bt_ktx2_is_astc_ldr(this: &Basisu, handle: u64) -> u32;
    #[wasm_bindgen(method, js_name = "_bt_ktx2_is_xuastc_ldr")]
    pub fn bt_ktx2_is_xuastc_ldr(this: &Basisu, handle: u64) -> u32;
    #[wasm_bindgen(method, js_name = "_bt_ktx2_get_block_width")]
    pub fn bt_ktx2_get_block_width(this: &Basisu, handle: u64) -> u32;
    #[wasm_bindgen(method, js_name = "_bt_ktx2_get_block_height")]
    pub fn bt_ktx2_get_block_height(this: &Basisu, handle: u64) -> u32;
    #[wasm_bindgen(method, js_name = "_bt_ktx2_has_alpha")]
    pub fn bt_ktx2_has_alpha(this: &Basisu, handle: u64) -> u32;
    #[wasm_bindgen(method, js_name = "_bt_ktx2_get_dfd_color_model")]
    pub fn bt_ktx2_get_dfd_color_model(this: &Basisu, handle: u64) -> u32;
    #[wasm_bindgen(method, js_name = "_bt_ktx2_get_dfd_color_primaries")]
    pub fn bt_ktx2_get_dfd_color_primaries(this: &Basisu, handle: u64) -> u32;
    #[wasm_bindgen(method, js_name = "_bt_ktx2_get_dfd_transfer_func")]
    pub fn bt_ktx2_get_dfd_transfer_func(this: &Basisu, handle: u64) -> u32;
    #[wasm_bindgen(method, js_name = "_bt_ktx2_is_srgb")]
    pub fn bt_ktx2_is_srgb(this: &Basisu, handle: u64) -> u32;
    #[wasm_bindgen(method, js_name = "_bt_ktx2_get_dfd_flags")]
    pub fn bt_ktx2_get_dfd_flags(this: &Basisu, handle: u64) -> u32;
    #[wasm_bindgen(method, js_name = "_bt_ktx2_get_dfd_total_samples")]
    pub fn bt_ktx2_get_dfd_total_samples(this: &Basisu, handle: u64) -> u32;
    #[wasm_bindgen(method, js_name = "_bt_ktx2_get_dfd_channel_id0")]
    pub fn bt_ktx2_get_dfd_channel_id0(this: &Basisu, handle: u64) -> u32;
    #[wasm_bindgen(method, js_name = "_bt_ktx2_get_dfd_channel_id1")]
    pub fn bt_ktx2_get_dfd_channel_id1(this: &Basisu, handle: u64) -> u32;
    #[wasm_bindgen(method, js_name = "_bt_ktx2_is_video")]
    pub fn bt_ktx2_is_video(this: &Basisu, handle: u64) -> u32;
    #[wasm_bindgen(method, js_name = "_bt_ktx2_get_ldr_hdr_upconversion_nit_multiplier")]
    pub fn bt_ktx2_get_ldr_hdr_upconversion_nit_multiplier(
        this: &Basisu,
        handle: u64,
    ) -> f32;
    #[wasm_bindgen(method, js_name = "_bt_ktx2_get_level_orig_width")]
    pub fn bt_ktx2_get_level_orig_width(
        this: &Basisu,
        handle: u64,
        level_index: u32,
        layer_index: u32,
        face_index: u32,
    ) -> u32;
    #[wasm_bindgen(method, js_name = "_bt_ktx2_get_level_orig_height")]
    pub fn bt_ktx2_get_level_orig_height(
        this: &Basisu,
        handle: u64,
        level_index: u32,
        layer_index: u32,
        face_index: u32,
    ) -> u32;
    #[wasm_bindgen(method, js_name = "_bt_ktx2_get_level_actual_width")]
    pub fn bt_ktx2_get_level_actual_width(
        this: &Basisu,
        handle: u64,
        level_index: u32,
        layer_index: u32,
        face_index: u32,
    ) -> u32;
    #[wasm_bindgen(method, js_name = "_bt_ktx2_get_level_actual_height")]
    pub fn bt_ktx2_get_level_actual_height(
        this: &Basisu,
        handle: u64,
        level_index: u32,
        layer_index: u32,
        face_index: u32,
    ) -> u32;
    #[wasm_bindgen(method, js_name = "_bt_ktx2_get_level_num_blocks_x")]
    pub fn bt_ktx2_get_level_num_blocks_x(
        this: &Basisu,
        handle: u64,
        level_index: u32,
        layer_index: u32,
        face_index: u32,
    ) -> u32;
    #[wasm_bindgen(method, js_name = "_bt_ktx2_get_level_num_blocks_y")]
    pub fn bt_ktx2_get_level_num_blocks_y(
        this: &Basisu,
        handle: u64,
        level_index: u32,
        layer_index: u32,
        face_index: u32,
    ) -> u32;
    #[wasm_bindgen(method, js_name = "_bt_ktx2_get_level_total_blocks")]
    pub fn bt_ktx2_get_level_total_blocks(
        this: &Basisu,
        handle: u64,
        level_index: u32,
        layer_index: u32,
        face_index: u32,
    ) -> u32;
    #[wasm_bindgen(method, js_name = "_bt_ktx2_get_level_alpha_flag")]
    pub fn bt_ktx2_get_level_alpha_flag(
        this: &Basisu,
        handle: u64,
        level_index: u32,
        layer_index: u32,
        face_index: u32,
    ) -> u32;
    #[wasm_bindgen(method, js_name = "_bt_ktx2_get_level_iframe_flag")]
    pub fn bt_ktx2_get_level_iframe_flag(
        this: &Basisu,
        handle: u64,
        level_index: u32,
        layer_index: u32,
        face_index: u32,
    ) -> u32;
    #[wasm_bindgen(method, js_name = "_bt_ktx2_start_transcoding")]
    pub fn bt_ktx2_start_transcoding(this: &Basisu, handle: u64) -> u32;
    #[wasm_bindgen(method, js_name = "_bt_ktx2_create_transcode_state")]
    pub fn bt_ktx2_create_transcode_state(this: &Basisu) -> u64;
    #[wasm_bindgen(method, js_name = "_bt_ktx2_destroy_transcode_state")]
    pub fn bt_ktx2_destroy_transcode_state(this: &Basisu, handle: u64);
    #[wasm_bindgen(method, js_name = "_bt_ktx2_transcode_image_level")]
    pub fn bt_ktx2_transcode_image_level(
        this: &Basisu,
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
    ) -> u32;
}

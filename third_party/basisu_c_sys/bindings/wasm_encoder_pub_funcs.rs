pub unsafe fn bu_get_version() -> u32 {
    BASISU_INSTANCE
        .with(|inst| {
            let inst = inst.get().unwrap();
            inst.bu_get_version()
        })
}
pub unsafe fn bu_enable_debug_printf(flag: u32) {
    BASISU_INSTANCE
        .with(|inst| {
            let inst = inst.get().unwrap();
            inst.bu_enable_debug_printf(flag)
        })
}
pub unsafe fn bu_init() {
    BASISU_INSTANCE
        .with(|inst| {
            let inst = inst.get().unwrap();
            inst.bu_init()
        })
}
pub unsafe fn bu_alloc(size: u64) -> u64 {
    BASISU_INSTANCE
        .with(|inst| {
            let inst = inst.get().unwrap();
            inst.bu_alloc(size)
        })
}
pub unsafe fn bu_free(ofs: u64) {
    BASISU_INSTANCE
        .with(|inst| {
            let inst = inst.get().unwrap();
            inst.bu_free(ofs)
        })
}
pub unsafe fn bu_new_comp_params() -> u64 {
    BASISU_INSTANCE
        .with(|inst| {
            let inst = inst.get().unwrap();
            inst.bu_new_comp_params()
        })
}
#[must_use]
pub unsafe fn bu_delete_comp_params(params_ofs: u64) -> Bool32 {
    BASISU_INSTANCE
        .with(|inst| {
            let inst = inst.get().unwrap();
            Bool32(inst.bu_delete_comp_params(params_ofs))
        })
}
pub unsafe fn bu_comp_params_get_comp_data_size(params_ofs: u64) -> u64 {
    BASISU_INSTANCE
        .with(|inst| {
            let inst = inst.get().unwrap();
            inst.bu_comp_params_get_comp_data_size(params_ofs)
        })
}
pub unsafe fn bu_comp_params_get_comp_data_ofs(params_ofs: u64) -> u64 {
    BASISU_INSTANCE
        .with(|inst| {
            let inst = inst.get().unwrap();
            inst.bu_comp_params_get_comp_data_ofs(params_ofs)
        })
}
#[must_use]
pub unsafe fn bu_comp_params_clear(params_ofs: u64) -> Bool32 {
    BASISU_INSTANCE
        .with(|inst| {
            let inst = inst.get().unwrap();
            Bool32(inst.bu_comp_params_clear(params_ofs))
        })
}
#[must_use]
pub unsafe fn bu_comp_params_set_image_rgba32(
    params_ofs: u64,
    image_index: u32,
    img_data_ofs: u64,
    width: u32,
    height: u32,
    pitch_in_bytes: u32,
) -> Bool32 {
    BASISU_INSTANCE
        .with(|inst| {
            let inst = inst.get().unwrap();
            Bool32(
                inst
                    .bu_comp_params_set_image_rgba32(
                        params_ofs,
                        image_index,
                        img_data_ofs,
                        width,
                        height,
                        pitch_in_bytes,
                    ),
            )
        })
}
#[must_use]
pub unsafe fn bu_comp_params_set_image_float_rgba(
    params_ofs: u64,
    image_index: u32,
    img_data_ofs: u64,
    width: u32,
    height: u32,
    pitch_in_bytes: u32,
) -> Bool32 {
    BASISU_INSTANCE
        .with(|inst| {
            let inst = inst.get().unwrap();
            Bool32(
                inst
                    .bu_comp_params_set_image_float_rgba(
                        params_ofs,
                        image_index,
                        img_data_ofs,
                        width,
                        height,
                        pitch_in_bytes,
                    ),
            )
        })
}
#[must_use]
pub unsafe fn bu_compress_texture(
    params_ofs: u64,
    desired_basis_tex_format: u32,
    quality_level: ::core::ffi::c_int,
    effort_level: ::core::ffi::c_int,
    flags_and_quality: u64,
    low_level_uastc_rdo_or_dct_quality: f32,
) -> Bool32 {
    BASISU_INSTANCE
        .with(|inst| {
            let inst = inst.get().unwrap();
            Bool32(
                inst
                    .bu_compress_texture(
                        params_ofs,
                        desired_basis_tex_format,
                        quality_level,
                        effort_level,
                        flags_and_quality,
                        low_level_uastc_rdo_or_dct_quality,
                    ),
            )
        })
}

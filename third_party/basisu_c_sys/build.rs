const FLAGS: &[&str] = &[
    "/wd4189",
    "-Wno-unused-variable",
    "-Wno-type-limits",
    "-Wno-unused-but-set-variable",
    "-Wno-unused-function",
    "-Wno-misleading-indentation",
    "-Wno-stringop-overflow",
    "-Wno-array-bounds",
    "-fno-exceptions",
    // Fix gcc optimization issue.
    // See vendor/basis_universal/transcoder/basisu.h
    // See https://github.com/godotengine/godot/pull/114839
    "-fno-strict-aliasing",
];

// Disable PVRTC1/2, ATC, FXT1 as wgpu does not support them.
const DEFINES: &[(&str, &str)] = &[
    // ("BASISU_FORCE_DEVEL_MESSAGES", "1"), // Enable debug message.
    // ("BASISD_SUPPORT_KTX2", "1"),
    // ("BASISD_SUPPORT_KTX2_ZSTD", "1"),
    // ("BASISD_SUPPORT_UASTC", "1"),
    ("BASISD_SUPPORT_DXT1", "0"), //(BC1)
    // ("BASISD_SUPPORT_DXT5A", "1"), //(BC3 / 4 / 5)
    // ("BASISD_SUPPORT_BC7", "1"),
    // ("BASISD_SUPPORT_BC7_MODE5", "1"),
    ("BASISD_SUPPORT_PVRTC1", "0"),
    // ("BASISD_SUPPORT_ETC2_EAC_A8", "1"),
    // ("BASISD_SUPPORT_ASTC", "1"),
    // ("BASISD_SUPPORT_XUASTC", "1"),
    ("BASISD_SUPPORT_ATC", "0"),
    // ("BASISD_SUPPORT_ETC2_EAC_RG11", "1"),
    // ("BASISD_SUPPORT_ASTC_HIGHER_OPAQUE_QUALITY", "1"),
    ("BASISD_SUPPORT_FXT1", "0"),
    ("BASISD_SUPPORT_PVRTC2", "0"),
    // ("BASISD_SUPPORT_UASTC_HDR", "1"),
];

const ENCODER_SRCS: &[&str] = &[
    "vendor/basis_universal/encoder/basisu_astc_hdr_6x6_enc.cpp",
    "vendor/basis_universal/encoder/basisu_astc_hdr_common.cpp",
    "vendor/basis_universal/encoder/basisu_astc_ldr_common.cpp",
    "vendor/basis_universal/encoder/basisu_astc_ldr_encode.cpp",
    "vendor/basis_universal/encoder/basisu_backend.cpp",
    "vendor/basis_universal/encoder/basisu_basis_file.cpp",
    "vendor/basis_universal/encoder/basisu_bc7enc.cpp",
    "vendor/basis_universal/encoder/basisu_comp.cpp",
    "vendor/basis_universal/encoder/basisu_enc.cpp",
    "vendor/basis_universal/encoder/basisu_etc.cpp",
    "vendor/basis_universal/encoder/basisu_frontend.cpp",
    "vendor/basis_universal/encoder/basisu_gpu_texture.cpp",
    "vendor/basis_universal/encoder/basisu_kernels_sse.cpp",
    "vendor/basis_universal/encoder/basisu_opencl.cpp",
    "vendor/basis_universal/encoder/basisu_pvrtc1_4.cpp",
    "vendor/basis_universal/encoder/basisu_resample_filters.cpp",
    "vendor/basis_universal/encoder/basisu_resampler.cpp",
    "vendor/basis_universal/encoder/basisu_ssim.cpp",
    "vendor/basis_universal/encoder/basisu_uastc_enc.cpp",
    "vendor/basis_universal/encoder/basisu_uastc_hdr_4x4_enc.cpp",
    "vendor/basis_universal/encoder/basisu_wasm_api.cpp",
    "vendor/basis_universal/encoder/basisu_wasm_transcoder_api.cpp",
    "vendor/basis_universal/encoder/jpgd.cpp",
    "vendor/basis_universal/encoder/pvpngreader.cpp",
    "vendor/basis_universal/encoder/3rdparty/android_astc_decomp.cpp",
    "vendor/basis_universal/encoder/3rdparty/tinyexr.cpp",
    "vendor/basis_universal/transcoder/basisu_transcoder.cpp",
    "vendor/basis_universal/zstd/zstd.c",
];

const TRANSCODER_SRCS: &[&str] = &[
    "vendor/basis_universal/encoder/basisu_wasm_transcoder_api.cpp",
    "vendor/basis_universal/transcoder/basisu_transcoder.cpp",
    "vendor/basis_universal/zstd/zstddeclib.c",
];

// SuperSpec: the generated bindings are checked in under `bindings/` (produced once by the
// upstream build script with bindgen), so builds need no libclang. They use fixed-width types
// only and are the same for native and wasm32 targets.
const BINDINGS: &[&str] = &[
    "basisu_api_common.rs",
    "basisu_c_api.rs",
    "basisu_c_transcoder_api.rs",
    "wasm_encoder_binding.rs",
    "wasm_encoder_pub_funcs.rs",
    "wasm_transcoder_binding.rs",
    "wasm_transcoder_pub_funcs.rs",
];

fn main() {
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap());
    for name in BINDINGS {
        std::fs::copy(std::path::Path::new("bindings").join(name), out.join(name))
            .expect("checked-in basisu binding");
    }
    let target = std::env::var("TARGET").unwrap();
    if target != "wasm32-unknown-unknown" {
        compile_basisu_static();
    }
    println!("cargo::rerun-if-changed=vendor/");
    println!("cargo::rerun-if-changed=bindings/");
}
fn compile_basisu_static() {
    for is_cpp in [true, false] {
        let mut build = cc::Build::new();

        // Use c++_static for Android.
        let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap();
        if target_os == "android" {
            build.flag("-U_GNU_SOURCE");
            build.cpp_link_stdlib("c++_static");
        }

        build.cpp(is_cpp).std(if is_cpp { "c++17" } else { "c17" });
        for f in FLAGS {
            build.flag_if_supported(f);
        }
        // SuperSpec: release-style C asserts off, so malformed input fails through the codec's
        // own checks instead of aborting the process.
        build.define("NDEBUG", None);
        for (define, value) in DEFINES {
            build.define(define, *value);
        }
        build.files(
            if cfg!(feature = "encoder") {
                ENCODER_SRCS
            } else {
                TRANSCODER_SRCS
            }
            .iter()
            .filter(|src| src.ends_with(if is_cpp { ".cpp" } else { ".c" })),
        );
        build.compile(&format!(
            "basisu_c_sys_{}",
            if is_cpp { "cpp" } else { "c" }
        ));
    }
}


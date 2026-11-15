use crate::architecture::V3;

// Frozen byte-to-linear sRGB transfer values avoid target-specific fractional powers.
pub const LINEAR: [f64; 256] = include!("srgb_table.rs");

/// Converts a six-digit sRGB color into linear vertex-color multipliers.
pub fn srgb(hex: &str) -> V3 {
    assert!(hex.len() == 7 && hex.starts_with('#'));
    std::array::from_fn(|i| {
        LINEAR[usize::from_str_radix(&hex[1 + i * 2..3 + i * 2], 16).expect("palette color")]
    })
}

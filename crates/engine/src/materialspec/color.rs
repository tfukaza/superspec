//! Reusable linear-color arithmetic and deterministic modulation.
use superspec_geometry::math::{V3, sin};
pub use superspec_types::color::{LINEAR, srgb};

pub fn shade(color: V3, factor: f64) -> V3 {
    // Preserve JS Math.min's NaN behavior so invalid appearance is rejected at admission.
    color.map(|v| {
        let value = v * factor;
        if value.is_nan() { value } else { value.min(1.) }
    })
}
pub fn blend(a: V3, b: V3) -> V3 {
    std::array::from_fn(|i| a[i] * b[i])
}
pub fn mix(a: V3, b: V3, t: f64) -> V3 {
    std::array::from_fn(|i| a[i] + (b[i] - a[i]) * t)
}
pub fn mottle(u: f64, z: f64, phase: f64) -> f64 {
    sin(u * 0.73 + phase) * sin(z * 0.61 + phase * 1.7) * 0.6
        + sin(u * 1.91 - z * 1.13 + phase * 0.3) * 0.4
}

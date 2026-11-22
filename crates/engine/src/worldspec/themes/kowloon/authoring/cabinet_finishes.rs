//! Worn stippled enamel on the normalized front of a folded steel enclosure.
use super::Files;
use crate::materialspec::authoring::{
    Maps,
    sampling::{anisotropic_lattice, lattice, mix},
};

pub(super) fn bake() -> Result<Files, String> {
    const SIZE: usize = 512;
    let mut maps = Maps::new(SIZE, SIZE);
    let mut heights = Vec::with_capacity(SIZE * SIZE);
    for y in 0..SIZE {
        for x in 0..SIZE {
            let u = (x as f64 + 0.5) / SIZE as f64;
            let v = (y as f64 + 0.5) / SIZE as f64;
            let edge = u.min(1. - u).min(v.min(1. - v) * 1.35);
            let pocket = lattice(u, v, 31, 93.);
            let broken =
                0.58 * pocket + 0.27 * lattice(u, v, 94, 7.) + 0.15 * lattice(u, v, 190, 52.);
            let corner = (1. - u.min(1. - u) * 6.).max(0.) * (1. - v.min(1. - v) * 5.).max(0.);
            let border = (1. - edge / (0.038 + 0.045 * corner)).clamp(0., 1.);
            let abrasion = broken + 0.27 * border + 0.075 * corner;
            let chip = ((abrasion - 0.803) * 28.).clamp(0., 1.) * border.sqrt();
            let exposed = ((abrasion - 0.884) * 24.).clamp(0., 1.) * chip;
            let pigment = 0.040 * (lattice(u, v, 8, 24.) - 0.5)
                + 0.026 * (anisotropic_lattice(u, v, [55, 12], 91.) - 0.5)
                + 0.020 * (pocket - 0.5);
            // Chalky topcoat, yellowed undercoat, then occasional oxidized substrate.
            let paint = [0.94 + pigment, 0.955 + pigment, 0.932 + pigment];
            let primer = [0.79, 0.68, 0.51];
            let oxide = [0.46, 0.32, 0.21];
            maps.pixel(
                0,
                std::array::from_fn(|k| mix(mix(paint[k], primer[k], chip), oxide[k], exposed)),
            );
            maps.pixel(
                2,
                [
                    1.,
                    0.58 + 0.13 * chip + 0.07 * (pocket - 0.5),
                    0.035 + 0.09 * exposed,
                ],
            );
            heights.push(0.00020 * (pocket - 0.5) - 0.00064 * chip);
        }
    }
    maps.normals(&heights, SIZE as f64, true);
    maps.encode("cabinet-enamel")
}

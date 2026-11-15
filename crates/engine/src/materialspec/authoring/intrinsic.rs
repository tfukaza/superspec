//! Common intrinsic material recipes.
use super::{
    Maps,
    sampling::{anisotropic_lattice, lattice},
};

pub fn brushed_metal() -> Maps {
    let size = 512;
    let mut maps = Maps::new(size, size);
    let mut heights = Vec::with_capacity(size * size);
    for y in 0..size {
        for x in 0..size {
            let (u, v) = (x as f64 / size as f64, y as f64 / size as f64);
            let cloud = lattice(u, v, 5, 173.) * 0.65 + lattice(u, v, 13, 619.) * 0.35;
            let brushed = anisotropic_lattice(u, v, [9, 240], 47.);
            let fine = anisotropic_lattice(u, v, [53, 251], 91.);
            let scuff = (anisotropic_lattice(u, v, [21, 95], 539.) - 0.58).max(0.) * 2.3;
            let haze = (cloud - 0.48).max(0.);
            let silver = 0.76 + (cloud - 0.5) * 0.02 + (brushed - 0.5) * 0.019 + scuff * 0.015;
            maps.pixel(0, [silver, silver + 0.006, silver + 0.009]);
            maps.pixel(
                2,
                [
                    1.,
                    0.31 + cloud * 0.14 + scuff * 0.12 + (brushed - 0.5) * 0.035,
                    0.97 - haze * 0.14,
                ],
            );
            heights.push((brushed - 0.5) * 0.000008 + (fine - 0.5) * 0.000003 - scuff * 0.000004);
        }
    }
    maps.normals(&heights, 1024., true);
    maps
}

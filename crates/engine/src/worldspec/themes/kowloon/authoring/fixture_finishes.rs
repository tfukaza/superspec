//! Paint and pressed glass for the arcade ceiling fixtures.
use super::Files;
use crate::materialspec::authoring::{
    Maps,
    sampling::{anisotropic_lattice, lattice},
};
use std::f64::consts::TAU;

pub fn bake() -> Result<Files, String> {
    const SIZE: usize = 512;
    let mut paint = Maps::new(SIZE, SIZE);
    let mut glass = Maps::new(SIZE, SIZE);
    let mut paint_height = Vec::with_capacity(SIZE * SIZE);
    let mut glass_height = Vec::with_capacity(SIZE * SIZE);
    for y in 0..SIZE {
        for x in 0..SIZE {
            let u = x as f64 / SIZE as f64;
            let v = y as f64 / SIZE as f64;
            let patch = lattice(u, v, 6, 83.) - 0.5;
            let grain = lattice(u, v, 94, 27.) - 0.5;
            let sweep = anisotropic_lattice(u, v, [6, 44], 51.) - 0.5;
            let age = (patch * 0.8 + sweep * 0.2).max(0.);
            let tone = 0.83 + 0.07 * patch + 0.045 * sweep + 0.038 * grain;
            paint.pixel(
                0,
                [tone + 0.026 * age, tone + 0.015, tone - 0.018 - 0.050 * age],
            );
            paint.pixel(2, [1., 0.47 + 0.16 * age + 0.10 * grain, 0.035]);
            paint_height.push(0.003 * sweep + 0.0006 * grain);

            // Low rounded flutes modulate the light transmitted by the molded cover.
            let flute = (u * 12. * TAU).cos();
            let cross = (v * 6. * TAU).cos();
            let crown = (0.5 + 0.5 * flute).powf(0.7);
            let transmission = 0.81 + 0.14 * crown + 0.022 * cross + 0.013 * patch;
            glass.pixel(0, [transmission, transmission * 0.98, transmission * 0.93]);
            glass.pixel(2, [1., 0.28 + 0.08 * (1. - crown), 0.]);
            glass_height.push(0.0035 * crown + 0.0012 * cross);
        }
    }
    paint.normals(&paint_height, SIZE as f64, true);
    glass.normals(&glass_height, SIZE as f64, true);
    let mut files = paint.encode("fixture-enamel")?;
    files.extend(glass.encode("bulkhead-glass")?);
    files.extend(super::cabinet_finishes::bake()?);
    Ok(files)
}

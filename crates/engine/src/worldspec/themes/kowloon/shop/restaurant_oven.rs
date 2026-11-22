//! Barrel oven, thermometer and gas cylinder for the roast-meat kitchen.
use super::super::{
    fixture::Fixture,
    palette::{shade, srgb},
    pen::{Pen, Tint},
};
use super::forms::{self, Tints};
use std::f64::consts::PI;
use superspec_geometry::{
    V3,
    math::{cos, sin},
};
use superspec_types::rng::SeededRng;
fn on<'a, 'c>(d: &'a mut Fixture<'_, 'c>, pen: Pen) -> Fixture<'a, 'c> {
    Fixture {
        pen,
        mesh: d.mesh,
        repeats: d.repeats,
    }
}
fn lathe(
    d: &mut Fixture<'_, '_>,
    slot: &str,
    at: V3,
    profile: &[[f64; 2]],
    colors: &[V3],
    sides: usize,
    repeat: Option<f64>,
) -> Result<(), String> {
    let tints: Vec<_> = colors.iter().map(|c| Tint::Color(*c)).collect();
    forms::lathe(d, slot, at, profile, &Tints::Bands(&tints), sides, repeat)
}
pub fn gas_cylinder(d: &mut Fixture<'_, '_>, at: V3, rng: &mut SeededRng) -> Result<(), String> {
    d.mesh.check_work(0)?;
    let paint = shade(
        rng.pick(&[srgb("#b8281e"), srgb("#2f5f9a"), srgb("#8a8f92")]),
        rng.float(0.8, 1.05),
    );
    lathe(
        d,
        "paint",
        at,
        &[
            [0.134, 0.],
            [0.144, 0.009],
            [0.144, 0.026],
            [0.15, 0.041],
            [0.15, 0.49],
            [0.143, 0.53],
            [0.11, 0.595],
            [0.05, 0.63],
            [0.05, 0.66],
            [0.087, 0.66],
            [0.09, 0.74],
            [0.08, 0.74],
            [0.08, 0.66],
        ],
        &[
            shade(paint, 0.53),
            shade(paint, 0.71),
            shade(paint, 0.84),
            shade(paint, 0.98),
            shade(paint, 1.05),
            shade(paint, 1.17),
            shade(paint, 1.12),
            shade(paint, 0.8),
            shade(paint, 0.65),
            shade(paint, 0.89),
            shade(paint, 1.26),
            shade(paint, 0.54),
        ],
        18,
        None,
    )?;
    lathe(
        d,
        "metal",
        [at[0], at[1], at[2] + 0.63],
        &[
            [0.02, 0.],
            [0.02, 0.07],
            [0.035, 0.075],
            [0.035, 0.09],
            [0., 0.09],
        ],
        &[srgb("#b8a04a")],
        6,
        None,
    )
}
#[allow(clippy::approx_constant)] // The 318 mm hatch depth is an authored dimension.
pub fn roaster(
    d: &mut Fixture<'_, '_>,
    at: V3,
    rng: &mut SeededRng,
    textured: bool,
    repeat: f64,
) -> Result<(), String> {
    d.mesh.check_work(0)?;
    let wear = rng.float(0.94, 1.04);
    let iron = shade([0.71, 0.74, 0.72], wear);
    let enamel = shade(srgb("#636a59"), wear);
    let [x, y, z] = at;
    lathe(
        d,
        "metal",
        at,
        &[
            [0.36, 1.22],
            [0.375, 1.24],
            [0.374, 1.254],
            [0.323, 1.292],
            [0.20, 1.335],
            [0.09, 1.368],
            [0., 1.37],
        ],
        &[
            shade(iron, 1.18),
            shade(iron, 1.03),
            shade(iron, 0.95),
            shade(iron, 0.92),
            shade(iron, 0.86),
            shade(iron, 0.82),
        ],
        22,
        textured.then_some(repeat),
    )?;
    for [bottom, top] in [[0., 0.3], [0.34, 0.9], [0.94, 1.22]] {
        let finish = |p: V3| {
            let height = (p[2] - z - bottom) / (top - bottom);
            let use_marks = sin((p[0] - x) * 21. + sin((p[1] - y) * 7.)) * 0.045;
            Ok(shade(enamel, 0.83 + 0.21 * height + use_marks))
        };
        forms::lathe(
            d,
            "paint",
            at,
            &[[0.36, bottom], [0.36, top]],
            &Tints::Uniform(&Tint::Field(&finish)),
            22,
            None,
        )?;
    }
    for [bottom, top] in [[0.3, 0.34], [0.9, 0.94]] {
        lathe(
            d,
            "metal",
            at,
            &[
                [0.36, bottom],
                [0.37, bottom + 0.005],
                [0.372, bottom + 0.009],
                [0.372, top - 0.007],
                [0.367, top - 0.002],
                [0.36, top],
            ],
            &[
                shade(iron, 0.79),
                shade(iron, 1.22),
                iron,
                shade(iron, 1.17),
                shade(iron, 0.75),
            ],
            22,
            textured.then_some(repeat),
        )?;
    }
    forms::limb(
        d,
        "metal",
        &[
            [x - 0.1, y - 0.2, z + 1.33],
            [x - 0.1, y - 0.2, z + 1.42],
            [x + 0.1, y - 0.2, z + 1.42],
            [x + 0.1, y - 0.2, z + 1.33],
        ],
        &[0.008; 4],
        &Tint::Color(shade(iron, 0.7)),
        4,
        1.,
    )?;
    d.cuboid(
        "paint",
        [x - 0.152, y - 0.369, z + 0.407],
        [x + 0.152, y - 0.318, z + 0.793],
        srgb("#383b37"),
        "f",
    )?;
    d.cuboid(
        "metal",
        [x - 0.14, y - 0.376, z + 0.42],
        [x + 0.14, y - 0.367, z + 0.78],
        shade(iron, 1.18),
        "f",
    )?;
    for level in [0.46, 0.70] {
        d.cuboid(
            "metal",
            [x - 0.151, y - 0.383, z + level],
            [x - 0.12, y - 0.367, z + level + 0.036],
            [0.82, 0.84, 0.83],
            "f",
        )?;
    }
    d.cuboid(
        "metal",
        [x + 0.075, y - 0.388, z + 0.56],
        [x + 0.108, y - 0.373, z + 0.65],
        [0.80, 0.82, 0.80],
        "f",
    )?;
    d.bar(
        "paint",
        [x + 0.085, y - 0.397, z + 0.575],
        [x + 0.085, y - 0.397, z + 0.637],
        0.018,
        srgb("#39362d"),
    )?;
    for dx in [-0.12, 0.12] {
        for dz in [0.44, 0.76] {
            let pen = forms::rotated(&d.pen, [x + dx, y - 0.379, z + dz], 0, PI / 2.)?
                .with_kitchen_metal(repeat)?;
            lathe(
                &mut on(d, pen),
                "metal",
                [0.; 3],
                &[[0.006, 0.], [0.006, 0.004], [0., 0.004]],
                &[[0.88, 0.90, 0.90]],
                6,
                None,
            )?;
        }
    }
    let gauge = forms::rotated(&d.pen, [x, y - 0.352, z + 1.08], 0, PI / 2.)?;
    lathe(
        &mut on(d, gauge.with_kitchen_metal(repeat)?),
        "metal",
        [0.; 3],
        &[[0.049, 0.], [0.049, 0.016], [0.043, 0.018]],
        &[[0.80, 0.82, 0.81]],
        16,
        None,
    )?;
    let d = &mut on(d, gauge);
    lathe(
        d,
        "paint",
        [0.; 3],
        &[[0.043, 0.017], [0., 0.017]],
        &[srgb("#d6cba7")],
        16,
        None,
    )?;
    for tick in 0..9 {
        let angle = (-0.8 + tick as f64 * 0.325) * PI;
        d.bar(
            "paint",
            [cos(angle) * 0.031, sin(angle) * 0.031, 0.018],
            [cos(angle) * 0.037, sin(angle) * 0.037, 0.018],
            0.0015,
            srgb("#514b3d"),
        )?;
    }
    d.bar(
        "paint",
        [0., 0., 0.02],
        [0.019, 0.022, 0.02],
        0.0025,
        srgb("#9a3f2c"),
    )
}

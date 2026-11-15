//! Fitted kitchen work surfaces and supported counter equipment.
use super::super::{
    fixture::Fixture,
    palette::{shade, srgb},
    pen::Tint,
};
use super::{
    forms::{self, Tints},
    furniture::Shelf,
    grocer_props::{self, Options},
    restaurant_surfaces::{self, Settings},
};
use superspec_geometry::V3;
use superspec_types::rng::SeededRng;
const WIPED: V3 = [0.88, 0.90, 0.91];
fn galvanized() -> V3 {
    srgb("#a4a9aa")
}
fn metal<'a, 'c>(d: &'a mut Fixture<'_, 'c>, repeat: f64) -> Result<Fixture<'a, 'c>, String> {
    Ok(Fixture {
        pen: d.pen.with_kitchen_metal(repeat)?,
        mesh: d.mesh,
        repeats: d.repeats,
    })
}
fn check(d: &Fixture<'_, '_>, at: V3, size: &[f64]) -> Result<(), String> {
    if at.iter().any(|v| !v.is_finite() || v.abs() > 1000.)
        || size
            .iter()
            .any(|v| !v.is_finite() || !(0.01..=40.).contains(v))
    {
        return Err(
            "Kitchen fittings require bounded positive dimensions and finite positions.".into(),
        );
    }
    d.mesh.check_work(0)
}
fn lathe(
    d: &mut Fixture<'_, '_>,
    at: V3,
    profile: &[[f64; 2]],
    colors: &[V3],
) -> Result<(), String> {
    let tints: Vec<_> = colors.iter().map(|c| Tint::Color(*c)).collect();
    forms::lathe(d, "pane", at, profile, &Tints::Bands(&tints), 10, None)
}
pub fn steel_top(
    d: &mut Fixture<'_, '_>,
    size: V3,
    thickness: f64,
    repeat: f64,
) -> Result<(), String> {
    check(d, [0.; 3], &size)?;
    if !thickness.is_finite() || thickness <= 0.006 || thickness >= size[2] {
        return Err("Steel top thickness must fit below its work plane.".into());
    }
    let d = &mut metal(d, repeat)?;
    let [w, depth, h] = size;
    let bevel = 0.004;
    d.cuboid(
        "metal",
        [0., 0., h - thickness],
        [w, depth, h - bevel],
        [0.62, 0.65, 0.65],
        "bu",
    )?;
    d.face(
        "metal",
        &[
            [bevel, bevel, h],
            [w - bevel, bevel, h],
            [w - bevel, depth - bevel, h],
            [bevel, depth - bevel, h],
        ],
        [0., 0., 1.],
        WIPED,
    )?;
    let low = [
        [0., 0., h - bevel],
        [w, 0., h - bevel],
        [w, depth, h - bevel],
        [0., depth, h - bevel],
    ];
    let high = [
        [bevel, bevel, h],
        [w - bevel, bevel, h],
        [w - bevel, depth - bevel, h],
        [bevel, depth - bevel, h],
    ];
    let hints = [[0., -1., 1.], [1., 0., 1.], [0., 1., 1.], [-1., 0., 1.]];
    for i in 0..4 {
        d.face(
            "metal",
            &[low[i], low[(i + 1) % 4], high[(i + 1) % 4], high[i]],
            hints[i],
            [1.; 3],
        )?;
    }
    d.face(
        "metal",
        &[
            [0., 0., h - thickness],
            [w, 0., h - thickness],
            [w, 0., h - thickness + 0.006],
            [0., 0., h - thickness + 0.006],
        ],
        [0., -1., 0.],
        [0.28, 0.30, 0.29],
    )?;
    d.face(
        "metal",
        &[
            [0., 0., h - thickness + 0.006],
            [w, 0., h - thickness + 0.006],
            [w, 0., h - bevel],
            [0., 0., h - bevel],
        ],
        [0., -1., 0.],
        [0.62, 0.65, 0.65],
    )
}
pub fn workbench(d: &mut Fixture<'_, '_>, size: V3, repeat: f64) -> Result<Shelf, String> {
    check(d, [0.; 3], &size)?;
    let [w, depth, h] = size;
    if w < 0.25 || depth < 0.2 || h < 0.5 {
        return Err("Kitchen workbench must leave usable cabinet space.".into());
    }
    let steel = &mut metal(d, repeat)?;
    let body = h - 0.04;
    steel.cuboid(
        "metal",
        [0.02, 0.03, 0.07],
        [w - 0.02, depth, body],
        [0.47, 0.50, 0.50],
        "bdu",
    )?;
    steel.cuboid(
        "metal",
        [0.035, 0.045, 0.07],
        [w - 0.035, depth - 0.02, 0.11],
        [0.23, 0.25, 0.25],
        "",
    )?;
    for x in [0.04, w - 0.08] {
        for y in [0.05, depth - 0.07] {
            steel.cuboid(
                "metal",
                [x, y, 0.],
                [x + 0.035, y + 0.035, 0.11],
                [0.50, 0.53, 0.53],
                "",
            )?;
        }
    }
    let bays = (w / 0.85).ceil().max(1.) as usize;
    let pitch = (w - 0.06) / bays as f64;
    let separator = body - 0.15;
    for i in 0..bays {
        let left = 0.03 + i as f64 * pitch;
        let right = left + pitch - 0.008;
        let brightness = 0.56 + (i % 3) as f64 * 0.045;
        steel.cuboid(
            "metal",
            [left, 0.025, 0.115],
            [right, 0.037, separator - 0.006],
            [brightness, brightness + 0.025, brightness + 0.025],
            "bf",
        )?;
        for [low, high] in [[0.115, 0.235], [0.235, separator - 0.006]] {
            let tint = |p: V3| {
                let wear = (1. - (p[2] - 0.115) / 0.12).max(0.) * 0.10;
                Ok([
                    brightness - wear,
                    brightness + 0.025 - wear,
                    brightness + 0.025 - wear,
                ])
            };
            steel.pen.face(
                steel.mesh,
                "metal",
                &[
                    [left, 0.025, low],
                    [right, 0.025, low],
                    [right, 0.025, high],
                    [left, 0.025, high],
                ],
                [0., -1., 0.],
                &Tint::Field(&tint),
                steel.repeat("metal"),
                None,
                None,
            )?;
        }
        steel.cuboid(
            "metal",
            [left, 0.023, separator],
            [right, 0.037, body - 0.012],
            [0.70, 0.72, 0.72],
            "f",
        )?;
        let handle = (left + right) / 2.;
        for z in [separator - 0.065, body - 0.08] {
            for x in [handle - 0.055, handle + 0.055] {
                steel.bar(
                    "metal",
                    [x, 0.027, z],
                    [x, 0.009, z],
                    0.009,
                    [0.86, 0.88, 0.88],
                )?;
            }
            steel.bar(
                "metal",
                [handle - 0.055, 0.009, z],
                [handle + 0.055, 0.009, z],
                0.012,
                WIPED,
            )?;
        }
        for z in [0.19, separator - 0.10] {
            steel.cuboid(
                "metal",
                [left + 0.012, 0.021, z],
                [left + 0.028, 0.033, z + 0.028],
                [0.82, 0.84, 0.84],
                "f",
            )?;
        }
    }
    steel_top(d, [w, depth + 0.02, h], 0.04, repeat)?;
    Ok(Shelf {
        x0: 0.03,
        x1: w - 0.03,
        y: 0.03,
        depth: depth - 0.05,
        z: h,
        clear: 1.,
    })
}
pub fn counter(
    d: &mut Fixture<'_, '_>,
    size: V3,
    seed: u32,
    settings: Settings,
) -> Result<f64, String> {
    check(d, [0.; 3], &size)?;
    let [w, depth, h] = size;
    if w < 0.12 || depth < 0.06 || h < 0.06 {
        return Err("Kitchen counter dimensions leave no body beneath the top.".into());
    }
    d.cuboid(
        "paint",
        [0.02, 0.03, 0.],
        [w - 0.02, depth, h - 0.045],
        srgb("#a6aaa0"),
        "dfu",
    )?;
    let pen = d.pen.moved([0.02, 0., 0.])?;
    restaurant_surfaces::tiles(
        &mut Fixture {
            pen,
            mesh: d.mesh,
            repeats: d.repeats,
        },
        [w - 0.04, h - 0.045],
        0.024,
        seed,
        false,
        settings,
    )?;
    steel_top(d, size, 0.045, settings.metal_repeat)?;
    metal(d, settings.metal_repeat)?.bar(
        "metal",
        [0.015, 0.014, 0.045],
        [w - 0.015, 0.014, 0.045],
        0.02,
        shade(galvanized(), 0.8),
    )?;
    Ok(h)
}
pub fn extractor(
    d: &mut Fixture<'_, '_>,
    at: V3,
    width: f64,
    ceiling: f64,
    repeat: f64,
) -> Result<(), String> {
    check(d, at, &[width])?;
    if width < 0.1 || !ceiling.is_finite() || ceiling <= at[2] + 0.13 || ceiling > 1000. {
        return Err("Extractor duct must fit beneath a finite ceiling.".into());
    }
    let d = &mut metal(d, repeat)?;
    let [x, y, z] = at;
    d.cuboid(
        "metal",
        at,
        [x + width, y + 0.34, z + 0.13],
        galvanized(),
        "d",
    )?;
    d.face(
        "metal",
        &[
            at,
            [x + width, y, z],
            [x + width, y + 0.34, z - 0.15],
            [x, y + 0.34, z - 0.15],
        ],
        [0., -0.4, -1.],
        shade(galvanized(), 0.65),
    )?;
    let mut offset = 0.035;
    while offset < width - 0.03 {
        d.bar(
            "metal",
            [x + offset, y + 0.025, z - 0.008],
            [x + offset, y + 0.31, z - 0.135],
            0.017,
            shade(galvanized(), 0.45),
        )?;
        offset += 0.065;
    }
    let duct = 0.27_f64.min(width * 0.5);
    d.cuboid(
        "metal",
        [x + (width - duct) / 2., y + 0.14, z + 0.13],
        [x + (width + duct) / 2., y + 0.32, ceiling],
        shade(galvanized(), 0.85),
        "du",
    )
}
pub fn bowl_stack(
    d: &mut Fixture<'_, '_>,
    at: V3,
    count: usize,
    r: f64,
    rim: V3,
) -> Result<(), String> {
    check(d, at, &[r])?;
    if count > 64 {
        return Err("Bowl stack exceeds 64 nested bowls.".into());
    }
    let glaze = srgb("#e5e0ce");
    for i in 0..count {
        let base = [at[0], at[1], at[2] + i as f64 * 0.022];
        lathe(
            d,
            base,
            &[[r * 0.42, 0.], [r, 0.046], [r * 1.015, 0.052]],
            &[glaze, rim],
        )?;
        if i == count - 1 {
            lathe(
                d,
                base,
                &[
                    [r * 1.015, 0.052],
                    [r * 0.94, 0.047],
                    [r * 0.4, 0.009],
                    [0., 0.009],
                ],
                &[rim, glaze, glaze],
            )?;
        }
    }
    Ok(())
}
pub fn condiment_group(
    d: &mut Fixture<'_, '_>,
    at: V3,
    rng: &mut SeededRng,
    textured: bool,
) -> Result<(), String> {
    check(d, at, &[])?;
    let pen = d.pen.moved(at)?;
    let d = &mut Fixture {
        pen,
        mesh: d.mesh,
        repeats: d.repeats,
    };
    let heights = [0.225, 0.27, 0.20, 0.25];
    for (i, height) in heights.iter().enumerate() {
        let at = [(i as f64 - 1.5) * 0.063, rng.float(-0.006, 0.006), 0.];
        let yaw = rng.float(-0.22, 0.22);
        let variant = rng.integer(1, 8);
        grocer_props::draw(
            d,
            "drink-bottle",
            at,
            [0.060, 0.060, *height],
            yaw,
            &Options {
                variant,
                supported: true,
                textured,
                ..Default::default()
            },
        )?;
    }
    for i in 0..3 {
        let yaw = rng.float(-0.25, 0.25);
        let variant = rng.integer(1, 8);
        grocer_props::draw(
            d,
            "seasoning-jar",
            [(i as f64 - 1.) * 0.08, -0.11, 0.],
            [0.072, 0.072, 0.105],
            yaw,
            &Options {
                variant,
                supported: true,
                textured,
                ..Default::default()
            },
        )?;
    }
    Ok(())
}
pub fn takeaway_stack(d: &mut Fixture<'_, '_>, at: V3, rng: &mut SeededRng) -> Result<(), String> {
    check(d, at, &[])?;
    let pen = forms::rotated(&d.pen, at, 2, rng.float(-0.07, 0.07))?;
    let d = &mut Fixture {
        pen,
        mesh: d.mesh,
        repeats: d.repeats,
    };
    let count = rng.integer(4, 6);
    for i in 0..count {
        let x = i as f64 * 0.002;
        let y = (i % 2) as f64 * 0.003;
        let z = i as f64 * 0.037;
        d.cuboid(
            "paint",
            [x - 0.12, y - 0.09, z],
            [x + 0.12, y + 0.09, z + 0.035],
            srgb("#e8e3d3"),
            "d",
        )?;
        d.cuboid(
            "paint",
            [x - 0.124, y - 0.094, z + 0.031],
            [x + 0.124, y + 0.094, z + 0.037],
            srgb("#d5d0c1"),
            "d",
        )?;
    }
    Ok(())
}
pub fn meat_tray(
    d: &mut Fixture<'_, '_>,
    at: V3,
    size: [f64; 2],
    repeat: f64,
) -> Result<f64, String> {
    check(d, at, &size)?;
    let [w, depth] = size;
    let [x, y, z] = at;
    let d = &mut metal(d, repeat)?;
    d.cuboid(
        "metal",
        [x - w / 2., y - depth / 2., z],
        [x + w / 2., y + depth / 2., z + 0.009],
        galvanized(),
        "d",
    )?;
    for sign in [-1., 1.] {
        d.cuboid(
            "metal",
            [x - w / 2., y + sign * depth / 2. - 0.004, z],
            [x + w / 2., y + sign * depth / 2. + 0.004, z + 0.035],
            galvanized(),
            "",
        )?;
        d.cuboid(
            "metal",
            [x + sign * w / 2. - 0.004, y - depth / 2., z],
            [x + sign * w / 2. + 0.004, y + depth / 2., z + 0.035],
            galvanized(),
            "",
        )?;
    }
    Ok(z + 0.009)
}
pub fn sauce_tray(d: &mut Fixture<'_, '_>, at: V3, repeat: f64) -> Result<(), String> {
    let z = meat_tray(d, at, [0.34, 0.24], repeat)?;
    for (i, color) in ["#536c27", "#873522"].iter().enumerate() {
        let base = [at[0] + (i as f64 - 0.5) * 0.15, at[1], z];
        lathe(
            d,
            base,
            &[
                [0.028, 0.],
                [0.053, 0.043],
                [0.055, 0.05],
                [0.049, 0.046],
                [0.04, 0.024],
                [0., 0.024],
            ],
            &[
                srgb("#e6e0cf"),
                srgb("#3a5b72"),
                srgb("#e6e0cf"),
                srgb("#e6e0cf"),
                srgb(color),
            ],
        )?;
        metal(d, repeat)?.bar(
            "metal",
            [base[0], base[1] - 0.015, z + 0.037],
            [base[0] + 0.027, base[1] + 0.07, z + 0.069],
            0.008,
            galvanized(),
        )?;
    }
    Ok(())
}

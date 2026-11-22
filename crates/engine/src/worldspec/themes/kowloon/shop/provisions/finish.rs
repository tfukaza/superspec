//! Close-range provisions stock. Shared atlas charts carry packaging and surface
//! detail, while practical fittings use their own geometry and reflected color.
use super::{super::grocer_props, Fixture, Tint, V3, Ware, shade, srgb};
use superspec_geometry::math::{add, scale};
use superspec_types::rng::SeededRng;

pub(super) fn shelf_pack(
    d: &mut Fixture<'_, '_>,
    kind: Ware,
    [x, y, z]: V3,
    [w, depth, h]: V3,
    levels: usize,
    [label, body]: [V3; 2],
    rng: &mut SeededRng,
) -> Result<(), String> {
    let name = match kind {
        Ware::Tins => "food-tin",
        Ware::Jars | Ware::BigJars => "seasoning-jar",
        Ware::Bottles => "drink-bottle",
        Ware::Boxes | Ware::TallBoxes => "stock-box",
        Ware::Bags => "dry-goods-bag",
    };
    let seed =
        super::super::super::wall::finish_seed(&format!("{}/{x:.3}/{y:.3}/{z:.3}", d.pen.part));
    let variants = grocer_props::variants(name)?;
    let label_index = ((label[0] * 29. + label[1] * 37. + label[2] * 43.) * 3.) as usize;
    let mut top = z;
    for level in 0..levels {
        let (inset, value) = match kind {
            Ware::Tins => (0., rng.float(0.85, 1.08)),
            Ware::Boxes | Ware::TallBoxes => (rng.float(0., 0.006), rng.float(0.86, 1.06)),
            _ => (0., 1.),
        };
        // Tiny breaks in brightness and paper color keep repeated packages distinct.
        let variation = 0.94 + ((seed >> (level * 4)) & 15) as f64 * 0.008;
        let tint = body.map(|v| ((0.92 + v * 0.25) * value * variation).clamp(0.82, 1.10));
        let size = [w - 2. * inset, depth - inset, h - 0.0015];
        let variant = label_index % variants;
        if matches!(kind, Ware::Boxes | Ware::TallBoxes) {
            carton(
                d,
                [x - size[0] * 0.5, y + inset * 0.5, top],
                size,
                tint,
                variant,
                false,
            )?;
            top += h;
        } else {
            let options = grocer_props::Options {
                tint,
                supported: true,
                variant: variant + 1,
                ..Default::default()
            };
            let center = [x, y + depth * 0.5, top];
            let size = if kind == Ware::Bottles {
                let [body_width, body_height] = match variant {
                    1 | 3 => [0.96, 0.86],     // squat amber
                    2 | 5 | 7 => [0.78, 0.99], // tall olive
                    6 => [0.83, 0.93],         // narrow smoked glass
                    _ => [0.89, 0.94],
                };
                let tilt = ((seed & 7) as f64 - 3.5) * 0.018;
                grocer_props::fitted_bottle(
                    d,
                    center,
                    [
                        size[0] * body_width,
                        size[1] * body_width,
                        size[2] * body_height,
                    ],
                    tilt,
                    &options,
                )?
            } else {
                grocer_props::draw(d, name, center, size, 0., &options)?
            };
            top += size[2];
        }
    }
    Ok(())
}

/// Cartons share the weathered print atlas. Tape and small shipment marks use basic geometry
/// so top and edge details still read after the print resolves to a few screen pixels.
pub(super) fn carton(
    d: &mut Fixture<'_, '_>,
    [x, y, z]: V3,
    [w, depth, h]: V3,
    tint: V3,
    variant: usize,
    handling_marks: bool,
) -> Result<(), String> {
    let [cx, cy] = [x + w * 0.5, y + depth * 0.5];
    let flipped = variant % 4 == 1 || variant % 4 == 3;
    let warm = [
        [1.02, 0.99, 0.92],
        [0.91, 0.87, 0.80],
        [0.99, 1.03, 0.97],
        [1.05, 0.94, 0.82],
    ][variant % 4];
    grocer_props::draw(
        d,
        "stock-box",
        [cx, cy, z],
        [w, depth, h],
        if flipped { std::f64::consts::PI } else { 0. },
        &grocer_props::Options {
            tint: std::array::from_fn(|i| (tint[i] * warm[i]).min(1.)),
            variant: variant + 1,
            ..Default::default()
        },
    )?;
    let tape = srgb(["#a48955", "#b9a77c", "#9e8558", "#8d6d47"][variant % 4]);
    let half = (w * 0.077).clamp(0.012, 0.024);
    let top = z + h + 0.0006;
    d.face(
        "paint",
        &[
            [cx - half, y, top],
            [cx + half, y, top],
            [cx + half, y + depth, top],
            [cx - half, y + depth, top],
        ],
        [0., 0., 1.],
        tape,
    )?;
    let front = y - 0.0008;
    d.face(
        "paint",
        &[
            [cx - half, front, top - h * 0.13],
            [cx + half, front, top - h * 0.12],
            [cx + half, front, top],
            [cx - half, front, top],
        ],
        [0., -1., 0.],
        shade(tape, 0.89),
    )?;
    if handling_marks && flipped {
        let paper = srgb(if variant % 4 == 1 {
            "#c2b494"
        } else {
            "#a4aa99"
        });
        let seal = srgb(if variant % 4 == 1 {
            "#6c5545"
        } else {
            "#586457"
        });
        let left = x + w * 0.12;
        let right = left + w * 0.39;
        let lo = z + h * 0.29;
        let hi = z + h * 0.58;
        let mark = |d: &mut Fixture<'_, '_>, [a, b, c, e]: [f64; 4], offset: f64, color: V3| {
            let y = front - offset;
            d.face(
                "paint",
                &[[a, y, c], [b, y, c], [b, y, e], [a, y, e]],
                [0., -1., 0.],
                color,
            )
        };
        mark(d, [left, right, lo, hi], 0.00025, paper)?;
        mark(
            d,
            [
                left + w * 0.025,
                right - w * 0.022,
                lo + h * 0.20,
                hi - h * 0.035,
            ],
            0.0004,
            seal,
        )?;
        for i in [0.059, 0.09, 0.13, 0.18, 0.20, 0.25] {
            mark(
                d,
                [
                    left + w * i,
                    left + w * (i + 0.012),
                    lo + h * 0.045,
                    lo + h * 0.15,
                ],
                0.0004,
                shade(seal, 0.74),
            )?;
        }
    }
    Ok(())
}

pub(super) fn crate_stock(
    d: &mut Fixture<'_, '_>,
    [x, y, z]: V3,
    [w, depth, h]: V3,
    color: V3,
    open: bool,
    loaded: bool,
) -> Result<(), String> {
    let center = [x + w / 2., y + depth / 2., z];
    let [r, g, b] = color;
    // The molded crate's atlas already contains pigment and baked surface relief.
    let variant = if r > 1.38 * g {
        0 // worn vermilion
    } else if g > 1.22 * r && g > b {
        1 // olive
    } else if b > r {
        2 // slate blue
    } else {
        3 // ochre
    };
    let stock_tint = color.map(|v| 0.86 + 0.20 * v);
    grocer_props::fitted_crate(
        d,
        center,
        [w, depth, h],
        &grocer_props::Options {
            tint: stock_tint,
            variant,
            ..Default::default()
        },
    )?;
    if open || loaded {
        let base = z + h * 0.15 - 0.010;
        let inside = [x + 0.026, y + 0.026, base];
        d.cuboid(
            "paint",
            inside,
            [x + w - 0.026, y + depth - 0.026, base + 0.007],
            shade(color, 0.77),
            "",
        )?;
        for fraction in [0.26, 0.50, 0.74] {
            let rib = x + w * fraction;
            d.cuboid(
                "paint",
                [rib - 0.002, inside[1], base + 0.007],
                [rib + 0.002, y + depth - 0.026, base + 0.010],
                shade(color, 0.94),
                "d",
            )?;
        }
    }
    if loaded {
        let seed = super::super::super::wall::finish_seed(&format!("{}/{center:?}", d.pen.part));
        let variants = grocer_props::variants("bottle-load")?;
        grocer_props::draw(
            d,
            "bottle-load",
            [center[0], center[1], z + h * 0.15],
            [w * 0.86, depth * 0.84, h * 1.05],
            0.,
            &grocer_props::Options {
                tint: [1.04, 1.0, 0.94],
                supported: true,
                variant: 1 + seed as usize % variants,
                ..Default::default()
            },
        )?;
    }
    Ok(())
}

pub(super) fn rack_post(
    d: &mut Fixture<'_, '_>,
    foot: V3,
    h: f64,
    color: V3,
) -> Result<(), String> {
    let [x, y, z] = foot;
    d.pen.beveled_bar(
        d.mesh,
        "paint",
        add(foot, [0., 0., 0.012]),
        add(foot, [0., 0., h]),
        [1., 0., 0.],
        0.024,
        0.024,
        0.0023,
        &Tint::Color(color),
        d.repeat("paint"),
    )?;
    d.cuboid(
        "paint",
        [x - 0.014, y - 0.014, z],
        [x + 0.014, y + 0.014, z + 0.018],
        srgb("#383e3a"),
        "d",
    )
}

pub(super) fn rack_deck(
    d: &mut Fixture<'_, '_>,
    [w, depth, z]: V3,
    color: V3,
) -> Result<(), String> {
    for y in [0.008, depth - 0.008] {
        d.pen.beveled_bar(
            d.mesh,
            "paint",
            [0., y, z - 0.009],
            [w, y, z - 0.009],
            [0., 1., 0.],
            0.016,
            0.018,
            0.0016,
            &Tint::Color(shade(color, if y < depth / 2. { 1.04 } else { 0.86 })),
            d.repeat("paint"),
        )?;
    }
    for i in 0..17 {
        let x = w * i as f64 / 16.;
        d.bar(
            "paint",
            [x.clamp(0.005, w - 0.005), 0.014, z - 0.004],
            [x.clamp(0.005, w - 0.005), depth - 0.014, z - 0.004],
            0.008,
            shade(color, if i % 3 == 0 { 0.96 } else { 0.84 }),
        )?;
    }
    Ok(())
}

pub(super) fn rack_top(d: &mut Fixture<'_, '_>, [w, depth, z]: V3) -> Result<(), String> {
    carton(
        d,
        [w * 0.08, depth * 0.32, z],
        [w * 0.32, depth * 0.59, 0.13],
        [0.95, 1., 0.95],
        1,
        true,
    )?;
    for (variant, x) in [(6, 0.62), (3, 0.77)] {
        grocer_props::draw(
            d,
            "seasoning-jar",
            [w * x, depth * 0.60, z],
            [0.116, 0.116, 0.128],
            0.,
            &grocer_props::Options {
                variant,
                supported: true,
                ..Default::default()
            },
        )?;
    }
    Ok(())
}

pub(super) fn broom(
    d: &mut Fixture<'_, '_>,
    foot: V3,
    tip: V3,
    bristle: V3,
    stock: V3,
) -> Result<(), String> {
    let [x, y, z] = foot;
    let wood = srgb("#aa8657");
    d.tube("paint", &[add(foot, [0., 0., 0.195]), tip], 0.015, wood, 9)?;
    let grip = add(scale(tip, 0.86), scale(foot, 0.14));
    d.tube("paint", &[grip, tip], 0.0165, shade(wood, 0.86), 9)?;
    d.pen.beveled_bar(
        d.mesh,
        "paint",
        [x - 0.18, y, z + 0.20],
        [x + 0.18, y, z + 0.20],
        [0., 1., 0.],
        0.088,
        0.056,
        0.008,
        &Tint::Color(stock),
        d.repeat("paint"),
    )?;
    for i in 0..15 {
        let at = x + (i as f64 - 7.) * 0.022;
        let shift = (i as f64 - 7.) * 0.0012;
        let uneven = (i * 7 % 6) as f64 * 0.0016;
        let color = shade(bristle, 0.83 + (i * 7 % 11) as f64 * 0.022);
        d.cuboid(
            "paint",
            [at + shift - 0.010, y - 0.039, z + uneven],
            [at + shift + 0.010, y + 0.039, z + 0.174],
            color,
            "u",
        )?;
    }
    Ok(())
}

pub(super) fn hand_truck(d: &mut Fixture<'_, '_>) -> Result<(), String> {
    let coat = srgb("#777d65");
    let steel = srgb("#a5a79b");
    let rubber = srgb("#333938");
    for x in [0., 0.4] {
        d.tube(
            "paint",
            &[
                [x, 0., 0.13],
                [x, 0., 1.32],
                [x, 0.016, 1.41],
                [x, 0.16, 1.46],
            ],
            0.020,
            coat,
            8,
        )?;
        d.tube(
            "paint",
            &[[x, 0.0592, 1.425], [x, 0.16, 1.46]],
            0.023,
            rubber,
            8,
        )?;
        let sign = if x > 0.2 { 1. } else { -1. };
        let inner = x - sign * 0.040;
        let outer = x + sign * 0.014;
        d.tube(
            "paint",
            &[[inner, 0.10, 0.12], [outer, 0.10, 0.12]],
            0.12,
            rubber,
            16,
        )?;
        d.disc(
            "paint",
            [outer + sign * 0.001, 0.10, 0.12],
            0.065,
            [sign, 0., 0.],
            steel,
            12,
        )?;
        d.disc(
            "paint",
            [outer + sign * 0.003, 0.10, 0.12],
            0.026,
            [sign, 0., 0.],
            shade(steel, 0.66),
            8,
        )?;
    }
    for z in [0.32, 0.76, 1.24] {
        d.bar(
            "paint",
            [0.01, 0., z],
            [0.39, 0., z],
            0.025,
            shade(coat, 0.91),
        )?;
    }
    d.cuboid(
        "paint",
        [-0.02, 0., 0.06],
        [0.42, 0.33, 0.095],
        shade(coat, 0.84),
        "",
    )?;
    for x in [0.08, 0.20, 0.32] {
        d.bar(
            "paint",
            [x, 0.04, 0.096],
            [x, 0.28, 0.096],
            0.005,
            shade(steel, 0.83),
        )?;
    }
    Ok(())
}

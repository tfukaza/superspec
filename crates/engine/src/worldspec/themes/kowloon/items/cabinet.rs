//! Folded wall cabinets. Dimensions and projection are measured from the wall at
//! the bottom centre of the enclosure; the facade pen maps outward onto -Y.
use super::{Arguments, draw, fixture};
use crate::{
    itemspec::definition::{NativeOutput, Values},
    worldspec::themes::kowloon::{fasteners, fixture::Fixture, palette::srgb, pen::Tint},
};
use std::f64::consts::FRAC_PI_2;
use superspec_geometry::V3;
use superspec_types::rng::SeededRng;

const FRONT: V3 = [0., 1., 0.];

// Equal vertex counts let the pressed outer flange turn into a recessed meter
// window, including the bevelled corners, without a solid sheet across the dial.
fn outline(x: f64, z: f64, width: f64, height: f64, chamfer: f64, y: f64) -> [V3; 8] {
    let (l, r, b, t) = (
        x - width / 2.,
        x + width / 2.,
        z - height / 2.,
        z + height / 2.,
    );
    [
        [l + chamfer, y, b],
        [r - chamfer, y, b],
        [r, y, b + chamfer],
        [r, y, t - chamfer],
        [r - chamfer, y, t],
        [l + chamfer, y, t],
        [l, y, t - chamfer],
        [l, y, b + chamfer],
    ]
}

fn band(
    d: &mut Fixture<'_, '_>,
    slot: &str,
    a: &[V3; 8],
    b: &[V3; 8],
    color: V3,
) -> Result<(), String> {
    for i in 0..8 {
        let j = (i + 1) % 8;
        let points = [a[i], a[j], b[j], b[i]];
        let u = superspec_geometry::math::sub(points[1], points[0]);
        let v = superspec_geometry::math::sub(points[2], points[0]);
        let n = superspec_geometry::math::unit(superspec_geometry::math::cross(v, u))?;
        d.face(slot, &points, n, color)?;
    }
    Ok(())
}

fn print(
    d: &mut Fixture<'_, '_>,
    slot: &str,
    x: f64,
    z: f64,
    w: f64,
    h: f64,
    y: f64,
    cell: usize,
) -> Result<(), String> {
    let (l, r, b, t) = (x - w / 2., x + w / 2., z - h / 2., z + h / 2.);
    let u = 0.5 * (cell % 2) as f64;
    let (v, height) = match cell {
        0 | 1 => (0.5, 0.5),
        2 | 3 => (0.25, 0.25),
        _ => (0., 0.25),
    };
    let (start, end) = (2. / 512., 0.5 - 2. / 512.);
    d.pen.face(
        d.mesh,
        slot,
        &[[l, y, b], [r, y, b], [r, y, t], [l, y, t]],
        FRONT,
        &Tint::Color([1.; 3]),
        1.,
        Some(&[
            [u + start, v + start],
            [u + end, v + start],
            [u + end, v + height - start],
            [u + start, v + height - start],
        ]),
        None,
    )
}

fn terminal(
    d: &mut Fixture<'_, '_>,
    x: f64,
    width: f64,
    level: f64,
    zinc: V3,
) -> Result<(), String> {
    let y = 0.068;
    let bend = 0.043;
    let mut path = vec![[x, y, 0.025], [x, y, level + bend]];
    // Pressed long-radius elbows, followed by a horizontal run into a wall box.
    for i in 1..=7 {
        let a = FRAC_PI_2 * i as f64 / 7.;
        path.push([x + bend * (1. - a.cos()), y, level + bend * (1. - a.sin())]);
    }
    path.push([width - bend, y, level]);
    for i in 1..=7 {
        let a = FRAC_PI_2 * i as f64 / 7.;
        path.push([
            width - bend + bend * a.sin(),
            y - bend * (1. - a.cos()),
            level,
        ]);
    }
    path.push([width, 0., level]);
    d.tube("conduit", &path, 0.0125, zinc, 12)?;
    d.drum("hinge", [x, y, -0.013], 0.023, 0.042, zinc, 6)?;
    d.drum("hinge", [x, y, -0.037], 0.017, 0.010, srgb("#9b9987"), 12)?;
    let clip = (width + x) * 0.5;
    d.cuboid(
        "hinge",
        [clip - 0.011, 0., level - 0.032],
        [clip + 0.011, y, level + 0.032],
        zinc,
        "b",
    )?;
    let saddle: Vec<_> = (0..=8)
        .map(|i| {
            let a = i as f64 * std::f64::consts::PI / 8.;
            [clip, y + 0.015 * a.sin(), level + 0.019 * a.cos()]
        })
        .collect();
    d.tube("hinge", &saddle, 0.004, zinc, 8)?;
    d.disc("hinge", [width, 0.003, level], 0.026, FRONT, zinc, 12)
}

pub(super) fn cabinet(
    name: &str,
    values: &Values,
    cancelled: &dyn Fn() -> bool,
) -> Result<NativeOutput, String> {
    let mut args = Arguments::new(name, values);
    let identity = args.seed()?;
    args.finish()?;
    let mut rng = SeededRng::from_text(&identity);
    let compact = values.get("seed").is_some_and(|seed| {
        seed.as_f64()
            .map_or_else(|| rng.chance(0.5), |n| n as u64 % 2 == 1)
    });
    let (w, h) = if compact { (0.69, 0.67) } else { (0.58, 0.88) };
    let tint = srgb(if compact { "#d9ddc9" } else { "#c1d3ce" });
    let zinc = srgb("#eeece1");
    let seam = srgb("#66675e");
    let mut result = draw(cancelled, |mesh| {
        let mut d = fixture(mesh, "utility-cabinet")?;
        for sx in [-1., 1.] {
            for z in [0.064, h - 0.064] {
                let x = sx * (w / 2. + 0.018);
                d.cuboid(
                    "cabinet",
                    [x - 0.045, 0., z - 0.032],
                    [x + 0.045, 0.018, z + 0.032],
                    tint,
                    "b",
                )?;
                fasteners::bolt(
                    &mut d,
                    "hinge",
                    [x + sx * 0.015, 0.018, z],
                    FRONT,
                    0.011,
                    zinc,
                    None,
                )?;
            }
        }
        let bottom = outline(0., h / 2., w - 0.025, h - 0.025, 0.012, 0.017);
        let case = outline(0., h / 2., w, h, 0.011, 0.027);
        let shoulder = outline(0., h / 2., w, h, 0.011, 0.153);
        let flange = outline(0., h / 2., w - 0.012, h - 0.012, 0.010, 0.168);
        let gap = outline(0., h / 2., w - 0.034, h - 0.034, 0.014, 0.168);
        let hem = outline(0., h / 2., w - 0.05, h - 0.05, 0.011, 0.184);
        let door = outline(0., h / 2., w - 0.062, h - 0.062, 0.010, 0.193);
        band(&mut d, "cabinet", &bottom, &case, tint)?;
        band(&mut d, "cabinet", &case, &shoulder, tint)?;
        band(&mut d, "cabinet", &shoulder, &flange, tint)?;
        band(&mut d, "cabinet", &flange, &gap, tint)?;
        band(&mut d, "seat", &gap, &hem, seam)?;
        band(&mut d, "door", &hem, &door, tint)?;
        let (x, z) = if compact {
            (-0.061, h * 0.60)
        } else {
            (0.038, h * 0.64)
        };
        let (mw, mh) = if compact {
            (0.25, 0.214)
        } else {
            (0.25, 0.242)
        };
        let outer = outline(x, z, mw + 0.042, mh + 0.042, 0.022, 0.193);
        let bezel = outline(x, z, mw + 0.026, mh + 0.026, 0.018, 0.199);
        let lip = outline(x, z, mw + 0.013, mh + 0.013, 0.016, 0.199);
        let well = outline(x, z, mw, mh, 0.011, 0.171);
        band(&mut d, "door", &door, &outer, tint)?;
        band(&mut d, "door", &outer, &bezel, tint)?;
        band(&mut d, "hinge", &bezel, &lip, zinc)?;
        band(&mut d, "seat", &lip, &well, seam)?;
        d.face("seat", &well, FRONT, srgb("#34382f"))?;
        let (dw, dh) = (mw - 0.022, mh - 0.022);
        print(&mut d, "meter", x, z, dw, dh, 0.173, usize::from(compact))?;
        let pivot = z - dh * 0.26;
        let angle = rng.float(-0.48, 0.43);
        let tip = [
            x + angle.sin() * dh * 0.52,
            0.177,
            pivot + angle.cos() * dh * 0.52,
        ];
        d.face(
            "seat",
            &[[x - 0.0027, 0.177, pivot], [x + 0.0027, 0.177, pivot], tip],
            FRONT,
            srgb("#302f26"),
        )?;
        d.disc("hinge", [x, 0.179, pivot], 0.007, FRONT, zinc, 12)?;
        d.face(
            "glazing",
            &outline(x, z, mw + 0.004, mh + 0.004, 0.014, 0.195),
            FRONT,
            [1.; 3],
        )?;
        for zz in [0.11, h - 0.11] {
            for k in 0..3 {
                d.drum(
                    "hinge",
                    [w / 2. - 0.021, 0.195, zz + (k as f64 - 1.) * 0.018],
                    0.011,
                    0.016,
                    zinc,
                    10,
                )?;
            }
        }
        let latch = if compact {
            w / 2. - 0.09
        } else {
            -w / 2. + 0.074
        };
        let key = outline(latch, h * 0.47, 0.045, 0.098, 0.012, 0.193);
        let keycap = outline(latch, h * 0.47, 0.039, 0.090, 0.010, 0.207);
        band(&mut d, "hinge", &key, &keycap, zinc)?;
        d.face("hinge", &keycap, FRONT, zinc)?;
        d.disc(
            "seat",
            [latch, 0.209, h * 0.47 + 0.006],
            0.008,
            FRONT,
            seam,
            12,
        )?;
        d.face(
            "seat",
            &[
                [latch - 0.003, 0.209, h * 0.47 + 0.005],
                [latch + 0.003, 0.209, h * 0.47 + 0.005],
                [latch + 0.005, 0.209, h * 0.47 - 0.012],
                [latch - 0.005, 0.209, h * 0.47 - 0.012],
            ],
            FRONT,
            seam,
        )?;
        for sx in [-1., 1.] {
            for zz in [0.052, h - 0.052] {
                fasteners::bolt(
                    &mut d,
                    "hinge",
                    [sx * (w / 2. - 0.053), 0.194, zz],
                    FRONT,
                    0.0065,
                    zinc,
                    None,
                )?;
            }
        }
        let tag = if compact { 4 } else { 2 };
        print(
            &mut d,
            "tag",
            w / 2. - 0.112,
            0.136,
            0.126,
            0.090,
            0.195,
            tag,
        )?;
        print(
            &mut d,
            "tag",
            x,
            z - mh / 2. - 0.049,
            0.115,
            0.035,
            0.195,
            tag + 1,
        )?;
        if compact {
            for k in 0..4 {
                let z = 0.08 + k as f64 * 0.021;
                d.cuboid(
                    "door",
                    [-0.26, 0.194, z],
                    [-0.08, 0.205, z + 0.006],
                    tint,
                    "b",
                )?;
            }
        }
        // Paint loss occurs chiefly at the lower folded edge and latch-side seam.
        for k in 0..44 {
            let (x, z) = if k >= 31 {
                let a = rng.float(0., std::f64::consts::TAU);
                (latch + 0.029 * a.cos(), h * 0.47 + 0.051 * a.sin())
            } else if k < 18 {
                (
                    rng.float(-w / 2. + 0.05, w / 2. - 0.05),
                    rng.float(0.035, 0.045),
                )
            } else {
                (-w / 2. + rng.float(0.033, 0.045), rng.float(0.07, h - 0.07))
            };
            let len = rng.float(0.003, 0.014);
            let col = srgb(if k % 4 == 0 { "#95715a" } else { "#d6c5a4" });
            d.face(
                "scuff",
                &[
                    [x, 0.194, z],
                    [x + len, 0.194, z + 0.001],
                    [x + len * 0.28, 0.194, z + 0.0035],
                ],
                FRONT,
                col,
            )?;
        }
        for i in 0..if compact { 2 } else { 3 } {
            let x = -w * 0.28 + i as f64 * 0.095;
            terminal(
                &mut d,
                x,
                0.402 + i as f64 * 0.026,
                -0.30 + i as f64 * 0.054,
                zinc,
            )?;
        }
        Ok(())
    })?;
    for part in &mut result.meshes {
        if part.material_slot == "door" {
            for ((p, n), uv) in part.positions.iter().zip(&part.normals).zip(&mut part.uvs) {
                if n[1].abs() > 0.5 {
                    *uv = [0.5 + p[0] / (w - 0.062), (p[2] - 0.031) / (h - 0.062)];
                }
            }
        }
    }
    Ok(result)
}

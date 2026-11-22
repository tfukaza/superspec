//! Wall-mounted sign light. The front edge is a returned metal hem over a
//! recessed prismatic lens. Datum is the centre of the wall-contact backplate.
use super::{Arguments, draw, on, room_pen};
use crate::itemspec::definition::{NativeOutput, Values};
use crate::worldspec::themes::kowloon::{fixture::Fixture, palette::srgb};

fn strip(
    d: &mut Fixture<'_, '_>,
    slot: &str,
    width: f64,
    a: [f64; 2],
    b: [f64; 2],
    tint: &str,
) -> Result<(), String> {
    let points = [
        [-width, a[0], a[1]],
        [width, a[0], a[1]],
        [width, b[0], b[1]],
        [-width, b[0], b[1]],
    ];
    d.sheet(slot, &points, srgb(tint))
}

pub(super) fn sign_lamp(
    name: &str,
    values: &Values,
    cancelled: &dyn Fn() -> bool,
) -> Result<NativeOutput, String> {
    Arguments::new(name, values).finish()?;
    draw(cancelled, |mesh| {
        let mut d = on(mesh, room_pen("passage-sign-lamp", [0.; 3])?)?;
        d.molding(
            "backplate",
            [-0.189, 0.189],
            &[
                [-0.014, -0.087],
                [-0.011, -0.102],
                [0., -0.102],
                [0., 0.102],
                [-0.011, 0.102],
                [-0.014, 0.094],
            ],
            srgb("#a0a5a8"),
        )?;
        // Side ears sit against masonry and carry a washer with a slotted screw.
        for sign in [-1., 1.] {
            let x = sign * 0.204;
            d.cuboid(
                "backplate",
                [x - 0.020, -0.008, -0.112],
                [x + 0.020, 0., -0.027],
                srgb("#989e9e"),
                "",
            )?;
            d.cuboid(
                "backplate",
                [(sign * 0.18).min(x), -0.008, -0.089],
                [(sign * 0.18).max(x), 0., -0.040],
                srgb("#989e9e"),
                "",
            )?;
            d.tube(
                "trim",
                &[[x, -0.009, -0.086], [x, -0.015, -0.086]],
                0.012,
                srgb("#9da29d"),
                16,
            )?;
            d.disc(
                "trim",
                [x, -0.0152, -0.086],
                0.012,
                [0., -1., 0.],
                srgb("#d8d4c8"),
                16,
            )?;
            d.tube(
                "seal",
                &[[x - 0.005, -0.0154, -0.089], [x + 0.005, -0.0154, -0.083]],
                0.0013,
                srgb("#58534d"),
                5,
            )?;
        }
        let hood = [
            [-0.013, 0.078],
            [-0.019, 0.091],
            [-0.040, 0.094],
            [-0.149, 0.060],
            [-0.181, 0.042],
            [-0.188, 0.035],
            [-0.188, 0.006],
            [-0.182, 0.000],
        ];
        for (i, edge) in hood.windows(2).enumerate() {
            let (slot, tint) = if i >= 5 {
                ("trim", "#d4d2c8")
            } else {
                ("hood", "#c0c5c5")
            };
            strip(&mut d, slot, 0.199, edge[0], edge[1], tint)?;
        }
        // The side returns close the housing. Small rivets lock the folds.
        let cheek = [
            [-0.015, 0.080],
            [-0.040, 0.091],
            [-0.148, 0.058],
            [-0.186, 0.035],
            [-0.185, 0.003],
            [-0.117, -0.066],
            [-0.043, -0.082],
            [-0.015, -0.073],
        ];
        for sign in [-1., 1.] {
            let points: Vec<_> = cheek.iter().map(|p| [sign * 0.197, p[0], p[1]]).collect();
            d.sheet("hood", &points, srgb("#b2b6b5"))?;
            for (y, z) in [(-0.048, 0.052), (-0.15, 0.020)] {
                d.disc(
                    "trim",
                    [sign * 0.1974, y, z],
                    0.004,
                    [sign, 0., 0.],
                    srgb("#bec3be"),
                    8,
                )?;
            }
        }
        d.tube(
            "trim",
            &[[-0.199, -0.184, 0.003], [0.199, -0.184, 0.003]],
            0.004,
            srgb("#dedbd0"),
            12,
        )?;
        // Stepped face and underside direct light down toward the wall sign;
        // vertical flutes interrupt the glow even when seen nearly level.
        let lens = [
            [-0.158, 0.008],
            [-0.158, -0.018],
            [-0.144, -0.044],
            [-0.119, -0.061],
            [-0.065, -0.070],
            [-0.041, -0.066],
        ];
        for edge in lens.windows(2) {
            strip(&mut d, "glass", 0.174, edge[0], edge[1], "#ffefd1")?;
        }
        for i in -14..=14 {
            let x = i as f64 * 0.0117;
            let rib: Vec<_> = lens
                .iter()
                .map(|p| [x, p[0] - 0.0013, p[1] - 0.0012])
                .collect();
            d.tube("flutes", &rib, 0.0018, srgb("#e9ddc8"), 6)?;
        }
        for sign in [-1., 1.] {
            let stations: Vec<_> = lens.iter().map(|p| [sign * 0.179, p[0], p[1]]).collect();
            d.tube("trim", &stations, 0.0055, srgb("#bcc1bb"), 8)?;
            let x = sign * 0.183;
            for edge in lens.windows(2) {
                let points = [
                    [x - 0.011, edge[0][0], edge[0][1]],
                    [x + 0.011, edge[0][0], edge[0][1]],
                    [x + 0.011, edge[1][0], edge[1][1]],
                    [x - 0.011, edge[1][0], edge[1][1]],
                ];
                d.sheet("seal", &points, srgb("#676761"))?;
            }
        }
        d.tube(
            "seal",
            &[[-0.19, -0.039, -0.068], [0.19, -0.039, -0.068]],
            0.005,
            srgb("#78776e"),
            8,
        )?;
        Ok(())
    })
}

//! A flange-mounted extraction grille. All interior depth sits inside the
//! raised sleeve, so the same fitting can mount on an uncut solid receiver.
use super::{Arguments, draw, on, room_pen};
use crate::itemspec::definition::{NativeOutput, Values};
use crate::worldspec::themes::kowloon::{fixture::Fixture, palette::srgb};

type Point = [f64; 3];

/// Octagonal perimeter of one fold station. Only the corners are clipped.
fn ring(half_width: f64, half_height: f64, front: f64, clip: f64) -> [Point; 8] {
    let w = half_width;
    let h = half_height;
    [
        [-w + clip, -front, -h],
        [w - clip, -front, -h],
        [w, -front, -h + clip],
        [w, -front, h - clip],
        [w - clip, -front, h],
        [-w + clip, -front, h],
        [-w, -front, h - clip],
        [-w, -front, -h + clip],
    ]
}

fn band(
    d: &mut Fixture<'_, '_>,
    slot: &str,
    from: &[Point; 8],
    to: &[Point; 8],
    color: &str,
) -> Result<(), String> {
    for i in 0..8 {
        let next = (i + 1) % 8;
        d.sheet(slot, &[from[i], from[next], to[next], to[i]], srgb(color))?;
    }
    Ok(())
}

pub(super) fn grille(
    name: &str,
    values: &Values,
    cancelled: &dyn Fn() -> bool,
) -> Result<NativeOutput, String> {
    Arguments::new(name, values).finish()?;
    let mut result = draw(cancelled, |mesh| {
        let mut d = on(mesh, room_pen("extraction-grille", [0.; 3])?)?;
        // Folded, exposed mounting flange tapers into a raised front rim and
        // returns into the throat. The dark backing lies against the wall.
        let wall = ring(0.346, 0.193, 0., 0.009);
        let gasket = ring(0.348, 0.195, 0.005, 0.009);
        let flange = ring(0.346, 0.193, 0.010, 0.010);
        let shoulder = ring(0.325, 0.171, 0.013, 0.008);
        let crown = ring(0.316, 0.163, 0.062, 0.009);
        let land = ring(0.311, 0.158, 0.066, 0.008);
        let lip = ring(0.300, 0.147, 0.066, 0.006);
        let throat = ring(0.297, 0.144, 0.061, 0.006);
        let back = ring(0.297, 0.144, 0.009, 0.006);
        for (a, b, slot, tint) in [
            (&wall, &gasket, "seal", "#6d6f66"),
            (&gasket, &flange, "frame", "#aab2a7"),
            (&flange, &shoulder, "frame", "#d2d7c8"),
            (&shoulder, &crown, "frame", "#b6c0b7"),
            (&crown, &land, "hem", "#c1c8bd"),
            (&land, &lip, "frame", "#d3d7c9"),
            (&lip, &throat, "hem", "#a1a79c"),
            (&throat, &back, "well", "#686b67"),
        ] {
            band(&mut d, slot, a, b, tint)?;
        }
        d.face("well", &back, [0., -1., 0.], srgb("#464b4b"))?;

        // A folded blade has a turned rear flange, pitched upper face and
        // tight four-millimetre front return. Open slots expose the plenum.
        let tints = [
            "#c6c8bb", "#bfc4b9", "#ccd0c2", "#bebfb4", "#c4c5b5", "#bbbeb1",
        ];
        for (i, tint) in tints.iter().enumerate() {
            let z = 0.113 - i as f64 * 0.0458;
            let profile = [
                [-0.019, z + 0.017],
                [-0.019, z + 0.010],
                [-0.022, z + 0.008],
                [-0.053, z - 0.008],
                [-0.057, z - 0.010],
                [-0.058, z - 0.014],
                [-0.053, z - 0.017],
            ];
            for (j, edge) in profile.windows(2).enumerate() {
                let slot = if j == 4 { "hem" } else { "blades" };
                let pts = [
                    [-0.298, edge[0][0], edge[0][1]],
                    [0.298, edge[0][0], edge[0][1]],
                    [0.298, edge[1][0], edge[1][1]],
                    [-0.298, edge[1][0], edge[1][1]],
                ];
                d.sheet(slot, &pts, srgb(tint))?;
            }
        }
        // Narrow pressed face of the T-stay with a deeper web behind the bank.
        d.cuboid(
            "well",
            [-0.0025, -0.021, -0.146],
            [0.0025, -0.010, 0.146],
            srgb("#737a72"),
            "",
        )?;
        d.cuboid(
            "frame",
            [-0.009, -0.0225, -0.146],
            [0.009, -0.018, 0.146],
            srgb("#a7afa4"),
            "",
        )?;
        for x in [-0.331, 0.331] {
            for z in [-0.178, 0.178] {
                d.disc(
                    "well",
                    [x, -0.0124, z],
                    0.009,
                    [0., -1., 0.],
                    srgb("#383c37"),
                    16,
                )?;
                d.tube(
                    "hem",
                    &[[x, -0.0126, z], [x, -0.0146, z]],
                    0.0069,
                    srgb("#b0afa2"),
                    12,
                )?;
                d.disc(
                    "hem",
                    [x, -0.0148, z],
                    0.0069,
                    [0., -1., 0.],
                    srgb("#b9b9ab"),
                    12,
                )?;
                d.tube(
                    "well",
                    &[
                        [x - 0.0017, -0.015, z + 0.0037],
                        [x + 0.0017, -0.015, z - 0.0037],
                    ],
                    0.00095,
                    srgb("#48483f"),
                    5,
                )?;
                d.tube(
                    "well",
                    &[
                        [x - 0.0037, -0.015, z - 0.0017],
                        [x + 0.0037, -0.015, z + 0.0017],
                    ],
                    0.00095,
                    srgb("#48483f"),
                    5,
                )?;
            }
        }
        Ok(())
    })?;
    // Frame paint is anchored to the flange corners. Blade charts unroll each
    // pitch into its own row; the exported glTF V axis runs rear to front.
    for mesh in &mut result.meshes {
        for (p, uv) in mesh.positions.iter().zip(&mut mesh.uvs) {
            let [x, y, z] = *p;
            match mesh.material_slot.as_str() {
                "frame" => *uv = [0.5 + x / 0.70, 0.5 + z / 0.40],
                "blades" => {
                    let row = ((0.113 - z) / 0.0458).round().clamp(0., 5.);
                    let base = 0.113 - row * 0.0458;
                    let chord =
                        ((-y - 0.022) * 0.031 + (base + 0.008 - z) * 0.016) / 0.031f64.hypot(0.016);
                    *uv = [x + 0.298, 1. - (row * 0.05 + chord)];
                }
                _ => {}
            }
        }
    }
    Ok(result)
}

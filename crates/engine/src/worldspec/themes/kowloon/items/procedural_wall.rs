//! Windowless wall slab and its paper notices, in the ItemSpec panel frame.
use super::{Arguments, draw, fixture, room_pen};
use crate::itemspec::definition::{NativeOutput, Values};
use crate::worldspec::themes::kowloon::{
    noise::{smooth_step, value_noise},
    pen::Tint,
    posters, wall,
};

/// Subdivide only the edge bands that carry grime. The broad middle stays clean.
pub(super) fn slab(
    name: &str,
    values: &Values,
    cancelled: &dyn Fn() -> bool,
) -> Result<NativeOutput, String> {
    let mut args = Arguments::new(name, values);
    let width = args.bounded("width", 4., 0.05..=40.)?;
    let depth = args.bounded("depth", 0.28, 0.05..=40.)?;
    let height = args.bounded("height", 2.2, 0.05..=30.)?;
    let seed = f64::from(wall::finish_seed(&args.seed()?));
    args.finish()?;

    let base = (height * 0.45).min(0.62);
    let head = (height * 0.45).min(0.85);
    let fractions = [0., 0.08, 0.2, 0.38, 0.58, 0.79, 1.];
    let rows: Vec<_> = fractions
        .iter()
        .map(|fraction| fraction * base)
        .chain(
            fractions
                .iter()
                .rev()
                .map(|fraction| height - fraction * head),
        )
        .collect();
    let bands = |x: f64, z: f64, seed: f64| {
        let at_base = z / base;
        let from_head = (height - z) / head;
        let bottom_reach =
            0.35 + 0.43 * value_noise(x * 2.1, 1.7, seed) + 0.14 * value_noise(x * 8.3, 2.5, seed);
        let splashes = 0.62 + 0.38 * value_noise(x * 6., z * 9., seed + 9.);
        let damp = (1. - smooth_step(0.03, bottom_reach, at_base)) * splashes;
        let runoff = smooth_step(0.29, 0.76, value_noise(x * 5.1, z * 0.65, seed + 21.));
        let runs = runoff * (1. - smooth_step(0.04, 0.44 + runoff * 0.53, from_head));
        let seam = 1. - smooth_step(0., 0.18, from_head);
        let amount = (0.52 * damp + 0.3 * runs + 0.19 * seam).clamp(0., 1.);
        [0.22, 0.19, 0.14].map(|deposit| 1. - amount * (1. - deposit))
    };
    draw(cancelled, |mesh| {
        let pen = room_pen("procedural-wall", [0.; 3])?;
        let slot = "concrete";
        let repeat = superspec_catalog::material("builtin:concrete")
            .expect("shipped concrete finish")
            .repeat;
        let perimeter = 2. * (width + depth);
        for (outward, origin, span, horizontal, offset, direction) in [
            ([0., -1., 0.], 0., width, true, 0., 1.),
            ([1., 0., 0.], width, depth, false, width, 1.),
            ([0., 1., 0.], depth, width, true, 2. * width + depth, -1.),
            ([-1., 0., 0.], 0., depth, false, perimeter, -1.),
        ] {
            let columns = (span / 0.13).ceil() as usize;
            let point = |u, z| {
                if horizontal {
                    [u, origin, z]
                } else {
                    [origin, u, z]
                }
            };
            // A continuous coordinate around the perimeter keeps the corner tints joined.
            let tint = Tint::Field(&|[x, y, z]| {
                let u = offset + direction * if horizontal { x } else { y };
                // Blend only near the loop seam, meeting the other side at its original sample.
                let first = bands(u, z, seed);
                let seam_width = (0.35_f64).min(perimeter / 4.);
                if u <= perimeter - seam_width {
                    return Ok(first);
                }
                let seam = smooth_step(perimeter - seam_width, perimeter, u);
                let last = bands(u - perimeter, z, seed);
                Ok(std::array::from_fn(|channel| {
                    first[channel] * (1. - seam) + last[channel] * seam
                }))
            });
            for column in 0..columns {
                let [u0, u1] = [column, column + 1].map(|i| span * (i as f64 / columns as f64));
                for row in rows.windows(2) {
                    let [z0, z1] = [row[0], row[1]];
                    let uv =
                        [[u0, z0], [u1, z0], [u1, z1], [u0, z1]].map(|q| q.map(|v| v * repeat));
                    pen.face(
                        mesh,
                        slot,
                        &[point(u0, z0), point(u1, z0), point(u1, z1), point(u0, z1)],
                        outward,
                        &tint,
                        1.,
                        Some(&uv),
                        None,
                    )?;
                }
            }
        }
        for (z, normal) in [(0., [0., 0., -1.]), (height, [0., 0., 1.])] {
            pen.face(
                mesh,
                slot,
                &[
                    [0., 0., z],
                    [width, 0., z],
                    [width, depth, z],
                    [0., depth, z],
                ],
                normal,
                &Tint::Color([1.; 3]),
                repeat,
                None,
                None,
            )?;
        }
        Ok(())
    })
}

/// A small group of atlas notices. Width and height bound their rotated paper edges.
pub(super) fn notices(
    name: &str,
    values: &Values,
    cancelled: &dyn Fn() -> bool,
) -> Result<NativeOutput, String> {
    let mut args = Arguments::new(name, values);
    let width = args.bounded("width", 1.2, 0.34..=2.)?;
    let height = args.bounded("height", 0.8, 0.42..=2.)?;
    let mut rng = args.rng()?;
    args.finish()?;
    draw(cancelled, |mesh| {
        posters::notices(
            &mut fixture(mesh, "wall-notices")?,
            [-width / 2., width / 2., -height / 2., height / 2.],
            &mut rng,
            true,
        )
    })
}

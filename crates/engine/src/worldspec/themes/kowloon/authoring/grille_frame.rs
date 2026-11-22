//! Metric lacquer and worn zinc on the folded wall extraction grille frame.
use super::Files;
use crate::materialspec::authoring::{
    Maps,
    sampling::{anisotropic_lattice, lattice, mix},
};

fn smooth(a: f64, b: f64, x: f64) -> f64 {
    let t = ((x - a) / (b - a)).clamp(0., 1.);
    t * t * (3. - 2. * t)
}

pub fn bake() -> Result<Files, String> {
    let (w, h) = (896, 504);
    let mut maps = Maps::new(w, h);
    let mut height = Vec::with_capacity(w * h);
    for j in 0..h {
        for i in 0..w {
            let u = (i as f64 + 0.5) / w as f64;
            let v = (j as f64 + 0.5) / h as f64;
            let x = 0.70 * (u - 0.5);
            let z = 0.40 * (v - 0.5);
            let side = x.abs() / 0.348 > z.abs() / 0.195;
            let (along, across) = if side { (v, u) } else { (u, v) };
            let brush = anisotropic_lattice(along, across, [13, 115], 31.);
            let patch = lattice(u, v, 12, 591.);
            let fleck = lattice(u, v, 191, 75.);
            let edge_distance = [
                [0.346, 0.193],
                [0.325, 0.171],
                [0.316, 0.163],
                [0.302, 0.148],
            ]
            .iter()
            .map(|r| (x.abs() - r[0]).max(z.abs() - r[1]).abs())
            .fold(f64::INFINITY, f64::min);
            let edge = 1. - smooth(0.0008, 0.0037, edge_distance);
            let chip = edge * smooth(0.50, 0.64, 0.49 * brush + 0.29 * fleck + 0.22 * patch);
            let screw = (x.abs() - 0.331).hypot(z.abs() - 0.178);
            let oxide = (1. - smooth(0.007, 0.026, screw))
                * smooth(0.56, 0.69, 0.58 * fleck + 0.42 * lattice(u, v, 53, 749.));
            let patina = smooth(0.28, 0.71, patch);
            let tone = 0.07 * (brush - 0.5) + 0.042 * (fleck - 0.5);
            let cool = [0.47, 0.51, 0.52];
            let warm = [0.59, 0.59, 0.54];
            maps.pixel(
                0,
                std::array::from_fn(|k| {
                    let paint = mix(cool[k], warm[k], patina) + tone;
                    mix(
                        mix(paint, [0.77, 0.75, 0.66][k], chip),
                        [0.46, 0.36, 0.25][k],
                        oxide * 0.82,
                    )
                }),
            );
            maps.pixel(
                2,
                [
                    1.,
                    0.50 + 0.18 * patina - 0.20 * chip + 0.17 * oxide,
                    0.40 + 0.32 * chip - 0.22 * oxide,
                ],
            );
            height.push(0.00025 * brush + 0.00007 * fleck - 0.00024 * chip + 0.00013 * oxide);
        }
    }
    maps.normals(&height, w as f64 / 0.70, false);
    maps.encode("extraction-grille-frame")
}

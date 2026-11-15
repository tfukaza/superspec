//! Periodic fields used by the shipped offline recipes.
use crate::materialspec::noise::{lattice_hash, smooth_step};

pub fn mix(a: f64, b: f64, t: f64) -> f64 {
    a * (1. - t) + b * t
}

pub fn lattice(u: f64, v: f64, count: i32, seed: f64) -> f64 {
    anisotropic_lattice(u, v, [count; 2], seed)
}

pub fn anisotropic_lattice(u: f64, v: f64, [nx, ny]: [i32; 2], seed: f64) -> f64 {
    let (x, y) = (u * f64::from(nx), v * f64::from(ny));
    let (ix, iy) = (x.floor() as i32, y.floor() as i32);
    let hash =
        |a: i32, b: i32| lattice_hash(a.rem_euclid(nx).into(), b.rem_euclid(ny).into(), seed);
    let (tx, ty) = (
        smooth_step(0., 1., x.fract()),
        smooth_step(0., 1., y.fract()),
    );
    mix(
        mix(hash(ix, iy), hash(ix + 1, iy), tx),
        mix(hash(ix, iy + 1), hash(ix + 1, iy + 1), tx),
        ty,
    )
}

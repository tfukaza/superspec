use crate::materialspec::noise::{smooth_step, value_noise, value_noise3};
use std::sync::Arc;
use superspec_geometry::{
    Matrix, V3,
    math::{add, cross, scale, unit},
};

#[derive(Clone)]
pub(crate) struct Room {
    pub inverse: Matrix,
    pub size: V3,
    pub seed: f64,
}

impl Room {
    pub fn tint(&self, point: V3, normal: V3) -> V3 {
        let [x, y, z] = self.inverse.point(point);
        let normal = self.inverse.direction(normal);
        let [width, depth, height] = self.size;
        let left = x.max(0.);
        let right = (width - x).max(0.);
        let back = (depth - y).max(0.);
        let base = (-z.max(0.) / 0.24).exp();
        let crown = (-(height - z).max(0.) / 0.24).exp();
        let wall = (-left.min(right).min(back) / 0.26).exp();
        let cloud = value_noise3(x * 2.1, y * 2.1, z * 1.4, self.seed);
        let mut occlusion = wall * 0.22 + base * 0.22;
        let mut dirt = (cloud - 0.35).max(0.) * 0.14;
        if z < 0.015 && normal[2] > 0.6 {
            occlusion = wall * 0.58;
            dirt = 0.;
        } else if height - z < 0.015 && normal[2] < -0.6 {
            occlusion = wall * 0.48;
            dirt = 0.14 + (cloud - 0.3).max(0.) * 0.24;
        } else if normal[0].abs() > 0.6 || normal[1].abs() > 0.6 {
            let rear = normal[1].abs() > 0.6;
            let distance = if rear {
                (depth - y).abs()
            } else {
                x.abs().min((width - x).abs())
            };
            let blend = 1. - smooth_step(0.02, 0.12, distance);
            let corner = if rear { left.min(right) } else { back };
            let across = if rear { x } else { y };
            let streak = value_noise(across * 4.2, z * 0.38, self.seed + 29.);
            let wash = value_noise(across * 1.25, z * 0.72, self.seed + 43.);
            let tide_height = 0.8 + value_noise(across * 1.4, 0., self.seed + 59.) * 0.95;
            let tide = (-((z - tide_height) / 0.19).powi(2)).exp();
            let damp = (1. - z / (tide_height + 0.3)).max(0.);
            let runoff = (streak - 0.51).max(0.) * (-(height - z).max(0.) / 1.5).exp();
            let wall_occlusion = base * 0.42 + crown * 0.30 + (-corner / 0.25).exp() * 0.28;
            let wall_dirt = (wash - 0.28).max(0.) * 0.75
                + tide * (0.16 + wash * 0.40)
                + damp * 0.13
                + runoff * 0.85
                + base * (0.10 + cloud * 0.18)
                + crown * 0.12;
            occlusion += (wall_occlusion - occlusion) * blend;
            dirt += (wall_dirt - dirt) * blend;
        }
        let visibility = (0.98 - occlusion - dirt).max(0.24);
        [
            visibility,
            visibility * (1. - dirt * 0.25),
            visibility * (1. - dirt * 0.56),
        ]
    }
}

#[derive(Clone)]
pub(crate) enum Shelf {
    Furniture {
        inverse: Matrix,
        boxes: Arc<[[V3; 2]]>,
    },
    Frame {
        inverse: Matrix,
        size: V3,
        decks: Arc<[f64]>,
        closed: bool,
        grime: f64,
        seed: f64,
    },
    Stock {
        inverse: Matrix,
        shelf: super::furniture::Shelf,
    },
}

impl Shelf {
    pub fn tint(&self, point: V3, normal: V3) -> V3 {
        match self {
            Self::Furniture { inverse, boxes } => {
                [furniture(inverse.point(point), inverse.direction(normal), boxes); 3]
            }
            Self::Frame {
                inverse,
                size,
                decks,
                closed,
                grime,
                seed,
            } => {
                let [x, y, z] = inverse.point(point);
                let normal = inverse.direction(normal);
                let depth = smooth_step(0.06, 0.96, y / size[1]);
                let rear = 1. - if *closed { 0.68 } else { 0.43 } * depth;
                let underside = 1. - 0.58 * (-normal[2]).max(0.);
                let floor = 1. - 0.25 * (-z.max(0.) / 0.14).exp();
                let end = 1.
                    - 0.18 * (-x.min(size[0] - x).max(0.) / 0.065).exp() * (1. - normal[1].abs());
                let deck = decks
                    .iter()
                    .copied()
                    .find(|top| *top > z + 0.025)
                    .unwrap_or(f64::INFINITY);
                let overhang =
                    1. - 0.24 * (-(deck - z - 0.04).max(0.) / 0.1).exp() * (1. - normal[2].abs());
                let amount = (rear * underside * floor * end * overhang).max(0.14);
                let cloud = value_noise3(x * 9., y * 11., z * 7., *seed);
                let dirt = grime * (0.035 + depth * 0.20 + (cloud - 0.35).max(0.) * 0.24);
                [
                    amount * (1. - dirt),
                    amount * (1. - dirt * 1.18),
                    amount * (1. - dirt * 1.48),
                ]
            }
            Self::Stock { inverse, shelf } => {
                let [_, y, z] = inverse.point(point);
                let rear = smooth_step(0.06, 0.96, (y - shelf.y) / shelf.depth.max(0.01));
                let contact = 1. - 0.55 * (-(z - shelf.z).max(0.) / 0.035).exp();
                let underside = 1. - 0.35 * (-inverse.direction(normal)[2]).max(0.);
                [((1. - 0.55 * rear) * contact * underside).max(0.18); 3]
            }
        }
    }
}

fn furniture(point: V3, normal: V3, boxes: &[[V3; 2]]) -> f64 {
    let Ok(n) = unit(normal) else {
        return 1.;
    };
    let axis = if n[2].abs() < 0.9 {
        [0., 0., 1.]
    } else {
        [0., 1., 0.]
    };
    let Ok(tangent) = unit(cross(n, axis)) else {
        return 1.;
    };
    let bitangent = cross(n, tangent);
    let origin = add(point, scale(n, 0.0008));
    let radius = 0.18;
    let mut blocked = 0.;
    for sample in 0..12 {
        let ring = ((sample as f64 + 0.5) / 12.).sqrt();
        let angle = sample as f64 * 2.399963229728653;
        let direction = add(
            add(
                scale(tangent, ring * angle.cos()),
                scale(bitangent, ring * angle.sin()),
            ),
            scale(n, (1. - ring * ring).sqrt()),
        );
        let mut closest = radius;
        for [low, high] in boxes {
            if (0..3)
                .all(|axis| point[axis] > low[axis] + 0.0001 && point[axis] < high[axis] - 0.0001)
            {
                continue;
            }
            let (mut near, mut far): (f64, f64) = (0., closest);
            for axis in 0..3 {
                if direction[axis].abs() < 1e-9 {
                    if origin[axis] < low[axis] || origin[axis] > high[axis] {
                        far = -1.;
                        break;
                    }
                } else {
                    let a = (low[axis] - origin[axis]) / direction[axis];
                    let b = (high[axis] - origin[axis]) / direction[axis];
                    near = near.max(a.min(b));
                    far = far.min(a.max(b));
                }
            }
            if near <= far {
                closest = near;
            }
        }
        blocked += 1. - closest / radius;
    }
    (1. - 0.68 * blocked / 12.).max(0.5)
}

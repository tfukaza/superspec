//! Canonical convex host outlines, independent of the AML decoder and drawing state.
use crate::worldspec::themes::kowloon::{
    massing::{Lot, ROOF_RESERVE},
    palette::lot_style,
    pavement::field,
};
use serde::Serialize;
use superspec_geometry::{V3, math::hypot};
use superspec_types::rng::SeededRng;

#[derive(Clone, Serialize)]
pub struct Footprint {
    pub(super) points: Vec<V3>,
    pub(super) elevation: f64,
    pub(super) width: f64,
    pub(super) depth: f64,
    pub(super) height: f64,
}
impl Footprint {
    /// One horizontal loop; clockwise input is normalized before selecting the lowest Y/X corner.
    pub fn new(loop_: &[V3], height: f64) -> Result<Self, String> {
        if !(3..=16).contains(&loop_.len())
            || !height.is_finite()
            || !(2.6..=40.).contains(&height)
            || loop_
                .iter()
                .flatten()
                .any(|v| !v.is_finite() || v.abs() > 1e6)
        {
            return Err(
                "Building footprint requires 3–16 finite corners and a bounded positive height."
                    .into(),
            );
        }
        let elevation = loop_[0][2];
        if loop_.iter().any(|p| (p[2] - elevation).abs() > 1e-6) {
            return Err("Building footprint profiles must be horizontal.".into());
        }
        let mut points: Vec<_> = loop_.iter().map(|p| [p[0], p[1], 0.]).collect();
        if field::area(&points) < 0. {
            points.reverse();
        }
        let first = (0..points.len())
            .min_by(|&a, &b| {
                points[a][1]
                    .total_cmp(&points[b][1])
                    .then(points[a][0].total_cmp(&points[b][0]))
            })
            .unwrap();
        points.rotate_left(first);
        for i in 0..points.len() {
            let [a, b, c] = [
                points[i],
                points[(i + 1) % points.len()],
                points[(i + 2) % points.len()],
            ];
            let ab = [b[0] - a[0], b[1] - a[1]];
            let bc = [c[0] - b[0], c[1] - b[1]];
            let length = hypot(ab[0], ab[1]);
            let next = hypot(bc[0], bc[1]);
            let turn = ab[0] * bc[1] - ab[1] * bc[0];
            let cosine = (ab[0] * bc[0] + ab[1] * bc[1]) / (length * next);
            if length > 40. {
                return Err(
                    "Polygon building edges above 40 m exceed the supported dressing envelope."
                        .into(),
                );
            }
            if length < 1. || turn <= 1e-6 || cosine < -std::f64::consts::FRAC_1_SQRT_2 {
                return Err("Building footprints must be strictly convex, with edges at least 1 m and interior angles at least 45 degrees.".into());
            }
        }
        let inner = inset(&points, 0.45);
        if field::area(&inner) < 1. || !inner.iter().all(|p| within(&points, *p, 0.45)) {
            return Err(
                "Building footprint is too narrow for recessed walls and a roof terrace.".into(),
            );
        }
        let span = |axis: usize| {
            points
                .iter()
                .map(|p| p[axis])
                .fold(f64::NEG_INFINITY, f64::max)
                - points.iter().map(|p| p[axis]).fold(f64::INFINITY, f64::min)
        };
        if span(0) > 40. || span(1) > 40. {
            return Err("Polygon buildings above 40 m width or depth exceed the supported dressing envelope.".into());
        }
        Ok(Self {
            width: span(0),
            depth: span(1),
            points,
            elevation,
            height,
        })
    }
    pub fn points(&self) -> &[V3] {
        &self.points
    }
    pub fn elevation(&self) -> f64 {
        self.elevation
    }
    /// The admitted inset remains inside every edge; callers cannot request an inverted terrace.
    pub fn inset(&self, distance: f64) -> Result<Vec<V3>, String> {
        if !distance.is_finite() || !(0. ..=100.).contains(&distance) {
            return Err("Invalid footprint inset.".into());
        }
        let points = inset(&self.points, distance);
        if field::area(&points) <= 0. || !points.iter().all(|p| within(&self.points, *p, distance))
        {
            return Err("Footprint inset leaves no interior.".into());
        }
        Ok(points)
    }
    pub fn lot(&self, stories: usize, seed: &str) -> Result<Lot, String> {
        if !(1..=16).contains(&stories) || seed.len() > 2048 {
            return Err("Polygon building exceeds story or identity bounds.".into());
        }
        let roof = self.height - ROOF_RESERVE;
        if roof / (stories as f64) < 2.4 {
            return Err(
                "Polygon building stories must fit the supported minimum 2.4 m story height."
                    .into(),
            );
        }
        let ground = if stories == 1 {
            roof
        } else {
            3.6_f64.min(roof / stories as f64)
        };
        Ok(Lot {
            index: 0,
            kind: "tenement",
            x0: 0.,
            x1: self.width,
            stories,
            ground,
            story_height: if stories > 1 {
                (roof - ground) / (stories - 1) as f64
            } else {
                0.
            },
            roof,
            top: self.height,
            front: 0.,
            back: 0.,
            style: lot_style(&mut SeededRng::from_text(&format!("{seed}/lot-0/style"))),
            key: "lot-0".into(),
        })
    }
    /// A wholly contained rectangular court for rooftop equipment; polygon corner wedges stay open.
    pub fn equipment_court(&self) -> Option<[f64; 4]> {
        let center = [
            self.points.iter().map(|p| p[0]).sum::<f64>() / self.points.len() as f64,
            self.points.iter().map(|p| p[1]).sum::<f64>() / self.points.len() as f64,
        ];
        let (mut width, mut depth) = (self.width / 2., self.depth / 2.);
        while width >= 1.5 && depth >= 1.5 {
            if [-1., 1.].iter().all(|x| {
                [-1., 1.].iter().all(|y| {
                    within(
                        &self.points,
                        [center[0] + x * width, center[1] + y * depth, 0.],
                        0.35,
                    )
                })
            }) {
                return Some([center[0] - width, center[1] - depth, 2. * width, 2. * depth]);
            }
            width *= 0.9;
            depth *= 0.9;
        }
        None
    }
}
fn inset(points: &[V3], distance: f64) -> Vec<V3> {
    points
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let a = points[(i + points.len() - 1) % points.len()];
            let b = points[(i + 1) % points.len()];
            let (ax, ay, bx, by) = (p[0] - a[0], p[1] - a[1], b[0] - p[0], b[1] - p[1]);
            let (al, bl) = (hypot(ax, ay), hypot(bx, by));
            let (nx, ny, mx, my) = (-ay / al, ax / al, -by / bl, bx / bl);
            let determinant = nx * my - ny * mx;
            [
                p[0] + distance * (my - ny) / determinant,
                p[1] + distance * (nx - mx) / determinant,
                p[2],
            ]
        })
        .collect()
}
fn within(points: &[V3], p: V3, margin: f64) -> bool {
    points.iter().enumerate().all(|(i, a)| {
        let b = points[(i + 1) % points.len()];
        let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
        (dx * (p[1] - a[1]) - dy * (p[0] - a[0])) / hypot(dx, dy) >= margin - 1e-7
    })
}

//! Footprint-filtered and varied image sampling.
//!
//! A [`Pyramid`] holds linear-light mip levels of one decoded color image, built lazily and shared
//! by every evaluator over the same [`super::Inputs`]. Level zero reads the source bytes directly;
//! coarser levels average 2x2 blocks with wrap-around, matching the periodic sampling contract.
use super::Image;
use std::{
    collections::BTreeMap,
    fmt,
    sync::{Arc, Mutex},
};

/// Decoded mip levels in linear RGB, plus the image's mean.
pub(super) struct Pyramid {
    width: usize,
    height: usize,
    /// Levels 1 and up; level 0 is the source image.
    levels: Vec<Level>,
    pub(super) mean: [f64; 3],
}

struct Level {
    width: usize,
    height: usize,
    rgb: Vec<[f32; 3]>,
}

/// Lazily built pyramids keyed by image identity, shared by clones of the same inputs.
#[derive(Clone, Default)]
pub struct MipCache(Arc<Mutex<BTreeMap<String, Arc<Pyramid>>>>);

impl fmt::Debug for MipCache {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("MipCache")
    }
}

impl MipCache {
    pub(super) fn pyramid(&self, id: &str, image: &Image) -> Arc<Pyramid> {
        let mut cache = self
            .0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(pyramid) = cache.get(id) {
            if pyramid.width == image.width && pyramid.height == image.height {
                return pyramid.clone();
            }
        }
        let pyramid = Arc::new(Pyramid::build(image));
        cache.insert(id.to_owned(), pyramid.clone());
        pyramid
    }
}

fn linear(byte: u8) -> f64 {
    superspec_types::color::LINEAR[byte as usize]
}

impl Pyramid {
    fn build(image: &Image) -> Self {
        let (width, height) = (image.width.max(1), image.height.max(1));
        let source = |x: usize, y: usize| -> [f32; 3] {
            let at = (y * width + x) * 4;
            std::array::from_fn(|c| linear(image.rgba[at + c]) as f32)
        };
        let mut sum = [0f64; 3];
        for y in 0..height {
            for x in 0..width {
                let texel = source(x, y);
                for c in 0..3 {
                    sum[c] += f64::from(texel[c]);
                }
            }
        }
        let count = (width * height) as f64;
        let mean = sum.map(|v| v / count);
        let mut levels: Vec<Level> = Vec::new();
        let (mut w, mut h) = (width, height);
        while w > 1 || h > 1 {
            let (nw, nh) = ((w / 2).max(1), (h / 2).max(1));
            let read = |x: usize, y: usize| -> [f32; 3] {
                let (x, y) = (x % w, y % h);
                match levels.last() {
                    None => source(x, y),
                    Some(level) => level.rgb[y * level.width + x],
                }
            };
            let mut rgb = Vec::with_capacity(nw * nh);
            for y in 0..nh {
                for x in 0..nw {
                    let block = [
                        read(2 * x, 2 * y),
                        read(2 * x + 1, 2 * y),
                        read(2 * x, 2 * y + 1),
                        read(2 * x + 1, 2 * y + 1),
                    ];
                    rgb.push(std::array::from_fn(|c| {
                        block.iter().map(|t| t[c]).sum::<f32>() * 0.25
                    }));
                }
            }
            levels.push(Level {
                width: nw,
                height: nh,
                rgb,
            });
            (w, h) = (nw, nh);
        }
        Self {
            width,
            height,
            levels,
            mean,
        }
    }

    /// Periodic bilinear sample of one level; level 0 reads the source image.
    fn bilinear(&self, image: &Image, level: usize, uv: [f64; 2]) -> [f64; 3] {
        let (width, height) = if level == 0 {
            (self.width, self.height)
        } else {
            let l = &self.levels[level - 1];
            (l.width, l.height)
        };
        let texel = |x: usize, y: usize| -> [f64; 3] {
            if level == 0 {
                let at = (y * width + x) * 4;
                std::array::from_fn(|c| linear(image.rgba[at + c]))
            } else {
                let l = &self.levels[level - 1];
                l.rgb[y * width + x].map(f64::from)
            }
        };
        let x = uv[0] * width as f64 - 0.5;
        let y = (1. - uv[1]) * height as f64 - 0.5;
        let (fx, fy) = (x - x.floor(), y - y.floor());
        let wrap = |v: f64, size: usize| v.rem_euclid(size as f64) as usize % size;
        let (left, right) = (wrap(x.floor(), width), wrap(x.floor() + 1., width));
        let (top, bottom) = (wrap(y.floor(), height), wrap(y.floor() + 1., height));
        let corners = [
            (texel(left, top), (1. - fx) * (1. - fy)),
            (texel(right, top), fx * (1. - fy)),
            (texel(left, bottom), (1. - fx) * fy),
            (texel(right, bottom), fx * fy),
        ];
        std::array::from_fn(|c| corners.iter().map(|(t, w)| t[c] * w).sum())
    }

    /// Trilinear sample whose level matches a footprint of `width` UV units.
    pub(super) fn filtered(&self, image: &Image, uv: [f64; 2], width: f64) -> [f64; 3] {
        let texels = width.max(0.) * self.width.max(self.height) as f64;
        let level = if texels > 1. {
            super::numeric::log2(texels)
        } else {
            0.
        };
        let top = self.levels.len() as f64;
        let level = level.min(top);
        let low = level.floor() as usize;
        let t = level - low as f64;
        let a = self.bilinear(image, low, uv);
        if t <= 0. || low as f64 >= top {
            return a;
        }
        let b = self.bilinear(image, low + 1, uv);
        std::array::from_fn(|c| a[c] + (b[c] - a[c]) * t)
    }
}

/// Stable pseudo-random value in [0, 1) for a lattice cell and an index.
pub(super) fn cell_hash(cell: [f64; 2], index: f64, seed: f64) -> f64 {
    super::super::noise::lattice_hash(cell[0] + index * 151., cell[1] + index * 269., seed)
}

/// Blends rotated, offset copies of an image over a triangular lattice in UV space (one lattice
/// cell per UV unit). Each lattice vertex rotates its copy by `(h - 0.5) * rotation` radians and
/// offsets it by a random UV, weights are the cubed barycentric coordinates, normalized. With
/// `correction` one, the blend keeps the image's contrast around its mean, `mean + (blend - mean)
/// / sqrt(sum of squared weights)`; zero leaves the plain blend. The result is clamped to [0, 1].
pub(super) fn varied(
    pyramid: &Pyramid,
    image: &Image,
    uv: [f64; 2],
    width: f64,
    seed: f64,
    rotation: f64,
    correction: f64,
) -> [f64; 3] {
    let root3 = 3f64.sqrt();
    let skewed = [uv[0] - uv[1] / root3, 2. * uv[1] / root3];
    let cell = skewed.map(f64::floor);
    let f = [skewed[0] - cell[0], skewed[1] - cell[1]];
    let (vertices, barycentric) = if f[0] + f[1] < 1. {
        (
            [cell, [cell[0] + 1., cell[1]], [cell[0], cell[1] + 1.]],
            [1. - f[0] - f[1], f[0], f[1]],
        )
    } else {
        (
            [
                [cell[0] + 1., cell[1] + 1.],
                [cell[0] + 1., cell[1]],
                [cell[0], cell[1] + 1.],
            ],
            [f[0] + f[1] - 1., 1. - f[1], 1. - f[0]],
        )
    };
    let cubed = barycentric.map(|w| {
        let w = w.max(0.);
        w * w * w
    });
    let total: f64 = cubed.iter().sum::<f64>().max(1e-12);
    let weights = cubed.map(|w| w / total);
    let mut blend = [0.; 3];
    for (vertex, weight) in vertices.iter().zip(weights) {
        if weight <= 0. {
            continue;
        }
        let angle = (cell_hash(*vertex, 2., seed) - 0.5) * rotation;
        let (s, c) = (super::numeric::sin(angle), super::numeric::cos(angle));
        let offset = [cell_hash(*vertex, 0., seed), cell_hash(*vertex, 1., seed)];
        let turned = [
            uv[0] * c - uv[1] * s + offset[0],
            uv[0] * s + uv[1] * c + offset[1],
        ];
        let sample = pyramid.filtered(image, turned, width);
        for channel in 0..3 {
            blend[channel] += sample[channel] * weight;
        }
    }
    let energy = weights.iter().map(|w| w * w).sum::<f64>().sqrt().max(1e-6);
    let gain = 1. + (1. / energy - 1.) * correction.clamp(0., 1.);
    std::array::from_fn(|c| (pyramid.mean[c] + (blend[c] - pyramid.mean[c]) * gain).clamp(0., 1.))
}

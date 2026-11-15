//! Offline channel accumulation and encoded material outputs.
use superspec_geometry::image::{self, Image};

pub struct Maps {
    pub(super) width: usize,
    pub(super) height: usize,
    pub(super) pixels: [Vec<u8>; 3],
}

impl Maps {
    pub fn new(width: usize, height: usize) -> Self {
        Self {
            width,
            height,
            pixels: std::array::from_fn(|_| Vec::with_capacity(width * height * 4)),
        }
    }

    pub fn pixel(&mut self, channel: usize, rgb: [f64; 3]) {
        self.pixels[channel].extend(rgb.map(|value| (value.clamp(0., 1.) * 255.).round() as u8));
        self.pixels[channel].push(255);
    }

    pub fn normals(&mut self, heights: &[f64], density: f64, wrap: bool) {
        let (width, height) = (self.width, self.height);
        let at = |x: isize, y: isize| {
            let index = |p, extent: usize| {
                if wrap {
                    isize::rem_euclid(p, extent as isize)
                } else {
                    isize::clamp(p, 0, extent as isize - 1)
                }
            };
            heights[index(y, height) as usize * width + index(x, width) as usize]
        };
        for y in 0..self.height as isize {
            for x in 0..self.width as isize {
                let dx = (at(x + 1, y) - at(x - 1, y)) * density / 2.;
                let dy = (at(x, y + 1) - at(x, y - 1)) * density / 2.;
                let length = dx.hypot(dy).hypot(1.);
                self.pixel(
                    1,
                    [
                        0.5 - dx / length / 2.,
                        0.5 + dy / length / 2.,
                        0.5 + 1. / length / 2.,
                    ],
                );
            }
        }
    }

    pub fn encode(self, name: &str) -> Result<Vec<(String, Vec<u8>)>, String> {
        let mut files = Vec::new();
        for (suffix, rgba) in ["", "-normal", "-properties"].into_iter().zip(self.pixels) {
            let bytes = image::encode(&Image {
                width: self.width,
                height: self.height,
                rgba,
                mime: "image/png",
            })?;
            files.push((format!("{name}{suffix}.png"), bytes));
        }
        Ok(files)
    }
}

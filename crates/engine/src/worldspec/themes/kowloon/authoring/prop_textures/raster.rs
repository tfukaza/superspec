use super::{CELL, SeededRng};
use std::f64::consts::TAU;

pub(super) type Rgb = [u8; 3];
pub(super) type Ink = [u8; 4];
pub(super) type Point = [f64; 2];
pub(super) type Bounds = [f64; 4];

pub(super) const fn ink([r, g, b]: Rgb, alpha: u8) -> Ink {
    [r, g, b, alpha]
}

pub(super) fn tinted(color: Rgb, scale: f64) -> Rgb {
    color.map(|channel| (f64::from(channel) * scale) as u8)
}

pub(super) fn ellipse(x: f64, y: f64, rx: f64, ry: f64) -> Bounds {
    [x - rx, y - ry, x + rx, y + ry]
}

fn blend(pixel: &mut [u8; 4], color: Ink) {
    let alpha = u16::from(color[3]);
    for channel in 0..3 {
        pixel[channel] =
            ((u16::from(pixel[channel]) * (255 - alpha) + u16::from(color[channel]) * alpha + 127)
                / 255) as u8;
    }
}

pub(super) struct Canvas {
    pub pixels: Vec<[u8; 4]>,
    pub size: usize,
    scale: f64,
}

impl Canvas {
    pub fn new(scale: usize, color: Rgb) -> Self {
        let size = CELL * scale;
        Self {
            pixels: vec![ink(color, 255); size * size],
            size,
            scale: scale as f64,
        }
    }

    fn draw(&mut self, bounds: Bounds, color: Ink, contains: impl Fn(f64, f64) -> bool) {
        let [x0, y0, x1, y1] = bounds.map(|coord| coord * self.scale);
        let hi = (self.size - 1) as f64;
        let (xa, ya) = (x0.max(0.).floor() as usize, y0.max(0.).floor() as usize);
        let (xb, yb) = (x1.min(hi).floor() as isize, y1.min(hi).floor() as isize);
        if xb < xa as isize || yb < ya as isize {
            return;
        }
        for y in ya..=yb as usize {
            for x in xa..=xb as usize {
                if contains(x as f64 / self.scale, y as f64 / self.scale) {
                    blend(&mut self.pixels[y * self.size + x], color);
                }
            }
        }
    }

    pub fn fill(&mut self, bounds: Bounds, color: Ink) {
        self.draw(bounds, color, |_, _| true);
    }

    pub fn frame(&mut self, bounds: Bounds, color: Ink, width: f64) {
        self.rounded(bounds, 0., None, Some(color), width);
    }

    pub fn rounded(
        &mut self,
        bounds: Bounds,
        radius: f64,
        fill: Option<Ink>,
        outline: Option<Ink>,
        width: f64,
    ) {
        let inside = |[x0, y0, x1, y1]: Bounds, radius: f64, x: f64, y: f64| {
            if x0 > x1 || y0 > y1 || x < x0 || x > x1 || y < y0 || y > y1 {
                return false;
            }
            let r = radius.min((x1 - x0).min(y1 - y0) * 0.5);
            let dx = x - x.clamp(x0 + r, x1 - r);
            let dy = y - y.clamp(y0 + r, y1 - r);
            dx * dx + dy * dy <= r * r
        };
        let [x0, y0, x1, y1] = bounds.map(|coord| (coord * self.scale).floor() / self.scale);
        let bounds = [x0, y0, x1, y1];
        let inner = [x0 + width, y0 + width, x1 - width, y1 - width];
        if let Some(color) = fill {
            self.draw(bounds, color, |x, y| inside(bounds, radius, x, y));
        }
        if let Some(color) = outline {
            self.draw(bounds, color, |x, y| {
                inside(bounds, radius, x, y) && !inside(inner, (radius - width).max(0.), x, y)
            });
        }
    }

    pub fn oval(&mut self, bounds: Bounds, fill: Ink) {
        self.oval_with_outline(bounds, Some(fill), None, 1.);
    }

    pub fn oval_with_outline(
        &mut self,
        bounds: Bounds,
        fill: Option<Ink>,
        outline: Option<Ink>,
        width: f64,
    ) {
        let [x0, y0, x1, y1] = bounds;
        let (cx, cy) = ((x0 + x1) * 0.5, (y0 + y1) * 0.5);
        let (rx, ry) = ((x1 - x0) * 0.5, (y1 - y0) * 0.5);
        let inside = |x: f64, y: f64, inset: f64| {
            rx > inset
                && ry > inset
                && ((x - cx) / (rx - inset)).powi(2) + ((y - cy) / (ry - inset)).powi(2) <= 1.
        };
        if let Some(color) = fill {
            self.draw(bounds, color, |x, y| inside(x, y, 0.));
        }
        if let Some(color) = outline {
            self.draw(bounds, color, |x, y| {
                inside(x, y, 0.) && !inside(x, y, width)
            });
        }
    }

    pub fn line(&mut self, points: &[Point], color: Ink, width: f64) {
        let scale = self.scale;
        let bounds = points.iter().fold(
            [
                f64::INFINITY,
                f64::INFINITY,
                f64::NEG_INFINITY,
                f64::NEG_INFINITY,
            ],
            |[left, top, right, bottom], [x, y]| {
                [left.min(*x), top.min(*y), right.max(*x), bottom.max(*y)]
            },
        );
        let radius = width * 0.5;
        let [x0, y0, x1, y1] = bounds;
        let segments: Vec<_> = points
            .windows(2)
            .map(|pair| {
                let snap = |coordinate: f64| {
                    ((coordinate * scale).floor()
                        - ((width * scale).floor() as u32 % 2 == 0) as u32 as f64 * 0.5)
                        / scale
                };
                let [x, y] = pair[0].map(snap);
                let [end_x, end_y] = pair[1].map(snap);
                let (dx, dy) = (end_x - x, end_y - y);
                let length_squared = dx * dx + dy * dy;
                (x, y, dx, dy, length_squared)
            })
            .collect();
        self.draw(
            [x0 - radius, y0 - radius, x1 + radius, y1 + radius],
            color,
            |x, y| {
                segments.iter().any(|&(ax, ay, dx, dy, length)| {
                    let t = if length > 0. {
                        (((x - ax) * dx + (y - ay) * dy) / length).clamp(0., 1.)
                    } else {
                        0.
                    };
                    (x - ax - dx * t).powi(2) + (y - ay - dy * t).powi(2) < radius.powi(2) + 1e-8
                })
            },
        );
    }

    pub fn arc(&mut self, [x0, y0, x1, y1]: Bounds, start: f64, end: f64, color: Ink, width: f64) {
        let rx = (x1 - x0) * 0.5;
        let ry = (y1 - y0) * 0.5;
        let sweep = (end - start).rem_euclid(360.);
        let steps = (rx.max(ry) * sweep.to_radians() / 2.).ceil() as usize;
        let points: Vec<_> = (0..=steps)
            .map(|step| {
                let angle = (start + sweep * step as f64 / steps.max(1) as f64).to_radians();
                [x0 + rx + rx * angle.cos(), y0 + ry + ry * angle.sin()]
            })
            .collect();
        // Rasterize by segment here, as the short edges occupy disjoint small bounds.
        for pair in points.windows(2) {
            self.line(pair, color, width);
        }
    }

    pub fn polygon(&mut self, points: &[Point], fill: Option<Ink>, outline: Option<Ink>) {
        let bounds = points.iter().fold(
            [
                f64::INFINITY,
                f64::INFINITY,
                f64::NEG_INFINITY,
                f64::NEG_INFINITY,
            ],
            |[left, top, right, bottom], [x, y]| {
                [left.min(*x), top.min(*y), right.max(*x), bottom.max(*y)]
            },
        );
        let mut closed = points.to_vec();
        closed.push(points[0]);
        if let Some(color) = fill {
            self.draw(bounds, color, |x, y| {
                closed.windows(2).fold(false, |inside, pair| {
                    let ([ax, ay], [bx, by]) = (pair[0], pair[1]);
                    inside ^ ((ay > y) != (by > y) && x < (bx - ax) * (y - ay) / (by - ay) + ax)
                })
            });
        }
        if let Some(color) = outline {
            self.line(&closed, color, 1.);
        }
    }

    pub fn glaze(&mut self, color: Rgb, mask: impl IntoIterator<Item = u8>) {
        for (pixel, alpha) in self.pixels.iter_mut().zip(mask) {
            blend(pixel, ink(color, alpha));
        }
    }

    pub fn shade(&mut self, field: &[u8]) {
        for (pixel, value) in self.pixels.iter_mut().zip(field) {
            let multiplier = u16::from(*value) * 2;
            for channel in &mut pixel[..3] {
                *channel = (u16::from(*channel) * multiplier.min(255) / 255) as u8;
            }
        }
    }

    pub fn mask(&self) -> Vec<u8> {
        self.pixels.iter().map(|p| p[0]).collect()
    }

    pub fn blur(&mut self, radius: f64) {
        for channel in 0..3 {
            let values: Vec<_> = self.pixels.iter().map(|p| p[channel]).collect();
            for (pixel, value) in self.pixels.iter_mut().zip(blur(&values, self.size, radius)) {
                pixel[channel] = value;
            }
        }
    }
}

pub(super) fn blur(field: &[u8], size: usize, radius: f64) -> Vec<u8> {
    let reach = (radius * 3.).ceil() as isize;
    let mut kernel: Vec<_> = (-reach..=reach)
        .map(|offset| (-0.5 * (offset as f64 / radius).powi(2)).exp())
        .collect();
    let total: f64 = kernel.iter().sum();
    for weight in &mut kernel {
        *weight /= total;
    }
    let position = |pixel: usize, offset: isize| {
        (pixel as isize + offset).clamp(0, size as isize - 1) as usize
    };
    let mut horizontal = vec![0.; field.len()];
    for (index, value) in horizontal.iter_mut().enumerate() {
        let (x, y) = (index % size, index / size);
        *value = (-reach..=reach)
            .zip(&kernel)
            .map(|(offset, weight)| f64::from(field[y * size + position(x, offset)]) * weight)
            .sum();
    }
    (0..field.len())
        .map(|index| {
            let (x, y) = (index % size, index / size);
            (-reach..=reach)
                .zip(&kernel)
                .map(|(offset, weight)| horizontal[position(y, offset) * size + x] * weight)
                .sum::<f64>()
                .round() as u8
        })
        .collect()
}

pub(super) fn normal(rng: &mut SeededRng, amplitude: f64) -> f64 {
    (-2. * (1. - rng.next_f64()).ln()).sqrt() * (TAU * rng.next_f64()).cos() * amplitude
}

pub(super) fn grain(size: usize, amplitude: f64, rng: &mut SeededRng) -> Vec<u8> {
    (0..size * size)
        .map(|_| (128. + normal(rng, amplitude).trunc()) as u8)
        .collect()
}

pub(super) fn smooth(
    size: usize,
    cells: [usize; 2],
    low: usize,
    high: usize,
    rng: &mut SeededRng,
) -> Vec<u8> {
    let values: Vec<_> = (0..cells[0] * cells[1])
        .map(|_| rng.integer(low, high) as f64)
        .collect();
    interpolate(size, cells, &values)
}

pub(super) fn interpolate(size: usize, cells: [usize; 2], values: &[f64]) -> Vec<u8> {
    let cubic = |[a, b, c, d]: [f64; 4], t: f64| {
        b + t * 0.5 * (c - a + t * (2. * a - 5. * b + 4. * c - d + t * (3. * (b - c) + d - a)))
    };
    let samples = |extent: usize| {
        (0..size)
            .map(|coordinate| {
                let source = (coordinate as f64 + 0.5) * extent as f64 / size as f64 - 0.5;
                let base = source.floor();
                let indices = [-1, 0, 1, 2]
                    .map(|offset| (base as isize + offset).clamp(0, extent as isize - 1) as usize);
                (indices, source - base)
            })
            .collect::<Vec<_>>()
    };
    let xs = samples(cells[0]);
    let ys = samples(cells[1]);
    let horizontal: Vec<Vec<_>> = values
        .chunks_exact(cells[0])
        .map(|row| {
            xs.iter()
                .map(|&(columns, t)| cubic(columns.map(|x| row[x]), t))
                .collect()
        })
        .collect();
    let mut result = Vec::with_capacity(size * size);
    for (rows, t) in ys {
        for x in 0..size {
            result.push(cubic(rows.map(|row| horizontal[row][x]), t).round() as u8);
        }
    }
    result
}

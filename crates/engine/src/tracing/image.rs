use super::Raster;
use std::collections::{BTreeMap, BTreeSet};
use superspec_geometry::image::Image;

pub struct Options {
    pub cells: usize,
    pub bands: usize,
    pub grid: f64,
    pub span: f64,
    pub fill: f64,
    pub background_tolerance: f64,
    pub alpha_cut: f64,
    pub smooth: usize,
    pub minimum_band: f64,
    pub crop: bool,
}
impl Default for Options {
    fn default() -> Self {
        Self {
            cells: 46,
            bands: 4,
            grid: 0.,
            span: 68.,
            fill: 0.45,
            background_tolerance: 26.,
            alpha_cut: 40.,
            smooth: 3,
            minimum_band: 0.03,
            crop: true,
        }
    }
}
struct Work<'a> {
    used: usize,
    cancelled: &'a dyn Fn() -> bool,
}
impl Work<'_> {
    fn spend(&mut self, n: usize) -> Result<(), String> {
        self.used = self.used.saturating_add(n);
        if (self.cancelled)() {
            return Err("Tracing cancelled.".into());
        }
        if self.used > 128_000_000 {
            return Err("Trace work allowance exceeded.".into());
        }
        Ok(())
    }
}
fn neighbors(i: usize, w: usize, h: usize) -> Vec<usize> {
    let (x, y) = (i % w, i / w);
    let mut out = vec![];
    if y + 1 < h {
        out.push(i + w);
    }
    if y > 0 {
        out.push(i - w);
    }
    if x + 1 < w {
        out.push(i + 1);
    }
    if x > 0 {
        out.push(i - 1);
    }
    out
}
fn components(
    labels: &[Option<usize>],
    w: usize,
    h: usize,
    same_band: bool,
    work: &mut Work<'_>,
) -> Result<Vec<Vec<usize>>, String> {
    let mut seen = vec![false; labels.len()];
    let mut result = vec![];
    for i in 0..labels.len() {
        work.spend(1)?;
        if seen[i] || labels[i].is_none() {
            continue;
        }
        let mut stack = vec![i];
        let mut component = vec![];
        seen[i] = true;
        while let Some(at) = stack.pop() {
            work.spend(1)?;
            component.push(at);
            for next in neighbors(at, w, h) {
                if !seen[next]
                    && labels[next].is_some()
                    && (!same_band || labels[next] == labels[i])
                {
                    seen[next] = true;
                    stack.push(next);
                }
            }
        }
        result.push(component);
    }
    Ok(result)
}
fn vote(votes: &mut Vec<(usize, usize)>, label: usize, weight: usize) {
    if let Some((_, count)) = votes.iter_mut().find(|(id, _)| *id == label) {
        *count += weight;
    } else {
        votes.push((label, weight));
    }
}
fn winner(votes: &[(usize, usize)]) -> Option<usize> {
    let mut best = None;
    let mut most = 0;
    for &(label, count) in votes {
        if count > most {
            most = count;
            best = Some(label);
        }
    }
    best
}

/// Segments bounded source pixels into deterministic north-first raster observations.
pub fn trace(
    image: &Image,
    id: &str,
    name: &str,
    source_image: &str,
    options: &Options,
    cancelled: &dyn Fn() -> bool,
) -> Result<Raster, String> {
    if !(1..=1024).contains(&options.cells)
        || !(1..=256).contains(&options.bands)
        || options.smooth > 32
        || [
            options.grid,
            options.span,
            options.fill,
            options.background_tolerance,
            options.alpha_cut,
            options.minimum_band,
        ]
        .iter()
        .any(|n| !n.is_finite())
        || options.grid < 0.
        || options.span <= 0.
        || !(0. ..=1.).contains(&options.fill)
        || !(0. ..=1.).contains(&options.minimum_band)
        || options.background_tolerance < 0.
        || !(0. ..=255.).contains(&options.alpha_cut)
        || image.width == 0
        || image.height == 0
        || image
            .width
            .checked_mul(image.height)
            .and_then(|n| n.checked_mul(4))
            != Some(image.rgba.len())
        || image.rgba.len() > 64 * 1024 * 1024
    {
        return Err("Invalid image tracing options or pixels.".into());
    }
    let mut work = Work { used: 0, cancelled };
    let (width, height) = (image.width, image.height);
    let mut mask: Vec<bool> = image
        .rgba
        .chunks_exact(4)
        .map(|p| p[3] as f64 > options.alpha_cut)
        .collect();
    let opaque = mask.iter().filter(|x| **x).count();
    if opaque as f64 >= mask.len() as f64 * 0.92 {
        let border = ((width.min(height) as f64 * 0.02) as usize).max(2);
        let mut histogram = BTreeMap::<u32, usize>::new();
        for y in 0..height {
            work.spend(width)?;
            for x in 0..width {
                if mask[y * width + x]
                    && (y < border || x < border || y + border >= height || x + border >= width)
                {
                    let p = &image.rgba[(y * width + x) * 4..];
                    let key = (p[0] as u32 / 8) * 4096 + (p[1] as u32 / 8) * 64 + p[2] as u32 / 8;
                    *histogram.entry(key).or_default() += 1;
                }
            }
        }
        let mut top = (0, 0);
        for (&key, &count) in &histogram {
            if count > top.1 {
                top = (key, count);
            }
        }
        let total = histogram.values().sum::<usize>();
        if total > 0 && top.1 as f64 >= total as f64 * 0.45 {
            let background = [
                (top.0 / 4096) * 8 + 4,
                ((top.0 / 64) % 64) * 8 + 4,
                (top.0 % 64) * 8 + 4,
            ];
            for (i, p) in image.rgba.chunks_exact(4).enumerate() {
                if i % width == 0 {
                    work.spend(width)?;
                }
                if (0..3).all(|c| {
                    (p[c] as f64 - background[c] as f64).abs() <= options.background_tolerance
                }) {
                    mask[i] = false;
                }
            }
        }
    }
    let (mut x0, mut y0, mut x1, mut y1) = (0, 0, width, height);
    if options.crop {
        x0 = width;
        y0 = height;
        x1 = 0;
        y1 = 0;
        for (i, &filled) in mask.iter().enumerate() {
            if filled {
                x0 = x0.min(i % width);
                y0 = y0.min(i / width);
                x1 = x1.max(i % width + 1);
                y1 = y1.max(i / width + 1);
            }
        }
    }
    if x0 >= x1 || y0 >= y1 || !mask.iter().any(|p| *p) {
        return Err("No floor pixels were found.".into());
    }
    let scale = (x1 - x0).max(y1 - y0) as f64 / options.cells as f64;
    let w = (((x1 - x0) as f64 / scale).round() as usize).max(1);
    let h = (((y1 - y0) as f64 / scale).round() as usize).max(1);
    let mut colors = vec![[0.; 3]; w * h];
    let mut labels = vec![None; w * h];
    for y in 0..h {
        for x in 0..w {
            let left = x0 + x * (x1 - x0) / w;
            let right = (x0 + (x + 1) * (x1 - x0) / w).max(left + 1).min(x1);
            let top = y0 + y * (y1 - y0) / h;
            let bottom = (y0 + (y + 1) * (y1 - y0) / h).max(top + 1).min(y1);
            let mut count = 0;
            let mut sum = [0.; 3];
            work.spend((right - left) * (bottom - top))?;
            for py in top..bottom {
                for px in left..right {
                    let i = py * width + px;
                    if mask[i] {
                        count += 1;
                        for c in 0..3 {
                            sum[c] += image.rgba[i * 4 + c] as f64;
                        }
                    }
                }
            }
            if count > 0 && count as f64 >= ((right - left) * (bottom - top)) as f64 * options.fill
            {
                colors[y * w + x] = sum.map(|n| n / count as f64);
                labels[y * w + x] = Some(0);
            }
        }
    }
    let blobs = components(&labels, w, h, false, &mut work)?;
    let largest = blobs
        .iter()
        .enumerate()
        .max_by_key(|(i, c)| (c.len(), std::cmp::Reverse(*i)))
        .map(|(i, _)| i)
        .ok_or("No floor cells survived tracing.")?;
    labels.fill(None);
    for &i in &blobs[largest] {
        labels[i] = Some(0);
    }
    let points: Vec<_> = labels
        .iter()
        .enumerate()
        .filter_map(|(i, l)| l.map(|_| i))
        .collect();
    let unique: BTreeSet<_> = points
        .iter()
        .map(|i| colors[*i].map(|v| v.round() as u8))
        .collect();
    let bands = options.bands.min(unique.len());
    let mut luminance = points.clone();
    luminance.sort_by(|a, b| {
        colors[*a]
            .iter()
            .sum::<f64>()
            .total_cmp(&colors[*b].iter().sum::<f64>())
    });
    let mut centers: Vec<_> = (0..bands)
        .map(|i| {
            colors[luminance[((i as f64 + 0.5) * points.len() as f64 / bands as f64) as usize]]
        })
        .collect();
    for _ in 0..24 {
        let mut sums = vec![[0.; 3]; bands];
        let mut counts = vec![0; bands];
        work.spend(points.len() * bands)?;
        for &i in &points {
            let nearest = (0..bands)
                .min_by(|a, b| {
                    let distance = |j: usize| {
                        (0..3)
                            .map(|c| (colors[i][c] - centers[j][c]).powi(2))
                            .sum::<f64>()
                    };
                    distance(*a).total_cmp(&distance(*b))
                })
                .unwrap();
            labels[i] = Some(nearest);
            counts[nearest] += 1;
            for c in 0..3 {
                sums[nearest][c] += colors[i][c];
            }
        }
        let mut moved = 0f64;
        for i in 0..bands {
            if counts[i] > 0 {
                let next = sums[i].map(|v| v / counts[i] as f64);
                for c in 0..3 {
                    moved = moved.max((next[c] - centers[i][c]).abs());
                }
                centers[i] = next;
            }
        }
        if moved < 0.5 {
            break;
        }
    }
    let mut rank: Vec<_> = (0..bands).collect();
    rank.sort_by(|a, b| {
        centers[*a]
            .iter()
            .sum::<f64>()
            .total_cmp(&centers[*b].iter().sum::<f64>())
    });
    let mut remap = vec![0; bands];
    for (i, &old) in rank.iter().enumerate() {
        remap[old] = i;
    }
    for label in labels.iter_mut().flatten() {
        *label = remap[*label];
    }
    for _ in 0..options.smooth {
        let mut next = labels.clone();
        work.spend(points.len() * 9)?;
        for &i in &points {
            let (x, y) = (i % w, i / w);
            let mut votes = vec![];
            for yy in y.saturating_sub(1)..=(y + 1).min(h - 1) {
                for xx in x.saturating_sub(1)..=(x + 1).min(w - 1) {
                    if let Some(label) = labels[yy * w + xx] {
                        vote(&mut votes, label, if xx == x || yy == y { 2 } else { 1 });
                    }
                }
            }
            next[i] = winner(&votes);
        }
        if next == labels {
            break;
        }
        labels = next;
    }
    let minimum = ((points.len() as f64 * options.minimum_band) as usize).max(6);
    for _ in 0..2 {
        let mut seen = vec![false; labels.len()];
        for start in 0..labels.len() {
            work.spend(1)?;
            if seen[start] || labels[start].is_none() {
                continue;
            }
            let band = labels[start];
            let mut stack = vec![start];
            let mut component = vec![];
            seen[start] = true;
            while let Some(i) = stack.pop() {
                work.spend(1)?;
                component.push(i);
                for n in neighbors(i, w, h) {
                    if !seen[n] && labels[n] == band {
                        seen[n] = true;
                        stack.push(n);
                    }
                }
            }
            if component.len() >= minimum {
                continue;
            }
            let mut votes = vec![];
            for &i in &component {
                for n in neighbors(i, w, h) {
                    if let Some(label) = labels[n] {
                        if Some(label) != band {
                            vote(&mut votes, label, 1);
                        }
                    }
                }
            }
            if let Some(label) = winner(&votes) {
                for i in component {
                    labels[i] = Some(label);
                }
            }
        }
    }
    let raster = Raster {
        format: "levelspec-raster/1".into(),
        id: id.into(),
        name: name.into(),
        source_image: source_image.into(),
        grid: if options.grid > 0. {
            options.grid
        } else {
            (options.span / w.max(h) as f64 * 100.).round() / 100.
        },
        colors: rank.into_iter().map(|i| centers[i]).collect(),
        rows: labels.chunks(w).map(|r| r.to_vec()).collect(),
    };
    raster.validate()?;
    Ok(raster)
}

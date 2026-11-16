use super::{Mapping, Receiver};
use crate::materialspec::program::Sample;
use std::sync::Arc;
use superspec_model::instances::{LocalMesh, determinant};

/// Texels of the inner chart rectangle keep this margin on every side, so filtering and mipmaps
/// read texels of the same chart.
pub const GUTTER: usize = 2;
/// The smallest chart edge in texels, gutters included.
const MIN_CHART: usize = 8;

/// One atlas rectangle: a triangle, or the two coplanar triangles of a quad.
///
/// The first triangle's chosen corner sits at the rectangle's upper-left inner texel, its first
/// leg runs along +x and its second along +y (image rows); a rotated chart turns that a quarter turn
/// (first leg along +y, second along -x) so every chart lies wider than tall and shelves pack
/// densely. The second triangle shares the first's hypotenuse and is turned a further half turn,
/// so together they fill the rectangle. Turns keep the tangent frame's handedness.
#[derive(Clone, Debug)]
pub struct Chart {
    pub x: usize,
    pub y: usize,
    pub width: usize,
    pub height: usize,
    pub first: usize,
    pub second: Option<usize>,
    pub rotated: bool,
    /// Distinct samples along the chart's width and height. A chart at least the minimum size
    /// has one per inner texel. A smaller one is padded to the minimum size but sampled at the
    /// atlas's density, so its texels repeat those samples instead of evaluating finer detail
    /// than the rest of the receiver has.
    pub samples: [usize; 2],
}

pub struct Atlas {
    pub mesh: Arc<LocalMesh>,
    pub receiver: Receiver,
    pub width: usize,
    pub height: usize,
    /// Texels per meter along every chart.
    pub density: f64,
    pub charts: Vec<Chart>,
    /// Receiver vertex indices of each triangle, starting at its chart corner (winding kept).
    corners: Vec<[u32; 3]>,
    /// Inner chart texels along each triangle's two legs.
    spans: Vec<[f64; 2]>,
    /// Quarter turns from a triangle's legs to the atlas axes: 0 or 1 for a chart's first triangle
    /// (unrotated or rotated chart), 2 or 3 for its half-turned second.
    turns: Vec<u8>,
    mapping: Mapping,
    domain: [f64; 2],
    gravity: [f64; 3],
    /// Minimum corner of the receiver's world-space bounds.
    origin: [f64; 3],
    /// Maximum corner of the receiver's world-space bounds.
    extent: [f64; 3],
}
pub fn add(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    std::array::from_fn(|i| a[i] + b[i])
}
pub fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    std::array::from_fn(|i| a[i] - b[i])
}
pub fn mul(a: [f64; 3], s: f64) -> [f64; 3] {
    a.map(|v| v * s)
}
pub fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a.into_iter().zip(b).map(|(a, b)| a * b).sum()
}
pub fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
pub fn unit(a: [f64; 3]) -> Result<[f64; 3], String> {
    let l = dot(a, a).sqrt();
    if l < 1e-12 || !l.is_finite() {
        Err("Receiver has a degenerate direction.".into())
    } else {
        Ok(mul(a, 1. / l))
    }
}

fn vertex(values: &[f64], index: u32) -> [f64; 3] {
    let start = index as usize * 3;
    values[start..start + 3].try_into().unwrap()
}
fn inverse_point(matrix: &[f64; 16], point: [f64; 3]) -> [f64; 3] {
    let a = [matrix[0], matrix[1], matrix[2]];
    let b = [matrix[4], matrix[5], matrix[6]];
    let c = [matrix[8], matrix[9], matrix[10]];
    let p = sub(point, [matrix[12], matrix[13], matrix[14]]);
    let determinant = dot(a, cross(b, c));
    [
        dot(p, cross(b, c)) / determinant,
        dot(p, cross(c, a)) / determinant,
        dot(p, cross(a, b)) / determinant,
    ]
}

pub fn swatch(domain: [f64; 2]) -> Receiver {
    Receiver {
        id: "studio/swatch".into(),
        object_id: "studio/swatch".into(),
        positions: vec![
            0., 0., 0., domain[0], 0., 0., domain[0], 0., domain[1], 0., 0., domain[1],
        ],
        normals: vec![0., -1., 0., 0., -1., 0., 0., -1., 0., 0., -1., 0.],
        uvs: vec![0., 0., 1., 0., 1., 1., 0., 1.],
        indices: vec![0, 1, 2, 0, 2, 3],
        object_to_world: None,
        contacts: Vec::new(),
    }
}

impl Atlas {
    pub fn new(
        receiver: Receiver,
        resolution: usize,
        mapping: Mapping,
        domain: [f64; 2],
        gravity: [f64; 3],
        max_resolution: usize,
        texel_density: Option<f64>,
    ) -> Result<Self, String> {
        if texel_density.is_some_and(|density| !(1. ..=100_000.).contains(&density)) {
            return Err("Texel density must be 1 to 100000 texels per meter.".into());
        }
        if !(16..=max_resolution).contains(&resolution)
            || domain
                .iter()
                .any(|v| !v.is_finite() || *v <= 0. || *v > 10000.)
        {
            return Err(format!(
                "Resolution must be 16–{max_resolution} and physical dimensions must be positive and bounded."
            ));
        }
        unit(gravity)?;
        let count = receiver.positions.len() / 3;
        if !receiver.positions.len().is_multiple_of(3)
            || receiver.normals.len() != count * 3
            || receiver.uvs.len() != count * 2
            || !receiver.indices.len().is_multiple_of(3)
            || receiver.indices.is_empty()
            || receiver.indices.len() > 98_304
            || receiver.indices.iter().any(|i| *i as usize >= count)
            || receiver
                .positions
                .iter()
                .chain(&receiver.normals)
                .chain(&receiver.uvs)
                .any(|v| !v.is_finite() || v.abs() > 1e9)
        {
            return Err("Invalid or excessive receiver triangle attributes.".into());
        }
        if let Some(transform) = receiver.object_to_world {
            determinant(&transform)?;
        }
        let triangles = receiver.indices.len() / 3;
        let point = |index: u32| vertex(&receiver.positions, index);
        // Start each triangle at its corner closest to a right angle, so its legs are the chart's
        // axes and texels stay square on rectangular faces. Rotating keeps the winding.
        let mut corners = Vec::with_capacity(triangles);
        for triangle in receiver.indices.chunks_exact(3) {
            let skew = |first: usize| {
                let a = point(triangle[first]);
                let u = sub(point(triangle[(first + 1) % 3]), a);
                let v = sub(point(triangle[(first + 2) % 3]), a);
                (dot(u, v) / (dot(u, u) * dot(v, v)).sqrt()).abs()
            };
            let first = (0..3)
                .min_by(|a, b| skew(*a).total_cmp(&skew(*b)))
                .unwrap_or(0);
            corners.push([0, 1, 2].map(|k| triangle[(first + k) % 3]));
        }
        let legs = |corner: &[u32; 3]| {
            let a = point(corner[0]);
            [
                dot(sub(point(corner[1]), a), sub(point(corner[1]), a)).sqrt(),
                dot(sub(point(corner[2]), a), sub(point(corner[2]), a)).sqrt(),
            ]
        };
        // Pair each triangle with a coplanar neighbor across its hypotenuse whose own legs end at
        // the hypotenuse's ends in reverse: the two halves of a quad. Vertices match by position and
        // UV, so split per-face vertices pair as well as shared ones.
        let key = |index: u32| {
            let at = index as usize;
            (
                point(index).map(f64::to_bits),
                [
                    receiver.uvs[at * 2].to_bits(),
                    receiver.uvs[at * 2 + 1].to_bits(),
                ],
            )
        };
        let face_normal = |corner: &[u32; 3]| {
            let a = point(corner[0]);
            unit(cross(sub(point(corner[1]), a), sub(point(corner[2]), a)))
        };
        let mut by_legs: std::collections::BTreeMap<_, Vec<usize>> = Default::default();
        for (triangle, corner) in corners.iter().enumerate() {
            by_legs
                .entry((key(corner[1]), key(corner[2])))
                .or_default()
                .push(triangle);
        }
        let mut partner = vec![None; triangles];
        let mut taken = vec![false; triangles];
        for triangle in 0..triangles {
            if taken[triangle] {
                continue;
            }
            let corner = corners[triangle];
            let normal = face_normal(&corner)?;
            let found = by_legs
                .get(&(key(corner[2]), key(corner[1])))
                .into_iter()
                .flatten()
                .copied()
                .find(|other| {
                    *other != triangle
                        && !taken[*other]
                        && face_normal(&corners[*other]).is_ok_and(|n| dot(n, normal) > 1. - 1e-9)
                });
            if let Some(other) = found {
                taken[triangle] = true;
                taken[other] = true;
                partner[triangle] = Some(other);
            }
        }
        let mut charts: Vec<Chart> = Vec::new();
        let mut sizes: Vec<[f64; 2]> = Vec::new();
        for triangle in 0..triangles {
            if taken[triangle] && partner[triangle].is_none() {
                continue;
            }
            let [mut along, mut across] = legs(&corners[triangle]);
            if let Some(other) = partner[triangle] {
                let [a, b] = legs(&corners[other]);
                along = along.max(a);
                across = across.max(b);
            }
            // Charts lie wider than tall: rotate when the first leg is the shorter one.
            let rotated = along < across;
            sizes.push(if rotated {
                [across, along]
            } else {
                [along, across]
            });
            charts.push(Chart {
                x: 0,
                y: 0,
                width: 0,
                height: 0,
                first: triangle,
                second: partner[triangle],
                rotated,
                samples: [1, 1],
            });
        }
        // One texel density for the whole receiver, the largest whose shelf packing fits the
        // requested square; tiny charts keep a minimum size, so a dense mesh may exceed it.
        let order = {
            let mut order: Vec<usize> = (0..charts.len()).collect();
            order.sort_by(|a, b| {
                sizes[*b][1]
                    .total_cmp(&sizes[*a][1])
                    .then(sizes[*b][0].total_cmp(&sizes[*a][0]))
                    .then(a.cmp(b))
            });
            order
        };
        let pack = |width: usize, density: f64, charts: &mut [Chart]| -> usize {
            let extent = |length: f64| {
                ((length * density).ceil() as usize + 2 * GUTTER).clamp(MIN_CHART, width)
            };
            let (mut x, mut y, mut row) = (0, 0, 0);
            for &index in &order {
                let w = extent(sizes[index][0]);
                let h = extent(sizes[index][1]);
                if x + w > width {
                    x = 0;
                    y += row;
                    row = 0;
                }
                let chart = &mut charts[index];
                (chart.x, chart.y, chart.width, chart.height) = (x, y, w, h);
                x += w;
                row = row.max(h);
            }
            y + row
        };
        let longest = sizes.iter().flatten().copied().fold(0f64, f64::max);
        // With a texel density, a receiver that reaches it in a smaller square takes the smallest
        // such square (the resolution halved some number of times) at exactly that density, so
        // small parts get small textures and every part of a scene has the same detail. A
        // receiver too large for it keeps the requested resolution at the density that fits.
        let sized = texel_density.and_then(|density| {
            let mut candidates: Vec<usize> = std::iter::successors(Some(resolution), |width| {
                Some(width / 2).filter(|half| *half >= 16)
            })
            .collect();
            candidates.reverse();
            candidates
                .into_iter()
                .find(|&width| {
                    longest * density + (2 * GUTTER) as f64 <= width as f64
                        && pack(width, density, &mut charts) <= width
                })
                .map(|width| (width, density))
        });
        let (width, density) = sized.unwrap_or_else(|| {
            let (mut low, mut high) = (0f64, resolution as f64 / longest.max(1e-9));
            for _ in 0..48 {
                let middle = (low + high) * 0.5;
                if pack(resolution, middle, &mut charts) <= resolution {
                    low = middle;
                } else {
                    high = middle;
                }
            }
            (resolution, low)
        });
        let height = pack(width, density, &mut charts);
        for (chart, size) in charts.iter_mut().zip(&sizes) {
            let inner = [chart.width - 2 * GUTTER, chart.height - 2 * GUTTER];
            chart.samples = std::array::from_fn(|axis| {
                ((size[axis] * density).ceil() as usize).clamp(1, inner[axis])
            });
        }
        let mut spans = vec![[1., 1.]; triangles];
        let mut turns = vec![0u8; triangles];
        for chart in &charts {
            // Samples per leg: footprints and height steps follow the samples, not the padding.
            let mut inner = chart.samples.map(|count| count as f64);
            if chart.rotated {
                inner.swap(0, 1);
            }
            let turn = u8::from(chart.rotated);
            spans[chart.first] = inner;
            turns[chart.first] = turn;
            if let Some(second) = chart.second {
                spans[second] = inner;
                turns[second] = turn + 2;
            }
        }
        let mut mesh = LocalMesh {
            positions: vec![],
            normals: vec![],
            uvs: vec![],
            colors: vec![],
            indices: vec![],
            tangents: vec![],
        };
        let mut origin = [f64::INFINITY; 3];
        let mut extent = [f64::NEG_INFINITY; 3];
        for point in receiver.positions.chunks_exact(3) {
            for axis in 0..3 {
                origin[axis] = origin[axis].min(point[axis]);
                extent[axis] = extent[axis].max(point[axis]);
            }
        }
        let mut placement = vec![(0usize, 0usize, [0f64; 2]); triangles];
        for chart in &charts {
            let inner = [
                (chart.width - 2 * GUTTER) as f64,
                (chart.height - 2 * GUTTER) as f64,
            ];
            placement[chart.first] = (chart.x, chart.y, inner);
            if let Some(second) = chart.second {
                placement[second] = (chart.x, chart.y, inner);
            }
        }
        for (triangle, corner) in corners.iter().enumerate() {
            let a = point(corner[0]);
            unit(cross(sub(point(corner[1]), a), sub(point(corner[2]), a)))?;
            let (x0, y0, inner) = placement[triangle];
            // Corner, first-leg end and second-leg end in the unit inner rectangle for each turn.
            let local: [[f64; 2]; 3] = match turns[triangle] {
                0 => [[0., 0.], [1., 0.], [0., 1.]],
                1 => [[1., 0.], [1., 1.], [0., 0.]],
                2 => [[1., 1.], [0., 1.], [1., 0.]],
                _ => [[0., 1.], [0., 0.], [1., 1.]],
            };
            for (k, index) in corner.iter().enumerate() {
                mesh.positions.push(point(*index));
                mesh.normals.push(unit(vertex(&receiver.normals, *index))?);
                mesh.colors.push([1.; 3]);
                let x = (x0 + GUTTER) as f64 + local[k][0] * inner[0];
                let y = (y0 + GUTTER) as f64 + local[k][1] * inner[1];
                mesh.uvs.push([x / width as f64, 1. - y / height as f64]);
                mesh.indices.push(mesh.indices.len() as u32);
            }
        }
        Ok(Self {
            mesh: Arc::new(mesh),
            receiver,
            width,
            height,
            density,
            charts,
            corners,
            spans,
            turns,
            mapping,
            domain,
            gravity,
            origin,
            extent,
        })
    }

    /// The triangle and leg parameters a chart texel samples. Gutter texels clamp to the nearest
    /// edge; a lone triangle's texels beyond its hypotenuse project onto it.
    pub fn texel(&self, chart: &Chart, x: usize, y: usize) -> (usize, f64, f64) {
        let inner = [
            (chart.width - 2 * GUTTER) as f64,
            (chart.height - 2 * GUTTER) as f64,
        ];
        // A padded chart's texels snap to the centers of its samples.
        let place = |texel: usize, inner: f64, samples: usize| {
            let position = ((texel as f64 + 0.5 - GUTTER as f64) / inner).clamp(0., 1.);
            let count = samples as f64;
            if count >= inner {
                position
            } else {
                ((position * count).floor().min(count - 1.) + 0.5) / count
            }
        };
        let s = place(x, inner[0], chart.samples[0]);
        let t = place(y, inner[1], chart.samples[1]);
        // The first triangle's leg parameters; its second lies past the hypotenuse, half turned.
        let (u, v) = if chart.rotated { (t, 1. - s) } else { (s, t) };
        match chart.second {
            Some(second) if u + v > 1. => (second, 1. - u, 1. - v),
            _ if u + v > 1. => (chart.first, u / (u + v), v / (u + v)),
            _ => (chart.first, u, v),
        }
    }

    /// Leg parameter steps of one texel, for height derivatives.
    pub fn steps(&self, triangle: usize) -> [f64; 2] {
        self.spans[triangle].map(|span| 1. / span)
    }

    pub fn sample(&self, triangle: usize, u: f64, v: f64) -> Result<Sample, String> {
        let indices = &self.corners[triangle];
        let a = vertex(&self.receiver.positions, indices[0]);
        let b = vertex(&self.receiver.positions, indices[1]);
        let c = vertex(&self.receiver.positions, indices[2]);
        let world = add(a, add(mul(sub(b, a), u), mul(sub(c, a), v)));
        let object = self
            .receiver
            .object_to_world
            .as_ref()
            .map(|m| inverse_point(m, world))
            .unwrap_or_else(|| sub(world, self.origin));
        let normal = unit(add(
            mul(vertex(&self.receiver.normals, indices[0]), 1. - u - v),
            add(
                mul(vertex(&self.receiver.normals, indices[1]), u),
                mul(vertex(&self.receiver.normals, indices[2]), v),
            ),
        ))?;
        let uv = std::array::from_fn(|axis| {
            let uv = &self.receiver.uvs;
            uv[indices[0] as usize * 2 + axis] * (1. - u - v)
                + uv[indices[1] as usize * 2 + axis] * u
                + uv[indices[2] as usize * 2 + axis] * v
        });
        let coordinates = match self.mapping {
            Mapping::World => world,
            _ => object,
        };
        let projection_normal = match (self.mapping, self.receiver.object_to_world.as_ref()) {
            (Mapping::Object, Some(m)) => unit([
                dot([m[0], m[1], m[2]], normal),
                dot([m[4], m[5], m[6]], normal),
                dot([m[8], m[9], m[10]], normal),
            ])?,
            _ => normal,
        };
        let surface = match self.mapping {
            Mapping::Uv => [uv[0] * self.domain[0], uv[1] * self.domain[1]],
            _ => {
                if projection_normal[2].abs()
                    >= projection_normal[0].abs().max(projection_normal[1].abs())
                {
                    [coordinates[0], coordinates[1]]
                } else if projection_normal[1].abs() >= projection_normal[0].abs() {
                    [coordinates[0], coordinates[2]]
                } else {
                    [coordinates[1], coordinates[2]]
                }
            }
        };
        let [along, across] = self.spans[triangle];
        let footprint = (dot(sub(b, a), sub(b, a)).sqrt() / along)
            .max(dot(sub(c, a), sub(c, a)).sqrt() / across);
        let uv_edge = |index: u32| {
            let data = &self.receiver.uvs;
            let start = indices[0] as usize * 2;
            let end = index as usize * 2;
            ((data[end] - data[start]).powi(2) + (data[end + 1] - data[start + 1]).powi(2)).sqrt()
        };
        let uv_footprint = Some((uv_edge(indices[1]) / along).max(uv_edge(indices[2]) / across));
        Ok(Sample {
            uv,
            surface,
            object,
            world,
            normal,
            footprint,
            uv_footprint,
            gravity: self.gravity,
            bounds_min: self.origin,
            bounds_max: self.extent,
        })
    }

    /// Height derivatives are measured along the chart edges, then resolved in its tangent frame.
    pub fn normal(&self, triangle: usize, du: f64, dv: f64) -> Result<[f64; 3], String> {
        let p = &self.mesh.positions[triangle * 3..triangle * 3 + 3];
        let u = sub(p[1], p[0]);
        let v = sub(p[2], p[0]);
        let t = unit(u)?;
        let n = unit(cross(u, v))?;
        let b = cross(n, t);
        let gt = du / dot(u, t);
        let gb = (dv - gt * dot(v, t)) / dot(v, b);
        // Tangent-space x is atlas +x and y is atlas -y (image rows run down). Each quarter turn of
        // the legs against those axes turns the slopes with them.
        match self.turns[triangle] {
            0 => unit([-gt, gb, 1.]),
            1 => unit([gb, gt, 1.]),
            2 => unit([gt, -gb, 1.]),
            _ => unit([-gb, -gt, 1.]),
        }
    }

    /// World-space x and y directions of the tangent space [`Self::normal`] encodes: the first
    /// edge and the reversed bitangent (atlas V runs down image rows).
    pub fn frame(&self, triangle: usize) -> Result<[[f64; 3]; 2], String> {
        let p = &self.mesh.positions[triangle * 3..triangle * 3 + 3];
        let u = sub(p[1], p[0]);
        let v = sub(p[2], p[0]);
        let t = unit(u)?;
        let n = unit(cross(u, v))?;
        let b = cross(n, t);
        let negate = |v: [f64; 3]| v.map(|c| -c);
        Ok(match self.turns[triangle] {
            0 => [t, negate(b)],
            1 => [negate(b), negate(t)],
            2 => [negate(t), b],
            _ => [b, t],
        })
    }
}

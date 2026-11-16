use super::{Mapping, Receiver};
use crate::materialspec::program::{self, Sample};
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
    /// For a chart on a pattern anchor's texel lattice: along its width and its height, how
    /// far into a lattice cell its rectangle starts and how many cells its edge is long (both
    /// in texels, neither whole in general). Texel centers are then lattice points, inside the
    /// receiver or just beyond its edge.
    pub lattice: Option<[[f64; 2]; 2]>,
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
    /// See [`Self::features`].
    features: std::sync::OnceLock<(program::FeatureSet, bool)>,
    /// The features the receiver's owner named, each name with its own grid.
    pub named: program::NamedFeatures,
    /// The pattern anchor's origin and axes (u, v, normal), under [`Mapping::Anchored`].
    frame: Option<([f64; 3], [[f64; 3]; 3])>,
    /// Whether each triangle's chart is on the anchor's texel lattice, and the lattice step.
    snapped: Vec<bool>,
    step: f64,
    /// See [`Self::legs`].
    framed: Vec<Option<[[f64; 3]; 3]>>,
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
    /// The atlas a bake of `request` gives `receiver`: the request's resolution, mapping, domain,
    /// gravity, profile allowance and texel density, so every consumer sizes it by one rule.
    pub fn for_request(receiver: Receiver, request: &super::Request) -> Result<Self, String> {
        let named = request.features.get(&receiver.id).cloned();
        let anchor = request.anchors.get(&receiver.id).copied();
        Self::build(
            receiver,
            request.resolution,
            request.mapping,
            request.domain,
            request.gravity,
            request.profile.max_resolution(),
            request.texel_density,
            anchor.filter(|_| request.mapping == Mapping::Anchored),
            true,
        )?
        .with_named(named.as_deref().unwrap_or(&[]))
    }

    /// Receiver vertex indices of a triangle, starting at its chart corner.
    pub fn corners(&self, triangle: usize) -> [u32; 3] {
        self.corners[triangle]
    }

    /// Quarter turns from a triangle's legs to the atlas axes (see [`Chart`]).
    pub fn turn(&self, triangle: usize) -> u8 {
        self.turns[triangle]
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn new(
        receiver: Receiver,
        resolution: usize,
        mapping: Mapping,
        domain: [f64; 2],
        gravity: [f64; 3],
        max_resolution: usize,
        texel_density: Option<f64>,
    ) -> Result<Self, String> {
        Self::build(
            receiver,
            resolution,
            mapping,
            domain,
            gravity,
            max_resolution,
            texel_density,
            None,
            true,
        )
    }

    /// [`Self::new`] for a receiver with a pattern anchor (see [`Mapping::Anchored`]); `snap`
    /// puts the charts that can be on the anchor's texel lattice.
    #[allow(clippy::too_many_arguments)]
    fn build(
        receiver: Receiver,
        resolution: usize,
        mapping: Mapping,
        domain: [f64; 2],
        gravity: [f64; 3],
        max_resolution: usize,
        texel_density: Option<f64>,
        anchor: Option<super::Anchor>,
        snap: bool,
    ) -> Result<Self, String> {
        // The anchor's frame: its origin and unit axes u, v and their normal. Without an anchor
        // the frame is the world's, so anchored coordinates are world coordinates.
        let frame = match (mapping, anchor) {
            (Mapping::Anchored, Some(anchor)) => {
                let u = unit(anchor.u)?;
                let v = unit(sub(anchor.v, mul(u, dot(anchor.v, u))))?;
                if anchor
                    .origin
                    .iter()
                    .any(|c| !c.is_finite() || c.abs() > 1e9)
                {
                    return Err("A pattern anchor needs a finite origin.".into());
                }
                Some((anchor.origin, [u, v, cross(u, v)]))
            }
            (Mapping::Anchored, None) => {
                Some(([0.; 3], [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]]))
            }
            _ => None,
        };
        // Charts go on the lattice only at exactly the requested density.
        let lattice = frame.filter(|_| snap).zip(texel_density);
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
        // For a triangle whose chart is not laid along its own legs: the corner and the two
        // edges of the chart's rectangle, which stand in for the triangle's.
        let mut framed: Vec<Option<[[f64; 3]; 3]>> = vec![None; triangles];
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
            // Legs equal up to rounding noise do not rotate, so a rigidly moved receiver keeps the
            // chart orientation of the original.
            let mut rotated = along < across * (1. - 1e-9);
            sizes.push(if rotated {
                [across, along]
            } else {
                [along, across]
            });
            // A chart whose legs run along the anchor's axes can sample the anchor's lattice:
            // one texel a lattice cell, wherever on the lattice its corner falls. It then
            // spans the cells its edges reach into, which may be one more than its length.
            let mut on_lattice = None;
            if let Some(((origin, axes), density)) = lattice {
                let corner = corners[triangle];
                let a = point(corner[0]);
                let (first, second) = (sub(point(corner[1]), a), sub(point(corner[2]), a));
                let along_axis = |leg: [f64; 3]| {
                    let length = dot(leg, leg).sqrt();
                    axes.iter()
                        .any(|axis| (dot(leg, *axis).abs() - length).abs() <= 1e-9 * length)
                };
                if along_axis(first) && along_axis(second) && dot(first, second).abs() < 1e-9 {
                    // Where the chart's rectangle starts, and the directions of its +x and +y.
                    let (start, x, y) = if rotated {
                        (add(a, second), mul(second, -1.), first)
                    } else {
                        (a, first, second)
                    };
                    let size = *sizes.last().expect("the chart's size");
                    let fit = |direction: [f64; 3], length: f64| {
                        let direction = mul(direction, 1. / dot(direction, direction).sqrt());
                        let cells = dot(sub(start, origin), direction) * density;
                        // A corner within a millionth of a cell of a boundary is on it.
                        let into = (cells - (cells + 1e-6).floor()).max(0.);
                        [into, length * density]
                    };
                    let fits = [fit(x, size[0]), fit(y, size[1])];
                    *sizes.last_mut().expect("the chart's size") =
                        fits.map(|[into, texels]| (into + texels - 1e-6).max(1e-6) / density);
                    on_lattice = Some(fits);
                } else if let (None, Ok(normal)) = (partner[triangle], face_normal(&corner)) {
                    // A lone triangle in a plane of the anchor's frame, whatever way its edges
                    // run: its chart is its bounding rectangle along the two axes of that
                    // plane, so its texels are lattice cells too. The chart's frame is then
                    // that rectangle's corner and edges rather than the triangle's own.
                    let across = axes
                        .iter()
                        .position(|axis| dot(normal, *axis).abs() > 1. - 1e-9);
                    if let Some(across) = across {
                        let extent = |axis: [f64; 3]| {
                            let along = corner.map(|index| dot(sub(point(index), origin), axis));
                            let low = along.into_iter().fold(f64::INFINITY, f64::min);
                            (
                                low,
                                along.into_iter().fold(f64::NEG_INFINITY, f64::max) - low,
                            )
                        };
                        // The longer extent runs along the chart's width; +y is a quarter turn
                        // from +x about the face's normal, which keeps the frame's handedness.
                        let [p, q] = [axes[(across + 1) % 3], axes[(across + 2) % 3]];
                        let x = if extent(p).1 >= extent(q).1 { p } else { q };
                        let y = cross(normal, x);
                        let ((low_x, width), (low_y, height)) = (extent(x), extent(y));
                        if width > 1e-9 && height > 1e-9 {
                            let start = add(
                                add(origin, mul(normal, dot(sub(a, origin), normal))),
                                add(mul(x, low_x), mul(y, low_y)),
                            );
                            let fit = |low: f64, length: f64| {
                                let cells = low * density;
                                [(cells - (cells + 1e-6).floor()).max(0.), length * density]
                            };
                            let fits = [fit(low_x, width), fit(low_y, height)];
                            *sizes.last_mut().expect("the chart's size") = fits
                                .map(|[into, texels]| (into + texels - 1e-6).max(1e-6) / density);
                            rotated = false;
                            on_lattice = Some(fits);
                            framed[triangle] = Some([start, mul(x, width), mul(y, height)]);
                        }
                    }
                }
            }
            charts.push(Chart {
                x: 0,
                y: 0,
                width: 0,
                height: 0,
                first: triangle,
                second: partner[triangle],
                rotated,
                samples: [1, 1],
                lattice: on_lattice,
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
        if lattice.is_some() && sized.is_none() {
            // The receiver cannot be laid out at the requested density, so its texels are not
            // lattice cells: it is laid out as any other, in anchored coordinates.
            return Self::build(
                receiver.clone(),
                resolution,
                mapping,
                domain,
                gravity,
                max_resolution,
                texel_density,
                anchor,
                false,
            );
        }
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
            // Every texel of a chart on the lattice is its own lattice cell.
            if chart.lattice.is_some() {
                chart.samples = inner;
            }
        }
        let mut spans = vec![[1., 1.]; triangles];
        let mut turns = vec![0u8; triangles];
        let mut snapped = vec![false; triangles];
        for chart in &charts {
            // Samples per leg: footprints and height steps follow the samples, not the padding.
            let mut inner = chart.samples.map(|count| count as f64);
            if let Some(fits) = chart.lattice {
                // On the lattice a leg is as many texels long as it is cells long.
                inner = fits.map(|[_, texels]| texels);
                snapped[chart.first] = true;
                if let Some(second) = chart.second {
                    snapped[second] = true;
                }
            }
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
        // Each triangle's chart corner in atlas texels, where its unit rectangle starts past
        // the gutter, and that rectangle's size in texels.
        let mut placement = vec![(0usize, 0usize, [0f64; 2], [0f64; 2]); triangles];
        for chart in &charts {
            let (into, inner) = match chart.lattice {
                Some([[x, width], [y, height]]) => ([x, y], [width, height]),
                None => (
                    [0.; 2],
                    [
                        (chart.width - 2 * GUTTER) as f64,
                        (chart.height - 2 * GUTTER) as f64,
                    ],
                ),
            };
            placement[chart.first] = (chart.x, chart.y, into, inner);
            if let Some(second) = chart.second {
                placement[second] = (chart.x, chart.y, into, inner);
            }
        }
        for (triangle, corner) in corners.iter().enumerate() {
            let a = point(corner[0]);
            unit(cross(sub(point(corner[1]), a), sub(point(corner[2]), a)))?;
            let (x0, y0, into, inner) = placement[triangle];
            // Corner, first-leg end and second-leg end in the unit inner rectangle for each turn.
            let mut local: [[f64; 2]; 3] = match turns[triangle] {
                0 => [[0., 0.], [1., 0.], [0., 1.]],
                1 => [[1., 0.], [1., 1.], [0., 0.]],
                2 => [[1., 1.], [0., 1.], [1., 0.]],
                _ => [[0., 1.], [0., 0.], [1., 1.]],
            };
            // In a chart framed by its rectangle, each corner lies where it is in that frame.
            if let Some([start, across, down]) = framed[triangle] {
                local = corner.map(|index| {
                    let from = sub(point(index), start);
                    [
                        dot(from, across) / dot(across, across),
                        dot(from, down) / dot(down, down),
                    ]
                });
            }
            for (k, index) in corner.iter().enumerate() {
                mesh.positions.push(point(*index));
                mesh.normals.push(unit(vertex(&receiver.normals, *index))?);
                mesh.colors.push([1.; 3]);
                let x = (x0 + GUTTER) as f64 + into[0] + local[k][0] * inner[0];
                let y = (y0 + GUTTER) as f64 + into[1] + local[k][1] * inner[1];
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
            features: std::sync::OnceLock::new(),
            named: program::NamedFeatures::default(),
            frame,
            snapped,
            step: texel_density.map_or(0., |density| 1. / density),
            framed,
        })
    }

    /// The corner and legs a triangle's chart is laid along: its own first corner and edges,
    /// or the rectangle its chart is framed by.
    fn legs(&self, triangle: usize) -> ([f64; 3], [f64; 3], [f64; 3]) {
        match self.framed[triangle] {
            Some([start, across, down]) => (start, across, down),
            None => {
                let p = &self.mesh.positions[triangle * 3..triangle * 3 + 3];
                (p[0], sub(p[1], p[0]), sub(p[2], p[0]))
            }
        }
    }

    /// This atlas with the receiver's named features, indexed by name once for every bake of
    /// the layout. Too many segments or names, a bad name or a point that is not finite fail.
    pub fn with_named(mut self, named: &[program::NamedFeature]) -> Result<Self, String> {
        self.named = program::NamedFeatures::new(named)?;
        Ok(self)
    }

    /// The receiver's own creases, open edges, UV seams and contacts with the grid that finds
    /// them, derived once for the layout: every bake of it, and every thread of a bake, reads
    /// these. The second value says whether features were left out. A receiver has at most
    /// `MAX_FEATURES` of them: creases and open edges come first, in the order of their welded
    /// vertices, then seams, then contacts, and what does not fit is dropped from the end.
    pub fn features(&self) -> (&program::FeatureSet, bool) {
        let (set, dropped) = self.features.get_or_init(|| {
            let receiver = &self.receiver;
            let mut features = program::receiver_features(&receiver.positions, &receiver.indices);
            // Deriving creases stops at the limit, so reaching it may have left some out.
            let mut dropped = features.len() == program::MAX_FEATURES;
            features.extend(program::receiver_seams(
                &receiver.positions,
                &receiver.uvs,
                &receiver.indices,
            ));
            features.extend(receiver.contacts.iter().map(|contact| program::Feature {
                start: contact.start,
                end: contact.end,
                kind: program::FeatureKind::Contact,
                angle: 0.,
                normal: None,
            }));
            dropped |= features.len() > program::MAX_FEATURES;
            features.truncate(program::MAX_FEATURES);
            (program::FeatureSet::new(features), dropped)
        });
        (set, *dropped)
    }

    /// The triangle and leg parameters a chart texel samples. Gutter texels clamp to the nearest
    /// edge; a lone triangle's texels beyond its hypotenuse project onto it.
    pub fn texel(&self, chart: &Chart, x: usize, y: usize) -> (usize, f64, f64) {
        // On the lattice every texel is its own cell: nothing clamps or repeats, and texels past
        // the receiver's edge sample the pattern where it continues.
        if let Some([[into_x, width], [into_y, height]]) = chart.lattice {
            let s = (x as f64 + 0.5 - GUTTER as f64 - into_x) / width;
            let t = (y as f64 + 0.5 - GUTTER as f64 - into_y) / height;
            let (u, v) = if chart.rotated { (t, 1. - s) } else { (s, t) };
            return match chart.second {
                Some(second) if u + v > 1. => (second, 1. - u, 1. - v),
                _ => (chart.first, u, v),
            };
        }
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
        // In a chart framed by its rectangle, (u, v) are along the rectangle's edges: the same
        // point of the triangle's plane in the triangle's own leg parameters.
        let (u, v) = match self.framed[triangle] {
            Some([start, across, down]) => {
                let (first, second) = (sub(b, a), sub(c, a));
                let from = sub(add(start, add(mul(across, u), mul(down, v))), a);
                let (ff, fs, ss) = (dot(first, first), dot(first, second), dot(second, second));
                let (pf, ps) = (dot(from, first), dot(from, second));
                let determinant = ff * ss - fs * fs;
                (
                    (pf * ss - ps * fs) / determinant,
                    (ps * ff - pf * fs) / determinant,
                )
            }
            None => (u, v),
        };
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
        // In the anchor's frame, the position from its origin along its axes.
        let anchored = self.frame.map(|(origin, axes)| {
            let from = sub(world, origin);
            (
                axes.map(|axis| dot(from, axis)),
                axes.map(|axis| dot(normal, axis)),
            )
        });
        let coordinates = match (self.mapping, anchored) {
            (_, Some((coordinates, _))) => coordinates,
            (Mapping::World, _) => world,
            _ => object,
        };
        let projection_normal = match (self.mapping, self.receiver.object_to_world.as_ref()) {
            _ if anchored.is_some() => anchored.expect("an anchored sample").1,
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
        // On the lattice the pattern is sampled at the lattice point itself and at the lattice
        // step: the same bits on every receiver and at every size, whatever rounding the way
        // from texel to position took. The other inputs keep the texel's own position.
        let (surface, footprint) = if self.snapped[triangle] {
            let step = self.step;
            (
                surface.map(|position| ((position / step - 0.5).round() + 0.5) * step),
                step,
            )
        } else {
            (surface, footprint)
        };
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
        let (_, u, v) = self.legs(triangle);
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
        let (_, u, v) = self.legs(triangle);
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

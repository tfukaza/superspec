//! Receiver features: creases and open boundaries derived from triangle geometry.
//!
//! Positions are welded so meshes with split per-face vertices still share edges. An edge shared
//! by two faces is a crease when the faces bend by at least [`COPLANAR_DEGREES`]; flatter edges
//! are triangulation seams and never features. An edge with one face is a boundary.
use super::{Feature, FeatureKind};
use std::collections::BTreeMap;

/// Edges whose faces bend by less than this many degrees are coplanar triangulation edges.
pub const COPLANAR_DEGREES: f64 = 1.0;
/// At most this many features are derived per receiver, in a deterministic order.
pub const MAX_FEATURES: usize = 4096;
/// Positions closer than this many meters are the same vertex.
const WELD: f64 = 1e-6;

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

/// Creases and open edges of one receiver, from world-space `positions` (xyz triples) and
/// triangle `indices`. Degenerate triangles and non-manifold edges (three or more faces) are
/// ignored. The order is deterministic, so equal geometry yields equal features.
pub fn receiver_features(positions: &[f64], indices: &[u32]) -> Vec<Feature> {
    let point = |index: u32| -> Option<[f64; 3]> {
        let at = (index as usize).checked_mul(3)?;
        let end = at.checked_add(3)?;
        positions.get(at..end).map(|p| [p[0], p[1], p[2]])
    };
    let mut welded: BTreeMap<[i64; 3], usize> = BTreeMap::new();
    let mut points: Vec<[f64; 3]> = Vec::new();
    let mut weld = |p: [f64; 3]| {
        let key = p.map(|v| (v / WELD).round() as i64);
        *welded.entry(key).or_insert_with(|| {
            points.push(p);
            points.len() - 1
        })
    };
    // Each face: welded vertex ids and unit normal.
    let mut faces: Vec<([usize; 3], [f64; 3])> = Vec::new();
    for triangle in indices.chunks_exact(3) {
        let Some(corners) = triangle
            .iter()
            .map(|index| point(*index))
            .collect::<Option<Vec<_>>>()
        else {
            continue;
        };
        if corners.iter().flatten().any(|v| !v.is_finite()) {
            continue;
        }
        let ids = [weld(corners[0]), weld(corners[1]), weld(corners[2])];
        if ids[0] == ids[1] || ids[1] == ids[2] || ids[0] == ids[2] {
            continue;
        }
        let normal = cross(sub(corners[1], corners[0]), sub(corners[2], corners[0]));
        let length = dot(normal, normal).sqrt();
        if length < 1e-18 {
            continue;
        }
        faces.push((ids, normal.map(|v| v / length)));
    }
    // Undirected edge -> faces using it, with the face's vertex opposite the edge.
    let mut edges: BTreeMap<(usize, usize), Vec<(usize, usize)>> = BTreeMap::new();
    for (face, (ids, _)) in faces.iter().enumerate() {
        for corner in 0..3 {
            let (a, b, opposite) = (ids[corner], ids[(corner + 1) % 3], ids[(corner + 2) % 3]);
            edges
                .entry((a.min(b), a.max(b)))
                .or_default()
                .push((face, opposite));
        }
    }
    let mut features = Vec::new();
    for ((a, b), users) in edges {
        let (start, end) = (points[a], points[b]);
        let feature = match users.as_slice() {
            [_] => Feature {
                start,
                end,
                kind: FeatureKind::Boundary,
                angle: 180.,
                normal: None,
            },
            [(first, _), (second, opposite)] => {
                let (n1, n2) = (faces[*first].1, faces[*second].1);
                let angle = super::numeric::acos(dot(n1, n2).clamp(-1., 1.))
                    * (180. / std::f64::consts::PI);
                if angle < COPLANAR_DEGREES {
                    continue;
                }
                // The second face bends below the first face's plane at a convex edge.
                let bend = dot(n1, sub(points[*opposite], start));
                let middle = [n1[0] + n2[0], n1[1] + n2[1], n1[2] + n2[2]];
                let middle_length = dot(middle, middle).sqrt();
                Feature {
                    // Faces folded flat onto each other have no single middle normal.
                    normal: (middle_length > 1e-9).then(|| middle.map(|v| v / middle_length)),
                    start,
                    end,
                    kind: if bend < 0. {
                        FeatureKind::Convex
                    } else {
                        FeatureKind::Concave
                    },
                    angle,
                }
            }
            _ => continue,
        };
        features.push(feature);
        if features.len() == MAX_FEATURES {
            break;
        }
    }
    features
}

/// Distance in meters from `point` to the segment `start..end`.
pub(super) fn segment_distance(point: [f64; 3], start: [f64; 3], end: [f64; 3]) -> f64 {
    let along = sub(end, start);
    let offset = sub(point, start);
    let squared = dot(along, along);
    let t = if squared > 0. {
        (dot(offset, along) / squared).clamp(0., 1.)
    } else {
        0.
    };
    let nearest = [
        start[0] + along[0] * t,
        start[1] + along[1] * t,
        start[2] + along[2] * t,
    ];
    let gap = sub(point, nearest);
    dot(gap, gap).sqrt()
}

fn unit(v: [f64; 3]) -> Option<[f64; 3]> {
    let length = dot(v, v).sqrt();
    (length.is_finite() && length > 1e-12).then(|| v.map(|c| c / length))
}

/// A uniform grid over one receiver's features, so a query visits the features near its sample
/// instead of all of them. Queries answer exactly as a scan of every feature in order would.
#[derive(Default)]
pub(super) struct Index {
    origin: [f64; 3],
    cell: f64,
    size: [usize; 3],
    /// Where each cell's features start in `items`; empty when every query scans all features.
    starts: Vec<u32>,
    items: Vec<u32>,
    /// Features crossing too many cells to list in each one; every query visits them.
    wide: Vec<u32>,
}

/// Receivers with fewer features are scanned whole.
const INDEX_MINIMUM: usize = 32;
/// A feature covering more cells than this is kept in the `wide` list.
const WIDE_CELLS: usize = 256;

impl Index {
    pub(super) fn new(features: &[Feature]) -> Self {
        if features.len() < INDEX_MINIMUM {
            return Self::default();
        }
        let mut low = [f64::INFINITY; 3];
        let mut high = [f64::NEG_INFINITY; 3];
        for point in features.iter().flat_map(|f| [f.start, f.end]) {
            for axis in 0..3 {
                low[axis] = low[axis].min(point[axis]);
                high[axis] = high[axis].max(point[axis]);
            }
        }
        let extent: [f64; 3] = std::array::from_fn(|axis| high[axis] - low[axis]);
        let longest = extent.into_iter().fold(0., f64::max);
        if !(longest.is_finite() && longest > 0.) {
            return Self::default();
        }
        // About two cells per feature, and at most 64 cells along an axis.
        let volume: f64 = extent.iter().map(|e| e.max(longest / 64.)).product();
        let cell = (volume / (features.len() as f64 * 2.))
            .cbrt()
            .max(longest / 64.);
        let mut index = Self {
            origin: low,
            cell,
            size: extent.map(|e| ((e / cell).ceil() as usize).clamp(1, 64)),
            ..Self::default()
        };
        let cells = index.size.iter().product::<usize>();
        let range = |index: &Self, feature: &Feature| -> [[usize; 2]; 3] {
            std::array::from_fn(|axis| {
                let (a, b) = (feature.start[axis], feature.end[axis]);
                [
                    index.coordinate(a.min(b), axis),
                    index.coordinate(a.max(b), axis),
                ]
            })
        };
        let covered =
            |range: &[[usize; 2]; 3]| range.iter().map(|[a, b]| b - a + 1).product::<usize>();
        let mut counts = vec![0u32; cells + 1];
        for (number, feature) in features.iter().enumerate() {
            let range = range(&index, feature);
            if covered(&range) > WIDE_CELLS {
                index.wide.push(number as u32);
                continue;
            }
            index.cells(&range, |cell| counts[cell + 1] += 1);
        }
        for cell in 0..cells {
            counts[cell + 1] += counts[cell];
        }
        let mut next = counts.clone();
        index.items = vec![0; counts[cells] as usize];
        for (number, feature) in features.iter().enumerate() {
            let range = range(&index, feature);
            if covered(&range) > WIDE_CELLS {
                continue;
            }
            let items = &mut index.items;
            Self::cells_of(index.size, &range, |cell| {
                items[next[cell] as usize] = number as u32;
                next[cell] += 1;
            });
        }
        index.starts = counts;
        index
    }
    fn coordinate(&self, value: f64, axis: usize) -> usize {
        // Monotonic in `value`, so overlapping intervals always share a cell.
        (((value - self.origin[axis]) / self.cell).floor().max(0.) as usize)
            .min(self.size[axis] - 1)
    }
    fn cells(&self, range: &[[usize; 2]; 3], visit: impl FnMut(usize)) {
        Self::cells_of(self.size, range, visit)
    }
    fn cells_of(size: [usize; 3], range: &[[usize; 2]; 3], mut visit: impl FnMut(usize)) {
        for z in range[2][0]..=range[2][1] {
            for y in range[1][0]..=range[1][1] {
                for x in range[0][0]..=range[0][1] {
                    visit((z * size[1] + y) * size[0] + x);
                }
            }
        }
    }
    /// Offers `visit` every feature that can be within `limit` meters of `point`, with its
    /// position in `features`; others may be offered too, and some more than once. Returns how
    /// many were offered, which is the work a query is charged for.
    pub(super) fn near(
        &self,
        features: &[Feature],
        point: [f64; 3],
        limit: f64,
        mut visit: impl FnMut(usize, &Feature),
    ) -> usize {
        let range: [[usize; 2]; 3] = if self.starts.is_empty() {
            [[0, 0]; 3]
        } else {
            std::array::from_fn(|axis| {
                [
                    self.coordinate(point[axis] - limit, axis),
                    self.coordinate(point[axis] + limit, axis),
                ]
            })
        };
        let covered = range.iter().map(|[a, b]| b - a + 1).product::<usize>();
        // A query reaching much of the grid is cheaper as one pass over every feature.
        if self.starts.is_empty() || covered * 4 > self.starts.len() {
            for (number, feature) in features.iter().enumerate() {
                visit(number, feature);
            }
            return features.len();
        }
        let mut offered = self.wide.len();
        for &number in &self.wide {
            visit(number as usize, &features[number as usize]);
        }
        self.cells(&range, |cell| {
            let listed = &self.items[self.starts[cell] as usize..self.starts[cell + 1] as usize];
            offered += listed.len();
            for &number in listed {
                visit(number as usize, &features[number as usize]);
            }
        });
        offered
    }
}

/// Distance in meters to the nearest selected feature, at most `limit`, and the features visited.
pub(super) fn crease_distance(
    index: &Index,
    features: &[Feature],
    point: [f64; 3],
    selector: Selector,
    min_angle: f64,
    limit: f64,
) -> (f64, usize) {
    let mut nearest = limit;
    let visited = index.near(features, point, limit, |_, feature| {
        if selected(feature, selector, min_angle) {
            nearest = nearest.min(segment_distance(point, feature.start, feature.end));
        }
    });
    (nearest, visited)
}

/// The nearest feature a query accepts within its limit: the earliest of equally near ones, as a
/// scan in feature order finds.
struct Nearest<T> {
    distance: f64,
    number: usize,
    value: T,
}
impl<T> Nearest<T> {
    fn offer(&mut self, distance: f64, number: usize, value: impl FnOnce() -> Option<T>) {
        if distance < self.distance || (distance == self.distance && number < self.number) {
            if let Some(value) = value() {
                *self = Self {
                    distance,
                    number,
                    value,
                };
            }
        }
    }
}

/// Middle normal of the nearest selected crease within `limit` meters that faces the same side as
/// `normal`, or `normal` itself when none does, with the features visited. Boundaries are never
/// chosen: they have one face. The facing test keeps a thin part's far-side edges from bending
/// this side's normal.
pub(super) fn crease_normal(
    index: &Index,
    features: &[Feature],
    point: [f64; 3],
    normal: [f64; 3],
    selector: Selector,
    min_angle: f64,
    limit: f64,
) -> ([f64; 3], usize) {
    let mut nearest = Nearest {
        distance: limit,
        number: 0,
        value: normal,
    };
    let visited = index.near(features, point, limit, |number, feature| {
        let Some(middle) = feature.normal else {
            return;
        };
        if !selected(feature, selector, min_angle) || dot(middle, normal) <= 0. {
            return;
        }
        let distance = segment_distance(point, feature.start, feature.end);
        // Only a feature strictly inside the limit replaces the receiver normal.
        if distance < limit {
            nearest.offer(distance, number, || Some(middle));
        }
    });
    (nearest.value, visited)
}

/// Unit direction along the nearest selected feature within `limit` meters, start to end, or zero
/// when none is within reach. Any selector applies, so boundaries, seams and contacts have tangents.
pub(super) fn crease_tangent(
    index: &Index,
    features: &[Feature],
    point: [f64; 3],
    selector: Selector,
    min_angle: f64,
    limit: f64,
) -> ([f64; 3], usize) {
    let mut nearest = Nearest {
        distance: limit,
        number: 0,
        value: [0.; 3],
    };
    let visited = index.near(features, point, limit, |number, feature| {
        if !selected(feature, selector, min_angle) {
            return;
        }
        let distance = segment_distance(point, feature.start, feature.end);
        if distance < limit {
            nearest.offer(distance, number, || unit(sub(feature.end, feature.start)));
        }
    });
    (nearest.value, visited)
}

/// The far face's normal across the crease [`crease_normal`] chooses: `normal` reflected across
/// the crease's middle normal, which bisects the two faces. Without a crease, `normal` itself.
pub(super) fn crease_adjacent_normal(
    index: &Index,
    features: &[Feature],
    point: [f64; 3],
    normal: [f64; 3],
    selector: Selector,
    min_angle: f64,
    limit: f64,
) -> ([f64; 3], usize) {
    let (middle, visited) =
        crease_normal(index, features, point, normal, selector, min_angle, limit);
    let along = 2. * dot(middle, normal);
    (
        unit(std::array::from_fn(|i| middle[i] * along - normal[i])).unwrap_or(normal),
        visited,
    )
}

/// Turns unit `from` toward unit `to` by `weight` in [0, 1] along the great circle, so a weight
/// of one reaches `to`. Opposite directions have no unique path and leave `from` unchanged.
pub(super) fn slerp(from: [f64; 3], to: [f64; 3], weight: f64) -> [f64; 3] {
    let cosine = dot(from, to).clamp(-1., 1.);
    if cosine < -1. + 1e-9 {
        return from;
    }
    let angle = super::numeric::acos(cosine);
    if angle < 1e-12 {
        return from;
    }
    let sine = super::numeric::sin(angle);
    let (a, b) = (
        super::numeric::sin((1. - weight) * angle) / sine,
        super::numeric::sin(weight * angle) / sine,
    );
    unit(std::array::from_fn(|i| from[i] * a + to[i] * b)).unwrap_or(from)
}

/// Applies a tangent-space relief normal around an adjusted receiver normal.
///
/// The encoded tangent space has its z axis on `receiver`, the sample's own shading normal (what a
/// renderer decodes the normal map around), and its x and y axes along the world directions
/// `tangent` and `bitangent`, made perpendicular to `receiver` with their handedness kept. The
/// relief is rotated by the smallest rotation taking `receiver` to `adjusted`, then expressed in
/// the same axes, so `adjusted == receiver` leaves it unchanged. An adjusted normal facing away
/// from the receiver is rejected rather than turning the surface inside out.
pub fn orient_relief(
    relief: [f64; 3],
    tangent: [f64; 3],
    bitangent: [f64; 3],
    receiver: [f64; 3],
    adjusted: [f64; 3],
) -> Result<[f64; 3], &'static str> {
    let n = unit(receiver).ok_or("The receiver has no shading normal.")?;
    let x = unit(sub(tangent, n.map(|c| c * dot(tangent, n))))
        .ok_or("The receiver tangent is parallel to its normal.")?;
    let mut y = cross(n, x);
    if dot(y, bitangent) < 0. {
        y = y.map(|c| -c);
    }
    let target =
        unit(adjusted).ok_or("The receiver normal output must be a finite nonzero vector.")?;
    let cosine = dot(n, target);
    if cosine <= 1e-6 {
        return Err("The receiver normal output must face the same side as the receiver.");
    }
    // An unturned normal (up to rounding from normalizing it again) keeps the relief bit for bit.
    if 1. - cosine < 1e-12 {
        return Ok(relief);
    }
    let world: [f64; 3] =
        std::array::from_fn(|i| relief[0] * x[i] + relief[1] * y[i] + relief[2] * n[i]);
    // Rodrigues rotation about n × target, written without normalizing the axis.
    let axis = cross(n, target);
    let turned: [f64; 3] = {
        let k_cross_w = cross(axis, world);
        let k_cross_k_cross_w = cross(axis, k_cross_w);
        std::array::from_fn(|i| world[i] + k_cross_w[i] + k_cross_k_cross_w[i] / (1. + cosine))
    };
    unit([dot(turned, x), dot(turned, y), dot(turned, n)])
        .ok_or("Height derivatives exceed the finite normal range.")
}

/// The smallest component along the receiver normal a bent shaded normal keeps (about one degree
/// above the surface).
pub const HORIZON: f64 = 0.02;

/// Adds a world-space `offset` to a shaded tangent-space normal and renormalizes, as a shader
/// bends a normal after normal mapping. The tangent space is the one [`orient_relief`] uses: z on
/// `receiver`, x and y along `tangent` and `bitangent` made perpendicular to it. A zero offset
/// returns `shaded` unchanged. A result that would face away from the receiver is held at
/// [`HORIZON`] above its plane, and an offset that cancels the shaded normal changes nothing.
pub fn bend_shaded(
    shaded: [f64; 3],
    tangent: [f64; 3],
    bitangent: [f64; 3],
    receiver: [f64; 3],
    offset: [f64; 3],
) -> Result<[f64; 3], &'static str> {
    if offset.iter().all(|v| *v == 0.) {
        return Ok(shaded);
    }
    let n = unit(receiver).ok_or("The receiver has no shading normal.")?;
    let x = unit(sub(tangent, n.map(|c| c * dot(tangent, n))))
        .ok_or("The receiver tangent is parallel to its normal.")?;
    let mut y = cross(n, x);
    if dot(y, bitangent) < 0. {
        y = y.map(|c| -c);
    }
    let world: [f64; 3] =
        std::array::from_fn(|i| shaded[0] * x[i] + shaded[1] * y[i] + shaded[2] * n[i] + offset[i]);
    // An offset that cancels the shaded normal leaves no direction to bend toward.
    let Some(bent) = unit(world) else {
        return Ok(shaded);
    };
    // Interpolated normals near a crease can push the sum behind the surface; such a normal
    // stops just above the receiver's horizon instead of failing the whole bake.
    let (along_x, along_y, facing) = (dot(bent, x), dot(bent, y), dot(bent, n));
    if facing > 1e-6 {
        return Ok([along_x, along_y, facing]);
    }
    let flat = (along_x * along_x + along_y * along_y).sqrt();
    if flat < 1e-12 {
        // Straight into the surface: no side to lean toward.
        return Ok(shaded);
    }
    let scale = (1. - HORIZON * HORIZON).sqrt() / flat;
    Ok([along_x * scale, along_y * scale, HORIZON])
}

/// UV seams of one receiver: edges shared by two faces whose texture coordinates differ across
/// the edge, bent or flat, from world-space `positions`, UV pairs and triangle `indices`. Positions
/// are welded like [`receiver_features`]; non-manifold and degenerate input is ignored.
pub fn receiver_seams(positions: &[f64], uvs: &[f64], indices: &[u32]) -> Vec<Feature> {
    let vertex = |index: u32| -> Option<([f64; 3], [f64; 2])> {
        let i = index as usize;
        let p = positions.get(i.checked_mul(3)?..i.checked_mul(3)?.checked_add(3)?)?;
        let t = uvs.get(i.checked_mul(2)?..i.checked_mul(2)?.checked_add(2)?)?;
        Some(([p[0], p[1], p[2]], [t[0], t[1]]))
    };
    let key = |p: [f64; 3]| p.map(|v| (v / WELD).round() as i64);
    // Undirected welded edge -> each face's endpoint UVs, ordered by the welded key.
    let mut edges: BTreeMap<([i64; 3], [i64; 3]), Vec<([f64; 3], [f64; 3], [f64; 2], [f64; 2])>> =
        BTreeMap::new();
    for triangle in indices.chunks_exact(3) {
        let Some(corners) = triangle
            .iter()
            .map(|i| vertex(*i))
            .collect::<Option<Vec<_>>>()
        else {
            continue;
        };
        if corners
            .iter()
            .any(|(p, t)| p.iter().chain(t).any(|v| !v.is_finite()))
        {
            continue;
        }
        for corner in 0..3 {
            let (a, b) = (corners[corner], corners[(corner + 1) % 3]);
            let (ka, kb) = (key(a.0), key(b.0));
            if ka == kb {
                continue;
            }
            let entry = if ka < kb {
                (a.0, b.0, a.1, b.1)
            } else {
                (b.0, a.0, b.1, a.1)
            };
            edges
                .entry((ka.min(kb), ka.max(kb)))
                .or_default()
                .push(entry);
        }
    }
    let differs =
        |a: [f64; 2], b: [f64; 2]| (a[0] - b[0]).abs() > 1e-6 || (a[1] - b[1]).abs() > 1e-6;
    let mut seams = Vec::new();
    for uses in edges.values() {
        let [first, second] = uses.as_slice() else {
            continue;
        };
        if differs(first.2, second.2) || differs(first.3, second.3) {
            seams.push(Feature {
                start: first.0,
                end: first.1,
                kind: FeatureKind::Seam,
                angle: 0.,
                normal: None,
            });
            if seams.len() == MAX_FEATURES {
                break;
            }
        }
    }
    seams
}

/// Whether a feature matches a `crease_distance` selector at a minimum crease angle. Seams and
/// contacts are selected only by their own names.
pub(super) fn selected(feature: &Feature, selector: Selector, min_angle: f64) -> bool {
    let crease = |kind| feature.kind == kind && feature.angle >= min_angle;
    match selector {
        Selector::Convex => crease(FeatureKind::Convex),
        Selector::Concave => crease(FeatureKind::Concave),
        Selector::Boundary => feature.kind == FeatureKind::Boundary,
        Selector::Exposed => crease(FeatureKind::Convex) || feature.kind == FeatureKind::Boundary,
        Selector::Any => {
            crease(FeatureKind::Convex)
                || crease(FeatureKind::Concave)
                || feature.kind == FeatureKind::Boundary
        }
        Selector::Seam => feature.kind == FeatureKind::Seam,
        Selector::Contact => feature.kind == FeatureKind::Contact,
        Selector::Every => true,
        Selector::Nothing => false,
    }
}

/// Features with the grid that finds the ones near a sample, built once and shared by every
/// evaluator that reads them (each thread of a bake, each bake of a layout).
#[derive(Default)]
pub struct FeatureSet {
    features: Vec<Feature>,
    index: Index,
}

impl FeatureSet {
    pub fn new(features: Vec<Feature>) -> Self {
        let index = Index::new(&features);
        Self { features, index }
    }
    pub fn features(&self) -> &[Feature] {
        &self.features
    }
    pub(super) fn index(&self) -> &Index {
        &self.index
    }
    /// Bytes the features and their grid hold.
    pub fn bytes(&self) -> usize {
        self.features.len() * size_of::<Feature>()
            + (self.index.starts.len() + self.index.items.len() + self.index.wide.len()) * 4
            + 128
    }
}

/// A polyline a geometry's owner names, in world meters: a sill, a joint, the base of a wall.
/// Programs measure to it with `feature_distance(name, limit)` and `feature_tangent`.
#[derive(Clone, Debug, Default, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct NamedFeature {
    pub name: String,
    pub points: Vec<[f64; 3]>,
    /// Whether the last point joins the first.
    #[serde(default)]
    pub closed: bool,
}

/// Segments of every named feature one receiver may hold. More are refused, never dropped.
pub const MAX_NAMED_SEGMENTS: usize = 65_536;
/// Names one receiver's features may have.
pub const MAX_FEATURE_NAMES: usize = 256;

/// A receiver's named features: the segments of each name with their own grid, so a query of
/// one name never visits another's, however many there are.
#[derive(Default)]
pub struct NamedFeatures {
    /// By name, ascending.
    sets: Vec<(String, FeatureSet)>,
}

static NO_NAMED_FEATURES: NamedFeatures = NamedFeatures { sets: Vec::new() };

impl NamedFeatures {
    /// A receiver with no named feature.
    pub fn none() -> &'static Self {
        &NO_NAMED_FEATURES
    }
    /// Polylines of one name are one set, in the order given. Names follow the rule for external
    /// inputs (at most 64 letters, digits, `.`, `_` or `-`); points are finite.
    pub fn new(named: &[NamedFeature]) -> Result<Self, String> {
        let mut by_name: BTreeMap<&str, Vec<Feature>> = BTreeMap::new();
        let mut segments = 0usize;
        for feature in named {
            if !valid_feature_name(&feature.name) {
                return Err(format!(
                    "Feature name '{}' is not at most 64 letters, digits, '.', '_' or '-'.",
                    feature.name
                ));
            }
            if feature
                .points
                .iter()
                .flatten()
                .any(|v| !v.is_finite() || v.abs() > 1e12)
            {
                return Err(format!(
                    "Feature '{}' needs finite world coordinates.",
                    feature.name
                ));
            }
            let set = by_name.entry(&feature.name).or_default();
            let ends = feature.points.iter().skip(1).chain(
                feature
                    .points
                    .first()
                    .filter(|_| feature.closed && feature.points.len() > 2),
            );
            for (start, end) in feature.points.iter().zip(ends) {
                set.push(Feature {
                    start: *start,
                    end: *end,
                    kind: FeatureKind::Named,
                    angle: 0.,
                    normal: None,
                });
                segments += 1;
            }
            if segments > MAX_NAMED_SEGMENTS {
                return Err(format!(
                    "A receiver holds at most {MAX_NAMED_SEGMENTS} named feature segments."
                ));
            }
        }
        if by_name.len() > MAX_FEATURE_NAMES {
            return Err(format!(
                "A receiver's features have at most {MAX_FEATURE_NAMES} names."
            ));
        }
        Ok(Self {
            sets: by_name
                .into_iter()
                .map(|(name, features)| (name.to_owned(), FeatureSet::new(features)))
                .collect(),
        })
    }
    /// The segments of `name`; none for a name the receiver has no feature of.
    pub fn get(&self, name: &str) -> Option<&FeatureSet> {
        let at = self
            .sets
            .binary_search_by(|(known, _)| known.as_str().cmp(name));
        at.ok().map(|at| &self.sets[at].1)
    }
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.sets.iter().map(|(name, _)| name.as_str())
    }
    /// Segments of the name that has the most.
    pub fn largest(&self) -> usize {
        self.sets
            .iter()
            .map(|(_, set)| set.features.len())
            .max()
            .unwrap_or(0)
    }
    pub fn is_empty(&self) -> bool {
        self.sets.is_empty()
    }
    pub fn bytes(&self) -> usize {
        self.sets
            .iter()
            .map(|(name, set)| name.len() + set.bytes())
            .sum::<usize>()
            + 64
    }
}

/// Whether `name` can name a feature (the rule for external inputs).
pub(super) fn valid_feature_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'_' | b'-'))
}

/// A feature query's selector, read from its name once per query instead of once per feature.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Selector {
    Convex,
    Concave,
    Boundary,
    Exposed,
    Any,
    Seam,
    Contact,
    /// Every feature of the set queried: the segments of one name.
    Every,
    /// An unknown name selects no feature.
    Nothing,
}

impl Selector {
    pub(super) fn named(name: &str) -> Self {
        match name {
            "convex" => Self::Convex,
            "concave" => Self::Concave,
            "boundary" => Self::Boundary,
            "exposed" => Self::Exposed,
            "any" => Self::Any,
            "seam" => Self::Seam,
            "contact" => Self::Contact,
            _ => Self::Nothing,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Deterministic values in [0, 1).
    fn sequence(mut state: u64) -> impl FnMut() -> f64 {
        move || {
            state = state
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (state >> 11) as f64 / (1u64 << 53) as f64
        }
    }

    /// A grid query answers exactly as a scan of every feature, for near and far limits.
    #[test]
    fn indexed_queries_equal_a_scan_of_every_feature() {
        let mut next = sequence(7);
        let mut features = Vec::new();
        for number in 0..600 {
            let start = [next() * 2., next() * 3., next() * 0.2];
            // Mostly short edges, a few that span the receiver, and exact duplicates.
            let end = if number % 50 == 0 {
                // Through the middle to the far side: an edge that crosses most of the grid.
                [2. - start[0], 3. - start[1], 0.2 - start[2]]
            } else {
                std::array::from_fn(|axis| start[axis] + (next() - 0.5) * 0.05)
            };
            let normal = unit([next() - 0.5, next() - 0.5, next() + 0.1]);
            let kind = match number % 3 {
                0 => FeatureKind::Convex,
                1 => FeatureKind::Concave,
                _ => FeatureKind::Boundary,
            };
            let feature = Feature {
                start,
                end,
                kind,
                angle: 10. + next() * 80.,
                normal: (kind != FeatureKind::Boundary).then_some(normal).flatten(),
            };
            features.push(feature.clone());
            if number % 97 == 0 {
                features.push(feature);
            }
        }
        let (scan, grid) = (Index::default(), Index::new(&features));
        assert!(!grid.starts.is_empty() && !grid.wide.is_empty());
        let mut fewer = 0;
        for query in 0..4000 {
            let point = [next() * 2.4 - 0.2, next() * 3.4 - 0.2, next() * 0.4 - 0.1];
            let limit = [0.004, 0.03, 0.2, 5.][query % 4];
            let selector = [Selector::Convex, Selector::Exposed, Selector::Any][query % 3];
            let facing = [0., 0., 1.];
            let all = crease_distance(&scan, &features, point, selector, 30., limit);
            let near = crease_distance(&grid, &features, point, selector, 30., limit);
            assert_eq!(
                near.0.to_bits(),
                all.0.to_bits(),
                "distance at {point:?} within {limit}"
            );
            fewer += usize::from(near.1 < all.1);
            let all = crease_normal(&scan, &features, point, facing, selector, 30., limit);
            let near = crease_normal(&grid, &features, point, facing, selector, 30., limit);
            assert_eq!(near.0, all.0, "normal at {point:?} within {limit}");
            let all = crease_tangent(&scan, &features, point, selector, 30., limit);
            let near = crease_tangent(&grid, &features, point, selector, 30., limit);
            assert_eq!(near.0, all.0, "tangent at {point:?} within {limit}");
        }
        assert!(fewer > 2500, "short queries visit fewer features ({fewer})");
    }
}

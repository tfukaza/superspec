//! Weathering zones: where a geometry's owner says an effect hangs from an edge, with the
//! zone's own coordinates. A zone is a named feature's polyline (the edge) with a downhill
//! direction and a length: every receiver point beneath the edge, within the zone's depth of
//! the edge's surface, has `u` (meters along the edge from its start, plus the zone's `u0`, so
//! zones laid end to end continue) and `v` (meters below the edge, along downhill). The owner
//! also says where the zone stops early (`stops`: a column ends at the next edge below) and what
//! it leaves out (`holes`: openings). A program reads them with `zone_coord` and `zone_extent`:
//! no search for edges per texel, and nothing above an edge is ever in its zone.

use super::NamedFeature;
use std::collections::BTreeMap;

/// What makes a named feature's polyline the edge of a zone, in the feature's space.
#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Zone {
    /// Downhill along the surface the zone lies on: a unit direction, not along the edge.
    pub down: [f64; 3],
    /// The greatest drop, in meters below the edge.
    pub length: f64,
    /// How far off the plane of the edge and `down` a point of the zone may lie, in meters.
    #[serde(default = "default_depth")]
    pub depth: f64,
    /// `u` at the polyline's first point.
    #[serde(default)]
    pub u0: f64,
    /// The zone's identity, in [0, 1): what varies one zone's pattern from another's.
    #[serde(default)]
    pub id: f64,
    /// A value the owner gives the zone for its materials (a length scale, a strength), 1 by
    /// default.
    #[serde(default = "one")]
    pub scale: f64,
    /// `[u from, u to, v]`: between those `u` the zone ends `v` below its edge (the next edge
    /// below, an opening's head).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub stops: Vec<[f64; 3]>,
    /// `[u from, u to, v from, v to]`: rectangles of the zone's coordinates left out.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub holes: Vec<[f64; 4]>,
}
fn default_depth() -> f64 {
    DEFAULT_DEPTH
}
fn one() -> f64 {
    1.
}

/// How far off its edge's surface a zone reaches unless its owner says.
pub const DEFAULT_DEPTH: f64 = 0.03;
/// Stops and holes one zone may have.
pub const MAX_ZONE_CLIPS: usize = 256;
/// Zone edge segments one receiver may hold.
pub const MAX_ZONE_SEGMENTS: usize = 16_384;

impl Zone {
    fn check(&self) -> Result<(), String> {
        let finite = |values: &[f64]| values.iter().all(|v| v.is_finite() && v.abs() <= 1e9);
        let length = (self.down.iter().map(|v| v * v).sum::<f64>()).sqrt();
        if !finite(&self.down) || (length - 1.).abs() > 1e-6 {
            return Err("A zone's down is a unit direction.".into());
        }
        if !(self.length > 0. && self.length <= 1000.) || !(self.depth > 0. && self.depth <= 10.) {
            return Err("A zone's length is from 0 to 1000 m and its depth from 0 to 10 m.".into());
        }
        if !finite(&[self.u0, self.scale]) || !(0. ..1.).contains(&self.id) {
            return Err("A zone's u0 and scale are finite and its id is in [0, 1).".into());
        }
        if self.stops.len() + self.holes.len() > MAX_ZONE_CLIPS
            || !self.stops.iter().all(|stop| finite(stop))
            || !self.holes.iter().all(|hole| finite(hole))
        {
            return Err(format!(
                "A zone has at most {MAX_ZONE_CLIPS} finite stops and holes."
            ));
        }
        Ok(())
    }
    /// Where the zone ends below its edge at `u`.
    fn limit(&self, u: f64) -> f64 {
        self.stops
            .iter()
            .filter(|stop| stop[0] <= u && u <= stop[1])
            .fold(self.length, |limit, stop| limit.min(stop[2]))
    }
    fn left_out(&self, u: f64, v: f64) -> bool {
        self.holes
            .iter()
            .any(|hole| hole[0] <= u && u <= hole[1] && hole[2] <= v && v <= hole[3])
    }
}

/// One straight piece of a zone's edge, solved once for every point measured against it.
struct Segment {
    zone: usize,
    start: [f64; 3],
    along: [f64; 3],
    /// Unit, across the plane of the edge and down.
    off: [f64; 3],
    along_down: f64,
    determinant: f64,
    length: f64,
    /// `u` at the segment's start.
    u: f64,
}

/// A point of a zone: its coordinates, the zone's identity, where its column ends and its scale.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ZoneHit {
    pub u: f64,
    pub v: f64,
    pub id: f64,
    pub limit: f64,
    pub scale: f64,
    /// `u` where the zone's edge starts and where it ends.
    pub span: [f64; 2],
}

/// The zones of one name over one receiver.
#[derive(Default)]
pub struct ZoneSet {
    zones: Vec<Zone>,
    /// Each zone's `u` at its edge's start and end.
    spans: Vec<[f64; 2]>,
    segments: Vec<Segment>,
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

impl ZoneSet {
    fn add(&mut self, feature: &NamedFeature, zone: &Zone) -> Result<(), String> {
        zone.check()
            .map_err(|message| format!("Feature '{}': {message}", feature.name))?;
        let index = self.zones.len();
        let mut u = zone.u0;
        let ends = feature.points.iter().skip(1).chain(
            feature
                .points
                .first()
                .filter(|_| feature.closed && feature.points.len() > 2),
        );
        for (start, end) in feature.points.iter().zip(ends) {
            let along = sub(*end, *start);
            let length = dot(along, along).sqrt();
            let off = cross(along, zone.down);
            let size = dot(off, off).sqrt();
            let along_down = dot(along, zone.down);
            let determinant = dot(along, along) - along_down * along_down;
            // A piece along downhill (or a point) has no width to hang anything from.
            if size > 1e-9 && determinant > 1e-12 {
                self.segments.push(Segment {
                    zone: index,
                    start: *start,
                    along,
                    off: off.map(|v| v / size),
                    along_down,
                    determinant,
                    length,
                    u,
                });
            }
            u += length;
        }
        self.zones.push(zone.clone());
        self.spans.push([zone.u0, u]);
        Ok(())
    }

    /// The zone `point` (with surface `normal`) is in: of every zone of the set that holds it,
    /// the one whose edge is nearest above (the least `v`; the first given on a tie). A point
    /// lies in a zone when it is below the edge's piece it projects onto (along downhill), within
    /// the zone's length, its column's stop and its depth of the edge's surface, outside every
    /// hole, on a surface that faces across the zone. The second value counts the pieces visited.
    pub fn at(&self, point: [f64; 3], normal: [f64; 3]) -> (Option<ZoneHit>, usize) {
        let mut best: Option<ZoneHit> = None;
        for segment in &self.segments {
            let zone = &self.zones[segment.zone];
            let from = sub(point, segment.start);
            let off = dot(from, segment.off);
            if off.abs() > zone.depth || dot(normal, segment.off).abs() < 0.5 {
                continue;
            }
            let in_plane = [
                from[0] - segment.off[0] * off,
                from[1] - segment.off[1] * off,
                from[2] - segment.off[2] * off,
            ];
            let (on_along, on_down) = (dot(in_plane, segment.along), dot(in_plane, zone.down));
            let t = (on_along - segment.along_down * on_down) / segment.determinant;
            if !(0. ..=1.).contains(&t) {
                continue;
            }
            let v = on_down - t * segment.along_down;
            if v < -1e-9 {
                continue;
            }
            let v = v.max(0.);
            if best.is_some_and(|best| best.v <= v) {
                continue;
            }
            let u = segment.u + t * segment.length;
            let limit = zone.limit(u);
            if v > limit || zone.left_out(u, v) {
                continue;
            }
            best = Some(ZoneHit {
                u,
                v,
                id: zone.id,
                limit,
                scale: zone.scale,
                span: self.spans[segment.zone],
            });
        }
        (best, self.segments.len())
    }
}

/// Texels of one row of [`pack`]: seven RGBA texels.
pub const ZONE_ROW_TEXELS: usize = 7;

/// A receiver's zones as a GPU bake reads them (`ms_zones`, `ms_zone_count`): every segment of
/// every zone a layer that follows `follow` sees under one of `names` (its place in `names` is
/// its kind), whose region (the segment swept down its zone's length, thickened by its depth)
/// meets `bounds`, the receiver's world box; then the zones' stops and holes. 28 numbers a row:
///
/// - a segment: start and its `u`; along and its length; down and the zone's depth; across
///   (unit) and the zone's length; id, scale, first clip row and clip rows; kind, along · down,
///   the determinant and 0; the zone's span start and end, and 0, 0.
/// - a clip: `[u from, u to, v (stop) or v from, v to (hole)]`, then `[0 stop or 1 hole, 0, 0,
///   0]`, then zeros.
///
/// The second value is the number of segment rows; the clip rows follow them.
pub fn pack(
    named: &[NamedFeature],
    names: &[String],
    follow: &BTreeMap<String, super::Follow>,
    bounds: [[f64; 3]; 2],
) -> (Vec<f64>, usize) {
    let mut segments: Vec<f64> = Vec::new();
    let mut clips: Vec<f64> = Vec::new();
    let mut count = 0;
    for feature in named {
        let Some(zone) = &feature.zone else {
            continue;
        };
        let places: Vec<usize> = super::followed_names(&feature.name, follow)
            .filter_map(|seen| names.iter().position(|name| name == seen))
            .collect();
        if places.is_empty() || zone.check().is_err() {
            continue;
        }
        let mut set = ZoneSet::default();
        if set.add(feature, zone).is_err() {
            continue;
        }
        let mut clip_rows: Option<(usize, usize)> = None;
        for segment in &set.segments {
            // The region the segment's zone can cover, against the receiver's box.
            let reach = zone.depth;
            let corners = [
                segment.start,
                std::array::from_fn(|i| segment.start[i] + segment.along[i]),
                std::array::from_fn(|i| segment.start[i] + zone.down[i] * zone.length),
                std::array::from_fn(|i| {
                    segment.start[i] + segment.along[i] + zone.down[i] * zone.length
                }),
            ];
            let misses = (0..3).any(|axis| {
                let low = corners
                    .iter()
                    .map(|c: &[f64; 3]| c[axis])
                    .fold(f64::INFINITY, f64::min);
                let high = corners
                    .iter()
                    .map(|c: &[f64; 3]| c[axis])
                    .fold(f64::NEG_INFINITY, f64::max);
                low - reach > bounds[1][axis] || high + reach < bounds[0][axis]
            });
            if misses {
                continue;
            }
            let (first, rows) = *clip_rows.get_or_insert_with(|| {
                let first = clips.len() / (ZONE_ROW_TEXELS * 4);
                for stop in &zone.stops {
                    clips.extend([stop[0], stop[1], stop[2], 0., 0., 0., 0., 0.]);
                    clips.extend([0.; ZONE_ROW_TEXELS * 4 - 8]);
                }
                for hole in &zone.holes {
                    clips.extend([hole[0], hole[1], hole[2], hole[3], 1., 0., 0., 0.]);
                    clips.extend([0.; ZONE_ROW_TEXELS * 4 - 8]);
                }
                (first, zone.stops.len() + zone.holes.len())
            });
            let span = set.spans[0];
            for place in &places {
                segments.extend(segment.start);
                segments.push(segment.u);
                segments.extend(segment.along);
                segments.push(segment.length);
                segments.extend(zone.down);
                segments.push(zone.depth);
                segments.extend(segment.off);
                segments.push(zone.length);
                segments.extend([zone.id, zone.scale, first as f64, rows as f64]);
                segments.extend([*place as f64, segment.along_down, segment.determinant, 0.]);
                segments.extend([span[0], span[1], 0., 0.]);
                count += 1;
            }
        }
    }
    // Clip rows are numbered after the segments.
    let mut rows = segments;
    let shift = count as f64;
    for (index, value) in rows.iter_mut().enumerate() {
        if index % (ZONE_ROW_TEXELS * 4) == 18 {
            *value += shift;
        }
    }
    rows.extend(clips);
    (rows, count)
}

/// Check zone inputs for a consumer that never samples them.
pub(super) fn validate_zones(named: &[NamedFeature], total_segments: usize) -> Result<(), String> {
    // A zone can have fewer usable segments than its polyline (e.g. one parallel to down).
    // Only a receiver large enough to reach the allowance needs that geometric count.
    if total_segments > MAX_ZONE_SEGMENTS {
        return zone_sets(named).map(drop);
    }
    for feature in named {
        if let Some(zone) = &feature.zone {
            zone.check()
                .map_err(|message| format!("Feature '{}': {message}", feature.name))?;
        }
    }
    Ok(())
}

/// A receiver's zones by name, from the features that carry one.
pub(super) fn zone_sets(named: &[NamedFeature]) -> Result<Vec<(String, ZoneSet)>, String> {
    let mut by_name: BTreeMap<&str, ZoneSet> = BTreeMap::new();
    let mut segments = 0;
    for feature in named {
        if let Some(zone) = &feature.zone {
            let set = by_name.entry(&feature.name).or_default();
            let before = set.segments.len();
            set.add(feature, zone)?;
            segments += set.segments.len() - before;
            if segments > MAX_ZONE_SEGMENTS {
                return Err(format!(
                    "A receiver holds at most {MAX_ZONE_SEGMENTS} zone edge segments."
                ));
            }
        }
    }
    Ok(by_name
        .into_iter()
        .map(|(name, set)| (name.to_owned(), set))
        .collect())
}

/// A zone identity in [0, 1) from integers that name it (an owner's part, a copy, an edge), the
/// same on every platform (SplitMix64).
pub fn zone_id(words: &[u64]) -> f64 {
    let mut state = 0x5a0e_5a0e_u64;
    for word in words {
        state = state
            .wrapping_add(*word)
            .wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = state;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        state = z ^ (z >> 31);
    }
    (state >> 11) as f64 / (1u64 << 53) as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wall_zone(stops: Vec<[f64; 3]>, holes: Vec<[f64; 4]>) -> NamedFeature {
        // An edge 3 m up a wall in the plane y = 0 (facing -y), from x = 0 to 4.
        NamedFeature {
            name: "runoff".into(),
            points: vec![[0., 0., 3.], [4., 0., 3.]],
            closed: false,
            zone: Some(Zone {
                down: [0., 0., -1.],
                length: 2.,
                depth: 0.03,
                u0: 10.,
                id: 0.25,
                scale: 1.5,
                stops,
                holes,
            }),
        }
    }
    const FACING: [f64; 3] = [0., -1., 0.];

    #[test]
    fn a_zone_holds_only_what_hangs_below_its_edge() {
        let sets = zone_sets(&[wall_zone(vec![], vec![])]).unwrap();
        let set = &sets[0].1;
        let hit = set.at([1., 0., 2.5], FACING).0.unwrap();
        assert_eq!((hit.u, hit.v, hit.id, hit.scale), (11., 0.5, 0.25, 1.5));
        assert_eq!(set.at([1., 0., 3.], FACING).0.unwrap().v, 0.);
        // Above the edge, past its ends, beyond its length, off its surface or on a face that
        // does not face across it: nothing.
        for point in [[1., 0., 3.01], [4.1, 0., 2.], [-0.1, 0., 2.], [1., 0., 0.9]] {
            assert!(set.at(point, FACING).0.is_none(), "{point:?}");
        }
        assert!(set.at([1., 0.05, 2.], FACING).0.is_none());
        assert!(set.at([1., 0., 2.], [0., 0., 1.]).0.is_none());
    }

    #[test]
    fn stops_end_a_column_and_holes_leave_a_rectangle_out() {
        let set = &zone_sets(&[wall_zone(
            vec![[11., 12., 0.4]],
            vec![[12.5, 13., 0.2, 0.6]],
        )])
        .unwrap()[0]
            .1;
        assert!(set.at([1.5, 0., 2.7], FACING).0.is_some());
        assert!(set.at([1.5, 0., 2.5], FACING).0.is_none());
        assert_eq!(set.at([1.5, 0., 2.5 - 0.1], FACING).0, None);
        assert_eq!(set.at([2.2, 0., 2.5], FACING).0.unwrap().limit, 2.);
        assert!(set.at([2.7, 0., 2.5], FACING).0.is_none());
        assert!(set.at([2.7, 0., 2.3], FACING).0.is_some());
    }

    #[test]
    fn the_edge_nearest_above_wins_and_u_runs_on_round_a_bend() {
        let mut lower = wall_zone(vec![], vec![]);
        lower.points = vec![[0., 0., 2.], [4., 0., 2.]];
        lower.zone.as_mut().unwrap().id = 0.75;
        let set = &zone_sets(&[wall_zone(vec![], vec![]), lower]).unwrap()[0].1;
        assert_eq!(set.at([1., 0., 2.5], FACING).0.unwrap().id, 0.25);
        let hit = set.at([1., 0., 1.8], FACING).0.unwrap();
        assert!((hit.v - 0.2).abs() < 1e-12 && hit.id == 0.75);
        // A sloping edge: the drop is measured straight down to it.
        let mut sloping = wall_zone(vec![], vec![]);
        sloping.points = vec![[0., 0., 3.], [4., 0., 2.]];
        let hit = zone_sets(&[sloping]).unwrap()[0]
            .1
            .at([2., 0., 2.], FACING)
            .0
            .unwrap();
        assert!((hit.v - 0.5).abs() < 1e-12, "{hit:?}");
        assert!((hit.u - (10. + 2f64.hypot(0.5))).abs() < 1e-12, "{hit:?}");
        // A polyline round a corner in plan: u is its length from the start, on either wall.
        let mut corner = wall_zone(vec![], vec![]);
        corner.points = vec![[0., 0., 3.], [4., 0., 3.], [4., 3., 3.]];
        let set = &zone_sets(&[corner]).unwrap()[0].1;
        let hit = set.at([4., 1., 2.], [1., 0., 0.]).0.unwrap();
        assert!((hit.u - 15.).abs() < 1e-12 && (hit.v - 1.).abs() < 1e-12);
    }

    #[test]
    fn bad_zones_are_refused_and_ids_are_stable() {
        let mut bad = wall_zone(vec![], vec![]);
        bad.zone.as_mut().unwrap().down = [0., 0., -2.];
        assert!(validate_zones(std::slice::from_ref(&bad), 1).is_err());
        assert!(zone_sets(&[bad]).is_err());
        let mut bad = wall_zone(vec![], vec![]);
        bad.zone.as_mut().unwrap().length = 0.;
        assert!(zone_sets(&[bad]).is_err());
        assert_eq!(zone_id(&[1, 2]), zone_id(&[1, 2]));
        assert_ne!(zone_id(&[1, 2]), zone_id(&[2, 1]));
        assert!((0. ..1.).contains(&zone_id(&[u64::MAX])));
    }

    #[test]
    fn chart_admission_counts_only_usable_zone_segments() {
        let mut edge = wall_zone(vec![], vec![]);
        edge.points = (0..=MAX_ZONE_SEGMENTS + 1)
            .map(|i| [i as f64 * 0.01, 0., 0.])
            .collect();
        let count = edge.points.len() - 1;
        assert!(validate_zones(std::slice::from_ref(&edge), count).is_err());
        // The last leg follows gravity, so it contributes no zone edge.
        let previous = edge.points[count - 1];
        edge.points[count] = [previous[0], previous[1], previous[2] - 0.01];
        assert!(validate_zones(&[edge], count).is_ok());
    }
}

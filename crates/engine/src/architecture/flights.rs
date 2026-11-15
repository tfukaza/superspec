use super::*;

pub(super) struct Flight {
    pub floor: Profile,
    pub rails: [Vec<V3>; 2],
    pub guide: [Vec<V3>; 2],
    pub solids: Vec<Geometry>,
}

fn fractions(path: &[V3]) -> Result<Vec<f64>, String> {
    let values = stations(path);
    let length = *values.last().ok_or("Empty flight rail.")?;
    if length <= 1e-8 || values.windows(2).any(|w| w[1] - w[0] <= 1e-8) {
        return Err("Flight rails require nonzero plan segments.".into());
    }
    Ok(values.iter().map(|s| s / length).collect())
}
fn sample(path: &[V3], values: &[f64], u: f64) -> V3 {
    let i = values.partition_point(|v| *v < u - 1e-10);
    if i == 0 {
        return path[0];
    }
    if i == path.len() {
        return path[i - 1];
    }
    lerp(
        path[i - 1],
        path[i],
        (u - values[i - 1]) / (values[i] - values[i - 1]),
    )
}
fn ordered_profile(points: &[V3], length: f64, strict: bool) -> bool {
    points.len() >= 2
        && points[0][0].abs() < 1e-8
        && (points.last().unwrap()[0] - length).abs() < 1e-7
        && points.windows(2).all(|w| {
            if strict {
                w[1][0] > w[0][0]
            } else {
                w[1][0] >= w[0][0]
            }
        })
}
impl Compiler<'_> {
    pub(super) fn flight(&self, node: &Resolved) -> Result<Flight, Diagnostic> {
        let source = &node.source;
        let is_stair = source.kind == "stairs";
        let (rails, length) = if source.text("left").is_some()
            && source.text("right").is_some()
            && source.text("path").is_none()
            && source.optional_number("width").is_none()
            && source.text("width-profile").is_none()
        {
            let left = self.path(source, "left")?;
            let right = self.path(source, "right")?;
            let length = (stations(&left).last().unwrap() + stations(&right).last().unwrap()) / 2.;
            ([left, right], length)
        } else {
            if source.text("path").is_none()
                || source.text("left").is_some()
                || source.text("right").is_some()
                || source.optional_number("width").is_some()
                    == source.text("width-profile").is_some()
            {
                return Err(source.fail(
                    "AML_FLIGHT",
                    "Choose a center path and width or width-profile, or two explicit rails.",
                ));
            }
            let path = self.path(source, "path")?;
            let values = fractions(&path).at(source)?;
            let length = *stations(&path).last().unwrap();
            let widths = source
                .text("width-profile")
                .map(|_| self.path(source, "width-profile"))
                .transpose()?;
            if widths
                .as_ref()
                .is_some_and(|p| !ordered_profile(p, length, true) || p.iter().any(|p| p[1] <= 0.))
            {
                return Err(source.fail("AML_FLIGHT","Width profile must cover the flight with increasing stations and positive widths."));
            }
            let offsets = path_offsets(&path).at(source)?;
            let mut cuts: Vec<_> = values.iter().map(|u| u * length).collect();
            if let Some(widths) = &widths {
                cuts.extend(widths.iter().map(|p| p[0]));
            }
            cuts.sort_by(f64::total_cmp);
            cuts.dedup_by(|a, b| (*a - *b).abs() < 1e-10);
            let mut left = vec![];
            let mut right = vec![];
            for distance in cuts {
                let center = sample(&path, &values, distance / length);
                let width = if let Some(widths) = &widths {
                    height(widths, distance, true).at(source)?
                } else {
                    positive(source, "width", 0.)?
                };
                let offset = scale(normal_at(&path, &offsets, distance), width / 2.);
                left.push(add(center, offset));
                right.push(sub(center, offset));
            }
            ([left, right], length)
        };
        let [left, right] = &rails;
        let ls = fractions(left).at(source)?;
        let rs = fractions(right).at(source)?;
        if is_stair
            && (node.matrix.0[0][2].abs() > 1e-8
                || node.matrix.0[1][2].abs() > 1e-8
                || node.matrix.0[2][2] <= 0.)
        {
            return Err(source.fail(
                "AML_FLIGHT",
                "Stair placement must preserve horizontal treads and upward local Z.",
            ));
        }
        let start = left[0][2];
        let end = left.last().unwrap()[2];
        let rise = (end - start).abs();
        if (right[0][2] - start).abs() > 1e-7 || (right.last().unwrap()[2] - end).abs() > 1e-7 {
            return Err(source.fail("AML_FLIGHT", "Flight rails must share landing elevations."));
        }
        let thickness = positive(source, "thickness", 0.2)?;
        if (!is_stair
            && (source.optional_number("count").is_some()
                || source.optional_number("maximum-riser").is_some()))
            || (source.optional_number("count").is_some()
                && source.optional_number("maximum-riser").is_some())
        {
            return Err(source.fail(
                "AML_FLIGHT",
                "Choose stair count or maximum-riser; ramps accept neither.",
            ));
        }
        let maximum = positive(source, "maximum-riser", 0.18)?;
        if is_stair && rise <= 1e-8 {
            return Err(source.fail("AML_FLIGHT", "Stairs require nonzero rise."));
        }
        let count = if is_stair {
            source.number(
                "count",
                (rise * node.matrix.0[2][2] / maximum - 1e-9).ceil(),
            )
        } else {
            1.
        };
        if count.fract() != 0. || !(1.0..=10000.).contains(&count) {
            return Err(source.fail(
                "AML_COMPLEXITY",
                "Stair count must be an integer between one and 10000.",
            ));
        }
        let count = count as usize;
        let underside = source.text("underside").unwrap_or("filled");
        if is_stair && underside == "stepped" && thickness <= rise / count as f64 + 1e-8 {
            return Err(source.fail(
                "AML_FLIGHT",
                "Stepped thickness must exceed the equalized riser.",
            ));
        }
        let custom = source
            .text("underside-profile")
            .map(|_| self.path(source, "underside-profile"))
            .transpose()?;
        if (underside == "custom") != custom.is_some()
            || custom
                .as_ref()
                .is_some_and(|p| !ordered_profile(p, length, false))
        {
            return Err(source.fail(
                "AML_FLIGHT",
                "Custom underside requires a complete ordered longitudinal profile.",
            ));
        }
        let mut cuts = ls.clone();
        cuts.extend(&rs);
        cuts.extend((0..=count).map(|i| i as f64 / count as f64));
        if let Some(custom) = &custom {
            cuts.extend(custom.iter().map(|p| p[0] / length));
        }
        cuts.sort_by(f64::total_cmp);
        cuts.dedup_by(|a, b| (*a - *b).abs() < 1e-10);
        if !is_stair {
            let mut refined = vec![cuts[0]];
            for pair in cuts.windows(2) {
                let p = [
                    sample(left, &ls, pair[0]),
                    sample(right, &rs, pair[0]),
                    sample(right, &rs, pair[1]),
                    sample(left, &ls, pair[1]),
                ];
                let normal = cross(sub(p[1], p[0]), sub(p[2], p[0]));
                let planar =
                    dot(normal, sub(p[3], p[0])).abs() <= 1e-10 * length_of(normal).max(1.);
                let range = p.iter().map(|p| p[2]).fold(f64::NEG_INFINITY, f64::max)
                    - p.iter().map(|p| p[2]).fold(f64::INFINITY, f64::min);
                let n = if planar {
                    1.
                } else {
                    (range
                        / self
                            .resolution
                            .document
                            .root
                            .number("curve-tolerance", 0.01))
                    .ceil()
                    .max(1.)
                };
                if !n.is_finite() || refined.len() as f64 + n > 40000. {
                    return Err(source.fail("AML_COMPLEXITY", "Flight station budget exceeded."));
                }
                let n = n as usize;
                refined
                    .extend((1..=n).map(|i| pair[0] + (pair[1] - pair[0]) * i as f64 / n as f64));
            }
            cuts = refined;
        }
        if cuts.len() > 40000 {
            return Err(source.fail("AML_COMPLEXITY", "Flight station budget exceeded."));
        }
        let mut ring = left.clone();
        ring.extend(right.iter().rev());
        for i in 0..ring.len() {
            for j in i + 1..ring.len() {
                if crossing(
                    ring[i],
                    ring[(i + 1) % ring.len()],
                    ring[j],
                    ring[(j + 1) % ring.len()],
                ) {
                    return Err(source.fail("AML_FLIGHT", "Flight rails cross."));
                }
            }
        }
        let mut floor = Profile {
            points: vec![],
            triangles: vec![],
            loops: vec![ring.iter().map(|p| node.matrix.point(*p)).collect()],
        };
        let mut solids = vec![];
        let mut actual_rails = [vec![], vec![]];
        let mut guides = [vec![], vec![]];
        let mut total_area = 0.;
        for pair in cuts.windows(2) {
            let (u, v) = (pair[0], pair[1]);
            let corners = [
                sample(left, &ls, u),
                sample(right, &rs, u),
                sample(right, &rs, v),
                sample(left, &ls, v),
            ];
            let step = (((u + v) / 2. * count as f64).floor() as usize).min(count - 1);
            let z = if end > start {
                start + (step + 1) as f64 * rise / count as f64
            } else {
                start - step as f64 * rise / count as f64
            };
            let mut points: Vec<_> = corners
                .iter()
                .map(|p| [p[0], p[1], if is_stair { z } else { p[2] }])
                .collect();
            for (side, ids) in [[0, 3], [1, 2]].iter().enumerate() {
                for i in ids {
                    actual_rails[side].push(node.matrix.point(points[*i]));
                    guides[side].push(node.matrix.point(corners[*i]));
                }
            }
            if area(&points) < 0. {
                points.reverse();
            }
            let choices = [vec![[0, 1, 2], [0, 2, 3]], vec![[0, 1, 3], [1, 2, 3]]];
            let triangles = choices
                .into_iter()
                .find(|ts| {
                    ts.iter()
                        .all(|t| orient(points[t[0]], points[t[1]], points[t[2]]) > 1e-12)
                })
                .ok_or_else(|| {
                    source.fail("AML_FLIGHT", "Flight sections fold or have zero width.")
                })?;
            total_area += area(&points);
            let mut bottoms = vec![];
            for p in &points {
                let corner = corners
                    .iter()
                    .position(|q| hypot(p[0] - q[0], p[1] - q[1]) < 1e-7)
                    .unwrap();
                let at_end = corner >= 2;
                let fraction = if at_end { v } else { u };
                let mut bottom = p[2] - thickness;
                if is_stair && underside == "filled" {
                    bottom = start.min(end) - thickness;
                }
                if is_stair && underside == "sloped" {
                    bottom = corners[corner][2] - thickness;
                }
                if let Some(custom) = &custom {
                    bottom = height(custom, fraction * length, !at_end).at(source)?;
                }
                if bottom >= p[2] - 1e-8 {
                    return Err(source.fail("AML_FLIGHT", "Underside intersects the walking top."));
                }
                bottoms.push([p[0], p[1], bottom]);
            }
            let profile = Profile {
                points: points.clone(),
                triangles: triangles.clone(),
                loops: vec![points.clone()],
            };
            let geometry = Geometry::surface(&profile, |p| {
                bottoms[points.iter().position(|q| *q == p).unwrap()]
            })
            .transformed(node.matrix);
            solids.push(geometry);
            let base = floor.points.len();
            let placed = profile.transformed(node.matrix);
            floor.points.extend(placed.points);
            floor
                .triangles
                .extend(placed.triangles.iter().map(|t| t.map(|i| i + base)));
        }
        if (total_area - area(&ring).abs()).abs() > 1e-6_f64.max(total_area * 1e-8) {
            return Err(source.fail(
                "AML_FLIGHT",
                "Flight sections overlap or fail to cover their footprint.",
            ));
        }
        Ok(Flight {
            floor,
            rails: actual_rails,
            guide: guides,
            solids,
        })
    }
}
fn length_of(p: V3) -> f64 {
    length(p)
}

use super::*;

#[derive(Clone)]
struct Span {
    base: [V3; 2],
    top: [V3; 2],
    normals: [V3; 2],
    start: f64,
    end: f64,
}
impl Compiler<'_> {
    pub(super) fn enclosure(
        &mut self,
        index: usize,
        node: &Resolved,
    ) -> Result<Vec<Geometry>, Diagnostic> {
        let source = &node.source;
        let host = self
            .resolution
            .reference(index, source.text("host").unwrap())?;
        self.build(host)?;
        let host_node = &self.resolution.nodes[host];
        let inverse = host_node.matrix.inverse().at(source)?;
        let matrix = host_node
            .matrix
            .then(self.resolution.node_placement(source)?);
        let side = source.text("side").unwrap_or("left");
        let construction = source.text("construction").unwrap_or("wall");
        let mut original: Vec<([V3; 2], [V3; 2])> = vec![];
        if let Some(flight) = self.flights.get(&host) {
            if source.optional_number("edge").is_some() || source.optional_number("loop").is_some()
            {
                return Err(source.fail(
                    "AML_ENCLOSURE",
                    "Flight enclosures select a side, not a footprint edge.",
                ));
            }
            if side == "start" || side == "end" {
                let i = if side == "start" {
                    0
                } else {
                    flight.rails[0].len() - 1
                };
                original.push((
                    [flight.rails[0][i], flight.rails[1][i]].map(|p| inverse.point(p)),
                    [flight.guide[0][i], flight.guide[1][i]].map(|p| inverse.point(p)),
                ));
            } else {
                let side = usize::from(side == "right");
                for (base, guide) in flight.rails[side]
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .zip(flight.guide[side].as_chunks::<2>().0)
                {
                    original.push((
                        base.map(|p| inverse.point(p)),
                        guide.map(|p| inverse.point(p)),
                    ));
                }
            }
        } else {
            let floor = self.floors.get(&host).ok_or_else(|| {
                source.fail(
                    "AML_ENCLOSURE",
                    "Enclosures require a native flight or footprint host.",
                )
            })?;
            let edge = source.number("edge", -1.);
            let loop_id = source.number("loop", 0.);
            if edge < 0. || edge.fract() != 0. || loop_id < 0. || loop_id.fract() != 0. {
                return Err(source.fail(
                    "AML_ENCLOSURE",
                    "Footprint enclosures require a valid loop and edge.",
                ));
            }
            let ring = floor
                .loops
                .get(loop_id as usize)
                .ok_or_else(|| source.fail("AML_ENCLOSURE", "Unknown footprint loop."))?;
            if edge as usize >= ring.len() {
                return Err(source.fail("AML_ENCLOSURE", "Unknown footprint edge."));
            }
            let pair = [ring[edge as usize], ring[(edge as usize + 1) % ring.len()]]
                .map(|p| inverse.point(p));
            original.push((pair, pair));
        }
        let normal = |span: &([V3; 2], [V3; 2])| {
            unit([
                -(span.1[1][1] - span.1[0][1]),
                span.1[1][0] - span.1[0][0],
                0.,
            ])
        };
        let length: f64 = original
            .iter()
            .map(|(_, p)| hypot(p[1][0] - p[0][0], p[1][1] - p[0][1]))
            .sum();
        let start = source.number("start", 0.);
        let end = source.number("end", length);
        if start < 0. || end <= start || end > length + 1e-7 {
            return Err(source.fail(
                "AML_ENCLOSURE",
                "Enclosure span must lie inside its host edge.",
            ));
        }
        let thickness = positive(source, "thickness", 0.15)?;
        let height_default = positive(source, "height", 1.)?;
        let offset = source.number("offset", 0.);
        let top = source
            .text("top")
            .map(|_| self.path(source, "top"))
            .transpose()?;
        if top.as_ref().is_some_and(|p| {
            p[0][0] > start + 1e-7
                || p.last().unwrap()[0] < end - 1e-7
                || p.windows(2).any(|w| w[0][0] > w[1][0])
        }) {
            return Err(source.fail(
                "AML_ENCLOSURE",
                "Top profile must cover the selected span in ordered stations.",
            ));
        }
        let sign = if side == "right" || side == "start" {
            -1.
        } else {
            1.
        };
        let mut spans = vec![];
        let mut distance = 0.;
        for (i, span) in original.iter().enumerate() {
            let run = hypot(span.1[1][0] - span.1[0][0], span.1[1][1] - span.1[0][1]);
            let n = normal(span).at(source)?;
            let miter = |j: usize| -> Result<V3, Diagnostic> {
                let other = normal(&original[j]).at(source)?;
                let denominator = 1. + dot(n, other);
                if denominator < 0.1 {
                    return Err(source.fail("AML_ENCLOSURE", "Enclosure corner reverses or folds."));
                }
                Ok(scale(add(n, other), 1. / denominator))
            };
            let (a, b) = (start.max(distance), end.min(distance + run));
            if b > a + 1e-8 {
                let mut cuts = vec![a, b];
                if let Some(top) = &top {
                    cuts.extend(
                        top.iter()
                            .map(|p| p[0])
                            .filter(|s| *s > a + 1e-8 && *s < b - 1e-8),
                    );
                }
                cuts.sort_by(f64::total_cmp);
                cuts.dedup_by(|a, b| (*a - *b).abs() < 1e-8);
                for cut in cuts.windows(2) {
                    let mut bases = [[0.; 3]; 2];
                    let mut tops = bases;
                    let mut normals = bases;
                    for (j, &station) in cut.iter().enumerate() {
                        let t = (station - distance) / run;
                        let mut base = lerp(span.0[0], span.0[1], t);
                        let guide = lerp(span.1[0], span.1[1], t);
                        let normal = if (station - distance).abs() < 1e-8 {
                            miter(i.saturating_sub(1))?
                        } else if (station - distance - run).abs() < 1e-8 {
                            miter((i + 1).min(original.len() - 1))?
                        } else {
                            n
                        };
                        base = add(base, scale(normal, sign * offset));
                        let z = if let Some(top) = &top {
                            height(top, station, j == 0).at(source)?
                        } else {
                            (if construction == "curb" {
                                base[2]
                            } else {
                                guide[2]
                            }) + height_default
                        };
                        if z <= base[2] + 1e-8 {
                            return Err(
                                source.fail("AML_ENCLOSURE", "Enclosure top crosses its support.")
                            );
                        }
                        bases[j] = base;
                        tops[j] = [base[0], base[1], z];
                        normals[j] = normal;
                    }
                    spans.push(Span {
                        base: bases,
                        top: tops,
                        normals,
                        start: cut[0],
                        end: cut[1],
                    });
                }
            }
            distance += run;
        }
        let mut branches = vec![spans.clone()];
        for (key, span, i) in [
            ("start-return", &spans[0], 0),
            ("end-return", spans.last().unwrap(), 1),
        ] {
            if source.optional_number(key).is_some() {
                let amount = positive(source, key, 0.)?;
                let a = span.base[i];
                let normal = unit(span.normals[i]).at(source)?;
                let b = add(a, scale(normal, sign * amount));
                let side = unit([-(b[1] - a[1]), b[0] - a[0], 0.]).at(source)?;
                branches.push(vec![Span {
                    base: [a, b],
                    top: [span.top[i], [b[0], b[1], span.top[i][2]]],
                    normals: [side, side],
                    start: 0.,
                    end: amount,
                }]);
            }
        }
        let mut output = vec![];
        if construction != "railing" {
            if !source.children.is_empty() {
                return Err(source.fail(
                    "AML_ENCLOSURE",
                    "Rails and posts require railing construction.",
                ));
            }
            for branch in branches {
                for span in branch {
                    let sections = (0..2)
                        .map(|i| {
                            let a = sub(span.base[i], scale(span.normals[i], thickness / 2.));
                            let b = add(span.base[i], scale(span.normals[i], thickness / 2.));
                            vec![
                                a,
                                b,
                                [b[0], b[1], span.top[i][2]],
                                [a[0], a[1], span.top[i][2]],
                            ]
                        })
                        .collect::<Vec<_>>();
                    output.push(
                        Geometry::loft(&sections, &[[0, 1, 2], [0, 2, 3]])
                            .at(source)?
                            .transformed(matrix),
                    );
                }
            }
        } else {
            let rails: Vec<_> = source
                .children
                .iter()
                .filter(|n| n.kind == "rail")
                .collect();
            if rails.is_empty() {
                return Err(
                    source.fail("AML_ENCLOSURE", "Railing requires explicit rail profiles.")
                );
            }
            let posts: Vec<_> = source
                .children
                .iter()
                .filter(|n| n.kind == "posts")
                .collect();
            if posts.len() > 1 {
                return Err(source.fail("AML_ENCLOSURE", "Railing accepts one posts declaration."));
            }
            for branch in branches {
                for rail in &rails {
                    let profile = self.profile(rail)?;
                    let offset = rail.number("top-offset", 0.);
                    let mut path = vec![];
                    for span in &branch {
                        for p in span.top {
                            let point = [p[0], p[1], p[2] + offset];
                            if path.last().is_none_or(|p| length_of(sub(*p, point)) > 1e-8) {
                                path.push(point);
                            }
                        }
                    }
                    output.push(sweep(&profile, &path, matrix).at(source)?);
                }
                if let Some(posts) = posts.first() {
                    let profile = self.profile(posts)?;
                    let spacing = positive(posts, "maximum-spacing", 0.)?;
                    if profile.points.iter().any(|p| p[2].abs() > 1e-7) {
                        return Err(
                            source.fail("AML_ENCLOSURE", "Posts require a local XY profile.")
                        );
                    }
                    let (start, end) = (branch[0].start, branch.last().unwrap().end);
                    let count = ((end - start) / spacing).ceil() as usize;
                    if count > 10000 {
                        return Err(
                            source.fail("AML_COMPLEXITY", "Enclosure post budget exceeded.")
                        );
                    }
                    for i in 0..=count {
                        let station = start + (end - start) * i as f64 / count as f64;
                        let span = branch.iter().find(|s| station <= s.end + 1e-8).unwrap();
                        let t = (station - span.start) / (span.end - span.start);
                        let base = lerp(span.base[0], span.base[1], t);
                        let top = lerp(span.top[0], span.top[1], t);
                        let mut cap = profile.clone();
                        for p in &mut cap.points {
                            *p = [p[0] + base[0], p[1] + base[1], top[2]];
                        }
                        output.push(
                            Geometry::surface(&cap, |p| [p[0], p[1], base[2]]).transformed(matrix),
                        );
                    }
                }
            }
        }
        if output.iter().map(|g| g.triangles.len()).sum::<usize>() > 1000000 {
            return Err(source.fail("AML_COMPLEXITY", "Enclosure triangle budget exceeded."));
        }
        Ok(output)
    }
}
fn length_of(p: V3) -> f64 {
    length(p)
}

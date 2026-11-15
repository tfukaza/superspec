use super::*;

impl Compiler<'_> {
    pub(super) fn retaining(
        &mut self,
        index: usize,
        node: &Resolved,
    ) -> Result<Vec<Geometry>, Diagnostic> {
        let source = &node.source;
        let low = self.path(source, "lower")?;
        let mut high = self.path(source, "upper")?;
        if hypot(low[0][0] - high[0][0], low[0][1] - high[0][1]) > 1e-7 {
            high.reverse();
        }
        let stations = stations(&low);
        let total = stations.last().unwrap();
        if (total - super::stations(&high).last().unwrap()).abs() > 0.01 {
            return Err(source.fail(
                "AML_RETAINING",
                "Retaining boundaries must have matching plan lengths.",
            ));
        }
        let thickness = positive(source, "thickness", 0.22)?;
        let height = positive(source, "height", 3.)?;
        let offsets = path_offsets(&low)
            .at(source)?
            .iter()
            .map(|n| scale(*n, thickness / 2.))
            .collect::<Vec<_>>();
        if offsets.iter().any(|n| length(*n) > thickness * 2.) {
            return Err(source.fail("AML_RETAINING", "Retaining path turns too sharply."));
        }
        let mut lower = None;
        let mut upper = None;
        let mut upper_thickness = 0.;
        let mut filled_bottom = None;
        let inverse = node.matrix.inverse().at(source)?;
        for key in ["lower-floor", "upper-floor"] {
            if let Some(id) = source.text(key) {
                let reference = self.resolution.reference(index, id)?;
                self.build(reference)?;
                let host = &self.resolution.nodes[reference].source;
                let floor = self.floors.get(&reference).ok_or_else(|| {
                    source.fail(
                        "AML_REFERENCE",
                        "Retaining contact requires a floor or flight.",
                    )
                })?;
                if key == "lower-floor" {
                    if host.kind == "stairs" && host.text("underside") == Some("stepped") {
                        lower = Some(floor.transformed(inverse));
                    }
                } else {
                    upper_thickness =
                        inverse.direction(self.resolution.nodes[reference].matrix.direction([
                            0.,
                            0.,
                            self.floor_thickness.get(&reference).copied().unwrap_or(0.2),
                        ]))[2];
                    if host.kind == "stairs"
                        && host.text("underside").unwrap_or("filled") == "stepped"
                    {
                        upper = Some(floor.transformed(inverse));
                    }
                    if host.kind == "stairs"
                        && host.text("underside").unwrap_or("filled") == "filled"
                    {
                        filled_bottom = Some(
                            floor.transformed(inverse).loops[0]
                                .iter()
                                .map(|p| p[2])
                                .fold(f64::INFINITY, f64::min)
                                - upper_thickness,
                        );
                    }
                }
            }
        }
        let mut output = vec![];
        for i in 1..low.len() {
            let (a, b) = (low[i - 1], low[i]);
            let (h0, h1) = (at(&high, stations[i - 1]), at(&high, stations[i]));
            let lower_spans = lower
                .as_ref()
                .map(|p| p.boundary_spans(a, b))
                .unwrap_or_default();
            let upper_spans = upper
                .as_ref()
                .map(|p| p.boundary_spans(h0, h1))
                .unwrap_or_default();
            let mut cuts = vec![0., 1.];
            for span in lower_spans.iter().chain(&upper_spans) {
                cuts.extend([span[0].clamp(0., 1.), span[1].clamp(0., 1.)]);
            }
            cuts.sort_by(f64::total_cmp);
            cuts.dedup_by(|a, b| (*a - *b).abs() < 1e-9);
            for cut in cuts.windows(2) {
                let (start, end) = (cut[0], cut[1]);
                let midpoint = (start + end) / 2.;
                let low_span = lower_spans
                    .iter()
                    .find(|s| midpoint >= s[0] - 1e-9 && midpoint <= s[1] + 1e-9);
                let high_span = upper_spans
                    .iter()
                    .find(|s| midpoint >= s[0] - 1e-9 && midpoint <= s[1] + 1e-9);
                if (lower.is_some() && low_span.is_none())
                    || (upper.is_some() && high_span.is_none())
                {
                    return Err(source.fail(
                        "AML_RETAINING",
                        "Cannot resolve actual stepped retaining contact.",
                    ));
                }
                let mut a = lerp(a, b, start);
                let mut b = lerp(low[i - 1], b, end);
                let mut n0 = lerp(offsets[i - 1], offsets[i], start);
                let mut n1 = lerp(offsets[i - 1], offsets[i], end);
                if let Some(s) = low_span {
                    a[2] = s[2] + s[3] * start;
                    b[2] = s[2] + s[3] * end;
                }
                let offset = if source.text("mode") == Some("wall") {
                    height
                } else {
                    -upper_thickness
                };
                let mut z0 = filled_bottom.unwrap_or(lerp(h0, h1, start)[2] + offset);
                let mut z1 = filled_bottom.unwrap_or(lerp(h0, h1, end)[2] + offset);
                if let Some(s) = high_span {
                    z0 = s[2] + s[3] * start - upper_thickness;
                    z1 = s[2] + s[3] * end - upper_thickness;
                }
                let (g0, g1) = (z0 - a[2], z1 - b[2]);
                if source.text("mode").unwrap_or("open") == "open" {
                    if g0 <= 1e-7 && g1 <= 1e-7 {
                        continue;
                    }
                    if g0 < 0. {
                        let t = g0 / (g0 - g1);
                        a = lerp(a, b, t);
                        z0 = a[2];
                        n0 = lerp(n0, n1, t);
                    } else if g1 < 0. {
                        let t = g0 / (g0 - g1);
                        b = lerp(a, b, t);
                        z1 = b[2];
                        n1 = lerp(n0, n1, t);
                    }
                }
                if hypot(a[0] - lerp(h0, h1, start)[0], a[1] - lerp(h0, h1, start)[1]) > 0.01
                    && g0 >= 0.
                {
                    return Err(source.fail("AML_RETAINING", "Retaining paths differ in plan."));
                }
                for sign in [1., -1.] {
                    let mut base = vec![add(a, scale(n0, sign)), a, b, add(b, scale(n1, sign))];
                    let mut top = base
                        .iter()
                        .enumerate()
                        .map(|(j, p)| [p[0], p[1], if j < 2 { z0 } else { z1 }])
                        .collect::<Vec<_>>();
                    if area(&base) < 0. {
                        base.reverse();
                        top.reverse();
                    }
                    let profile = Profile {
                        points: top.clone(),
                        triangles: vec![[0, 1, 2], [0, 2, 3]],
                        loops: vec![top.clone()],
                    };
                    let mut geometry = Geometry::surface(&profile, |p| {
                        base[top.iter().position(|q| *q == p).unwrap()]
                    });
                    geometry.triangles.retain(|t| {
                        length(cross(
                            sub(geometry.points[t[1]], geometry.points[t[0]]),
                            sub(geometry.points[t[2]], geometry.points[t[0]]),
                        )) > 1e-12
                    });
                    output.push(geometry.transformed(node.matrix));
                }
            }
        }
        Ok(output)
    }
}

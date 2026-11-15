use super::*;
impl Compiler<'_> {
    pub(super) fn wall(
        &mut self,
        index: usize,
        node: &Resolved,
    ) -> Result<Vec<Geometry>, Diagnostic> {
        let source = &node.source;
        let path = self.path(source, "path")?;
        let thickness = positive(source, "thickness", 0.22)?;
        let height_default = positive(source, "height", 3.)?;
        let spanning = source.boolean("span-floors", false);
        if (spanning && source.optional_number("height").is_some())
            || (!spanning && source.text("top-floor").is_some())
        {
            return Err(source.fail(
                "AML_ANCHOR",
                "top-floor requires span-floors; spanning cannot also specify height.",
            ));
        }
        let mut cuts = stations(&path);
        let total = *cuts.last().unwrap();
        if total <= 1e-8 {
            return Err(source.fail("AML_GEOMETRY", "Wall path must have positive plan length."));
        }
        let base = if source.text("base").is_some() {
            Some(self.path(source, "base")?)
        } else {
            None
        };
        let top = if source.text("top").is_some() {
            Some(self.path(source, "top")?)
        } else {
            None
        };
        for profile in [&base, &top].into_iter().flatten() {
            if profile[0][0].abs() > 1e-7
                || (profile.last().unwrap()[0] - total).abs() > 1e-7
                || profile.windows(2).any(|p| p[0][0] > p[1][0])
            {
                return Err(source.fail(
                    "AML_GEOMETRY",
                    "Wall profile must cover the complete path in station order.",
                ));
            }
            cuts.extend(profile.iter().map(|p| p[0]));
        }
        let mut supports = BTreeMap::new();
        for key in ["base-floor", "contact-floor", "fit-floor", "top-floor"] {
            if let Some(id) = source.text(key) {
                let reference = self.resolution.reference(index, id)?;
                self.build(reference)?;
                let floor = self.floors.get(&reference).ok_or_else(|| {
                    source.fail(
                        "AML_REFERENCE",
                        format!("{key} does not reference a floor."),
                    )
                })?;
                supports.insert(key, (reference, floor.clone()));
            }
        }
        let inverse = node.matrix.inverse().at(source)?;
        let mut candidates = BTreeMap::new();
        if spanning {
            let floors: Vec<_> = self
                .resolution
                .nodes
                .iter()
                .enumerate()
                .filter(|(_, n)| {
                    ["slab", "landing", "roof", "canopy", "ramp"].contains(&n.source.kind.as_str())
                        && n.source.boolean("collidable", true)
                })
                .map(|(i, _)| i)
                .collect();
            for id in floors {
                self.build(id)?;
                if let Some(floor) = self.floors.get(&id) {
                    candidates.insert(id, floor.clone());
                }
            }
        }
        for (id, floor) in supports.values() {
            candidates.insert(*id, floor.clone());
        }
        // Triangle boundary stations make support checks continuous across holes
        // and creases instead of checking only the authored path endpoints.
        let original_stations = stations(&path);
        for i in 1..path.len() {
            let a = node.matrix.point(path[i - 1]);
            let b = node.matrix.point(path[i]);
            for floor in candidates.values() {
                for span in floor.boundary_spans(a, b) {
                    cuts.extend([span[0], span[1]].iter().map(|t| {
                        original_stations[i - 1]
                            + t * (original_stations[i] - original_stations[i - 1])
                    }));
                }
            }
        }
        let offsets = path_offsets(&path).at(source)?;
        let low = match source.text("alignment") {
            Some("left") => 0.,
            Some("right") => -thickness,
            _ => -thickness / 2.,
        };
        cuts.sort_by(f64::total_cmp);
        cuts.dedup_by(|a, b| (*a - *b).abs() < 1e-8);
        if spanning {
            let seed = node.matrix.point(at(&path, (cuts[0] + cuts[1]) / 2.));
            let mut hits = candidates
                .iter()
                .filter_map(|(id, p)| p.height_at(seed[0], seed[1]).map(|z| (*id, z)))
                .collect::<Vec<_>>();
            hits.sort_by(|a, b| a.1.total_cmp(&b.1));
            let base = supports.get("base-floor").map(|v| v.0);
            let top = supports.get("top-floor").map(|v| v.0);
            match (base, top) {
                (None, None) => {
                    if hits.len() != 2 {
                        return Err(source.fail(
                            "AML_ANCHOR",
                            "Spanning requires exactly two overlapping floors or explicit anchors.",
                        ));
                    }
                    for (key, id) in [("base-floor", hits[0].0), ("top-floor", hits[1].0)] {
                        supports.insert(key, (id, candidates[&id].clone()));
                    }
                }
                (Some(base), None) => {
                    let z = candidates[&base]
                        .height_at(seed[0], seed[1])
                        .ok_or_else(|| source.fail("AML_ANCHOR", "Wall leaves its base floor."))?;
                    let above = hits.iter().filter(|h| h.1 > z + 1e-7).collect::<Vec<_>>();
                    if above.len() != 1 {
                        return Err(
                            source.fail("AML_ANCHOR", "Cannot select a unique upper floor.")
                        );
                    }
                    let id = above[0].0;
                    supports.insert("top-floor", (id, candidates[&id].clone()));
                }
                (None, Some(top)) => {
                    let z = candidates[&top]
                        .height_at(seed[0], seed[1])
                        .ok_or_else(|| source.fail("AML_ANCHOR", "Wall leaves its top floor."))?;
                    let below = hits.iter().filter(|h| h.1 < z - 1e-7).collect::<Vec<_>>();
                    if below.len() != 1 {
                        return Err(
                            source.fail("AML_ANCHOR", "Cannot select a unique lower floor.")
                        );
                    }
                    let id = below[0].0;
                    supports.insert("base-floor", (id, candidates[&id].clone()));
                }
                _ => {}
            }
        }
        let soffit = supports.get("top-floor").map(|(id, floor)| {
            let displacement = self.resolution.nodes[*id].matrix.direction([
                0.,
                0.,
                self.floor_thickness.get(id).copied().unwrap_or(0.2),
            ]);
            let mut floor = floor.clone();
            for p in &mut floor.points {
                *p = sub(*p, displacement);
            }
            floor
        });
        let section = |station: f64, after: bool| -> Result<Vec<V3>, Diagnostic> {
            let center = at(&path, station);
            let normal = wall_normal(&path, &offsets, station);
            let world = node.matrix.point(center);
            let mut bottom = center[2]
                + if let Some(profile) = &base {
                    height(profile, station, after).at(source)?
                } else {
                    0.
                };
            if let Some((_, floor)) = supports.get("base-floor") {
                let z = floor.height_at(world[0], world[1]).ok_or_else(|| {
                    source.fail(
                        "AML_WALL_SUPPORT",
                        "Wall base has no declared floor support.",
                    )
                })?;
                bottom = inverse.point([world[0], world[1], z])[2];
            }
            let mut ceiling = (if supports.contains_key("base-floor") {
                bottom
            } else {
                center[2]
            }) + if let Some(profile) = &top {
                height(profile, station, after).at(source)?
            } else {
                height_default
            };
            if let Some(floor) = &soffit {
                let z = floor.height_at(world[0], world[1]).ok_or_else(|| {
                    source.fail("AML_WALL_SUPPORT", "Wall top has no declared soffit.")
                })?;
                ceiling = inverse.point([world[0], world[1], z])[2];
            }
            if let Some((_, floor)) = supports.get("contact-floor") {
                let z = floor.height_at(world[0], world[1]).ok_or_else(|| {
                    source.fail("AML_WALL_SUPPORT", "Wall contact is unsupported.")
                })?;
                if (node.matrix.point([center[0], center[1], bottom])[2] - z).abs() > 0.0010001 {
                    return Err(
                        source.fail("AML_WALL_SUPPORT", "Wall contact does not meet its floor.")
                    );
                }
            }
            if ceiling <= bottom {
                return Err(source.fail("AML_GEOMETRY", "Wall top must remain above its base."));
            }
            let mut points = vec![];
            for (side, z) in [
                (low, bottom),
                (low + thickness, bottom),
                (low + thickness, ceiling),
                (low, ceiling),
            ] {
                let mut p = node.matrix.point([
                    center[0] + normal[0] * side,
                    center[1] + normal[1] * side,
                    z,
                ]);
                if z == bottom
                    && let Some((_, floor)) = supports
                        .get("contact-floor")
                        .or_else(|| supports.get("fit-floor"))
                        .or_else(|| supports.get("base-floor"))
                    && let Some(z) =
                        foot_height(floor, node.matrix.point([center[0], center[1], bottom]), p)
                {
                    p[2] = z;
                }
                points.push(p);
            }
            Ok(points)
        };
        let mut output = vec![];
        for interval in cuts.windows(2) {
            section((interval[0] + interval[1]) / 2., true)?;
            if spanning
                && (source.text("base-floor").is_none() || source.text("top-floor").is_none())
            {
                let p = node
                    .matrix
                    .point(at(&path, (interval[0] + interval[1]) / 2.));
                let low = supports["base-floor"].1.height_at(p[0], p[1]).unwrap();
                let high = supports["top-floor"].1.height_at(p[0], p[1]).unwrap();
                let hits = candidates
                    .values()
                    .filter_map(|f| f.height_at(p[0], p[1]))
                    .collect::<Vec<_>>();
                let ambiguous =
                    if source.text("base-floor").is_none() && source.text("top-floor").is_none() {
                        hits.len() != 2
                    } else if source.text("top-floor").is_none() {
                        hits.iter().filter(|z| **z > low + 1e-7).count() != 1
                    } else {
                        hits.iter().filter(|z| **z < high - 1e-7).count() != 1
                    };
                if ambiguous {
                    return Err(source.fail(
                        "AML_ANCHOR",
                        "Floor selection becomes ambiguous along the wall.",
                    ));
                }
            }
        }
        for interval in cuts.windows(2) {
            let a = section(interval[0], true)?;
            let b = section(interval[1], false)?;
            let mut geometry = Geometry::loft(&[a, b], &[[0, 1, 2], [0, 2, 3]]).at(source)?;
            if node.matrix.determinant() < 0. {
                for triangle in &mut geometry.triangles {
                    triangle.swap(0, 2);
                }
            }
            output.push(geometry);
        }
        let mut resolved = vec![];
        for station in &cuts {
            let mut p = at(&path, *station);
            if let Some((_, floor)) = supports.get("base-floor") {
                let world = node.matrix.point(p);
                p = inverse.point([
                    world[0],
                    world[1],
                    floor.height_at(world[0], world[1]).unwrap(),
                ]);
            }
            resolved.push(p);
        }
        let mut lower = vec![];
        let mut upper = vec![];
        for (i, station) in cuts.iter().enumerate() {
            for after in [false, true] {
                let points = section(*station, after)?;
                lower.push([*station, inverse.point(points[0])[2] - resolved[i][2], 0.]);
                upper.push([*station, inverse.point(points[3])[2] - resolved[i][2], 0.]);
            }
        }
        lower.extend(upper.into_iter().rev());
        if !source.boolean("dynamic", false)
            && node.matrix.0[0][2].abs() < 1e-9
            && node.matrix.0[1][2].abs() < 1e-9
        {
            for i in [0, resolved.len() - 1] {
                let neighbor = if i == 0 { 1 } else { i - 1 };
                let point = node.matrix.point(resolved[i]);
                let adjacent = node.matrix.point(resolved[neighbor]);
                let direction =
                    unit([adjacent[0] - point[0], adjacent[1] - point[1], 0.]).at(source)?;
                let section = section(cuts[i], i == 0)?;
                self.incidents.push(joins::Incident {
                    node: index,
                    point,
                    direction,
                    thickness: thickness * length(node.matrix.direction([1., 0., 0.])),
                    bottom: section[0][2],
                    top: section[3][2],
                });
            }
        }
        self.wall_silhouettes.insert(index, lower);
        self.wall_offsets.insert(
            index,
            cuts.iter()
                .map(|s| wall_normal(&path, &offsets, *s))
                .collect(),
        );
        self.wall_paths.insert(index, resolved);
        if source.text("join") == Some("round") {
            for station in [0., total] {
                let reaches_opening = self
                    .resolution
                    .nodes
                    .iter()
                    .enumerate()
                    .filter(|(_, n)| n.source.kind == "opening")
                    .any(|(i, n)| {
                        self.resolution
                            .reference(i, n.source.text("host").unwrap())
                            .ok()
                            == Some(index)
                            && opening_shape(&n.source, &self.resolution).is_ok_and(|shape| {
                                let lo = shape.iter().map(|p| p[0]).fold(f64::INFINITY, f64::min);
                                let hi =
                                    shape.iter().map(|p| p[0]).fold(f64::NEG_INFINITY, f64::max);
                                station >= lo - 1e-7 && station <= hi + 1e-7
                            })
                    });
                if reaches_opening {
                    continue;
                }
                let section = section(station, station == 0.)?;
                let a = inverse.point(section[0]);
                let b = inverse.point(section[3]);
                let center = at(&path, station);
                let points = (0..16)
                    .map(|i| {
                        let angle = std::f64::consts::TAU * i as f64 / 16.;
                        [
                            center[0] + thickness / 2. * cos(angle),
                            center[1] + thickness / 2. * sin(angle),
                            b[2],
                        ]
                    })
                    .collect::<Vec<_>>();
                let profile = Profile {
                    points: points.clone(),
                    triangles: (1..15).map(|i| [0, i, i + 1]).collect(),
                    loops: vec![points],
                };
                output.push(
                    Geometry::surface(&profile, |p| [p[0], p[1], a[2]]).transformed(node.matrix),
                );
            }
        }
        Ok(output)
    }
}
pub(super) fn wall_normal(path: &[V3], offsets: &[V3], station: f64) -> V3 {
    let cuts = stations(path);
    let i = cuts.partition_point(|s| *s < station - 1e-7);
    if i == 0 {
        offsets[0]
    } else if i >= cuts.len() {
        *offsets.last().unwrap()
    } else {
        lerp(
            offsets[i - 1],
            offsets[i],
            ((station - cuts[i - 1]) / (cuts[i] - cuts[i - 1])).clamp(0., 1.),
        )
    }
}
fn foot_height(floor: &Profile, center: V3, point: V3) -> Option<f64> {
    if (floor.height_at(center[0], center[1])? - center[2]).abs() > 0.0010001 {
        return None;
    }
    if let Some(z) = floor.height_at(point[0], point[1]) {
        return Some(z);
    }
    for t in &floor.triangles {
        let p = t.map(|i| floor.points[i]);
        let den = orient(p[0], p[1], p[2]);
        if den.abs() < 1e-7 {
            continue;
        }
        let weights = |q| {
            [
                orient(q, p[1], p[2]) / den,
                orient(p[0], q, p[2]) / den,
                orient(p[0], p[1], q) / den,
            ]
        };
        if weights(center).iter().any(|w| *w < -1e-7) {
            continue;
        }
        let w = weights(point);
        return Some((0..3).map(|i| w[i] * p[i][2]).sum());
    }
    None
}

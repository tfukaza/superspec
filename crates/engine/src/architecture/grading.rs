use super::*;
use superspec_geometry::grading::{
    self, Budget, Cell, Ribbon, bounds, crossings, overlap_area, overlaps,
};

impl Compiler<'_> {
    fn ribbon(&self, node: &Resolved, budget: &mut Budget) -> Result<Ribbon, Diagnostic> {
        let source = &node.source;
        let path = self.path(source, "path")?;
        let width = positive(source, "width", 1.)?;
        let transition = positive(source, "transition", 1.)?;
        let cross_slope = source.number("cross-slope", 0.);
        if source
            .optional_number("order")
            .is_some_and(|v| v.fract() != 0.)
        {
            return Err(source.fail("AML_GRADE", "Grading order must be an integer."));
        }
        let matrix = self.resolution.node_placement(source)?;
        if hypot(matrix.0[2][0], matrix.0[2][1]) > 1e-8 {
            return Err(source.fail(
                "AML_GRADE",
                "Grading placement must preserve the terrain vertical axis.",
            ));
        }
        let length = *stations(&path).last().unwrap();
        let width_profile = source
            .text("width-profile")
            .map(|_| self.path(source, "width-profile"))
            .transpose()?;
        if width_profile.as_ref().is_some_and(|p| {
            p[0][0] != 0.
                || p.last().unwrap()[0] < length - 1e-7
                || p.iter().any(|p| p[1] <= 0.)
                || p.windows(2).any(|w| w[1][0] <= w[0][0])
        }) {
            return Err(source.fail(
                "AML_GRADE",
                "Width profile must cover the path with positive widths and increasing stations.",
            ));
        }
        let width_at = |s, after| -> Result<f64, Diagnostic> {
            if let Some(p) = &width_profile {
                height(p, s, after).at(source)
            } else {
                Ok(width)
            }
        };
        let mut cuts = stations(&path);
        if cuts.windows(2).any(|w| w[1] - w[0] <= 1e-9) {
            return Err(source.fail("AML_GRADE", "Grading path has zero plan run."));
        }
        if let Some(p) = &width_profile {
            cuts.extend(p.iter().map(|p| p[0]).filter(|s| *s >= 0. && *s <= length));
        }
        cuts.sort_by(f64::total_cmp);
        cuts.dedup();
        let path: Vec<_> = cuts.iter().map(|s| at(&path, *s)).collect();
        let normals = path_offsets(&path).at(source)?;
        let mut rows = vec![];
        for (i, p) in path.iter().enumerate() {
            let half = width_at(cuts[i], true)? / 2.;
            let mut row = vec![];
            for (j, offset) in [-half - transition, -half, 0., half, half + transition]
                .iter()
                .enumerate()
            {
                row.push((
                    [
                        p[0] + normals[i][0] * offset,
                        p[1] + normals[i][1] * offset,
                        p[2] + cross_slope * offset.clamp(-half, half),
                    ],
                    if j == 0 || j == 4 { 0. } else { 1. },
                ));
            }
            rows.push(row);
        }
        let start = source.number("start-transition", width_at(0., true)? / 2.);
        let end = source.number("end-transition", width_at(length, false)? / 2.);
        if start <= 0. || end <= 0. {
            return Err(source.fail(
                "AML_GRADE",
                "Grading endpoint transitions must be positive.",
            ));
        }
        let first = unit([path[1][0] - path[0][0], path[1][1] - path[0][1], 0.]).at(source)?;
        let last = unit([
            path.last().unwrap()[0] - path[path.len() - 2][0],
            path.last().unwrap()[1] - path[path.len() - 2][1],
            0.,
        ])
        .at(source)?;
        let first_row = rows[0]
            .iter()
            .map(|(p, _)| (add(*p, scale(first, -start)), 0.))
            .collect();
        let last_row = rows
            .last()
            .unwrap()
            .iter()
            .map(|(p, _)| (add(*p, scale(last, end)), 0.))
            .collect();
        rows.insert(0, first_row);
        rows.push(last_row);
        let mut cells = vec![];
        for pair in rows.windows(2) {
            for i in 0..4 {
                let values = [pair[0][i], pair[1][i], pair[1][i + 1], pair[0][i + 1]];
                let points = values.map(|(p, _)| matrix.point(p));
                let weights = values.map(|(_, w)| w);
                let sign = orient(points[0], points[1], points[2]).signum();
                for ids in [[0, 1, 2], [0, 2, 3]] {
                    budget.spend().at(source)?;
                    let p = ids.map(|i| points[i]);
                    if orient(p[0], p[1], p[2]) * sign <= 1e-12 {
                        return Err(
                            source.fail("AML_GRADE", "Grading ribbon folds across its width.")
                        );
                    }
                    cells.push(Cell {
                        points: p,
                        weights: ids.map(|i| weights[i]),
                    });
                }
            }
        }
        for i in 0..cells.len() {
            for j in i + 1..cells.len() {
                budget.spend().at(source)?;
                if overlaps(bounds(&cells[i].points), bounds(&cells[j].points))
                    && overlap_area(cells[i].points, cells[j].points) > 1e-9
                {
                    return Err(source.fail("AML_GRADE", "Grading ribbon overlaps itself."));
                }
            }
        }
        Ok(Ribbon {
            cells,
            path: path.iter().map(|p| matrix.point(*p)).collect(),
        })
    }
    pub(super) fn grade_terrain(
        &self,
        profile: &Profile,
        nodes: &[&Resolved],
    ) -> Result<Profile, Diagnostic> {
        let mut budget = Budget::default();
        let mut grades = nodes
            .iter()
            .map(|node| self.ribbon(node, &mut budget).map(|r| (*node, r)))
            .collect::<Result<Vec<_>, _>>()?;
        for i in 0..grades.len() {
            for j in i + 1..grades.len() {
                let (a, b) = (&grades[i], &grades[j]);
                let mut overlap = false;
                for x in a.1.path.windows(2) {
                    for y in b.1.path.windows(2) {
                        budget.spend().at(&a.0.source)?;
                        for [t, u] in crossings(x[0], x[1], y[0], y[1]) {
                            if (lerp(x[0], x[1], t)[2] - lerp(y[0], y[1], u)[2]).abs() > 1e-6 {
                                let mut d = a.0.source.fail(
                                    "AML_GRADE_JUNCTION",
                                    "Intersecting grade centers have contradictory elevations.",
                                );
                                d.objects.push(b.0.id.clone());
                                return Err(d);
                            }
                        }
                    }
                }
                for x in &a.1.cells {
                    for y in &b.1.cells {
                        budget.spend().at(&a.0.source)?;
                        if overlaps(bounds(&x.points), bounds(&y.points))
                            && overlap_area(x.points, y.points) > 1e-10
                        {
                            overlap = true;
                        }
                    }
                }
                if overlap
                    && (a.0.source.optional_number("order").is_none()
                        || b.0.source.optional_number("order").is_none()
                        || a.0.source.optional_number("order")
                            == b.0.source.optional_number("order"))
                {
                    let mut d = a.0.source.fail(
                        "AML_GRADE_ORDER",
                        "Overlapping grading influences require distinct explicit orders.",
                    );
                    d.objects.push(b.0.id.clone());
                    return Err(d);
                }
            }
        }
        grades.sort_by(|a, b| {
            a.0.source
                .number("order", 0.)
                .total_cmp(&b.0.source.number("order", 0.))
                .then(a.0.id.cmp(&b.0.id))
        });
        let mut profile = profile.clone();
        for (node, ribbon) in grades {
            profile = grading::apply(
                &profile,
                &ribbon,
                self.resolution
                    .document
                    .root
                    .number("curve-tolerance", 0.01),
                &mut budget,
            )
            .at(&node.source)?;
        }
        Ok(profile)
    }
}

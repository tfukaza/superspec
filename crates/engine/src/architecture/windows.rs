//! A rigid casement seats in the same local opening plane as the native through cutter.
use super::*;

#[derive(Clone, Copy)]
pub(super) struct Fit {
    pub matrix: Matrix,
    pub width: f64,
    pub height: f64,
}

pub(super) fn opening_references(
    resolution: &Resolution,
) -> Result<BTreeMap<usize, usize>, Diagnostic> {
    let mut uses = BTreeMap::new();
    for (index, node) in resolution
        .nodes
        .iter()
        .enumerate()
        .filter(|(_, n)| n.source.kind == "window")
    {
        let source = &node.source;
        let opening =
            resolution.reference(index, source.text("opening").expect("required reference"))?;
        let aperture = &resolution.nodes[opening].source;
        if aperture.kind != "opening"
            || aperture.string("mode", "through") != "through"
            || aperture.string("shape", "rectangle") != "rectangle"
        {
            return Err(source.fail(
                "AML_WINDOW",
                "A window requires a rectangular through opening.",
            ));
        }
        let host = resolution.reference(opening, aperture.text("host").expect("required host"))?;
        if !matches!(resolution.nodes[host].source.kind.as_str(), "wall" | "box") {
            return Err(source.fail("AML_WINDOW", "The opening must belong to a wall or a box."));
        }
        if uses.insert(opening, index).is_some() {
            return Err(source.fail("AML_WINDOW", "This opening already has a window."));
        }
        let mut parent = node.parent;
        while let Some(i) = parent {
            if matches!(
                resolution.nodes[i].source.kind.as_str(),
                "subtract" | "intersect" | "union"
            ) {
                return Err(source.fail(
                    "AML_WINDOW",
                    "Place the window outside boolean operands and reference its cut opening.",
                ));
            }
            parent = resolution.nodes[i].parent;
        }
    }
    Ok(uses)
}

impl Compiler<'_> {
    pub(super) fn fit_window(
        &self,
        index: usize,
        opening: usize,
        host: usize,
        uncut: &Solid,
        cutter: &Solid,
    ) -> Result<Fit, Diagnostic> {
        let source = &self.resolution.nodes[index].source;
        let aperture = &self.resolution.nodes[opening].source;
        let host_node = &self.resolution.nodes[host];
        let width = aperture.number("width", 1.);
        let height = aperture.number("height", 2.);
        if !(0.4..=4.0).contains(&width) || !(0.4..=3.0).contains(&height) {
            return Err(source.fail(
                "AML_WINDOW",
                "Window width must be 0.4–4 m and height 0.4–3 m.",
            ));
        }
        let [x, z] = aperture.vector("center", [0.; 2]);
        let mut local = self
            .resolution
            .node_placement(aperture)?
            .then(Matrix::translation([x, 0., z - height / 2.]));
        if local.0[0][1].abs() > 1e-7
            || local.0[2][1].abs() > 1e-7
            || local.0[1][0].abs() > 1e-7
            || local.0[1][2].abs() > 1e-7
        {
            return Err(source.fail(
                "AML_WINDOW",
                "The window opening must lie in the host face plane.",
            ));
        }
        let (path_frame, front, thickness) = if host_node.source.kind == "wall" {
            let path = &self.wall_paths[&host];
            let cuts = stations(path);
            let extent = [-width / 2., width / 2.]
                .into_iter()
                .flat_map(|x| [0., height].map(|z| local.point([x, 0., z])[0]))
                .fold([f64::INFINITY, f64::NEG_INFINITY], |[a, b], v| {
                    [a.min(v), b.max(v)]
                });
            if extent[0] < 0. || extent[1] > *cuts.last().unwrap() {
                return Err(source.fail("AML_WINDOW", "Window extends beyond its wall path."));
            }
            let station = (extent[0] + extent[1]) / 2.;
            let segment = cuts
                .partition_point(|v| *v < station)
                .clamp(1, path.len() - 1);
            let tangent = scale(
                sub(path[segment], path[segment - 1]),
                1. / (cuts[segment] - cuts[segment - 1]),
            );
            let frame = Matrix([
                tangent,
                [-tangent[1], tangent[0], 0.],
                [0., 0., 1.],
                sub(path[segment], scale(tangent, cuts[segment])),
            ]);
            for s in cuts
                .iter()
                .copied()
                .filter(|s| *s > extent[0] && *s < extent[1])
                .chain(extent)
            {
                if length(sub(frame.point([s, 0., 0.]), at(path, s))) > 1e-6 {
                    return Err(source.fail(
                        "AML_WINDOW",
                        "A rigid window cannot span a change of wall direction.",
                    ));
                }
            }
            let thickness = host_node.source.number("thickness", 0.22);
            let front = match host_node.source.text("alignment") {
                Some("left") => 0.,
                Some("right") => -thickness,
                _ => -thickness / 2.,
            };
            (frame, front, thickness)
        } else {
            let depth = host_node.source.number("depth", 1.);
            let front = if host_node.source.text("anchor") == Some("bottom-center") {
                -depth / 2.
            } else {
                0.
            };
            (Matrix::default(), front, depth)
        };
        let receiver = host_node.matrix.then(path_frame);
        let mut probe_frame = local;
        probe_frame.0[1] = [0., 1., 0.];
        probe_frame.0[3][1] = front;
        let probe = Solid::cube([width, thickness, height])
            .at(source)?
            .transform(
                receiver
                    .then(probe_frame)
                    .then(Matrix::translation([-width / 2., 0., 0.])),
            )
            .at(source)?;
        let volume = probe.volume();
        for support in [uncut, cutter] {
            let missing = probe.boolean(support, Operation::Difference).at(source)?;
            if missing.volume() > (volume * 1e-4).max(1e-8) {
                return Err(source.fail("AML_WINDOW","The complete window rectangle must fit inside its host and pass through both faces."));
            }
        }
        local.0[3][1] = front + (0.035_f64).min(thickness / 2.);
        Ok(Fit {
            matrix: receiver.then(local),
            width,
            height,
        })
    }

    pub(super) fn window_collision(
        &mut self,
        index: usize,
        source: &Node,
    ) -> Result<Vec<Solid>, Diagnostic> {
        let opening = self
            .resolution
            .reference(index, source.text("opening").expect("required reference"))?;
        let host = self.resolution.reference(
            opening,
            self.resolution.nodes[opening]
                .source
                .text("host")
                .expect("required host"),
        )?;
        self.build(host)?;
        let fit = self.window_fits.get(&index).ok_or_else(|| {
            source.fail("AML_WINDOW", "Opening has no physical receiving surface.")
        })?;
        let Fit {
            matrix,
            width: w,
            height: h,
        } = *fit;
        let owner = self.owner(index, None)?;
        let make = |position: V3, size: V3, angle: f64| -> Result<Solid, Diagnostic> {
            Solid::cube(size)
                .at(source)?
                .owned(owner)
                .at(source)?
                .transform(
                    matrix.then(Matrix::placement(position, [0., 0., angle], [1.; 3]).at(source)?),
                )
                .at(source)
        };
        let face = 0.046;
        Ok(vec![
            make([-w / 2., -0.036, 0.], [face, 0.072, h], 0.)?,
            make([w / 2. - face, -0.036, 0.], [face, 0.072, h], 0.)?,
            make(
                [-w / 2. + face, -0.036, 0.],
                [w - 2. * face, 0.072, face],
                0.,
            )?,
            make(
                [-w / 2. + face, -0.036, h - face],
                [w - 2. * face, 0.072, face],
                0.,
            )?,
            make(
                [-w / 2. + 0.048, -0.040, 0.048],
                [w - 0.096, 0.032, h - 0.096],
                if source.boolean("open", false) {
                    -90.
                } else {
                    0.
                },
            )?,
        ])
    }
}

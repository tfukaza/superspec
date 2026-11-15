//! Modular plumbing, with one periodic appearance chart per barrel, elbow or sleeve.
use super::{
    fasteners, fixture::Fixture, metal, pen::Tint, pipe_runoff, wall::finish_seed, weathered,
};
use std::f64::consts::PI;
use superspec_geometry::{
    V3,
    math::{acos, add, cos, cross, dot, length, scale, sin, sub, unit},
};

#[derive(Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Options {
    pub radius: f64,
    pub color: V3,
    pub sides: usize,
    pub variant: String,
    pub weathering: Option<weathered::Settings>,
}
pub struct PipeSystem {
    options: Options,
}
#[derive(Clone, Copy)]
struct Ring {
    center: V3,
    radial: V3,
    tangent: V3,
    distance: f64,
}
fn rounded(value: f64) -> f64 {
    super::mesh::rounded(value, 1e6)
}
fn tan(value: f64) -> f64 {
    sin(value) / cos(value)
}
fn perpendicular(axis: V3) -> Result<V3, String> {
    unit(cross(
        axis,
        if axis[2].abs() > 0.9 {
            [1., 0., 0.]
        } else {
            [0., 0., 1.]
        },
    ))
}
fn point(p: V3) -> Result<(), String> {
    if p.iter().any(|v| !v.is_finite() || v.abs() > 10000.) {
        Err("Pipe positions must be finite and within 10 km of their local origin.".into())
    } else {
        Ok(())
    }
}
impl PipeSystem {
    pub fn new(options: Options) -> Result<Self, String> {
        if !options.radius.is_finite()
            || !(0.00001..=10.).contains(&options.radius)
            || options.color.iter().any(|v| !v.is_finite() || *v < 0.)
            || !(8..=64).contains(&options.sides)
            || options.variant.len() > 4096
        {
            return Err(
                "Pipes require a bounded positive radius, nonnegative color and 8–64 sides.".into(),
            );
        }
        if let Some(s) = &options.weathering
            && ([s.wear, s.grime, s.rust]
                .iter()
                .any(|v| !v.is_finite() || !(0. ..=1.).contains(v))
                || !s.texture_density.is_finite()
                || !(0. ..=4096.).contains(&s.texture_density))
        {
            return Err("Pipe weathering requires normalized factors and bounded density.".into());
        }
        Ok(Self { options })
    }
    fn socket_length(&self) -> f64 {
        self.options.radius * 1.4
    }
    fn strap_length(&self) -> f64 {
        (self.options.radius * 0.65).max(0.024)
    }

    pub fn straight(
        &self,
        drawing: &mut Fixture<'_, '_>,
        a: V3,
        b: V3,
        seams: &[f64],
    ) -> Result<(), String> {
        point(a)?;
        point(b)?;
        let distance = length(sub(b, a));
        if !(1e-6..=1000.).contains(&distance)
            || seams.len() > 4096
            || seams
                .iter()
                .any(|s| !s.is_finite() || *s < 0. || *s > distance)
        {
            return Err(
                "Pipe barrel needs distinct endpoints within 1 km and bounded seams on its axis."
                    .into(),
            );
        }
        let tangent = unit(sub(b, a))?;
        let radial = perpendicular(tangent)?;
        self.sweep(
            drawing,
            &[
                Ring {
                    center: a,
                    radial,
                    tangent,
                    distance: 0.,
                },
                Ring {
                    center: b,
                    radial,
                    tangent,
                    distance,
                },
            ],
            self.options.radius,
            seams,
            None,
        )
    }
    pub fn bend(
        &self,
        drawing: &mut Fixture<'_, '_>,
        a: V3,
        corner: V3,
        b: V3,
        socketed: bool,
    ) -> Result<(), String> {
        for p in [a, corner, b] {
            point(p)?;
        }
        let incoming = sub(corner, a);
        let outgoing = sub(b, corner);
        let setback = length(incoming);
        if setback < 1e-6 || (setback - length(outgoing)).abs() > 1e-6 {
            return Err("Elbow tangent setbacks must be equal and positive.".into());
        }
        let start = unit(incoming)?;
        let end = unit(outgoing)?;
        let angle = acos(dot(start, end).clamp(-1., 1.));
        let radius = setback / tan(angle / 2.);
        if !(1e-4..=PI - 1e-4).contains(&angle)
            || radius <= self.options.radius * 1.05
            || radius * angle > 1000.
        {
            return Err("Elbow must turn without reversing and clear its bore within 1 km.".into());
        }
        let toward_center = cross(unit(cross(start, end))?, start);
        let center = add(a, scale(toward_center, radius));
        let radial = scale(toward_center, -1.);
        let steps = ((angle / (PI / 24.)).ceil() as usize).max(4);
        let rings: Vec<_> = (0..=steps)
            .map(|i| {
                let theta = angle * i as f64 / steps as f64;
                let direction = add(scale(radial, cos(theta)), scale(start, sin(theta)));
                Ring {
                    center: add(center, scale(direction, radius)),
                    radial: direction,
                    tangent: add(scale(start, cos(theta)), scale(radial, -sin(theta))),
                    distance: radius * theta,
                }
            })
            .collect();
        let half = self.socket_length() / 2.;
        let seams: Vec<_> = [half, radius * angle - half]
            .into_iter()
            .filter(|at| socketed && *at > 0. && *at < radius * angle)
            .collect();
        self.sweep(drawing, &rings, self.options.radius, &seams, None)
    }
    pub fn coupling(
        &self,
        drawing: &mut Fixture<'_, '_>,
        center: V3,
        axis: V3,
    ) -> Result<(), String> {
        point(center)?;
        point(axis)?;
        if length(axis) < 1e-6 {
            return Err("Coupling axis must be nonzero.".into());
        }
        self.sleeve(
            drawing,
            center,
            unit(axis)?,
            self.options.radius * 1.23,
            self.socket_length(),
        )
    }
    pub fn mount(
        &self,
        drawing: &mut Fixture<'_, '_>,
        center: V3,
        axis: V3,
        wall: V3,
    ) -> Result<(), String> {
        for p in [center, axis, wall] {
            point(p)?;
        }
        let r = self.options.radius;
        if length(axis) < 1e-6 || length(sub(center, wall)) < r * 1.15 {
            return Err("Pipe mount requires a nonzero axis and wall clearance.".into());
        }
        let tangent = unit(axis)?;
        let out = unit(sub(center, wall))?;
        if dot(tangent, out).abs() > 1e-4 {
            return Err("Pipe mount axis must run parallel to its wall.".into());
        }
        let across = unit(cross(tangent, out))?;
        self.sleeve(drawing, center, tangent, r * 1.12, self.strap_length())?;
        self.member(
            drawing,
            add(wall, scale(out, 0.006)),
            add(center, scale(out, -r * 0.9)),
            across,
            r * 0.6,
            (r * 0.45).max(0.018),
        )?;
        let plate = add(wall, scale(out, 0.006));
        self.member(
            drawing,
            add(plate, scale(across, -r * 1.9)),
            add(plate, scale(across, r * 1.9)),
            tangent,
            (r * 0.9).max(0.045),
            0.012,
        )?;
        for sign in [-1., 1.] {
            fasteners::bolt(
                drawing,
                "metal",
                add(add(wall, scale(out, 0.012)), scale(across, sign * r * 1.5)),
                out,
                (r * 0.14).max(0.006),
                self.options.color,
                self.options.weathering.as_ref(),
            )?;
        }
        Ok(())
    }
    fn member(
        &self,
        drawing: &mut Fixture<'_, '_>,
        a: V3,
        b: V3,
        side: V3,
        width: f64,
        depth: f64,
    ) -> Result<(), String> {
        let pen = match &self.options.weathering {
            Some(s) => drawing.pen.with_weathering(
                drawing.mesh,
                s.clone(),
                metal::Kind::Frame,
                None,
                vec!["metal".into()],
            )?,
            None => drawing.pen.clone(),
        };
        pen.beveled_bar(
            drawing.mesh,
            "metal",
            a,
            b,
            side,
            width,
            depth,
            if self.options.weathering.is_some() {
                0.0015
            } else {
                0.
            },
            &Tint::Color(self.options.color),
            drawing.repeat("metal"),
        )
    }
    /// Validates the complete route before emission. On later cancellation/resource errors discard the detached drawing.
    pub fn run(
        &self,
        drawing: &mut Fixture<'_, '_>,
        path: &[V3],
        wall_y: Option<f64>,
    ) -> Result<(), String> {
        if !(2..=128).contains(&path.len())
            || wall_y.is_some_and(|y| !y.is_finite() || y.abs() > 10000.)
        {
            return Err("Pipe route requires 2–128 points and a finite bounded wall plane.".into());
        }
        for p in path {
            point(*p)?;
        }
        if path
            .windows(2)
            .map(|w| length(sub(w[1], w[0])))
            .sum::<f64>()
            > 1000.
        {
            return Err("Pipe route exceeds 1 km.".into());
        }
        let route: Vec<_> = path
            .iter()
            .enumerate()
            .filter(|(i, p)| *i == 0 || length(sub(**p, path[i - 1])) > 1e-6)
            .map(|(_, p)| *p)
            .collect();
        if route.len() < 2 {
            return Err("Pipe route needs distinct points.".into());
        }
        let mut starts = route.clone();
        let mut ends = route.clone();
        let mut bends = Vec::new();
        for i in 1..route.len() - 1 {
            let before = sub(route[i], route[i - 1]);
            let after = sub(route[i + 1], route[i]);
            let a = unit(before)?;
            let b = unit(after)?;
            let angle = acos(dot(a, b).clamp(-1., 1.));
            if angle < 1e-4 {
                continue;
            }
            let setback = (self.options.radius * 2.5 * tan(angle / 2.))
                .min(length(before) * 0.4)
                .min(length(after) * 0.4);
            if angle > PI - 1e-4 || setback / tan(angle / 2.) <= self.options.radius * 1.05 {
                return Err("Pipe route corner is too tight for its diameter.".into());
            }
            ends[i] = add(route[i], scale(a, -setback));
            starts[i] = add(route[i], scale(b, setback));
            bends.push((ends[i], route[i], starts[i]));
        }
        for i in 1..route.len().saturating_sub(2) {
            if length(sub(starts[i], route[i])) > 1e-6
                && length(sub(ends[i + 1], route[i + 1])) > 1e-6
                && length(sub(ends[i + 1], starts[i])) < self.socket_length()
            {
                return Err("Pipe route leaves insufficient room between elbow sockets.".into());
            }
        }
        for i in 0..route.len() - 1 {
            let a = starts[i];
            let b = ends[i + 1];
            let distance = length(sub(b, a));
            let axis = unit(sub(b, a))?;
            let mut mounts = Vec::new();
            let mut seams = Vec::new();
            if wall_y
                .is_some_and(|y| axis[2].abs() > 0.999 && a[1] - y >= self.options.radius * 1.15)
            {
                let mut at = 0.6;
                while at < distance - 0.2 {
                    mounts.push(at);
                    at += 1.5;
                }
            }
            let mut at = 2.4;
            while at < distance - 0.3 {
                seams.push(at);
                at += 2.4;
            }
            let half = self.socket_length() / 2.;
            let strap = self.strap_length() / 2.;
            let mut rims: Vec<_> = mounts
                .iter()
                .flat_map(|at| [at - strap, at + strap])
                .chain(seams.iter().flat_map(|at| [at - half, at + half]))
                .collect();
            if length(sub(a, route[i])) > 1e-6 {
                rims.push(half);
            }
            if length(sub(b, route[i + 1])) > 1e-6 {
                rims.push(distance - half);
            }
            rims.retain(|at| *at >= 0. && *at <= distance);
            self.straight(drawing, a, b, &rims)?;
            for at in seams {
                self.coupling(drawing, add(a, scale(axis, at)), axis)?;
            }
            for at in mounts {
                let center = add(a, scale(axis, at));
                self.mount(
                    drawing,
                    center,
                    axis,
                    [center[0], wall_y.unwrap(), center[2]],
                )?;
            }
        }
        for (a, corner, b) in bends {
            self.bend(drawing, a, corner, b, true)?;
            self.coupling(drawing, a, sub(corner, a))?;
            self.coupling(drawing, b, sub(b, corner))?;
        }
        Ok(())
    }
    fn sleeve(
        &self,
        drawing: &mut Fixture<'_, '_>,
        center: V3,
        tangent: V3,
        outer: f64,
        extent: f64,
    ) -> Result<(), String> {
        let r = self.options.radius;
        let radial = perpendicular(tangent)?;
        let profile = [
            [-extent / 2., r * 1.005],
            [-extent * 0.38, outer],
            [extent * 0.38, outer],
            [extent / 2., r * 1.005],
        ];
        let rings = profile.map(|[z, _]| Ring {
            center: add(center, scale(tangent, z)),
            tangent,
            radial,
            distance: z + extent / 2.,
        });
        self.sweep(drawing, &rings, outer, &[], Some(&profile.map(|p| p[1])))
    }
    fn sweep(
        &self,
        drawing: &mut Fixture<'_, '_>,
        rings: &[Ring],
        radius: f64,
        seams: &[f64],
        radii: Option<&[f64]>,
    ) -> Result<(), String> {
        let circumference = 2. * PI * radius;
        let extent = rounded(rings.last().unwrap().distance);
        let finish = self
            .options
            .weathering
            .as_ref()
            .filter(|s| s.texture_density > 0. && !drawing.mesh.surveying());
        let tint = Tint::Color(if finish.is_some() {
            [1.; 3]
        } else {
            self.options.color
        });
        let first = drawing.mesh.triangle_count();
        let vertex = |row: usize, side: usize| {
            let ring = rings[row];
            let angle = side as f64 / self.options.sides as f64 * PI * 2.;
            let normal = add(
                scale(ring.radial, cos(angle)),
                scale(cross(ring.tangent, ring.radial), sin(angle)),
            );
            (
                add(ring.center, scale(normal, radii.map_or(radius, |r| r[row]))),
                normal,
                [
                    circumference * side as f64 / self.options.sides as f64,
                    rounded(ring.distance),
                ],
            )
        };
        for row in 0..rings.len() - 1 {
            for side in 0..self.options.sides {
                let slope = radii.map_or(0., |r| {
                    (r[row + 1] - r[row]) / (rings[row + 1].distance - rings[row].distance)
                });
                let mut corners = [
                    vertex(row, side),
                    vertex(row, side + 1),
                    vertex(row + 1, side + 1),
                    vertex(row + 1, side),
                ];
                if slope != 0. {
                    for (i, c) in corners.iter_mut().enumerate() {
                        c.1 = unit(sub(
                            c.1,
                            scale(rings[if i < 2 { row } else { row + 1 }].tangent, slope),
                        ))?;
                    }
                }
                // The complete pipe chart owns appearance; inherited member finishes cannot register overlapping ranges.
                drawing.pen.face_raw(
                    drawing.mesh,
                    "metal",
                    &corners.map(|c| c.0),
                    corners[0].1,
                    &tint,
                    drawing.repeat("metal"),
                    Some(&corners.map(|c| c.2)),
                    Some(&corners.map(|c| c.1)),
                )?;
            }
        }
        let Some(finish) = finish else {
            return Ok(());
        };
        let origin = drawing.pen.world(rings[0].center);
        let zero = drawing.pen.world([0.; 3]);
        let direction = |v| sub(drawing.pen.world(v), zero).map(rounded);
        let recipe = pipe_runoff::Recipe {
            rings: rings
                .iter()
                .enumerate()
                .map(|(i, r)| pipe_runoff::Ring {
                    center: sub(drawing.pen.world(r.center), origin).map(rounded),
                    radial: direction(r.radial),
                    across: direction(cross(r.tangent, r.radial)),
                    radius: radii.map_or(radius, |r| r[i]),
                    distance: rounded(r.distance),
                })
                .collect(),
            circumference,
            seams: seams.iter().copied().map(rounded).collect(),
            seed: finish_seed(&self.options.variant) as f64,
        };
        // Orientation participates even without runoff, matching the scene-level sharing contract.
        let key = serde_json::to_string(&recipe).map_err(|e| e.to_string())?;
        let settings = metal::Settings {
            kind: metal::Kind::Pipe,
            bounds: [0., 0., circumference, extent],
            paint: self.options.color,
            seed: recipe.seed,
            wear: (finish.wear * 1.6).min(1.),
            grime: finish.grime,
            rust: finish.rust,
            paint_coverage: None,
            edge_axes: Some([false, true]),
            contacts: None,
            metric: Some(metal::Metric {
                scale: [1., 1.],
                gravity: [0., 0.],
            }),
            folds: seams
                .iter()
                .map(|at| metal::Fold {
                    axis: 1,
                    at: rounded(*at),
                    concave: true,
                })
                .collect(),
            wrap_u: Some(circumference),
            substrate: finish.metal.clone(),
            coating: finish.paint.clone(),
        };
        weathered::receive(
            drawing.mesh,
            first,
            drawing.mesh.triangle_count() - first,
            settings,
            finish.texture_density.max(128.),
            key,
            if radii.is_none() && !seams.is_empty() {
                Some(recipe)
            } else {
                None
            },
        )
    }
}

//! Convex trim sections with inward fillets and joined, mitered endpoints.
use super::{
    mesh::Mesh,
    pen::{Pen, Tint},
};
use superspec_geometry::{
    V3,
    math::{add, hypot},
};

#[derive(Clone, Copy, Debug)]
pub struct Edge {
    pub a: [f64; 2],
    pub b: [f64; 2],
}
struct RoundedEdge {
    edge: Edge,
    na: V3,
    nb: V3,
    original: Option<usize>,
}
struct Corner {
    a: [f64; 2],
    b: [f64; 2],
    middle: [f64; 2],
    na: V3,
    nb: V3,
}
pub struct Strip<'a> {
    pub span: [f64; 2],
    pub section: &'a [[f64; 2]],
    pub omitted_edges: &'a [usize],
    pub miters: [Option<f64>; 2],
    pub setback: f64,
}
impl Strip<'_> {
    fn validate(&self) -> Result<f64, String> {
        let n = self.section.len();
        if !(3..=256).contains(&n)
            || self
                .section
                .iter()
                .flatten()
                .chain(&self.span)
                .chain(self.miters.iter().flatten())
                .any(|v| !v.is_finite())
            || !self.setback.is_finite()
            || self.setback < 0.
            || self.span[1] <= self.span[0]
            || self.omitted_edges.len() > n
            || self.omitted_edges.iter().any(|i| *i >= n)
            || self
                .omitted_edges
                .iter()
                .enumerate()
                .any(|(i, edge)| self.omitted_edges[..i].contains(edge))
        {
            return Err("Molding requires a finite convex section, positive span and valid edge identities.".into());
        }
        let area: f64 = (0..n)
            .map(|i| {
                let a = self.section[i];
                let b = self.section[(i + 1) % n];
                a[0] * b[1] - a[1] * b[0]
            })
            .sum();
        if !area.is_finite() || area.abs() < 1e-12 {
            return Err("Molding section is degenerate.".into());
        }
        let sign = area.signum();
        for i in 0..n {
            let a = self.section[i];
            let b = self.section[(i + 1) % n];
            if self.section[..i]
                .iter()
                .any(|p| hypot(a[0] - p[0], a[1] - p[1]) < 1e-9)
                || hypot(b[0] - a[0], b[1] - a[1]) < 1e-9
                || self.section.iter().any(|c| {
                    ((b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])) * sign < -1e-12
                })
            {
                return Err("Molding section must be convex without repeated edges.".into());
            }
            if self.span[0] + self.miters[0].unwrap_or(0.) * a[0]
                >= self.span[1] + self.miters[1].unwrap_or(0.) * a[0]
            {
                return Err("Molding miters cross or collapse the strip.".into());
            }
        }
        Ok(sign)
    }
}

/// Emits trim without changing its original envelope. Returned faces identify trimmed original edges.
pub fn profiled_strip(
    pen: &Pen,
    mesh: &mut Mesh<'_>,
    slot: &str,
    strip: &Strip<'_>,
    tint: &Tint<'_>,
    repeat: f64,
) -> Result<Vec<Edge>, String> {
    let sign = strip.validate()?;
    let section = strip.section;
    let count = section.len();
    mesh.check_work(count * 12)?;
    let normal = |a: [f64; 2], b: [f64; 2]| {
        let dy = b[0] - a[0];
        let dz = b[1] - a[1];
        let size = hypot(dy, dz);
        [0., sign * dz / size, -sign * dy / size]
    };
    let corners: Vec<_> = (0..count)
        .map(|i| {
            let point = section[i];
            let previous = section[(i + count - 1) % count];
            let next = section[(i + 1) % count];
            let before = hypot(point[0] - previous[0], point[1] - previous[1]);
            let after = hypot(next[0] - point[0], next[1] - point[1]);
            let cut = strip.setback.min(before * 0.24).min(after * 0.24);
            let a = std::array::from_fn(|axis| {
                point[axis] + (previous[axis] - point[axis]) * cut / before
            });
            let b =
                std::array::from_fn(|axis| point[axis] + (next[axis] - point[axis]) * cut / after);
            let middle = std::array::from_fn(|axis| (a[axis] + 2. * point[axis] + b[axis]) / 4.);
            Corner {
                a,
                b,
                middle,
                na: normal(previous, point),
                nb: normal(point, next),
            }
        })
        .collect();
    let mut edges = vec![];
    let mut faces = vec![];
    for (i, corner) in corners.iter().enumerate() {
        let next = &corners[(i + 1) % count];
        let face = Edge {
            a: corner.b,
            b: next.a,
        };
        faces.push(face);
        edges.push(RoundedEdge {
            edge: face,
            na: corner.nb,
            nb: corner.nb,
            original: Some(i),
        });
        if strip.setback > 0. {
            let nm = normal(next.a, next.b);
            edges.push(RoundedEdge {
                edge: Edge {
                    a: next.a,
                    b: next.middle,
                },
                na: next.na,
                nb: nm,
                original: None,
            });
            edges.push(RoundedEdge {
                edge: Edge {
                    a: next.middle,
                    b: next.b,
                },
                na: nm,
                nb: next.nb,
                original: None,
            });
        }
    }
    let [u0, u1] = strip.span;
    let mut distance = 0.;
    for edge in &edges {
        let Edge { a, b } = edge.edge;
        let start = distance;
        distance += hypot(b[0] - a[0], b[1] - a[1]);
        if edge
            .original
            .is_some_and(|i| strip.omitted_edges.contains(&i))
        {
            continue;
        }
        let points = [
            [u0 + strip.miters[0].unwrap_or(0.) * a[0], a[0], a[1]],
            [u1 + strip.miters[1].unwrap_or(0.) * a[0], a[0], a[1]],
            [u1 + strip.miters[1].unwrap_or(0.) * b[0], b[0], b[1]],
            [u0 + strip.miters[0].unwrap_or(0.) * b[0], b[0], b[1]],
        ];
        let uv = std::array::from_fn::<_, 4, _>(|i| {
            [
                points[i][0] * repeat,
                if i < 2 {
                    start * repeat
                } else {
                    distance * repeat
                },
            ]
        });
        pen.face(
            mesh,
            slot,
            &points,
            add(edge.na, edge.nb),
            tint,
            repeat,
            Some(&uv),
            Some(&[edge.na, edge.na, edge.nb, edge.nb]),
        )?;
    }
    let outline: Vec<_> = edges.iter().map(|e| e.edge.a).collect();
    for (index, (u, normal)) in [(u0, -1.), (u1, 1.)].into_iter().enumerate() {
        if strip.miters[index].is_some() {
            continue;
        }
        for i in 1..outline.len() - 1 {
            let points = [outline[0], outline[i], outline[i + 1]].map(|[y, z]| [u, y, z]);
            pen.face(
                mesh,
                slot,
                &points,
                [normal, 0., 0.],
                tint,
                repeat,
                None,
                None,
            )?;
        }
    }
    Ok(faces)
}

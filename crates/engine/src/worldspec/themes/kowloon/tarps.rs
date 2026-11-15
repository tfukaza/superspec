//! Static settled cloth fitted with physical weave UVs, stripes, hems and attachment hardware.
mod cloth;
mod fittings;
use super::{
    fixture::Fixture,
    palette::{mix, shade, srgb},
    weathered,
};
use serde::Deserialize;
use std::{collections::BTreeMap, sync::OnceLock};
use superspec_geometry::{
    V3,
    math::{add, cross, dot, hypot, scale, sub},
};
use superspec_types::rng::SeededRng;

#[derive(Clone, Copy, Deserialize)]
pub struct Envelope {
    pub x0: f64,
    pub x1: f64,
    pub z: f64,
    pub depth: f64,
    pub drop: f64,
    #[serde(default)]
    pub valance: f64,
}
#[derive(Clone, Copy, Deserialize)]
pub struct Pattern {
    pub color: V3,
    pub width: f64,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Form {
    reference_dimensions: Dimensions,
    positions: Vec<V3>,
    uv: Vec<[f64; 2]>,
    indices: Vec<usize>,
    attachments: Vec<Attachment>,
    supports: Supports,
    #[serde(skip)]
    edges: Vec<[usize; 2]>,
}
#[derive(Deserialize)]
struct Dimensions {
    width: f64,
    depth: f64,
    drop: f64,
}
#[derive(Deserialize)]
struct Attachment {
    vertex: usize,
    kind: String,
    normalized: V3,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Supports {
    rear_rail: [V3; 2],
    front_rail: [V3; 2],
}
fn forms() -> Result<&'static Vec<Form>, String> {
    static FORMS: OnceLock<Result<Vec<Form>, String>> = OnceLock::new();
    FORMS
        .get_or_init(|| {
            #[derive(Deserialize)]
            struct Catalog {
                forms: Vec<Form>,
            }
            let mut catalog: Catalog =
                serde_json::from_str(include_str!("assets/cloth-forms.json"))
                    .map_err(|e| e.to_string())?;
            if catalog.forms.is_empty() || catalog.forms.len() > 16 {
                return Err("Invalid cloth catalog count.".into());
            }
            for form in &mut catalog.forms {
                if form.positions.len() < 3
                    || form.positions.len() > 16384
                    || form.positions.len() != form.uv.len()
                    || form.indices.len() < 3
                    || form.indices.len() > 196608
                    || !form.indices.len().is_multiple_of(3)
                    || form.indices.iter().any(|i| *i >= form.positions.len())
                    || form
                        .positions
                        .iter()
                        .flatten()
                        .chain(form.uv.iter().flatten())
                        .any(|v| !v.is_finite())
                    || form.attachments.iter().any(|a| {
                        a.vertex >= form.positions.len()
                            || !["rear-seam", "front-tie", "side-tie"].contains(&a.kind.as_str())
                            || a.normalized.iter().any(|v| !v.is_finite())
                    })
                    || form.attachments.len() > 4096
                    || form
                        .supports
                        .rear_rail
                        .iter()
                        .chain(form.supports.front_rail.iter())
                        .flatten()
                        .any(|v| !v.is_finite())
                    || [
                        form.reference_dimensions.width,
                        form.reference_dimensions.depth,
                    ]
                    .iter()
                    .any(|v| !v.is_finite() || *v <= 0.)
                    || !form.reference_dimensions.drop.is_finite()
                    || (0..2).any(|axis| {
                        form.positions
                            .iter()
                            .map(|p| p[axis])
                            .fold(f64::NEG_INFINITY, f64::max)
                            - form
                                .positions
                                .iter()
                                .map(|p| p[axis])
                                .fold(f64::INFINITY, f64::min)
                            < 1e-6
                    })
                {
                    return Err("Invalid embedded cloth form.".into());
                }
                let mut lookup = BTreeMap::new();
                let mut edges: Vec<([usize; 2], usize)> = vec![];
                for triangle in form.indices.as_chunks::<3>().0 {
                    for i in 0..3 {
                        let a = triangle[i];
                        let b = triangle[(i + 1) % 3];
                        let key = (a.min(b), a.max(b));
                        if let Some(index) = lookup.get(&key) {
                            let index: usize = *index;
                            edges[index].1 += 1;
                        } else {
                            lookup.insert(key, edges.len());
                            edges.push(([a, b], 1));
                        }
                    }
                }
                form.edges = edges
                    .into_iter()
                    .filter(|(_, count)| *count == 1)
                    .map(|(pair, _)| pair)
                    .collect();
            }
            Ok(catalog.forms)
        })
        .as_ref()
        .map_err(Clone::clone)
}
/// Match the source Vector3 normalization, including its zero-vector behavior.
fn normal(v: V3) -> V3 {
    let n = dot(v, v).sqrt();
    scale(v, 1. / if n == 0. { 1. } else { n })
}
fn smooth_normals(points: &[V3], indices: &[usize]) -> Vec<V3> {
    let mut result = vec![[0.; 3]; points.len()];
    for t in indices.as_chunks::<3>().0 {
        let n = cross(
            sub(points[t[1]], points[t[0]]),
            sub(points[t[2]], points[t[0]]),
        );
        for i in t {
            result[*i] = add(result[*i], n);
        }
    }
    result.into_iter().map(normal).collect()
}
struct Fit<'a> {
    form: &'a Form,
    envelope: Envelope,
    bounds: [[f64; 2]; 2],
    sag: f64,
    points: Vec<V3>,
}
impl<'a> Fit<'a> {
    fn new(form: &'a Form, envelope: Envelope) -> Self {
        let bounds = std::array::from_fn(|axis| {
            [
                form.positions
                    .iter()
                    .map(|p| p[axis])
                    .fold(f64::INFINITY, f64::min),
                form.positions
                    .iter()
                    .map(|p| p[axis])
                    .fold(f64::NEG_INFINITY, f64::max),
            ]
        });
        let mut result = Self {
            form,
            envelope,
            bounds,
            sag: (envelope.depth / form.reference_dimensions.depth).min(1.),
            points: vec![],
        };
        for point in &form.positions {
            let sag = point[2] * form.reference_dimensions.depth;
            let t = result.t(*point);
            if sag < 0. {
                result.sag = result.sag.min((envelope.drop * (1. - t) + 0.145) / -sag);
            }
            if sag > 0. {
                result.sag = result.sag.min((envelope.drop * t + 0.025) / sag);
            }
        }
        result.points = form.positions.iter().map(|p| result.at(*p)).collect();
        result
    }
    fn t(&self, p: V3) -> f64 {
        (p[1] - self.bounds[1][0]) / (self.bounds[1][1] - self.bounds[1][0])
    }
    fn at(&self, p: V3) -> V3 {
        let e = self.envelope;
        [
            e.x0 + 0.025
                + (p[0] - self.bounds[0][0]) / (self.bounds[0][1] - self.bounds[0][0])
                    * (e.x1 - e.x0 - 0.05),
            0.025 + self.t(p) * (e.depth - 0.05),
            e.z - e.drop * self.t(p)
                + p[2] * self.form.reference_dimensions.depth * self.sag * 0.18,
        ]
    }
    fn reference(&self, p: V3) -> V3 {
        let r = &self.form.reference_dimensions;
        let mut fitted = self.at([p[0] / r.width, p[1] / r.depth, 0.]);
        fitted[2] += p[2] + r.drop * p[1] / r.depth;
        fitted
    }
}
/// Wide awnings share stripe phase across fitted modules; no runtime cloth simulation is used.
pub fn canvas(
    d: &mut Fixture<'_, '_>,
    envelope: Envelope,
    tint: V3,
    hem: V3,
    key: &str,
    settings: Option<&weathered::Settings>,
    pattern: Option<Pattern>,
) -> Result<(), String> {
    let e = envelope;
    let width = e.x1 - e.x0;
    if [e.x0, e.x1, e.z, e.depth, e.drop]
        .iter()
        .any(|v| !v.is_finite() || v.abs() > 1000.)
        || width <= 0.05
        || width > 100.
        || e.depth <= 0.05
        || e.depth > 20.
        || e.drop < 0.
        || !e.valance.is_finite()
        || !(0. ..=0.6).contains(&e.valance)
        || key.len() > 2048
        || tint
            .iter()
            .chain(hem.iter())
            .any(|v| !v.is_finite() || !(0. ..=1.).contains(v))
        || pattern.is_some_and(|p| {
            !p.width.is_finite()
                || p.width < 0.08
                || p.width > 100.
                || p.color
                    .iter()
                    .any(|v| !v.is_finite() || !(0. ..=1.).contains(v))
        })
    {
        return Err("Canvas envelope, tints or stripe pattern exceed finite bounds.".into());
    }
    if let Some(s) = settings {
        s.validate()?;
    }
    let forms = forms()?;
    let count = (width / (e.depth * 4.4)).ceil().max(1.) as usize;
    if count > 128 {
        return Err("Canvas module count exceeds 128.".into());
    }
    for i in 0..count {
        d.mesh.spend_work(1)?;
        let mut rng = SeededRng::from_text(&format!(
            "{}/{key}/{i}",
            settings.map_or("canvas", |s| &s.seed)
        ));
        let form = &forms[rng.integer(0, forms.len() - 1)];
        let envelope = Envelope {
            x0: e.x0 + width * i as f64 / count as f64,
            x1: e.x0 + width * (i + 1) as f64 / count as f64,
            ..e
        };
        panel(
            d,
            form,
            envelope,
            tint,
            hem,
            &mut rng,
            settings,
            pattern,
            width * i as f64 / count as f64,
        )?;
    }
    Ok(())
}
#[allow(clippy::too_many_arguments)]
fn panel(
    d: &mut Fixture<'_, '_>,
    form: &Form,
    e: Envelope,
    tint: V3,
    hem: V3,
    rng: &mut SeededRng,
    settings: Option<&weathered::Settings>,
    pattern: Option<Pattern>,
    module_offset: f64,
) -> Result<(), String> {
    let fit = Fit::new(form, e);
    let normals = smooth_normals(&fit.points, &form.indices);
    let width = e.x1 - e.x0;
    let run = hypot(e.depth, e.drop);
    let reference = hypot(
        form.reference_dimensions.depth,
        form.reference_dimensions.drop,
    );
    let uv: Vec<[f64; 2]> = form
        .uv
        .iter()
        .map(|uv| {
            [
                uv[0] * width / form.reference_dimensions.width,
                uv[1] * run / reference,
            ]
        })
        .collect();
    let offset = [rng.next_f64(), rng.next_f64()];
    let repeat = d.repeat("tarp");
    let seed = rng.integer(0, 2147483647) as f64;
    let base = mix(tint, srgb("#a99d88"), 0.12);
    let edge = shade(mix(base, hem, 0.1), 0.62);
    let field = cloth::Field {
        width: uv.iter().map(|v| v[0]).fold(f64::NEG_INFINITY, f64::max),
        run: uv.iter().map(|v| v[1]).fold(f64::NEG_INFINITY, f64::max),
        base,
        grime: settings.map_or(0.7, |s| s.grime),
        seed,
        repeat,
        offset,
        module_offset,
        pattern,
    };
    for ids in form.indices.as_chunks::<3>().0 {
        field.triangle(
            d,
            &ids.iter()
                .map(|i| cloth::Vertex {
                    point: fit.points[*i],
                    uv: uv[*i],
                    normal: normals[*i],
                })
                .collect::<Vec<_>>(),
        )?;
    }
    for [a, b] in &form.edges {
        d.tube("tarp", &[fit.points[*a], fit.points[*b]], 0.0025, edge, 4)?;
    }
    if e.valance > 0. {
        cloth::valance(d, form, &fit.points, &uv, e.valance, &field, edge)?;
    }
    fittings::draw(d, form, &fit, &normals, settings)
}

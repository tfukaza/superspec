//! Offline-baked provisions, label variants and touching bag groups.
use super::super::{fixture::Fixture, pen::Tint};
use super::forms;
use serde::Deserialize;
use std::{collections::BTreeMap, sync::OnceLock};
use superspec_geometry::{
    V3,
    math::{add, cross, dot, scale as scale_vector, sub, unit},
};
type Faces = Vec<Vec<[f64; 11]>>;
type Shape = Vec<Vec<[f64; 6]>>;
#[derive(Deserialize)]
struct Asset {
    size: V3,
    faces: Faces,
    #[serde(default, rename = "paintedFaces")]
    painted_faces: Faces,
}
#[derive(Deserialize)]
struct Variant {
    uv: [f64; 4],
    #[serde(rename = "plainUv")]
    plain_uv: Option<[f64; 4]>,
    #[serde(rename = "topUv")]
    top_uv: Option<[f64; 4]>,
    colors: Vec<Vec<V3>>,
}
#[derive(Deserialize)]
struct Appearance {
    source: [f64; 4],
    variants: Vec<Variant>,
}
#[derive(Deserialize)]
struct Pose {
    scale: f64,
    yaw: f64,
    at: V3,
    #[serde(rename = "labelOffset")]
    label_offset: usize,
    shape: Shape,
}
#[derive(Deserialize)]
struct Arrangement {
    size: V3,
    poses: Vec<Pose>,
}
#[derive(Deserialize)]
struct Bags {
    regular: Vec<Arrangement>,
    mirrored: Vec<Arrangement>,
}
fn catalog() -> &'static BTreeMap<String, Asset> {
    static DATA: OnceLock<BTreeMap<String, Asset>> = OnceLock::new();
    DATA.get_or_init(|| {
        serde_json::from_str(include_str!("assets/grocer-props.json"))
            .expect("shipped grocer props")
    })
}
fn appearances() -> &'static BTreeMap<String, Appearance> {
    static DATA: OnceLock<BTreeMap<String, Appearance>> = OnceLock::new();
    DATA.get_or_init(|| {
        serde_json::from_str(include_str!("assets/stock-appearances.json"))
            .expect("shipped stock appearances")
    })
}
fn bags() -> &'static BTreeMap<String, Bags> {
    static DATA: OnceLock<BTreeMap<String, Bags>> = OnceLock::new();
    DATA.get_or_init(|| {
        serde_json::from_str(include_str!("assets/bag-clusters.json"))
            .expect("shipped bag arrangements")
    })
}
fn envelope(size: V3) -> Result<(), String> {
    if size
        .iter()
        .any(|v| !v.is_finite() || !(0.001..=100.).contains(v))
    {
        Err("Grocer prop envelope must be positive and bounded.".into())
    } else {
        Ok(())
    }
}
fn fitted(source: V3, envelope: V3) -> V3 {
    let scale = (0..3)
        .map(|i| envelope[i] / source[i])
        .fold(f64::INFINITY, f64::min);
    source.map(|v| v * scale)
}
pub fn size(name: &str, requested: V3) -> Result<V3, String> {
    envelope(requested)?;
    let asset = catalog().get(name).ok_or("Unknown grocer prop.")?;
    Ok(if matches!(name, "shop-counter" | "stock-box") {
        requested
    } else {
        fitted(asset.size, requested)
    })
}
pub fn variants(name: &str) -> Result<usize, String> {
    if !catalog().contains_key(name) {
        return Err("Unknown grocer prop.".into());
    }
    Ok(appearances().get(name).map_or(1, |a| a.variants.len()))
}
#[derive(Clone, Copy)]
pub struct Options {
    pub tint: V3,
    pub supported: bool,
    pub variant: usize,
    pub textured: bool,
}
impl Default for Options {
    fn default() -> Self {
        Self {
            tint: [1.; 3],
            supported: false,
            variant: 0,
            textured: true,
        }
    }
}
pub fn draw(
    d: &mut Fixture<'_, '_>,
    name: &str,
    at: V3,
    requested: V3,
    yaw: f64,
    options: &Options,
) -> Result<V3, String> {
    draw_shape(d, name, at, requested, yaw, options, None, false)
}
/// Fit a crate to a structural stand footprint and preserve its full support height.
pub(super) fn fitted_crate(
    d: &mut Fixture<'_, '_>,
    at: V3,
    dimensions: V3,
    options: &Options,
) -> Result<V3, String> {
    draw_shape(d, "bottle-crate", at, dimensions, 0., options, None, true)
}

/// Distinct glass families use the same padded label charts and independent mold proportions.
pub(super) fn fitted_bottle(
    d: &mut Fixture<'_, '_>,
    at: V3,
    dimensions: V3,
    yaw: f64,
    options: &Options,
) -> Result<V3, String> {
    draw_shape(d, "drink-bottle", at, dimensions, yaw, options, None, true)
}

#[allow(clippy::too_many_arguments)]
fn draw_shape(
    d: &mut Fixture<'_, '_>,
    name: &str,
    at: V3,
    requested: V3,
    yaw: f64,
    o: &Options,
    shape: Option<&Shape>,
    fit_dimensions: bool,
) -> Result<V3, String> {
    let size = if fit_dimensions {
        envelope(requested)?;
        requested
    } else {
        size(name, requested)?
    };
    if at.iter().any(|v| !v.is_finite() || v.abs() > 1000.)
        || !yaw.is_finite()
        || o.tint
            .iter()
            .any(|v| !v.is_finite() || !(0. ..=4.).contains(v))
        || o.variant > variants(name)?
    {
        return Err("Grocer prop placement, tint or label variant exceeds bounds.".into());
    }
    let asset = &catalog()[name];
    let appearance = appearances().get(name);
    let selected = o
        .variant
        .checked_sub(1)
        .and_then(|i| appearance.and_then(|a| a.variants.get(i)));
    d.mesh
        .check_work((asset.faces.len() + asset.painted_faces.len()) * 2)?;
    let scale: V3 = std::array::from_fn(|i| size[i] / asset.size[i]);
    let pen = forms::rotated(&d.pen, at, 2, yaw)?;
    let sign = if d.pen.mirrored { -1. } else { 1. };
    let carton = name == "stock-box";
    let crate_wear = name == "bottle-crate";
    let seed =
        super::super::wall::finish_seed(&format!("{}/{at:?}/{requested:?}", pen.part)) as f64;
    let worn;
    let body = if carton && shape.is_none() {
        worn = carton_faces(
            &asset.faces,
            asset.size,
            seed,
            selected.map(|variant| variant.colors.as_slice()),
        )?;
        &worn
    } else {
        &asset.faces
    };
    for (faces, textured, primary) in [
        (body, o.textured, true),
        (&asset.painted_faces, false, false),
    ] {
        for (face_index, face) in faces.iter().enumerate() {
            if o.supported && face.iter().all(|v| v[2] <= 0.00001) {
                continue;
            }
            let geometry = if primary {
                shape.and_then(|s| s.get(face_index))
            } else {
                None
            };
            let positions: Vec<_> = (0..face.len())
                .map(|i| {
                    let v = geometry.map_or(&face[i][..6], |f| &f[i][..]);
                    [v[0] * scale[0] * sign, v[1] * scale[1], v[2] * scale[2]]
                })
                .collect();
            let normals: Vec<_> = (0..face.len())
                .map(|i| {
                    let v = geometry.map_or(&face[i][..6], |f| &f[i][..]);
                    unit([v[3] * sign / scale[0], v[4] / scale[1], v[5] / scale[2]])
                })
                .collect::<Result<_, _>>()?;
            let hint = normals.iter().fold([0.; 3], |sum, n| add(sum, *n));
            let colors: Vec<V3> = face
                .iter()
                .enumerate()
                .map(|(i, v)| {
                    let color = if primary && !carton {
                        selected.map_or([v[8], v[9], v[10]], |s| s.colors[face_index][i])
                    } else {
                        [v[8], v[9], v[10]]
                    };
                    let wear = if carton {
                        carton_wear([v[0], v[1], v[2]], asset.size, seed)
                    } else if crate_wear {
                        plastic_wear([v[0], v[1], v[2]], asset.size, seed)
                    } else {
                        [1.; 3]
                    };
                    std::array::from_fn(|axis| {
                        (if textured { 1. } else { color[axis] }) * o.tint[axis] * wear[axis]
                    })
                })
                .collect();
            let color = |p| {
                positions
                    .iter()
                    .position(|v| *v == p)
                    .map(|i| colors[i])
                    .ok_or_else(|| "Missing stock vertex tint.".into())
            };
            let tint = if textured && !carton && !crate_wear {
                Tint::Color(o.tint)
            } else {
                Tint::Field(&color)
            };
            let uv: Vec<_> = face
                .iter()
                .map(|v| {
                    std::array::from_fn(|axis| {
                        let coordinate = v[6 + axis];
                        if let (Some(selected), Some(a), true) = (selected, appearance, primary) {
                            let unit = (coordinate - a.source[axis])
                                / (a.source[axis + 2] - a.source[axis]);
                            let region = if name == "stock-box" && face[0][5] > 0.9 {
                                selected.top_uv.unwrap_or(selected.uv)
                            } else if name == "stock-box" && face[0][4] > -0.9 {
                                selected.plain_uv.unwrap_or(selected.uv)
                            } else {
                                selected.uv
                            };
                            region[axis] + unit * (region[axis + 2] - region[axis])
                        } else {
                            coordinate
                        }
                    })
                })
                .collect();
            let slot = if textured { "grocer-props" } else { "paint" };
            pen.face(
                d.mesh,
                slot,
                &positions,
                hint,
                &tint,
                d.repeat(slot),
                Some(&uv),
                Some(&normals),
            )?;
        }
    }
    Ok(size)
}

fn plastic_wear([x, y, z]: V3, size: V3, seed: f64) -> V3 {
    use super::super::noise::{smooth_step, value_noise3};
    let (x, y, z) = (x / size[0] * 2., y / size[1] * 2., z / size[2]);
    let patches = value_noise3(x * 4.1, y * 4.1, z * 4.6, seed);
    let flecks = value_noise3(x * 13., y * 13., z * 16., seed + 37.);
    let corner = smooth_step(0.69, 0.99, x.abs()) * smooth_step(0.69, 0.99, y.abs());
    let rim = smooth_step(0.78, 1., z);
    let scuff = rim.max(corner) * (0.23 + 0.42 * flecks);
    let foot = (1. - smooth_step(0.06, 0.44, z)) * (0.23 + 0.28 * patches);
    let dirt = smooth_step(0.34, 0.86, patches) * (0.19 + 0.28 * flecks);
    let brightness = 0.83 - foot - dirt * 0.73 + scuff * 0.73;
    [
        brightness + dirt * 0.08,
        brightness,
        brightness - dirt * 0.09 + scuff * 0.14,
    ]
    .map(|value| value.clamp(0.2, 1.))
}

fn carton_faces(
    faces: &Faces,
    size: V3,
    seed: f64,
    colors: Option<&[Vec<V3>]>,
) -> Result<Faces, String> {
    use super::super::noise::{smooth_step, value_noise};
    let mut output = Vec::with_capacity(faces.len() * 4);
    for (index, face) in faces.iter().enumerate() {
        for triangle in 1..face.len().saturating_sub(1) {
            let triangle = [0, triangle, triangle + 1];
            let vertex = |weights: V3| {
                let mut values: [f64; 11] = std::array::from_fn(|channel| {
                    triangle
                        .iter()
                        .enumerate()
                        .map(|(i, corner)| {
                            let value = if channel >= 8 {
                                colors.map_or(face[*corner][channel], |colors| {
                                    colors[index][*corner][channel - 8]
                                })
                            } else {
                                face[*corner][channel]
                            };
                            value * weights[i]
                        })
                        .sum()
                });
                let [x, y, z] = [
                    values[0] / size[0] * 2.,
                    values[1] / size[1] * 2.,
                    values[2] / size[2],
                ];
                let corner = smooth_step(0.45, 1., x.abs()) * smooth_step(0.45, 1., y.abs());
                let uneven = value_noise(x * 2. + 3., y * 2. + 5., seed);
                let compression = corner * (0.008 + 0.012 * uneven) * smooth_step(0., 0.25, z);
                let middle = (z * std::f64::consts::PI).sin();
                values[0] *= (1. - compression)
                    * (1.
                        - 0.008
                            * middle
                            * (1. - y.abs())
                            * value_noise(y * 4., z * 5., seed + 17.));
                values[1] *= (1. - compression * 0.8)
                    * (1.
                        - 0.01 * middle * (1. - x.abs()) * value_noise(x * 4., z * 5., seed + 31.));
                values[2] -= size[2] * corner * 0.012 * z * z * uneven;
                values
            };
            let a = vertex([1., 0., 0.]);
            let b = vertex([0., 1., 0.]);
            let c = vertex([0., 0., 1.]);
            let ab = vertex([0.5, 0.5, 0.]);
            let ac = vertex([0.5, 0., 0.5]);
            let bc = vertex([0., 0.5, 0.5]);
            let hint = [a[3], a[4], a[5]];
            for mut vertices in [[a, ab, ac], [ab, b, bc], [ac, bc, c], [ab, bc, ac]] {
                let [a, b, c] = vertices.map(|v| [v[0], v[1], v[2]]);
                let raw = cross(sub(b, a), sub(c, a));
                let normal = unit(raw).unwrap_or(hint);
                let normal = scale_vector(normal, if dot(normal, hint) < 0. { -1. } else { 1. });
                for v in &mut vertices {
                    v[3..6].copy_from_slice(&normal);
                }
                output.push(vertices.into());
            }
        }
    }
    Ok(output)
}

fn carton_wear([x, y, z]: V3, size: V3, seed: f64) -> V3 {
    use super::super::noise::{smooth_step, value_noise};
    let (x, y, z) = (x / size[0] * 2., y / size[1] * 2., z / size[2]);
    let mut edges = [1. - x.abs(), 1. - y.abs(), z.min(1. - z) * 2.];
    edges.sort_by(f64::total_cmp);
    let edge = 1. - smooth_step(0.015, 0.13, edges[1]);
    let mottle = value_noise(x * 9. + z * 3., y * 8. + z * 11., seed + 81.);
    let abrasion = edge * (0.35 + mottle * 0.65);
    let grime = (1. - smooth_step(0.02, 0.3, z)) * (0.08 + mottle * 0.15);
    let stain = smooth_step(
        0.5,
        0.9,
        value_noise(x * 4. + z * 5., y * 5. + z * 3., seed + 123.),
    ) * 0.16;
    [0.32, 0.34, 0.4].map(|gain| (1. + abrasion * gain - grime - stain).min(1.))
}
fn arrangement(name: &str, mirrored: bool, index: usize) -> Result<&'static Arrangement, String> {
    let bag = bags().get(name).ok_or("Unknown bag group.")?;
    (if mirrored {
        &bag.mirrored
    } else {
        &bag.regular
    })
    .get(index)
    .ok_or_else(|| "Unknown bag arrangement.".into())
}
pub fn bag_size(name: &str, requested: V3, mirrored: bool, index: usize) -> Result<V3, String> {
    envelope(requested)?;
    Ok(fitted(arrangement(name, mirrored, index)?.size, requested))
}
#[derive(Clone, Copy)]
pub struct Group {
    pub yaw: f64,
    pub variant: usize,
    pub arrangement: usize,
    pub textured: bool,
}
pub fn bag_cluster(
    d: &mut Fixture<'_, '_>,
    name: &str,
    at: V3,
    requested: V3,
    g: &Group,
) -> Result<(), String> {
    let profile = arrangement(name, d.pen.mirrored, g.arrangement)?;
    let size = bag_size(name, requested, d.pen.mirrored, g.arrangement)?;
    if !(1..=4).contains(&g.variant) {
        return Err("Bag groups require label variant 1–4.".into());
    }
    let scale = size[2] / profile.size[2];
    let pen = forms::rotated(&d.pen, at, 2, g.yaw)?;
    let local = &mut Fixture {
        pen,
        mesh: d.mesh,
        repeats: d.repeats,
    };
    for pose in &profile.poses {
        draw_shape(
            local,
            name,
            pose.at.map(|v| v * scale),
            catalog()[name].size.map(|v| v * pose.scale * scale),
            pose.yaw,
            &Options {
                variant: (g.variant - 1 + pose.label_offset) % 4 + 1,
                textured: g.textured,
                ..Default::default()
            },
            Some(&pose.shape),
            false,
        )?;
    }
    Ok(())
}

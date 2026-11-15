use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use superspec_geometry::{
    V3,
    math::{add, cross, dot, sub, unit},
};

#[derive(Deserialize)]
struct Asset {
    size: V3,
    faces: Vec<[[f64; 11]; 3]>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Pose {
    scale: f64,
    yaw: f64,
    at: V3,
    label_offset: usize,
    changed_vertices: usize,
    shape: Vec<[[f64; 6]; 3]>,
}

#[derive(Serialize)]
struct Group {
    size: V3,
    poses: Vec<Pose>,
}

fn flatten(value: f64, low: Option<f64>, high: Option<f64>, [left, right]: [f64; 2]) -> f64 {
    if let Some(low) = low {
        let start = low + 0.004 * (low - left);
        if value < start {
            return low + (value - left) * (start - low) / (start - left);
        }
    }
    if let Some(high) = high {
        let start = high - 0.004 * (right - high);
        if value > start {
            return start + (value - start) * (high - start) / (right - start);
        }
    }
    value
}

fn position(v: &[f64; 11]) -> V3 {
    [v[0], v[1], v[2]]
}

fn cluster(
    asset: &Asset,
    scales: V3,
    yaws: V3,
    depths: V3,
    labels: [usize; 3],
    mirrored: bool,
) -> Result<Group, String> {
    let sign = if mirrored { -1. } else { 1. };
    let mut poses = Vec::new();
    let mut bodies = Vec::new();
    let mut bounds = Vec::<[f64; 2]>::new();
    for i in 0..3 {
        let mut pose = Pose {
            scale: scales[i],
            yaw: yaws[i],
            at: [0., depths[i] * asset.size[1], 0.],
            label_offset: labels[i],
            changed_vertices: 0,
            shape: Vec::new(),
        };
        let (sin, cos) = pose.yaw.sin_cos();
        let mut body: Vec<[V3; 3]> = asset
            .faces
            .iter()
            .map(|face| {
                face.map(|v| {
                    let [x, y, z] = position(&v).map(|p| p * pose.scale);
                    [
                        x * sign * cos - y * sin,
                        x * sign * sin + y * cos + pose.at[1],
                        z,
                    ]
                })
            })
            .collect();
        let mut low = body
            .iter()
            .flatten()
            .map(|v| v[0])
            .fold(f64::INFINITY, f64::min);
        let mut high = body
            .iter()
            .flatten()
            .map(|v| v[0])
            .fold(f64::NEG_INFINITY, f64::max);
        if let Some(&[left, right]) = bounds.last() {
            pose.at[0] = right - 0.18 * (right - left).min(high - low) - low;
            for v in body.iter_mut().flatten() {
                v[0] += pose.at[0];
            }
            low += pose.at[0];
            high += pose.at[0];
        }
        bounds.push([low, high]);
        poses.push(pose);
        bodies.push(body);
    }
    let planes = [
        (bounds[0][1] + bounds[1][0]) / 2.,
        (bounds[1][1] + bounds[2][0]) / 2.,
    ];
    let key = |p: V3| p.map(|v| (v * 1e9).round_ties_even() as i64);
    for (i, (body, pose)) in bodies.iter_mut().zip(&mut poses).enumerate() {
        let low = i.checked_sub(1).map(|j| planes[j] + 0.00015);
        let high = planes.get(i).map(|plane| plane - 0.00015);
        let (sin, cos) = pose.yaw.sin_cos();
        let mut moved = BTreeSet::new();
        let mut local = Vec::with_capacity(body.len());
        let mut normals = BTreeMap::new();
        for (points, source) in body.iter_mut().zip(&asset.faces) {
            let face: [V3; 3] = std::array::from_fn(|corner| {
                let point = &mut points[corner];
                let clipped = flatten(point[0], low, high, bounds[i]);
                if (clipped - point[0]).abs() <= 1e-10 {
                    return position(&source[corner]);
                }
                point[0] = clipped;
                moved.insert(position(&source[corner]).map(f64::to_bits));
                let [x, y, z] = sub(*point, pose.at);
                [
                    (x * cos + y * sin) * sign / pose.scale,
                    (-x * sin + y * cos) / pose.scale,
                    z / pose.scale,
                ]
            });
            let raw = cross(sub(face[1], face[0]), sub(face[2], face[0]));
            unit(raw)?;
            for p in face {
                let entry = normals.entry(key(p)).or_insert([0.; 3]);
                *entry = add(*entry, raw);
            }
            local.push(face);
        }
        pose.changed_vertices = moved.len();
        for (face, source) in local.into_iter().zip(&asset.faces) {
            let face_normal = unit(cross(sub(face[1], face[0]), sub(face[2], face[0])))?;
            let mut shaped = [[0.; 6]; 3];
            for (j, p) in face.into_iter().enumerate() {
                let original = source[j];
                let mut normal = if moved.contains(&position(&original).map(f64::to_bits)) {
                    unit(normals[&key(p)]).unwrap_or(face_normal)
                } else {
                    [original[3], original[4], original[5]]
                };
                if dot(normal, face_normal) <= 0.15 {
                    normal = face_normal;
                }
                shaped[j] = [p[0], p[1], p[2], normal[0], normal[1], normal[2]]
                    .map(|v| (v * 1e6).round_ties_even() / 1e6);
            }
            pose.shape.push(shaped);
        }
    }
    let low: V3 = std::array::from_fn(|i| {
        bodies
            .iter()
            .flatten()
            .flatten()
            .map(|v| v[i])
            .fold(f64::INFINITY, f64::min)
    });
    let high: V3 = std::array::from_fn(|i| {
        bodies
            .iter()
            .flatten()
            .flatten()
            .map(|v| v[i])
            .fold(f64::NEG_INFINITY, f64::max)
    });
    for pose in &mut poses {
        for i in 0..2 {
            pose.at[i] -= (low[i] + high[i]) / 2.;
        }
    }
    Ok(Group {
        size: sub(high, low),
        poses,
    })
}

pub fn bake(source: &[u8]) -> Result<Vec<u8>, String> {
    let catalog: serde_json::Value = serde_json::from_slice(source).map_err(|e| e.to_string())?;
    let mut output = BTreeMap::new();
    for name in ["rice-sack", "dry-goods-bag"] {
        let asset: Asset =
            serde_json::from_value(catalog[name].clone()).map_err(|e| e.to_string())?;
        if asset.faces.is_empty()
            || asset.faces.len() > 50_000
            || asset.size.iter().any(|v| !v.is_finite() || *v <= 0.)
        {
            return Err(format!("Invalid bag mesh {name}."));
        }
        let mut variants = BTreeMap::new();
        for (key, mirrored) in [("regular", false), ("mirrored", true)] {
            let mut groups = Vec::new();
            for (scales, yaws, depths, labels) in [
                (
                    [1., 0.80, 1.08],
                    [-0.35, 0.17, 0.48],
                    [0., 0.04, 0.04],
                    [0, 2, 1],
                ),
                (
                    [0.84, 1.10, 0.94],
                    [0.42, -0.28, 0.06],
                    [0.03, -0.04, 0.06],
                    [3, 0, 2],
                ),
                (
                    [1.08, 0.88, 0.78],
                    [-0.55, 0.28, -0.08],
                    [-0.04, 0.05, 0.],
                    [1, 3, 0],
                ),
                (
                    [0.90, 0.78, 1.08],
                    [0.15, -0.45, 0.42],
                    [0.04, -0.05, 0.03],
                    [2, 1, 3],
                ),
            ] {
                groups.push(cluster(&asset, scales, yaws, depths, labels, mirrored)?);
            }
            variants.insert(key, groups);
        }
        output.insert(name, variants);
    }
    let json = serde_json::to_vec(&output).map_err(|e| e.to_string())?;
    Ok(json)
}

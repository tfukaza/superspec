use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    f64::consts::{PI, TAU},
};
use superspec_geometry::{
    V3,
    math::{add, cross, dot, scale, sub, unit},
};

type Vertex = [f64; 11];
type Triangle = [Vertex; 3];
type RawTriangle = [[f64; 10]; 3];

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct Asset {
    size: V3,
    faces: Vec<Triangle>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    painted_faces: Vec<Triangle>,
    #[serde(flatten)]
    metadata: BTreeMap<String, Value>,
}

fn xyz(v: Vertex) -> V3 {
    [v[0], v[1], v[2]]
}

fn geometric([a, b, c]: Triangle) -> V3 {
    cross(sub(xyz(b), xyz(a)), sub(xyz(c), xyz(a)))
}

fn cavity(mut face: Triangle) -> Result<Triangle, String> {
    let mut normal = unit(geometric(face))?;
    if normal[2] < 0. {
        face.reverse();
        normal = scale(normal, -1.);
    }
    for vertex in &mut face {
        vertex[3..6].copy_from_slice(&normal);
    }
    Ok(face)
}

fn height(triangles: &[Triangle], x: f64, y: f64) -> Result<f64, String> {
    triangles
        .iter()
        .filter_map(|&[a, b, c]| {
            let determinant = (b[1] - c[1]) * (a[0] - c[0]) + (c[0] - b[0]) * (a[1] - c[1]);
            if determinant.abs() < 1e-12 {
                return None;
            }
            let u = ((b[1] - c[1]) * (x - c[0]) + (c[0] - b[0]) * (y - c[1])) / determinant;
            let v = ((c[1] - a[1]) * (x - c[0]) + (a[0] - c[0]) * (y - c[1])) / determinant;
            let w = 1. - u - v;
            (u.min(v).min(w) >= -1e-7).then_some(u * a[2] + v * b[2] + w * c[2])
        })
        .max_by(f64::total_cmp)
        .ok_or_else(|| "Egg footprint lies outside the basket cavity.".into())
}

fn egg_basket(basket: &mut Asset) -> Result<(), String> {
    let mut shell = Vec::new();
    let mut interior = Vec::new();
    for &face in &basket.faces {
        let centers: Vec<_> = face
            .iter()
            .enumerate()
            .filter(|(_, v)| v[0].hypot(v[1]) < 0.01)
            .map(|(i, _)| i)
            .collect();
        let index = match centers.as_slice() {
            [] => {
                shell.push(face);
                continue;
            }
            [index] => *index,
            _ => return Err("Basket closing triangle has multiple center vertices.".into()),
        };
        let [center, a, b] = [face[index], face[(index + 1) % 3], face[(index + 2) % 3]];
        let inner = |edge: Vertex| {
            let fraction = 0.052 / edge[0].hypot(edge[1]);
            let mut vertex = std::array::from_fn(|i| center[i] + (edge[i] - center[i]) * fraction);
            vertex[2] = 0.007;
            vertex
        };
        let mut floor = center;
        floor[2] = 0.007;
        let (ia, ib) = (inner(a), inner(b));
        interior.extend([
            cavity([floor, ia, ib])?,
            cavity([ia, a, b])?,
            cavity([ia, b, ib])?,
        ]);
    }
    if interior.len() != 144 {
        return Err("Basket source must have a 48-triangle closing fan.".into());
    }
    let mut placements: Vec<_> = (0..6)
        .map(|i| {
            let angle = f64::from(i) * TAU / 6. + 0.12;
            (
                0.05 * angle.cos(),
                0.05 * angle.sin(),
                angle + PI / 2.,
                1.4 + 0.12 * angle.sin(),
            )
        })
        .collect();
    placements.extend([
        (-0.0165, -0.003, PI / 2. - 0.05, 1.45),
        (0.0165, 0.003, PI / 2. + 0.05, 1.5),
    ]);
    let vertex = interior[0][0];
    let mut painted = Vec::new();
    for (x, y, yaw, tilt) in placements {
        let (sy, cy) = yaw.sin_cos();
        let (st, ct) = tilt.sin_cos();
        let rotate = |[x, y, z]: V3| {
            let q = ct * x + st * z;
            [cy * q - sy * y, sy * q + cy * y, -st * x + ct * z]
        };
        let mut rings = [[[0.; 11]; 10]; 5];
        let mut support = f64::NEG_INFINITY;
        for (ring, vertices) in rings.iter_mut().enumerate() {
            let (sp, cp) = (PI * ring as f64 / 4.).sin_cos();
            for (side, v) in vertices.iter_mut().enumerate() {
                let (sin, cos) = (TAU * side as f64 / 10.).sin_cos();
                let radius = 0.016 * sp * (1. - 0.12 * cp);
                let [px, py, pz] =
                    add(rotate([radius * cos, radius * sin, 0.025 * cp]), [x, y, 0.]);
                let [nx, ny, nz] = unit(rotate([sp * cos, sp * sin, 0.64 * cp]))?;
                *v = [
                    px, py, pz, nx, ny, nz, vertex[6], vertex[7], 0.78, 0.70, 0.56,
                ];
                support = support.max(height(&interior, px, py)? - pz);
            }
        }
        for v in rings.iter_mut().flatten() {
            v[2] += support;
        }
        for ring in rings.windows(2) {
            for i in 0..10 {
                let j = (i + 1) % 10;
                let (a, b, c, d) = (ring[0][i], ring[1][i], ring[1][j], ring[0][j]);
                for mut face in [[a, b, c], [a, c, d]] {
                    let normal = geometric(face);
                    if dot(normal, normal) < 1e-18 {
                        continue;
                    }
                    if dot(normal, [a[3], a[4], a[5]]) < 0. {
                        face.reverse();
                    }
                    painted.push(face);
                }
            }
        }
    }
    shell.extend(interior);
    basket.faces = shell;
    basket.painted_faces = painted;
    remeasure(basket)
}

fn remeasure(asset: &mut Asset) -> Result<(), String> {
    let mut low = [f64::INFINITY; 3];
    let mut high = [f64::NEG_INFINITY; 3];
    for vertex in asset.faces.iter().chain(&asset.painted_faces).flatten() {
        for i in 0..3 {
            low[i] = low[i].min(vertex[i]);
            high[i] = high[i].max(vertex[i]);
        }
    }
    if !low[0].is_finite() {
        return Err("Cannot measure empty stock mesh.".into());
    }
    let origin = [(low[0] + high[0]) / 2., (low[1] + high[1]) / 2., low[2]];
    let round = |x: f64| (x * 1e6).round_ties_even() / 1e6;
    for vertex in asset
        .faces
        .iter_mut()
        .chain(&mut asset.painted_faces)
        .flatten()
    {
        for i in 0..3 {
            vertex[i] = round(vertex[i] - origin[i]);
        }
    }
    asset.size = sub(high, low).map(round);
    Ok(())
}

pub fn finish(source: &[u8]) -> Result<Vec<u8>, String> {
    let mut assets: BTreeMap<String, Asset> =
        serde_json::from_slice(source).map_err(|e| e.to_string())?;
    egg_basket(assets.get_mut("egg-basket").ok_or("Missing egg-basket.")?)?;
    remeasure(
        assets
            .get_mut("bottle-load")
            .ok_or("Missing bottle-load.")?,
    )?;
    let mut bytes = serde_json::to_vec(&assets).map_err(|e| e.to_string())?;
    bytes.push(b'\n');
    Ok(bytes)
}

const PITCH: f64 = 0.074;

fn crate_faces(size: V3, tile: usize, load: V3) -> Result<Vec<RawTriangle>, String> {
    let [w, d, h] = size;
    let pitch = PITCH
        * (w * 0.9 / load[0])
            .min(d * 0.85 / load[1])
            .min(h * 1.2 / load[2]);
    let floor = h * 0.075;
    let mut faces = Vec::new();
    let mut beam = |low, high, beveled| -> Result<(), String> {
        let span = sub(high, low);
        let center = scale(add(low, high), 0.5);
        let axis = (0..3)
            .rev()
            .max_by(|&i, &j| span[i].total_cmp(&span[j]))
            .unwrap();
        let axes: Vec<_> = (0..3).filter(|&i| i != axis).collect();
        let (u, v) = (axes[0], axes[1]);
        let (a, b) = (span[u] / 2., span[v] / 2.);
        let cut = a.min(b) * 0.22;
        let ring = if beveled {
            vec![
                [-a + cut, -b],
                [a - cut, -b],
                [a, -b + cut],
                [a, b - cut],
                [a - cut, b],
                [-a + cut, b],
                [-a, b - cut],
                [-a, -b + cut],
            ]
        } else {
            vec![[-a, -b], [a, -b], [a, b], [-a, b]]
        };
        let [left, right] = [low[axis], high[axis]].map(|coordinate| {
            ring.iter()
                .map(|[x, y]| {
                    let mut point = center;
                    point[axis] = coordinate;
                    point[u] += x;
                    point[v] += y;
                    point
                })
                .collect::<Vec<_>>()
        });
        let sides = (0..ring.len())
            .map(|i| {
                let j = (i + 1) % ring.len();
                vec![left[i], left[j], right[j], right[i]]
            })
            .collect::<Vec<_>>();
        for mut polygon in [left, right].into_iter().chain(sides) {
            let mut normal = unit(cross(
                sub(polygon[1], polygon[0]),
                sub(polygon[2], polygon[0]),
            ))?;
            let centroid = scale(
                polygon.iter().copied().fold([0.; 3], add),
                1. / polygon.len() as f64,
            );
            if dot(normal, sub(centroid, center)) < 0. {
                polygon.reverse();
                normal = scale(normal, -1.);
            }
            let vertices = polygon
                .into_iter()
                .enumerate()
                .map(|(i, [x, y, z])| {
                    let u = 0.035 + if matches!(i, 1 | 2) { 0.235 } else { 0. };
                    let v = 0.335 + if i >= 2 { 0.020 } else { 0. };
                    let atlas_u = ((tile % 5 * 512 + 4) as f64 + u * 504.) / 2560.;
                    let atlas_v = (((3 - tile / 5) * 512 + 4) as f64 + v * 504.) / 2048.;
                    [
                        x, y, z, normal[0], normal[1], normal[2], atlas_u, atlas_v, u, v,
                    ]
                })
                .collect::<Vec<_>>();
            faces.extend(
                vertices[1..]
                    .windows(2)
                    .map(|pair| [vertices[0], pair[0], pair[1]]),
            );
        }
        Ok(())
    };
    let (x, y, post) = (w / 2., d / 2., 0.022);
    for a in [-x, x - post] {
        for b in [-y, y - post] {
            beam([a, b, 0.], [a + post, b + post, h], true)?;
        }
    }
    for z in [0., h - 0.023] {
        for b in [-y, y - 0.018] {
            beam([-x + post, b, z], [x - post, b + 0.018, z + 0.023], true)?;
        }
        for a in [-x, x - 0.018] {
            beam([a, -y + post, z], [a + 0.018, y - post, z + 0.023], true)?;
        }
    }
    for b in [-y, y - 0.012] {
        beam([-x + post, b, 0.14], [x - post, b + 0.012, 0.151], false)?;
        for a in [-w / 4., 0., w / 4.] {
            beam([a - 0.0045, b, 0.023], [a + 0.0045, b + 0.012, 0.14], false)?;
        }
    }
    for a in [-x, x - 0.012] {
        beam([a, -y + post, 0.14], [a + 0.012, y - post, 0.151], false)?;
        for b in [-d / 6., d / 6.] {
            beam([a, b - 0.0045, 0.023], [a + 0.012, b + 0.0045, 0.14], false)?;
        }
    }
    for i in 0..4 {
        let a = -w * 0.34 + f64::from(i) * w * 0.68 / 3.;
        beam(
            [a - 0.009, -y + post, 0.007],
            [a + 0.009, y - post, floor],
            false,
        )?;
    }
    for b in [-d * 0.3, 0., d * 0.3] {
        beam(
            [-x + post, b - 0.007, 0.007],
            [x - post, b + 0.007, floor],
            false,
        )?;
    }
    for a in [-pitch, 0., pitch] {
        beam(
            [a - 0.001, -y + post, floor],
            [a + 0.001, y - post, 0.105],
            false,
        )?;
    }
    for b in [-pitch / 2., pitch / 2.] {
        beam(
            [-x + post, b - 0.001, floor],
            [x - post, b + 0.001, 0.105],
            false,
        )?;
    }
    Ok(faces)
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct RawAsset {
    size: V3,
    faces: Vec<RawTriangle>,
    tile: usize,
    #[serde(flatten)]
    metadata: BTreeMap<String, Value>,
}

pub fn prepare(source: &[u8]) -> Result<Vec<u8>, String> {
    let mut assets: BTreeMap<String, RawAsset> =
        serde_json::from_slice(source).map_err(|e| e.to_string())?;
    let bottle = assets
        .remove("bottle-load-source")
        .ok_or("Missing low-detail bottle.")?;
    if bottle.size.iter().any(|&size| size <= 0.) {
        return Err("Low-detail bottle has invalid dimensions.".into());
    }
    let factor = (0.07 / bottle.size[0])
        .min(0.07 / bottle.size[1])
        .min(0.27 / bottle.size[2]);
    let size = add([3. * PITCH, 2. * PITCH, 0.], scale(bottle.size, factor));
    let mut faces = Vec::new();
    for row in 0..3 {
        for column in 0..4 {
            let x = (f64::from(column) - 1.5) * PITCH;
            let y = (f64::from(row) - 1.) * PITCH;
            faces.extend(
                bottle
                    .faces
                    .iter()
                    .filter(|face| face.iter().any(|v| v[2] >= 0.003))
                    .map(|face| {
                        face.map(|mut v| {
                            v[0] = v[0] * factor + x;
                            v[1] = v[1] * factor + y;
                            v[2] *= factor;
                            v
                        })
                    }),
            );
        }
    }
    let crate_asset = assets
        .get_mut("bottle-crate")
        .ok_or("Missing bottle-crate.")?;
    if crate_asset.tile >= 20 {
        return Err("Bottle crate tile falls outside the atlas.".into());
    }
    crate_asset.faces = crate_faces(crate_asset.size, crate_asset.tile, size)?;
    assets.insert(
        "bottle-load".into(),
        RawAsset {
            size,
            faces,
            tile: bottle.tile,
            metadata: BTreeMap::from([(
                "sourceTexture".into(),
                Value::String("drink-bottle".into()),
            )]),
        },
    );
    let bytes = serde_json::to_vec(&assets).map_err(|e| e.to_string())?;
    Ok(bytes)
}

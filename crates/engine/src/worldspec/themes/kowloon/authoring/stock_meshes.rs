use std::f64::consts::PI;
use superspec_geometry::image::Image;
use superspec_geometry::{
    V3,
    math::{add, cross, dot, scale, sub, unit},
};
use superspec_types::rng::SeededRng;

pub type Face = [[f64; 8]; 3];

pub fn encode_faces(faces: &mut [Face]) -> Result<Vec<u8>, String> {
    for value in faces.iter_mut().flatten().flatten() {
        *value = (*value * 100_000.).round_ties_even() / 100_000.;
    }
    let mut bytes = serde_json::to_vec(faces).map_err(|e| e.to_string())?;
    bytes.push(b'\n');
    Ok(bytes)
}

pub fn apples(source: &[u8]) -> Result<Vec<u8>, String> {
    let apple: Vec<Face> = serde_json::from_slice(source).map_err(|e| e.to_string())?;
    let mut rng = SeededRng::from_u32(871);
    let mut items = Vec::new();
    for (columns, rows, z) in [(6, 3, 0.), (3, 2, 0.4)] {
        for row in 0..rows {
            for column in 0..columns {
                let x = (f64::from(column) - f64::from(columns - 1) / 2.) * 0.149;
                let y = (f64::from(row) - f64::from(rows - 1) / 2.) * 0.28;
                items.push((
                    x + rng.float(-0.008, 0.008),
                    y + rng.float(-0.009, 0.009),
                    z,
                    rng.float(-PI, PI),
                ));
            }
        }
    }
    let mut faces = Vec::new();
    for (index, &(x, y, z, angle)) in items.iter().enumerate() {
        let (s, c) = angle.sin_cos();
        for &face in &apple {
            let transformed = face.map(|[px, py, pz, nx, ny, nz, u, v]| {
                [
                    x + (px * c - py * s) * 0.156,
                    y + (px * s + py * c) * 0.218,
                    z + pz * 0.60,
                    (nx * c - ny * s) / 0.156,
                    (nx * s + ny * c) / 0.218,
                    nz / 0.60,
                    u,
                    v,
                ]
            });
            if transformed.iter().all(|p| p[2] < 0.15) {
                continue;
            }
            if items.iter().enumerate().any(|(other, &(ox, oy, oz, _))| {
                other != index
                    && transformed.iter().all(|p| {
                        ((p[0] - ox) / 0.069).powi(2)
                            + ((p[1] - oy) / 0.095).powi(2)
                            + ((p[2] - oz - 0.30) / 0.26).powi(2)
                            < 1.
                    })
            }) {
                continue;
            }
            faces.push(transformed);
        }
    }
    encode_faces(&mut faces)
}

pub fn packet_faces(aspect: f64) -> Result<Vec<Face>, String> {
    if !aspect.is_finite() || aspect <= 0. {
        return Err("Packet aspect ratio must be positive and finite.".into());
    }
    const GRID: usize = 5;
    const EDGE: usize = GRID - 1;
    const SIDE: usize = GRID * GRID;
    let mut points = Vec::new();
    for side in [-1., 1.] {
        for row in 0..GRID {
            let z = row as f64 / EDGE as f64;
            for column in 0..GRID {
                let t = column as f64 / EDGE as f64;
                let sine = (PI * z).sin();
                let x = (t - 0.5) * (1. - 0.12 * sine);
                let bulge = (0.025 + 0.475 * sine) * (1. - 0.68 * (2. * t - 1.).abs().powi(3));
                let wrinkle = 0.025 * (column as f64 * 2.1 + row as f64 * 0.7).sin() * sine;
                points.push([x, side * (bulge + wrinkle), z]);
            }
        }
    }
    let thickness = 2. * points.iter().map(|p| p[1].abs()).fold(0., f64::max);
    for point in &mut points {
        point[1] /= thickness;
    }
    let mut triangles = Vec::new();
    for side in 0..2 {
        for row in 0..EDGE {
            for column in 0..EDGE {
                let a = side * SIDE + row * GRID + column;
                triangles.extend([[a, a + 1, a + GRID + 1], [a, a + GRID + 1, a + GRID]]);
            }
        }
    }
    let perimeter: Vec<_> = (0..GRID)
        .chain((1..GRID).map(|row| row * GRID + EDGE))
        .chain((GRID * EDGE..SIDE - 1).rev())
        .chain((1..EDGE).rev().map(|row| row * GRID))
        .collect();
    for (&a, &b) in perimeter.iter().zip(perimeter.iter().cycle().skip(1)) {
        triangles.extend([[a, b, b + SIDE], [a, b + SIDE, a + SIDE]]);
    }
    let mut normals: [V3; SIDE * 2] = [[0.; 3]; SIDE * 2];
    for triangle in &mut triangles {
        let [a, b, c] = triangle.map(|i| points[i]);
        let mut normal = cross(sub(b, a), sub(c, a));
        let center = sub(scale(add(add(a, b), c), 1. / 3.), [0., 0., 0.5]);
        if dot(normal, center) < 0. {
            triangle.reverse();
            normal = scale(normal, -1.);
        }
        for &i in triangle.iter() {
            normals[i] = add(normals[i], normal);
        }
    }
    Ok(triangles
        .into_iter()
        .map(|face| {
            face.map(|i| {
                let [x, y, z] = points[i];
                let [nx, ny, nz] = unit(normals[i]).expect("packet grid has nonzero normals");
                [
                    x,
                    y,
                    z,
                    nx,
                    ny,
                    nz,
                    0.5 + x * aspect / 1.02,
                    0.5 + (z - 0.5) / 1.02,
                ]
            })
        })
        .collect())
}

pub fn packet_image(mut extended: Image) -> Result<Image, String> {
    let pixels = extended.rgba.clone();
    let width = extended.width;
    let rows: Vec<_> = (0..extended.height)
        .map(|y| {
            (0..width)
                .filter(|x| pixels[(y * width + x) * 4 + 3] >= 243)
                .collect::<Vec<_>>()
        })
        .collect();
    let occupied: Vec<_> = rows
        .iter()
        .enumerate()
        .filter(|(_, row)| !row.is_empty())
        .map(|(i, _)| i)
        .collect();
    if occupied.is_empty() {
        return Err("Packet render contains no opaque pixels.".into());
    }
    for y in 0..extended.height {
        let sy = *occupied.iter().min_by_key(|&&row| y.abs_diff(row)).unwrap();
        let row = &rows[sy];
        let left = row[0];
        let right = row[row.len() - 1];
        let inset = 2.min((right - left) / 2);
        for x in 0..width {
            let column = x.clamp(left + inset, right - inset);
            let split = row.partition_point(|&candidate| candidate < column);
            let low = row[split.saturating_sub(1)];
            let high = row[split.min(row.len() - 1)];
            let sx = if column.abs_diff(low) <= column.abs_diff(high) {
                low
            } else {
                high
            };
            let source = (sy * width + sx) * 4;
            let target = (y * width + x) * 4;
            extended.rgba[target..target + 3].copy_from_slice(&pixels[source..source + 3]);
            extended.rgba[target + 3] = 255;
        }
    }
    Ok(extended)
}

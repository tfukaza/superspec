use super::{Files, Inputs};
use super::{
    Image,
    clean_stock::{paste, pixel, sample},
    image,
};
use serde_json::{Value, json};
use std::path::Path;
type Vertex = [f64; 11];

fn maps(inputs: &Inputs<'_>, path: &Path, roughness: f64) -> Result<[Image; 3], String> {
    crate::materialspec::authoring::refine::maps(&inputs.decode(path)?, roughness)
}

fn region(tile: usize) -> [f64; 4] {
    let (x, y) = (tile % 5 * 512 + 4, tile / 5 * 512 + 4);
    [
        x as f64 / 2560.,
        1. - (y + 504) as f64 / 2048.,
        (x + 504) as f64 / 2560.,
        1. - y as f64 / 2048.,
    ]
}

fn corner(point: [f64; 3], normal: [f64; 3], u: f64, v: f64, tile: usize, color: &Image) -> Vertex {
    let (u, v) = (u.clamp(0.01, 0.99), v.clamp(0.01, 0.99));
    let r = region(tile);
    let x = (u * 503.).round_ties_even() as usize;
    let y = ((1. - v) * 503.).round_ties_even() as usize;
    let rgb = pixel(color, x, y).map(|p| {
        let p = f64::from(p) / 255.;
        if p <= 0.04045 {
            p / 12.92
        } else {
            ((p + 0.055) / 1.055).powf(2.4)
        }
    });
    [
        point[0],
        point[1],
        point[2],
        normal[0],
        normal[1],
        normal[2],
        r[0] + u * (r[2] - r[0]),
        r[1] + v * (r[3] - r[1]),
        rgb[0],
        rgb[1],
        rgb[2],
    ]
}

fn board(faces: &mut Vec<[Vertex; 3]>, low: [f64; 3], high: [f64; 3], tile: usize, color: &Image) {
    let span = superspec_geometry::math::sub(high, low);
    let length = (0..3).max_by(|&a, &b| span[a].total_cmp(&span[b])).unwrap();
    let points: [[f64; 3]; 8] = std::array::from_fn(|i| {
        let axes = [
            [0, 0, 0],
            [1, 0, 0],
            [1, 1, 0],
            [0, 1, 0],
            [0, 0, 1],
            [1, 0, 1],
            [1, 1, 1],
            [0, 1, 1],
        ][i];
        std::array::from_fn(|axis| {
            if axes[axis] == 0 {
                low[axis]
            } else {
                high[axis]
            }
        })
    });
    let crop = (faces.len() as f64 * 0.017) % 0.2;
    for (ids, normal) in [
        ([0, 4, 7, 3], [-1_f64, 0., 0.]),
        ([1, 2, 6, 5], [1., 0., 0.]),
        ([0, 1, 5, 4], [0., -1., 0.]),
        ([3, 7, 6, 2], [0., 1., 0.]),
        ([0, 3, 2, 1], [0., 0., -1.]),
        ([4, 5, 6, 7], [0., 0., 1.]),
    ] {
        let across = (0..3)
            .find(|&i| i != length && normal[i].abs() < 0.5)
            .unwrap();
        let along = if normal[length].abs() < 0.5 {
            length
        } else {
            (0..3)
                .find(|&i| i != across && normal[i].abs() < 0.5)
                .unwrap()
        };
        let [a, b, c, d] = ids.map(|i| {
            let p = points[i];
            corner(
                p,
                normal,
                0.05 + crop + (p[across] - low[across]) / span[across] * 0.65,
                0.04 + (p[along] - low[along]) / span[along] * 0.9,
                tile,
                color,
            )
        });
        faces.extend([[a, b, c], [a, c, d]]);
    }
}

pub fn bake(inputs: &Inputs<'_>, root: &Path, source: &Path) -> Result<Files, String> {
    let textures = root.join("projects/dressing-demo/textures");
    let materials = root.join("projects/kowloon-liquor-props/materials");
    let read = |path: &Path| -> Result<Value, String> {
        serde_json::from_slice(&(inputs.read)(path)?).map_err(|e| e.to_string())
    };
    let source = read(source)?;
    let mut catalog: Value = serde_json::from_slice(&(inputs.catalog)("grocer-props.json")?)
        .map_err(|e| e.to_string())?;
    let names = [
        "grocer-props.png",
        "grocer-props-normal.png",
        "grocer-props-properties.png",
    ];
    let mut atlases = [
        inputs.decode(&textures.join(names[0]))?,
        inputs.decode(&textures.join(names[1]))?,
        inputs.decode(&textures.join(names[2]))?,
    ];
    if atlases.iter().any(|a| (a.width, a.height) != (2560, 2048)) {
        return Err("Expected three 2560 × 2048 stock atlases.".into());
    }
    let wood = maps(inputs, &materials.join("worn-shop-wood.png"), 205.)?;
    let plastic = maps(inputs, &materials.join("worn-crate-plastic.png"), 175.)?;
    let tile = |name: &str| {
        source[name]["tile"]
            .as_u64()
            .filter(|t| *t < 20)
            .map(|t| t as usize)
            .ok_or_else(|| format!("Missing atlas tile for {name}."))
    };
    let wood_tile = tile("shop-stool")?;
    for (name, maps) in [("shop-stool", &wood), ("bottle-crate", &plastic)] {
        let tile = tile(name)?;
        let region = region(tile);
        for (atlas, map) in atlases.iter_mut().zip(maps) {
            paste(atlas, map, region)?;
        }
        let size: [f64; 3] =
            serde_json::from_value(catalog[name]["size"].clone()).map_err(|e| e.to_string())?;
        if size.iter().any(|v| *v <= 0.) {
            return Err("Invalid stock prop size.".into());
        }
        let mut faces: Vec<Vec<Vertex>> =
            serde_json::from_value(catalog[name]["faces"].clone()).map_err(|e| e.to_string())?;
        for face in &mut faces {
            let sum = |axis: usize| face.iter().map(|v| v[axis + 3]).sum::<f64>().abs();
            let axis = (0..3).max_by(|&a, &b| sum(a).total_cmp(&sum(b))).unwrap();
            let horizontal = usize::from(axis == 0);
            for v in face {
                let [x, y, z] = [v[0], v[1], v[2]];
                let (u, t) = if name == "shop-stool" && z > size[2] * 0.78 {
                    (0.5 + y / size[1] * 0.9, 0.5 + x / size[0] * 0.9)
                } else if axis == 2 {
                    (0.5 + x / size[0] * 0.9, 0.5 + y / size[1] * 0.9)
                } else {
                    (
                        0.5 + v[horizontal] / size[horizontal] * 0.9,
                        0.04 + z / size[2] * 0.92,
                    )
                };
                *v = corner([x, y, z], [v[3], v[4], v[5]], u, t, tile, &maps[0]);
                let rgb = sample(&atlases[0], region, u, t);
                v[8..11].copy_from_slice(&rgb);
            }
        }
        catalog[name]["faces"] = json!(faces);
    }
    let mut faces = Vec::new();
    let mut plank = |low, high| board(&mut faces, low, high, wood_tile, &wood[0]);
    for row in 0..3 {
        let y = -0.155 + row as f64 * 0.103333;
        plank([-0.2, y, 0.], [0.2, y + 0.103333, 0.014]);
    }
    for z in [0.014, 0.045] {
        for y in [-0.155, 0.143] {
            plank([-0.2, y, z], [0.2, y + 0.012, z + 0.026]);
        }
        for x in [-0.2, 0.188] {
            plank([x, -0.143, z], [x + 0.012, 0.143, z + 0.026]);
        }
    }
    for x in [-0.2, 0.182] {
        for y in [-0.155, 0.137] {
            plank([x, y, 0.014], [x + 0.018, y + 0.018, 0.071]);
        }
    }
    let mut fasteners = Vec::new();
    for (y, ny) in [(-0.15501, -1.), (0.15501, 1.)] {
        for x in [-0.188, 0.188] {
            for z in [0.026, 0.058] {
                let mut points = [
                    [x - 0.002, z - 0.002],
                    [x + 0.002, z - 0.002],
                    [x + 0.002, z + 0.002],
                    [x - 0.002, z + 0.002],
                ];
                if ny > 0. {
                    points.reverse();
                }
                let [a, b, c, d] =
                    points.map(|[x, z]| [x, y, z, 0., ny, 0., 0., 0., 0.12, 0.13, 0.12]);
                fasteners.extend([[a, b, c], [a, c, d]]);
            }
        }
    }
    catalog["produce-crate"] =
        json!({"size":[0.4,0.31002,0.071],"faces":faces,"paintedFaces":fasteners});
    let bytes = atlases
        .iter()
        .map(image::encode)
        .collect::<Result<Vec<_>, _>>()?;
    let mut files: Files = names
        .into_iter()
        .zip(bytes)
        .map(|(name, bytes)| (name.into(), bytes))
        .collect();
    files.push((
        "grocer-props.json".into(),
        serde_json::to_vec(&catalog).map_err(|e| e.to_string())?,
    ));
    Ok(files)
}

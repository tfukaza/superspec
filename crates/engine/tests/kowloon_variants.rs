//! A survey of how many distinct shapes each part type of the Kowloon building draws, and the
//! checks that the variant catalogue keeps them few.
//!
//! `cargo test -p superspec-engine --test kowloon_variants -- --ignored --nocapture survey`
//! prints, per part type, the instances, distinct shapes and triangles of one building and of
//! the Stoa city's prototypes at each graphics detail, and writes the same table to
//! `generated/graphics-detail/variants/survey.txt` (or `VARIANT_SURVEY_OUTPUT`).
use serde_json::json;
use std::{
    collections::{BTreeMap, BTreeSet},
    hash::{Hash, Hasher},
    sync::Arc,
};
use superspec_engine::{
    itemspec::definition::{self as item, Environment, GeneratedItem, ItemGeometry, Values},
    worldspec::themes::kowloon::items,
};

fn library() -> BTreeMap<String, String> {
    let directory =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/worldspec/themes/kowloon/items");
    std::fs::read_dir(directory)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.to_string_lossy().ends_with(".item.json"))
        .map(|path| {
            (
                path.file_name().unwrap().to_string_lossy().into_owned(),
                std::fs::read_to_string(&path).unwrap(),
            )
        })
        .collect()
}

fn generate(name: &str, values: &Values) -> GeneratedItem {
    let files = library();
    let reader = |name: &str| -> Result<Arc<[u8]>, String> {
        files
            .get(name)
            .map(|text| Arc::from(text.as_bytes()))
            .ok_or(format!("Missing {name}"))
    };
    let definition = item::parse(name, &files[name]).unwrap();
    let mut environment = Environment::new(&reader).with_natives(items::natives());
    item::prepare_in(&definition, values, &mut environment, &|| false)
        .unwrap_or_else(|error| panic!("{name}: {error}"))
        .build(&|| false)
        .unwrap_or_else(|error| panic!("{name}: {error}"))
}

/// One prototype of the Stoa city (`levelspec/projects/klali-stoa/scene.aml`), as `city.rs`
/// hands it to the building item, and how many buildings stand from it.
struct Prototype {
    name: &'static str,
    floors: usize,
    seed: usize,
    trade: Option<&'static str>,
    courtyard: bool,
    placed: usize,
}

const STOA: [Prototype; 7] = [
    Prototype {
        name: "5 floors, seed 8",
        floors: 5,
        seed: 8,
        trade: None,
        courtyard: false,
        placed: 6,
    },
    Prototype {
        name: "4 floors, seed 4",
        floors: 4,
        seed: 4,
        trade: None,
        courtyard: false,
        placed: 8,
    },
    Prototype {
        name: "4 floors, seed 4, roast shop",
        floors: 4,
        seed: 4,
        trade: Some("roast"),
        courtyard: false,
        placed: 1,
    },
    Prototype {
        name: "6 floors, seed 6",
        floors: 6,
        seed: 6,
        trade: None,
        courtyard: false,
        placed: 5,
    },
    Prototype {
        name: "3 floors, seed 9",
        floors: 3,
        seed: 9,
        trade: None,
        courtyard: false,
        placed: 4,
    },
    Prototype {
        name: "7 floors, seed 7",
        floors: 7,
        seed: 7,
        trade: None,
        courtyard: false,
        placed: 4,
    },
    Prototype {
        name: "courtyard, 2 floors, seed 6",
        floors: 2,
        seed: 6,
        trade: None,
        courtyard: true,
        placed: 1,
    },
];

impl Prototype {
    /// `budget`: the variant budget, -1 for the graphics detail's own and 0 for none.
    fn budgeted(&self, level: u8, budget: i64) -> Values {
        let mut values = self.values(level);
        values.insert("variantBudget".into(), budget.into());
        values
    }
    fn values(&self, level: u8) -> Values {
        let families: Vec<Values> = serde_json::from_str(include_str!(
            "../src/worldspec/themes/kowloon/materials/building-families.json"
        ))
        .unwrap();
        let mut values = families[self.seed % families.len()].clone();
        if let Some(trade) = self.trade {
            for (plan, value) in [("shopPlans", "open"), ("tradePlans", trade)] {
                let rows = values.entry(plan.into()).or_insert_with(|| json!([[]]));
                let row = rows[0].as_array_mut().unwrap();
                if row.is_empty() {
                    row.push(value.into());
                } else {
                    row[0] = value.into();
                }
            }
        }
        let footprint = if self.courtyard {
            values.insert("groundShops".into(), false.into());
            values.insert("pavement".into(), false.into());
            json!([[-6.9, -8.9], [6.9, -8.9], [6.9, 8.9], [-6.9, 8.9]])
        } else {
            json!([[0, 0], [9.8, 0], [9.8, 7.], [0, 7.]])
        };
        values.insert("footprint".into(), footprint);
        values.insert("seed".into(), self.seed.into());
        values.insert("floors".into(), self.floors.into());
        values.insert("graphicsDetail".into(), level.into());
        values
    }
}

/// The kind of part a leaf part is: the last two named steps of its path, without indices
/// (`aircon/condenser`, `floor/cage`). A building flattens its nested items, so a leaf part
/// is known by its meshes' shared path: `side-1/part/floor-0/floor-0/part/cage-1/cage-1`.
fn kind(path: &str) -> String {
    let mut steps: Vec<&str> = Vec::new();
    for step in path.split('/') {
        let step = step.trim_end_matches(|c: char| c.is_ascii_digit() || c == '-');
        if !step.is_empty() && step != "part" && steps.last() != Some(&step) {
            steps.push(step);
        }
    }
    steps[steps.len().saturating_sub(2)..].join("/")
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
struct Shape {
    /// The triangles up to a rigid motion (or a reflection), with their slots.
    geometry: u64,
    /// `geometry` and where each triangle samples its slot's texture.
    textured: u64,
    /// `textured` and every vertex colour.
    coloured: u64,
}

fn hashed(value: &impl Hash) -> u64 {
    #[allow(deprecated)]
    let mut hasher = std::hash::SipHasher::new();
    value.hash(&mut hasher);
    hasher.finish()
}

/// The shape of a part's meshes: each triangle's slot, sorted edge lengths and distance from
/// the part's area centroid, in hundredths of a millimetre, as a sorted list. Two parts that
/// differ by a rotation and a translation have the same list; so do mirror images.
fn shape(geometry: &ItemGeometry, meshes: &[usize]) -> (Shape, usize) {
    let q = |v: f64| (v * 1e5).round() as i64;
    let sub = |a: [f64; 3], b: [f64; 3]| [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
    let len = |a: [f64; 3]| (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt();
    let mut centre = [0.; 3];
    let mut total = 0.;
    let mut triangles = Vec::new();
    for &mesh in meshes {
        let mesh = &geometry.meshes[mesh];
        for corner in mesh.indices.chunks_exact(3) {
            let p = [0, 1, 2].map(|k| mesh.positions[corner[k] as usize]);
            let (u, v) = (sub(p[1], p[0]), sub(p[2], p[0]));
            let area = len([
                u[1] * v[2] - u[2] * v[1],
                u[2] * v[0] - u[0] * v[2],
                u[0] * v[1] - u[1] * v[0],
            ]) / 2.;
            let middle = [0, 1, 2].map(|k| (p[0][k] + p[1][k] + p[2][k]) / 3.);
            for k in 0..3 {
                centre[k] += middle[k] * area;
            }
            total += area;
            triangles.push((mesh, corner, p, middle));
        }
    }
    if total > 0. {
        centre = centre.map(|v| v / total);
    }
    let count = triangles.len();
    let mut plain = Vec::with_capacity(count);
    let mut textured = Vec::with_capacity(count);
    let mut coloured = Vec::with_capacity(count);
    for (mesh, corner, p, middle) in triangles {
        let mut edges = [
            q(len(sub(p[1], p[0]))),
            q(len(sub(p[2], p[1]))),
            q(len(sub(p[0], p[2]))),
        ];
        edges.sort_unstable();
        let base = (
            hashed(&mesh.material_slot),
            edges,
            q(len(sub(middle, centre))),
        );
        let mut uvs: Vec<[i64; 2]> = corner
            .iter()
            .filter_map(|&i| mesh.uvs.get(i as usize))
            .map(|uv| [q(uv[0] as f64), q(uv[1] as f64)])
            .collect();
        uvs.sort_unstable();
        let mut tints: Vec<[i64; 4]> = corner
            .iter()
            .filter_map(|&i| mesh.colors.get(i as usize))
            .map(|c| c.map(|v| (v as f64 * 255.).round() as i64))
            .collect();
        tints.sort_unstable();
        plain.push(base);
        textured.push((base, uvs.clone()));
        coloured.push((base, uvs, tints));
    }
    plain.sort_unstable();
    textured.sort_unstable();
    coloured.sort_unstable();
    (
        Shape {
            geometry: hashed(&plain),
            textured: hashed(&textured),
            coloured: hashed(&coloured),
        },
        count,
    )
}

#[derive(Clone, Default)]
struct Row {
    instances: usize,
    triangles: usize,
    geometry: BTreeSet<u64>,
    textured: BTreeSet<u64>,
    coloured: BTreeSet<u64>,
    /// Triangles of one instance of each distinct geometry.
    distinct_triangles: usize,
}

impl Row {
    fn add(&mut self, shape: Shape, triangles: usize, placed: usize) {
        self.instances += placed;
        self.triangles += triangles * placed;
        if self.geometry.insert(shape.geometry) {
            self.distinct_triangles += triangles;
        }
        self.textured.insert(shape.textured);
        self.coloured.insert(shape.coloured);
    }
}

/// The parts of one building by kind. A part that draws nothing is not counted.
fn parts(building: &GeneratedItem) -> Vec<(String, Shape, usize)> {
    leaves(&building.geometry)
        .into_iter()
        .filter_map(|(path, meshes)| {
            let (shape, triangles) = shape(&building.geometry, &meshes);
            (triangles > 0).then(|| (kind(&path), shape, triangles))
        })
        .collect()
}

/// Each leaf part's path and meshes: a mesh id is its part's path and a slot or face group.
fn leaves(geometry: &ItemGeometry) -> BTreeMap<String, Vec<usize>> {
    let mut leaves: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (index, mesh) in geometry.meshes.iter().enumerate() {
        let path = mesh.id.rsplit_once('/').map_or("", |(path, _)| path);
        leaves.entry(path.to_owned()).or_default().push(index);
    }
    leaves
}

fn table(title: &str, rows: &BTreeMap<String, Row>, most: usize) -> String {
    let mut sorted: Vec<_> = rows.iter().collect();
    sorted.sort_by_key(|(_, row)| std::cmp::Reverse(row.triangles));
    let mut text = format!(
        "{title}\n  {:>9} {:>8} {:>8} {:>8} {:>10} {:>10}  part\n",
        "instances", "shapes", "+uv", "+colour", "triangles", "distinct"
    );
    let mut total = Row::default();
    for (index, (name, row)) in sorted.iter().enumerate() {
        total.instances += row.instances;
        total.triangles += row.triangles;
        total.distinct_triangles += row.distinct_triangles;
        if index < most {
            text += &format!(
                "  {:>9} {:>8} {:>8} {:>8} {:>10} {:>10}  {name}\n",
                row.instances,
                row.geometry.len(),
                row.textured.len(),
                row.coloured.len(),
                row.triangles,
                row.distinct_triangles
            );
        }
    }
    text += &format!(
        "  {:>9} {:>8} {:>8} {:>8} {:>10} {:>10}  all parts\n",
        total.instances,
        sorted.iter().map(|(_, r)| r.geometry.len()).sum::<usize>(),
        sorted.iter().map(|(_, r)| r.textured.len()).sum::<usize>(),
        sorted.iter().map(|(_, r)| r.coloured.len()).sum::<usize>(),
        total.triangles,
        total.distinct_triangles
    );
    text
}

/// Instances, distinct shapes and triangles per part kind, for the first prototype alone and
/// for the whole Stoa city, at each graphics detail. Run with `--ignored --nocapture`.
#[test]
#[ignore = "a report, not a check"]
fn survey() {
    let most = std::env::var("VARIANT_SURVEY_ROWS")
        .ok()
        .and_then(|rows| rows.parse().ok())
        .unwrap_or(70);
    let mut report = String::new();
    for level in 0..=2u8 {
        let mut city: BTreeMap<String, Row> = BTreeMap::new();
        for (index, prototype) in STOA.iter().enumerate() {
            let building = generate("kowloon-building.item.json", &prototype.values(level));
            let mut one: BTreeMap<String, Row> = BTreeMap::new();
            for (kind, shape, triangles) in parts(&building) {
                one.entry(kind.clone())
                    .or_default()
                    .add(shape, triangles, 1);
                city.entry(kind)
                    .or_default()
                    .add(shape, triangles, prototype.placed);
            }
            if index == 0 {
                report += &table(
                    &format!("One building ({}), graphics detail {level}", prototype.name),
                    &one,
                    most,
                );
            }
        }
        report += &table(
            &format!("Stoa city (29 buildings, 7 prototypes), graphics detail {level}"),
            &city,
            most,
        );
    }
    println!("{report}");
    let output = std::env::var("VARIANT_SURVEY_OUTPUT").unwrap_or_else(|_| {
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../generated/graphics-detail/variants/survey.txt"
        )
        .into()
    });
    let output = std::path::Path::new(&output);
    std::fs::create_dir_all(output.parent().unwrap()).unwrap();
    std::fs::write(output, report).unwrap();
}

/// Lists part ids of the first prototype: `VARIANT_SURVEY_KIND=<substring>` narrows it.
#[test]
#[ignore = "a report, not a check"]
fn list_parts() {
    let filter = std::env::var("VARIANT_SURVEY_KIND").unwrap_or_default();
    let building = generate("kowloon-building.item.json", &STOA[0].values(0));
    let geometry = &building.geometry;
    for (path, meshes) in leaves(geometry) {
        if !path.contains(&filter) {
            continue;
        }
        let (shape, triangles) = shape(geometry, &meshes);
        println!(
            "{:>6} {:016x} {:016x} {:016x} {path}",
            triangles, shape.geometry, shape.textured, shape.coloured
        );
    }
}

/// The catalogue entries of the air conditioners: `--ignored --nocapture catalogue`.
#[test]
#[ignore = "a report, not a check"]
fn catalogue() {
    use superspec_engine::worldspec::themes::kowloon::aircon;
    // The span the building's item gives a condenser: 0.95 m wide, 0.6 m of free height.
    for line in aircon::catalogue(8, [-0.475, 0.475, 0., 0.6]) {
        println!("{line}");
    }
}
/// A plain orthographic picture of buildings in their vertex colours, lit from the upper
/// left: enough to compare shapes and tints between two budgets. It samples no texture.
struct Picture {
    width: usize,
    height: usize,
    depth: Vec<f32>,
    colour: Vec<[f32; 3]>,
}

type V = [f64; 3];
fn dot(a: V, b: V) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: V, b: V) -> V {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn unit(a: V) -> V {
    let length = dot(a, a).sqrt().max(1e-12);
    a.map(|v| v / length)
}

/// Draws `buildings` (each moved along X by its offset) looking along `yaw` degrees from
/// the front (-Y) and down by `pitch` degrees, at `scale` pixels per metre, cropped to
/// `crop` (`[right0, right1, up0, up1]` in view metres) when given.
fn picture(
    buildings: &[(GeneratedItem, f64)],
    [yaw, pitch]: [f64; 2],
    scale: f64,
    crop: Option<[f64; 4]>,
) -> Picture {
    let (yaw, pitch) = (yaw.to_radians(), pitch.to_radians());
    let forward = [
        yaw.sin() * pitch.cos(),
        yaw.cos() * pitch.cos(),
        -pitch.sin(),
    ];
    let right = unit(cross(forward, [0., 0., 1.]));
    let up = cross(right, forward);
    let light = unit([-0.45, -0.6, 0.66]);
    // Each triangle in view space with its colours.
    let mut triangles: Vec<([V; 3], [[f32; 3]; 3], f32)> = Vec::new();
    for (building, offset) in buildings {
        let geometry = &building.geometry;
        let transforms = geometry.world_transforms().unwrap();
        for (node, matrix) in geometry.nodes.iter().zip(transforms) {
            for &mesh in &node.meshes {
                let mesh = &geometry.meshes[mesh];
                let glazed = ["glass", "pane"].contains(&mesh.material_slot.as_str());
                for corner in mesh.indices.chunks_exact(3) {
                    let world = [0, 1, 2].map(|k| {
                        let p = matrix.point(mesh.positions[corner[k] as usize]);
                        [p[0] + offset, p[1], p[2]]
                    });
                    let mut normal = unit(cross(
                        [0, 1, 2].map(|k| world[1][k] - world[0][k]),
                        [0, 1, 2].map(|k| world[2][k] - world[0][k]),
                    ));
                    if dot(normal, forward) > 0. {
                        normal = normal.map(|v| -v);
                    }
                    let lit = (0.5 + 0.55 * dot(normal, light).max(0.)) as f32;
                    let colours = [0, 1, 2].map(|k| {
                        if glazed {
                            return [0.05, 0.08, 0.1];
                        }
                        match mesh.colors.get(corner[k] as usize) {
                            Some(c) => [c[0] as f32, c[1] as f32, c[2] as f32],
                            None => [0.6; 3],
                        }
                    });
                    triangles.push((
                        world.map(|p| [dot(p, right), dot(p, up), dot(p, forward)]),
                        colours,
                        lit,
                    ));
                }
            }
        }
    }
    let frame = crop.unwrap_or_else(|| {
        let mut frame = [
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
        ];
        for (points, _, _) in &triangles {
            for p in points {
                frame[0] = frame[0].min(p[0]);
                frame[1] = frame[1].max(p[0]);
                frame[2] = frame[2].min(p[1]);
                frame[3] = frame[3].max(p[1]);
            }
        }
        [
            frame[0] - 0.3,
            frame[1] + 0.3,
            frame[2] - 0.3,
            frame[3] + 0.3,
        ]
    });
    // Two samples a pixel each way, averaged at the end.
    let fine = scale * 2.;
    let (width, height) = (
        ((frame[1] - frame[0]) * fine).ceil() as usize,
        ((frame[3] - frame[2]) * fine).ceil() as usize,
    );
    let mut depth = vec![f32::INFINITY; width * height];
    let mut colour = vec![[0.62f32, 0.7, 0.78]; width * height];
    for (points, colours, lit) in &triangles {
        let p = points.map(|p| [(p[0] - frame[0]) * fine, (frame[3] - p[1]) * fine, p[2]]);
        let area =
            (p[1][0] - p[0][0]) * (p[2][1] - p[0][1]) - (p[2][0] - p[0][0]) * (p[1][1] - p[0][1]);
        if area.abs() < 1e-12 {
            continue;
        }
        let low = |axis: usize| p.iter().map(|p| p[axis]).fold(f64::INFINITY, f64::min);
        let high = |axis: usize| p.iter().map(|p| p[axis]).fold(f64::NEG_INFINITY, f64::max);
        let (x0, x1) = (
            low(0).floor().max(0.) as usize,
            (high(0).ceil().min(width as f64 - 1.)).max(0.) as usize,
        );
        let (y0, y1) = (
            low(1).floor().max(0.) as usize,
            (high(1).ceil().min(height as f64 - 1.)).max(0.) as usize,
        );
        if high(0) < 0. || high(1) < 0. || low(0) >= width as f64 || low(1) >= height as f64 {
            continue;
        }
        for y in y0..=y1 {
            for x in x0..=x1 {
                let (sx, sy) = (x as f64 + 0.5, y as f64 + 0.5);
                let w0 = ((p[1][0] - sx) * (p[2][1] - sy) - (p[2][0] - sx) * (p[1][1] - sy)) / area;
                let w1 = ((p[2][0] - sx) * (p[0][1] - sy) - (p[0][0] - sx) * (p[2][1] - sy)) / area;
                let w2 = 1. - w0 - w1;
                if w0 < 0. || w1 < 0. || w2 < 0. {
                    continue;
                }
                let z = (w0 * p[0][2] + w1 * p[1][2] + w2 * p[2][2]) as f32;
                let at = y * width + x;
                if z < depth[at] {
                    depth[at] = z;
                    colour[at] = [0, 1, 2].map(|k| {
                        (w0 as f32 * colours[0][k]
                            + w1 as f32 * colours[1][k]
                            + w2 as f32 * colours[2][k])
                            * lit
                    });
                }
            }
        }
    }
    Picture {
        width,
        height,
        depth,
        colour,
    }
}

impl Picture {
    fn png(&self) -> Vec<u8> {
        let (width, height) = (self.width / 2, self.height / 2);
        let mut rgba = Vec::with_capacity(width * height * 4);
        for y in 0..height {
            for x in 0..width {
                let mut sum = [0f32; 3];
                // A sample at the edge of a surface is darkened, so parts read as drawn.
                for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                    let at = (y * 2 + dy) * self.width + x * 2 + dx;
                    let near = self.depth[at];
                    let edge = [
                        at.saturating_sub(1),
                        (at + 1).min(self.depth.len() - 1),
                        at.saturating_sub(self.width),
                        (at + self.width).min(self.depth.len() - 1),
                    ]
                    .iter()
                    .any(|&other| self.depth[other] - near > 0.04);
                    for k in 0..3 {
                        sum[k] += self.colour[at][k] * if edge { 0.55 } else { 1. };
                    }
                }
                for value in sum {
                    let linear = (value / 4.).clamp(0., 1.);
                    let encoded = if linear <= 0.0031308 {
                        linear * 12.92
                    } else {
                        1.055 * linear.powf(1. / 2.4) - 0.055
                    };
                    rgba.push((encoded * 255.).round() as u8);
                }
                rgba.push(255);
            }
        }
        superspec_geometry::image::encode(&superspec_geometry::image::Image {
            width,
            height,
            rgba,
            mime: "image/png",
        })
        .unwrap()
    }
}

/// Writes one picture: `VARIANT_RENDER` is the PNG path, `VARIANT_RENDER_PROTOTYPES` the
/// Stoa prototypes in a row (`0,1,3`; default `0`), `VARIANT_RENDER_LEVEL` the graphics
/// detail, `VARIANT_RENDER_BUDGET` the variant budget (-1: the detail's own, 0: none, as
/// before the catalogue), `VARIANT_RENDER_VIEW` `yaw,pitch,pixels per metre` and
/// `VARIANT_RENDER_CROP` `right0,right1,up0,up1` in view metres.
#[test]
#[ignore = "a picture, not a check"]
fn render() {
    let numbers = |name: &str, default: &str| -> Vec<f64> {
        std::env::var(name)
            .unwrap_or_else(|_| default.into())
            .split(',')
            .filter(|text| !text.is_empty())
            .map(|text| text.trim().parse().unwrap())
            .collect()
    };
    let Ok(output) = std::env::var("VARIANT_RENDER") else {
        println!("Set VARIANT_RENDER to a PNG path.");
        return;
    };
    let level = numbers("VARIANT_RENDER_LEVEL", "0")[0] as u8;
    let view = numbers("VARIANT_RENDER_VIEW", "0,0,60");
    let crop = numbers("VARIANT_RENDER_CROP", "");
    let mut offset = 0.;
    let mut buildings = Vec::new();
    for prototype in numbers("VARIANT_RENDER_PROTOTYPES", "0") {
        for budget in numbers("VARIANT_RENDER_BUDGET", "-1") {
            let values = STOA[prototype as usize].budgeted(level, budget as i64);
            buildings.push((generate("kowloon-building.item.json", &values), offset));
            offset += 11.;
        }
    }
    let picture = picture(
        &buildings,
        [view[0], view[1]],
        view[2],
        (crop.len() == 4).then(|| [crop[0], crop[1], crop[2], crop[3]]),
    );
    let output = std::path::Path::new(&output);
    std::fs::create_dir_all(output.parent().unwrap()).unwrap();
    std::fs::write(output, picture.png()).unwrap();
    println!(
        "{} x {} written to {}",
        picture.width / 2,
        picture.height / 2,
        output.display()
    );
}
/// One native fixture's meshes for a seed, at a graphics detail and a variant budget
/// (`None`: the detail's own).
fn native(
    name: &str,
    parameters: &serde_json::Value,
    seed: u32,
    level: u8,
    budget: Option<u32>,
) -> Vec<item::ItemMesh> {
    let mut values: Values = serde_json::from_value(parameters.clone()).unwrap();
    values.insert("seed".into(), seed.into());
    values.insert("graphicsDetail".into(), level.into());
    if let Some(budget) = budget {
        values.insert("variantBudget".into(), budget.into());
    }
    items::natives()
        .generate(name, &values, &|| false)
        .unwrap_or_else(|error| panic!("{name} {parameters}: {error}"))
        .meshes
}

/// Everything of a fixture but its colours, bit for bit: slots, indices, positions, normals
/// and texture coordinates. Equal content is one geometry a later stage can instance.
fn content(meshes: &[item::ItemMesh]) -> u64 {
    #[allow(deprecated)]
    let mut hasher = std::hash::SipHasher::new();
    for mesh in meshes {
        mesh.material_slot.hash(&mut hasher);
        mesh.indices.hash(&mut hasher);
        for v in mesh.positions.iter().chain(&mesh.normals).flatten() {
            v.to_bits().hash(&mut hasher);
        }
        for v in mesh.uvs.iter().flatten() {
            v.to_bits().hash(&mut hasher);
        }
    }
    hasher.finish()
}

/// A fixture's vertex colours, bit for bit.
fn colours(meshes: &[item::ItemMesh]) -> u64 {
    #[allow(deprecated)]
    let mut hasher = std::hash::SipHasher::new();
    for v in meshes.iter().flat_map(|mesh| &mesh.colors).flatten() {
        v.to_bits().hash(&mut hasher);
    }
    hasher.finish()
}

/// The fixtures the catalogue covers, with parameters a building gives them.
fn catalogued() -> Vec<(&'static str, serde_json::Value)> {
    vec![
        ("kowloon/aircon", json!({"plates": true})),
        ("kowloon/condenser", json!({"plates": true})),
        // Room for the wider cases, a second fan and a taller duct.
        ("kowloon/condenser", json!({"width": 1.6, "height": 1.5})),
        ("kowloon/window", json!({})),
        (
            "kowloon/window",
            json!({"kind": "slider", "openChance": 1.3}),
        ),
        ("kowloon/window", json!({"lit": true, "room": 2.6})),
        ("kowloon/window", json!({"storage": 0.7})),
        ("kowloon/cage", json!({})),
        (
            "kowloon/cage",
            json!({"contents": "aircon", "width": 0.98, "height": 0.68, "depth": 0.52}),
        ),
        ("kowloon/plants", json!({})),
        ("kowloon/drying-rack", json!({})),
        ("kowloon/sheet-canopy", json!({})),
        ("kowloon/sagging-tarp", json!({})),
    ]
}

const SEEDS: u32 = 40;

#[test]
fn a_part_type_draws_no_more_shapes_than_its_budget_and_budgets_nest() {
    for (name, parameters) in catalogued() {
        let shapes = |level: u8, budget: Option<u32>| -> BTreeSet<u64> {
            (0..SEEDS)
                .map(|seed| content(&native(name, &parameters, seed, level, budget)))
                .collect()
        };
        let mut smaller: Option<BTreeSet<u64>> = None;
        for budget in [1, 2, 4, 8] {
            let drawn = shapes(0, Some(budget));
            println!(
                "{name} {parameters}: {} shapes at budget {budget}",
                drawn.len()
            );
            assert!(
                drawn.len() <= budget as usize,
                "{name} {parameters}: {} shapes at budget {budget}",
                drawn.len()
            );
            // The catalogue is not one shape under several names.
            assert!(
                budget < 4 || drawn.len() >= 2,
                "{name} {parameters}: one shape at budget {budget}"
            );
            if let Some(smaller) = &smaller {
                assert!(
                    smaller.is_subset(&drawn),
                    "{name} {parameters}: budget {budget} lacks a shape of the budget below"
                );
            }
            smaller = Some(drawn);
        }
        // A graphics detail that states no budget has its preset's: 4, 2 and 1.
        for (level, budget) in [(0, 4), (1, 2), (2, 1)] {
            assert_eq!(
                shapes(level, None),
                shapes(level, Some(budget)),
                "{name} {parameters} level {level}"
            );
            // A budget that is no power of two is the power of two below it.
            assert_eq!(
                shapes(level, Some(budget * 2 - 1)),
                shapes(level, Some(budget)),
                "{name} {parameters} level {level}"
            );
        }
    }
    // Without a catalogue every seeded unit is its own shape.
    let (name, parameters) = &catalogued()[0];
    let free: BTreeSet<u64> = (0..SEEDS)
        .map(|seed| content(&native(name, parameters, seed, 0, Some(0))))
        .collect();
    assert_eq!(free.len(), SEEDS as usize);
}

#[test]
fn a_pick_is_the_same_for_the_same_seed() {
    for (name, parameters) in catalogued() {
        for seed in [0, 7, 31] {
            for (level, budget) in [(0, None), (0, Some(8)), (1, None), (2, None), (0, Some(0))] {
                assert_eq!(
                    native(name, &parameters, seed, level, budget),
                    native(name, &parameters, seed, level, budget),
                    "{name} {parameters} seed {seed} level {level} budget {budget:?}"
                );
            }
        }
    }
}

/// The colour most vertices of a slot carry: a case's body paint.
fn body(meshes: &[item::ItemMesh], slot: &str) -> [u64; 3] {
    let mut counts: BTreeMap<[u64; 3], usize> = BTreeMap::new();
    for mesh in meshes.iter().filter(|mesh| mesh.material_slot == slot) {
        for colour in &mesh.colors {
            *counts
                .entry([0, 1, 2].map(|k| (colour[k] as f64).to_bits()))
                .or_default() += 1;
        }
    }
    counts
        .into_iter()
        .max_by_key(|(_, count)| *count)
        .map(|(colour, _)| colour)
        .unwrap_or_else(|| panic!("nothing drawn in {slot}"))
}

#[test]
fn colour_is_the_instances_own_and_does_not_change_the_geometry() {
    for (name, parameters, slot) in [
        ("kowloon/aircon", json!({"plates": true}), "aircon"),
        ("kowloon/condenser", json!({"plates": true}), "aircon"),
        ("kowloon/cage", json!({"contents": "empty"}), "paint"),
    ] {
        // Under one budget many colours share each shape.
        let drawn: Vec<(u64, u64)> = (0..SEEDS)
            .map(|seed| {
                let meshes = native(name, &parameters, seed, 0, Some(4));
                (content(&meshes), colours(&meshes))
            })
            .collect();
        let shapes: BTreeSet<u64> = drawn.iter().map(|(shape, _)| *shape).collect();
        let coloured: BTreeSet<(u64, u64)> = drawn.iter().copied().collect();
        println!(
            "{name}: {} shapes in {} colourings over {SEEDS} seeds",
            shapes.len(),
            coloured.len()
        );
        assert!(shapes.len() <= 4, "{name}");
        assert!(
            coloured.len() > 3 * shapes.len(),
            "{name}: {}",
            coloured.len()
        );
        // And a fixture keeps its body paint whichever shape a budget or a level gives it.
        for seed in 0..12 {
            let own = body(&native(name, &parameters, seed, 0, Some(64)), slot);
            for (level, budget) in [(0, 1), (0, 2), (0, 4), (0, 16), (1, 2), (2, 1)] {
                assert_eq!(
                    body(&native(name, &parameters, seed, level, Some(budget)), slot),
                    own,
                    "{name} seed {seed} level {level} budget {budget}"
                );
            }
        }
    }
}

/// Washing is a catalogue entry's garments in the fixture's own dyes: racks and cages of one
/// entry are one geometry, and no two of them need hang the same colours.
#[test]
fn washing_is_dyed_by_its_fixture_and_the_dye_does_not_change_the_geometry() {
    for (name, parameters) in [
        ("kowloon/drying-rack", json!({})),
        ("kowloon/cage", json!({"contents": "washing"})),
    ] {
        let cloth = |meshes: &[item::ItemMesh]| -> u64 {
            let cloths: Vec<item::ItemMesh> = meshes
                .iter()
                .filter(|mesh| mesh.material_slot == "balcony-cloth")
                .cloned()
                .collect();
            colours(&cloths)
        };
        for level in 0..=2 {
            let budget = [4, 2, 1][level as usize];
            let drawn: Vec<(u64, u64)> = (0..SEEDS)
                .map(|seed| {
                    let meshes = native(name, &parameters, seed, level, None);
                    (content(&meshes), cloth(&meshes))
                })
                .collect();
            let shapes: BTreeSet<u64> = drawn.iter().map(|(shape, _)| *shape).collect();
            let dyed: BTreeSet<u64> = drawn.iter().map(|(_, dyes)| *dyes).collect();
            println!(
                "{name} level {level}: {} shapes hung in {} colourings over {SEEDS} seeds",
                shapes.len(),
                dyed.len()
            );
            // Bit for bit the same positions, normals, texture coordinates and indices
            // under every dye.
            assert!(shapes.len() <= budget, "{name} level {level}");
            assert!(
                dyed.len() > 3 * shapes.len() && dyed.len() > SEEDS as usize / 2,
                "{name} level {level}: {} colourings of {} shapes",
                dyed.len(),
                shapes.len()
            );
        }
        // A fixture's dyes are its seed's at every level and budget that hangs the same
        // garments, and the same every time.
        for seed in [0, 7, 31] {
            assert_eq!(
                native(name, &parameters, seed, 0, Some(4)),
                native(name, &parameters, seed, 0, Some(4)),
                "{name} seed {seed}"
            );
        }
        // Without a catalogue a fixture is drawn from its own stream throughout.
        let free: BTreeSet<u64> = (0..SEEDS)
            .map(|seed| content(&native(name, &parameters, seed, 0, Some(0))))
            .collect();
        assert!(free.len() > SEEDS as usize / 2, "{name}: {}", free.len());
    }
}

/// What must not depend on the budget: where the parts stand, what they report and what
/// they collide with.
#[test]
fn a_building_keeps_its_placements_features_and_collision_under_every_budget() {
    let prototype = &STOA[1];
    let built =
        |budget: i64| generate("kowloon-building.item.json", &prototype.budgeted(0, budget));
    let free = built(0);
    for budget in [-1, 1, 8] {
        let drawn = built(budget);
        let placed = |geometry: &ItemGeometry| {
            geometry
                .nodes
                .iter()
                .map(|node| {
                    format!(
                        "{} {:?} {:?} {:?}",
                        node.id, node.name, node.parent, node.transform
                    )
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(
            placed(&free.geometry),
            placed(&drawn.geometry),
            "budget {budget}"
        );
        let parts = |geometry: &ItemGeometry| {
            geometry
                .parts
                .iter()
                .map(|part| (part.id.clone(), part.node))
                .collect::<Vec<_>>()
        };
        assert_eq!(
            parts(&free.geometry),
            parts(&drawn.geometry),
            "budget {budget}"
        );
        assert_eq!(
            free.geometry.features, drawn.geometry.features,
            "budget {budget}"
        );
        assert_eq!(
            serde_json::to_string(&free.collision).unwrap(),
            serde_json::to_string(&drawn.collision).unwrap(),
            "budget {budget}"
        );
        assert_eq!(
            serde_json::to_string(&free.attachments).unwrap(),
            serde_json::to_string(&drawn.attachments).unwrap(),
            "budget {budget}"
        );
        // The same leaf parts are drawn, in the same slots.
        let leaf_slots = |geometry: &ItemGeometry| {
            geometry
                .meshes
                .iter()
                .map(|mesh| (mesh.id.clone(), mesh.material_slot.clone()))
                .collect::<BTreeSet<_>>()
        };
        let (before, after) = (leaf_slots(&free.geometry), leaf_slots(&drawn.geometry));
        let kinds = |slots: &BTreeSet<(String, String)>| {
            slots
                .iter()
                .map(|(id, _)| id.rsplit_once('/').map_or("", |(path, _)| path).to_owned())
                .collect::<BTreeSet<_>>()
        };
        assert_eq!(kinds(&before), kinds(&after), "budget {budget}");
    }
}

fn stock_asset(id: &str) -> Result<Vec<u8>, String> {
    let name = superspec_catalog::image_name(id).ok_or(format!("Unknown stock texture {id}"))?;
    std::fs::read(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../assets/materials")
            .join(name),
    )
    .map_err(|e| e.to_string())
}

/// `variant-budget` on a city building reaches its generators through both compilers (the
/// publication exports with the stateless one and bakes on a session's), stands in for the
/// level's own budget and leaves the physical output alone.
#[test]
fn a_city_building_takes_the_variant_budget_from_its_aml() {
    let session = superspec_engine::Session::default();
    let mut revision = 0;
    let mut compiled = |level: u8, budget: Option<u32>| {
        let budget = budget.map_or(String::new(), |b| format!(r#" variant-budget="{b}""#));
        let aml = format!(
            r#"<architecture version="1" id="building-0"><city><building id="prototype" position="0 0 0" floors="5" seed="8" graphics-detail="{level}"{budget} /></city></architecture>"#
        );
        revision += 1;
        let exported = superspec_engine::compile_with_assets(&aml, revision, &stock_asset).unwrap();
        let baked = session
            .compile_cancellable(&aml, revision, &stock_asset, true, &|| false)
            .unwrap();
        assert_eq!(exported.indices, baked.indices, "level {level}{budget}");
        assert_eq!(exported.positions, baked.positions, "level {level}{budget}");
        exported
    };
    let same = |a: &superspec_engine::Scene, b: &superspec_engine::Scene| {
        a.indices == b.indices && a.positions == b.positions
    };
    // Unstated, the budget is the level's own.
    let own = [(0, 4), (1, 2), (2, 1)].map(|(level, budget)| {
        let unstated = compiled(level, None);
        assert!(
            same(&unstated, &compiled(level, Some(budget))),
            "level {level}"
        );
        unstated
    });
    // Another budget draws other shapes at the same level, and none every seed's own.
    let wide = compiled(0, Some(64));
    let free = compiled(0, Some(0));
    let one = compiled(0, Some(1));
    assert!(!same(&own[0], &wide) && !same(&own[0], &free) && !same(&own[0], &one));
    assert!(!same(&wide, &free));
    // A budget that is no power of two is the power of two below it.
    assert!(same(&compiled(0, Some(5)), &own[0]));
    // A reduced level takes a stated budget too.
    assert!(!same(&own[1], &compiled(1, Some(16))));
    for scene in [&wide, &free, &one] {
        assert_eq!(scene.manifest.colliders, own[0].manifest.colliders);
    }
}

#[test]
fn a_building_draws_each_air_conditioner_part_within_its_budget() {
    for (level, budget) in [(0u8, 4usize), (1, 2), (2, 1)] {
        let building = generate("kowloon-building.item.json", &STOA[0].values(level));
        let mut shapes: BTreeMap<String, (usize, BTreeSet<u64>)> = BTreeMap::new();
        for (kind, shape, _) in parts(&building) {
            let entry = shapes.entry(kind).or_default();
            entry.0 += 1;
            entry.1.insert(shape.textured);
        }
        for (kind, spare) in [
            ("aircon/unit", 0),
            ("aircon/condenser", 0),
            ("aircon/lines", 0),
            // A hose over a shop front is cut to the room above the shop's sign.
            ("aircon/hose", 1),
        ] {
            let Some((instances, drawn)) = shapes.get(kind) else {
                assert_eq!(level, 2, "{kind} is not drawn at level {level}");
                continue;
            };
            println!(
                "level {level}: {instances} of {kind} in {} shapes",
                drawn.len()
            );
            assert!(
                drawn.len() <= budget + spare,
                "level {level}: {kind} draws {} shapes of {instances}",
                drawn.len()
            );
        }
    }
}

//! Coincident surfaces of the Kowloon building: two meshes whose faces lie in one plane, look
//! the same way and overlap. Which of the two a viewer shows is then decided by the order the
//! meshes are drawn in, so a renderer that reorders its draws (to switch shader less often)
//! turns one into a stipple of the other.
//!
//! `cargo test --release -p superspec-engine --test kowloon_coincident -- --ignored --nocapture survey`
//! prints every pair of generators that draws such faces, at each graphics detail, and writes
//! the table to `generated/graphics-detail/coincident/survey.txt` (or
//! `COINCIDENT_SURVEY_OUTPUT`): first everything within 1.5 mm, which a depth buffer tells apart
//! up close only, then what lies in one plane. The checks below keep the second kind out of
//! the generators it was found in, and small everywhere.
use serde_json::json;
use std::{
    collections::{BTreeMap, HashMap},
    sync::Arc,
};
use superspec_engine::{
    itemspec::definition::{self as item, Environment, GeneratedItem, ItemMesh, Values},
    worldspec::themes::kowloon::items,
};

type V3 = [f64; 3];

/// Faces nearer than this fight for a pixel: at 40 m a 24-bit depth buffer with a 0.1 m near
/// plane resolves about a millimetre, and the bake's distance levels move vertices as well.
const GAP: f64 = 0.0015;
/// Faces this near are in one plane: no depth buffer tells them apart at any distance, and
/// the order of their draws decides which shows.
const EXACT: f64 = 0.0002;
/// Overlaps smaller than this (a square centimetre) are a shared edge, not a shared face.
const AREA: f64 = 1e-4;

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

/// A city building as `city.rs` hands it to the building item.
struct Building {
    name: &'static str,
    floors: usize,
    seed: usize,
    trade: Option<&'static str>,
    courtyard: bool,
}

/// The Stoa city's prototypes: every family, a traded front and a courtyard block.
const BUILDINGS: [Building; 7] = [
    Building {
        name: "5 floors, seed 8",
        floors: 5,
        seed: 8,
        trade: None,
        courtyard: false,
    },
    Building {
        name: "4 floors, seed 4",
        floors: 4,
        seed: 4,
        trade: None,
        courtyard: false,
    },
    Building {
        name: "4 floors, seed 4, roast shop",
        floors: 4,
        seed: 4,
        trade: Some("roast"),
        courtyard: false,
    },
    Building {
        name: "6 floors, seed 6",
        floors: 6,
        seed: 6,
        trade: None,
        courtyard: false,
    },
    Building {
        name: "3 floors, seed 9",
        floors: 3,
        seed: 9,
        trade: None,
        courtyard: false,
    },
    Building {
        name: "7 floors, seed 7",
        floors: 7,
        seed: 7,
        trade: None,
        courtyard: false,
    },
    Building {
        name: "courtyard, 2 floors, seed 6",
        floors: 2,
        seed: 6,
        trade: None,
        courtyard: true,
    },
];

impl Building {
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
            values.insert(
                "courtyards".into(),
                json!([[[-2.9, -4.9], [-2.9, 4.9], [2.9, 4.9], [2.9, -4.9]]]),
            );
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

fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn dot(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: V3, b: V3) -> V3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

struct Triangle {
    mesh: usize,
    points: [V3; 3],
    normal: V3,
    /// Along the normal.
    offset: f64,
}

/// The area two triangles of (nearly) one plane share, by clipping the first to the second
/// in the plane of `normal`.
fn shared_area(a: &[V3; 3], b: &[V3; 3], normal: V3) -> f64 {
    let helper = if normal[0].abs() < 0.9 {
        [1., 0., 0.]
    } else {
        [0., 1., 0.]
    };
    let u = cross(normal, helper);
    let size = dot(u, u).sqrt();
    let u = u.map(|v| v / size);
    let v = cross(normal, u);
    let flat = |p: &V3| [dot(*p, u), dot(*p, v)];
    let mut polygon: Vec<[f64; 2]> = a.iter().map(flat).collect();
    let clip: Vec<[f64; 2]> = b.iter().map(flat).collect();
    let signed = |p: &[[f64; 2]]| -> f64 {
        (0..p.len())
            .map(|i| {
                let (a, b) = (p[i], p[(i + 1) % p.len()]);
                a[0] * b[1] - b[0] * a[1]
            })
            .sum::<f64>()
            / 2.
    };
    let turn = signed(&clip).signum();
    for i in 0..3 {
        let (c0, c1) = (clip[i], clip[(i + 1) % 3]);
        let side = |p: [f64; 2]| {
            turn * ((c1[0] - c0[0]) * (p[1] - c0[1]) - (c1[1] - c0[1]) * (p[0] - c0[0]))
        };
        let mut next = Vec::with_capacity(polygon.len() + 1);
        for k in 0..polygon.len() {
            let (p, q) = (polygon[k], polygon[(k + 1) % polygon.len()]);
            let (sp, sq) = (side(p), side(q));
            if sp >= 0. {
                next.push(p);
            }
            if (sp > 0. && sq < 0.) || (sp < 0. && sq > 0.) {
                let t = sp / (sp - sq);
                next.push([p[0] + (q[0] - p[0]) * t, p[1] + (q[1] - p[1]) * t]);
            }
        }
        polygon = next;
        if polygon.len() < 3 {
            return 0.;
        }
    }
    signed(&polygon).abs()
}

/// One pair of meshes with coincident faces.
struct Found {
    meshes: [usize; 2],
    area: f64,
    /// The largest gap between the two planes over the pair's triangles.
    gap: f64,
    at: V3,
    /// The way the first triangle found faces.
    facing: V3,
}

/// Every pair of different meshes with faces within `reach` of one plane, facing one way,
/// that overlap.
fn coincident(meshes: &[ItemMesh], reach: f64) -> Vec<Found> {
    let mut triangles = Vec::new();
    for (index, mesh) in meshes.iter().enumerate() {
        for corner in mesh.indices.chunks_exact(3) {
            let points = [0, 1, 2].map(|k| mesh.positions[corner[k] as usize]);
            let normal = cross(sub(points[1], points[0]), sub(points[2], points[0]));
            let size = dot(normal, normal).sqrt();
            // Slivers have no plane to speak of and no area to fight over.
            if size < 2. * AREA {
                continue;
            }
            let normal = normal.map(|v| v / size);
            triangles.push(Triangle {
                mesh: index,
                points,
                normal,
                offset: dot(normal, points[0]),
            });
        }
    }
    // Planes: normals to a thousandth, offsets in cells of two gaps. A neighbour within a
    // gap is in the same cell or the next.
    let cell = 2. * reach;
    let key = |t: &Triangle, shift: i64| {
        (
            t.normal.map(|v| (v * 1000.).round() as i64),
            (t.offset / cell).floor() as i64 + shift,
        )
    };
    let mut planes: HashMap<_, Vec<usize>> = HashMap::new();
    for (index, triangle) in triangles.iter().enumerate() {
        planes.entry(key(triangle, 0)).or_default().push(index);
    }
    let bounds = |t: &Triangle| {
        let mut low = [f64::INFINITY; 3];
        let mut high = [f64::NEG_INFINITY; 3];
        for p in &t.points {
            for k in 0..3 {
                low[k] = low[k].min(p[k]);
                high[k] = high[k].max(p[k]);
            }
        }
        (low, high)
    };
    let boxes: Vec<_> = triangles.iter().map(bounds).collect();
    let mut found: BTreeMap<[usize; 2], Found> = BTreeMap::new();
    let mut test = |i: usize, j: usize| {
        let (a, b) = (&triangles[i], &triangles[j]);
        if a.mesh == b.mesh || dot(a.normal, b.normal) < 0.9999 {
            return;
        }
        let ((al, ah), (bl, bh)) = (&boxes[i], &boxes[j]);
        if (0..3).any(|k| al[k] > bh[k] + reach || bl[k] > ah[k] + reach) {
            return;
        }
        let gap = b
            .points
            .iter()
            .map(|p| (dot(a.normal, *p) - a.offset).abs())
            .fold(0., f64::max);
        if gap > reach {
            return;
        }
        let area = shared_area(&a.points, &b.points, a.normal);
        if area < AREA {
            return;
        }
        let pair = [a.mesh.min(b.mesh), a.mesh.max(b.mesh)];
        let entry = found.entry(pair).or_insert(Found {
            meshes: pair,
            area: 0.,
            gap: 0.,
            at: a.points[0],
            facing: a.normal,
        });
        entry.area += area;
        entry.gap = entry.gap.max(gap);
    };
    for (index, triangle) in triangles.iter().enumerate() {
        if let Some(same) = planes.get(&key(triangle, 0)) {
            for &other in same {
                if other > index {
                    test(index, other);
                }
            }
        }
        if let Some(next) = planes.get(&key(triangle, 1)) {
            for &other in next {
                test(index, other);
            }
        }
    }
    found.into_values().collect()
}

/// What draws a mesh: the named steps of its part's path without their indices, and its slot.
fn generator(mesh: &ItemMesh) -> String {
    let mut steps: Vec<&str> = Vec::new();
    let (path, group) = mesh.id.rsplit_once('/').unwrap_or(("", &mesh.id));
    for step in path.split('/') {
        let step = step.trim_end_matches(|c: char| c.is_ascii_digit() || c == '-');
        if !step.is_empty() && step != "part" && steps.last() != Some(&step) {
            steps.push(step);
        }
    }
    let group = group.trim_end_matches(|c: char| c.is_ascii_digit() || c == '-');
    format!(
        "{} [{}{}]",
        steps[steps.len().saturating_sub(3)..].join("/"),
        mesh.material_slot,
        if group == mesh.material_slot {
            String::new()
        } else {
            format!(" {group}")
        }
    )
}

#[derive(Default)]
struct Row {
    pairs: usize,
    area: f64,
    gap: f64,
    example: String,
}

fn rows(meshes: &[ItemMesh], reach: f64) -> BTreeMap<(String, String), Row> {
    let mut rows: BTreeMap<(String, String), Row> = BTreeMap::new();
    for pair in coincident(meshes, reach) {
        let [a, b] = pair.meshes.map(|index| &meshes[index]);
        let (mut first, mut second) = (generator(a), generator(b));
        if second < first {
            std::mem::swap(&mut first, &mut second);
        }
        let row = rows.entry((first, second)).or_default();
        row.pairs += 1;
        row.area += pair.area;
        row.gap = row.gap.max(pair.gap);
        if row.example.is_empty() {
            row.example = format!(
                "{} x {} near [{:.2}, {:.2}, {:.2}] facing [{:.1}, {:.1}, {:.1}]",
                a.id,
                b.id,
                pair.at[0],
                pair.at[1],
                pair.at[2],
                pair.facing[0],
                pair.facing[1],
                pair.facing[2]
            );
        }
    }
    rows
}

fn table(title: &str, rows: &BTreeMap<(String, String), Row>) -> String {
    let mut sorted: Vec<_> = rows.iter().collect();
    sorted.sort_by(|a, b| b.1.area.total_cmp(&a.1.area));
    let mut text = format!(
        "{title}\n  {:>6} {:>9} {:>7}  generators\n",
        "pairs", "area m2", "gap mm"
    );
    for ((first, second), row) in &sorted {
        text += &format!(
            "  {:>6} {:>9.3} {:>7.2}  {first}  x  {second}\n           e.g. {}\n",
            row.pairs,
            row.area,
            row.gap * 1000.,
            row.example
        );
    }
    text += &format!(
        "  {:>6} {:>9.3}          total\n\n",
        sorted.iter().map(|(_, row)| row.pairs).sum::<usize>(),
        sorted.iter().map(|(_, row)| row.area).sum::<f64>()
    );
    text
}

fn meshes(building: &Building, level: u8) -> Vec<ItemMesh> {
    generate("kowloon-building.item.json", &building.values(level))
        .flattened()
        .unwrap()
}

#[test]
#[ignore = "a report, not a check"]
fn survey() {
    let only = std::env::var("COINCIDENT_SURVEY_BUILDING").ok();
    let mut report = String::new();
    for level in 0..=2 {
        let mut all: BTreeMap<(String, String), Row> = BTreeMap::new();
        let mut one_plane: BTreeMap<(String, String), Row> = BTreeMap::new();
        for building in &BUILDINGS {
            if only
                .as_deref()
                .is_some_and(|only| !building.name.contains(only))
            {
                continue;
            }
            let meshes = meshes(building, level);
            let near = rows(&meshes, GAP);
            report += &table(
                &format!("{}, graphics detail {level}", building.name),
                &near,
            );
            for (rows, sum) in [(near, &mut all), (rows(&meshes, EXACT), &mut one_plane)] {
                for (key, row) in rows {
                    let total = sum.entry(key).or_default();
                    total.pairs += row.pairs;
                    total.area += row.area;
                    total.gap = total.gap.max(row.gap);
                    if total.example.is_empty() {
                        total.example = format!("{}: {}", building.name, row.example);
                    }
                }
            }
        }
        report += &table(&format!("All buildings, graphics detail {level}"), &all);
        report += &table(
            &format!("All buildings, graphics detail {level}, in one plane (draw order decides)"),
            &one_plane,
        );
    }
    println!("{report}");
    let output = std::env::var("COINCIDENT_SURVEY_OUTPUT").unwrap_or_else(|_| {
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../generated/graphics-detail/coincident/survey.txt"
        )
        .into()
    });
    let output = std::path::Path::new(&output);
    std::fs::create_dir_all(output.parent().unwrap()).unwrap();
    std::fs::write(output, report).unwrap();
}

#[test]
fn the_survey_finds_a_face_laid_on_another_and_passes_one_stood_off() {
    let quad = |id: &str, slot: &str, z: f64, x: f64, flip: bool| ItemMesh {
        id: id.into(),
        positions: vec![[x, 0., z], [x + 1., 0., z], [x + 1., 1., z], [x, 1., z]],
        normals: vec![[0., 0., if flip { -1. } else { 1. }]; 4],
        uvs: vec![],
        additional_uvs: Default::default(),
        colors: vec![],
        tangents: vec![],
        indices: if flip {
            vec![0, 2, 1, 0, 3, 2]
        } else {
            vec![0, 1, 2, 0, 2, 3]
        },
        material_slot: slot.into(),
        roles: vec![],
    };
    let wall = quad("wall/wall", "wall", 0., 0., false);
    let on = |z: f64, x: f64, flip: bool| {
        coincident(
            &[wall.clone(), quad("poster/posters", "posters", z, x, flip)],
            GAP,
        )
    };
    let laid = on(0., 0.25, false);
    assert_eq!(laid.len(), 1);
    assert!((laid[0].area - 0.75).abs() < 1e-9, "{}", laid[0].area);
    assert_eq!(on(0.0005, 0.25, false).len(), 1, "half a millimetre off");
    assert!(on(0.003, 0.25, false).is_empty(), "stood off");
    assert!(on(0., 1., false).is_empty(), "sharing an edge");
    assert!(on(0., 0.25, true).is_empty(), "back to back");
    // Two faces of one mesh are drawn in one order always.
    let mut twice = wall.clone();
    twice.positions.extend(wall.positions.clone());
    twice.normals.extend(wall.normals.clone());
    twice.indices.extend([4, 5, 6, 4, 6, 7]);
    assert!(coincident(&[twice], GAP).is_empty());
}

/// Whether a pair is one a viewer can never see: both faces look straight down from the
/// building's own ground, which they stand on.
fn underfoot(pair: &Found) -> bool {
    pair.facing[2] < -0.99 && pair.at[2] < 0.001
}

/// Faces of two materials in one plane, at every graphics detail. The first five rows are
/// what the survey found and the generators no longer draw: a roof slab under the top of the
/// walls around it, a shop room's side walls in the reveals of the opening it stands behind
/// (the stippled panel of the market view), a fascia under a balcony slab and behind its end,
/// and a duct's fittings inside the duct. What is left is small: a cage's fixings on a window
/// frame, a shutter guide at the end of a plinth.
#[test]
fn two_materials_do_not_draw_one_face() {
    const GONE: [(&str, &str); 5] = [
        ("roof [", "/wall ["),
        ("shop/room [interior]", "/wall ["),
        ("side/balcony [", "ground/fascia ["),
        ("side/riser [", "riser/elbow ["),
        ("side/riser [", "riser/branch ["),
    ];
    for level in 0..=2 {
        for building in &BUILDINGS {
            let meshes = meshes(building, level);
            let mut area = 0.;
            for pair in coincident(&meshes, EXACT) {
                if underfoot(&pair) {
                    continue;
                }
                let [a, b] = pair.meshes.map(|index| &meshes[index]);
                let (first, second) = (generator(a), generator(b));
                for (one, other) in GONE {
                    assert!(
                        !(first.contains(one) && second.contains(other)
                            || first.contains(other) && second.contains(one)),
                        "{} level {level}: {} and {} share {:.4} m2 of one plane",
                        building.name,
                        a.id,
                        b.id,
                        pair.area
                    );
                }
                if a.material_slot != b.material_slot {
                    area += pair.area;
                }
            }
            println!(
                "{} level {level}: {area:.3} m2 of two materials in one plane",
                building.name
            );
            assert!(
                area <= 0.25,
                "{} level {level}: {area:.3} m2 of two materials in one plane",
                building.name
            );
        }
    }
}

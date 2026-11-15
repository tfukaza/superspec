//! Raster observations and editable AML proposals, independent of file and process adapters.
mod image;
pub use image::{Options, trace};
pub fn trace_encoded(
    bytes: &[u8],
    id: &str,
    name: &str,
    source: &str,
    options: &Options,
    cancelled: &dyn Fn() -> bool,
) -> Result<Raster, String> {
    let pixels = superspec_geometry::image::decode(bytes)?;
    trace(&pixels, id, name, source, options, cancelled)
}
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Write,
};
use superspec_aml::format::escape;

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Raster {
    pub format: String,
    pub id: String,
    pub name: String,
    pub source_image: String,
    pub grid: f64,
    pub colors: Vec<[f64; 3]>,
    pub rows: Vec<Vec<Option<usize>>>,
}
struct Tile {
    band: usize,
    x: usize,
    y: usize,
    width: usize,
    height: usize,
}
fn identity(text: &str) -> String {
    let mut id: String = text
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || "_.:/-".contains(c) {
                c
            } else {
                '_'
            }
        })
        .collect();
    if !id.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_') {
        id.insert_str(0, "id_");
    }
    id
}
impl Raster {
    pub fn validate(&self) -> Result<(), String> {
        let width = self.rows.first().map_or(0, Vec::len);
        if self.format != "levelspec-raster/1"
            || self.id.is_empty()
            || self.name.is_empty()
            || self.source_image.is_empty()
            || !self.grid.is_finite()
            || self.grid < 0.001
            || self.rows.is_empty()
            || self.rows.len() > 1024
            || width == 0
            || width > 1024
            || self.colors.is_empty()
            || self.colors.len() > 256
            || self
                .colors
                .iter()
                .flatten()
                .any(|v| !v.is_finite() || !(0. ..=255.).contains(v))
            || self
                .rows
                .iter()
                .any(|r| r.len() != width || r.iter().flatten().any(|i| *i >= self.colors.len()))
            || !self.rows.iter().flatten().any(Option::is_some)
        {
            return Err("Invalid raster observations, dimensions, colors or grid.".into());
        }
        Ok(())
    }
    /// Merges equal cell runs while retaining every cell boundary along shared edges.
    pub fn aml(&self, cancelled: &dyn Fn() -> bool) -> Result<String, String> {
        self.validate()?;
        let width = self.rows[0].len();
        let mut tiles = vec![];
        let mut active: Vec<Tile> = vec![];
        for y in 0..self.rows.len() {
            if cancelled() {
                return Err("Tracing cancelled.".into());
            }
            let row = &self.rows[self.rows.len() - 1 - y];
            let mut next = vec![];
            let mut x = 0;
            while x < width {
                let start = x;
                let band = row[x];
                x += 1;
                while x < width && row[x] == band {
                    x += 1;
                }
                let Some(band) = band else { continue };
                let w = x - start;
                let mut tile = if let Some(i) = active
                    .iter()
                    .position(|t| t.band == band && t.x == start && t.width == w)
                {
                    active.remove(i)
                } else {
                    Tile {
                        band,
                        x: start,
                        y,
                        width: w,
                        height: 0,
                    }
                };
                tile.height += 1;
                next.push(tile);
            }
            tiles.extend(active);
            active = next;
            if tiles.len() + active.len() > 100000 {
                return Err("Trace element allowance exceeded.".into());
            }
        }
        tiles.extend(active);
        if 1 + tiles
            .iter()
            .map(|t| 1 + 2 * t.width + 2 * t.height)
            .sum::<usize>()
            > 100000
        {
            return Err("Trace element allowance exceeded.".into());
        }
        let id = identity(&self.id);
        let namespace = if id.starts_with("trace/") {
            "_trace"
        } else {
            "trace"
        };
        let mut used = BTreeSet::from([id.clone()]);
        let mut allocate = |base: String| {
            let mut name = base;
            while used.contains(&name) {
                name.insert(0, '_');
            }
            used.insert(name.clone());
            name
        };
        let materials: BTreeMap<usize, String> = tiles
            .iter()
            .map(|t| t.band)
            .collect::<BTreeSet<_>>()
            .into_iter()
            .map(|i| (i, allocate(format!("band_{i}"))))
            .collect();
        let mut source = format!(
            "<architecture version=\"1\" id=\"{}\" name=\"{}\" validation=\"playable\">\n",
            escape(&id),
            escape(&self.name)
        );
        writeln!(
            source,
            "  <player radius=\"0.35\" height=\"1.8\" step=\"0.45\" crouch=\"1.1\" vault=\"0\"/>"
        )
        .unwrap();
        for (&band, name) in &materials {
            let color = self.colors[band].map(|v| v.round() as u8);
            writeln!(
                source,
                "  <material id=\"{name}\" color=\"#{:02x}{:02x}{:02x}\" roughness=\"1\"/>",
                color[0], color[1], color[2]
            )
            .unwrap();
        }
        let mut geometry = String::new();
        let mut edges: BTreeMap<[(usize, usize); 2], usize> = BTreeMap::new();
        for (i, t) in tiles.iter().enumerate() {
            if cancelled() {
                return Err("Tracing cancelled.".into());
            }
            let mut points = vec![];
            points.extend((0..t.width).map(|dx| (t.x + dx, t.y)));
            points.extend((0..t.height).map(|dy| (t.x + t.width, t.y + dy)));
            points.extend((1..=t.width).rev().map(|dx| (t.x + dx, t.y + t.height)));
            points.extend((1..=t.height).rev().map(|dy| (t.x, t.y + dy)));
            let values = points
                .iter()
                .map(|(x, y)| format!("{} {} 0", *x as f64 * self.grid, *y as f64 * self.grid))
                .collect::<Vec<_>>()
                .join("; ");
            let floor = format!("{namespace}/floor_{i}");
            writeln!(
                source,
                "  <profile id=\"{floor}:profile\" dimensions=\"3\" points=\"{values}\"/>"
            )
            .unwrap();
            writeln!(geometry,"    <slab id=\"{floor}\" profile=\"{floor}:profile\" material=\"{}\" thickness=\"0.2\" area=\"area_{}\"/>",materials[&t.band],t.band).unwrap();
            for j in 0..points.len() {
                let mut key = [points[j], points[(j + 1) % points.len()]];
                key.sort();
                *edges.entry(key).or_default() += 1;
            }
        }
        let mut wall = 0;
        for (edge, count) in edges {
            if count != 1 {
                continue;
            }
            let [(x, y), (xx, yy)] = edge;
            let name = format!("{namespace}/wall_{wall}");
            wall += 1;
            writeln!(
                source,
                "  <path id=\"{name}:path\" points=\"{} {} 0; {} {} 0\"/>",
                x as f64 * self.grid,
                y as f64 * self.grid,
                xx as f64 * self.grid,
                yy as f64 * self.grid
            )
            .unwrap();
            writeln!(
                geometry,
                "    <wall id=\"{name}\" path=\"{name}:path\" height=\"5\" thickness=\"0.32\"/>"
            )
            .unwrap();
        }
        writeln!(
            source,
            "  <assembly id=\"{}\">\n{geometry}  </assembly>\n</architecture>",
            allocate("ground".into())
        )
        .unwrap();
        superspec_profiles::parse(&source).map_err(|e| e.to_string())?;
        Ok(source)
    }
}

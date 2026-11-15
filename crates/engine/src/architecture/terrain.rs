use super::*;
use superspec_aml::architecture::Value;

pub(super) fn validate(geometry: &Geometry) -> Result<(), String> {
    if geometry.points.is_empty()
        || geometry.triangles.is_empty()
        || geometry.points.len() > 1_048_576
        || geometry.triangles.len() > 2_000_000
    {
        return Err("Terrain or mesh exceeds its sample/triangle budget or is empty.".into());
    }
    if geometry
        .points
        .iter()
        .flatten()
        .any(|v| !v.is_finite() || v.abs() > 1e7)
    {
        return Err("Mesh coordinates must be finite within ten million meters.".into());
    }
    for t in &geometry.triangles {
        if t.iter().any(|i| *i >= geometry.points.len()) {
            return Err("Mesh triangle index out of bounds.".into());
        }
        let [a, b, c] = t.map(|i| geometry.points[i]);
        if length(cross(sub(b, a), sub(c, a))) < 1e-10 {
            return Err("Mesh contains a degenerate triangle.".into());
        }
    }
    Ok(())
}
impl Compiler<'_> {
    pub(super) fn terrain(&self, index: usize, node: &Resolved) -> Result<Profile, Diagnostic> {
        let source = &node.source;
        let sources = [
            source.text("profile").is_some(),
            source.text("heightmap").is_some(),
            ["columns", "rows", "heights"]
                .iter()
                .any(|k| source.attributes.contains_key(*k)),
            ["points", "triangles"]
                .iter()
                .any(|k| source.attributes.contains_key(*k)),
        ];
        if sources.iter().filter(|v| **v).count() != 1 {
            return Err(source.fail(
                "AML_TERRAIN",
                "Terrain requires exactly one profile, heightmap, numeric grid, or explicit patch.",
            ));
        }
        if !sources[1]
            && (source.attributes.contains_key("min-height")
                || source.attributes.contains_key("max-height")
                || source.boolean("flip-y", false))
        {
            return Err(source.fail(
                "AML_TERRAIN",
                "Height range and row flipping require a heightmap.",
            ));
        }
        let tile = source.number("tile-size", 64.);
        if !(1.0..=64.).contains(&tile) || tile.fract() != 0. {
            return Err(source.fail(
                "AML_TERRAIN",
                "Tile size must be an integer from 1 to 64 cells.",
            ));
        }
        let mut profile = if sources[0] {
            self.profile(source)?
        } else if sources[1] || sources[2] {
            let raster = source
                .text("heightmap")
                .map(|path| {
                    self.asset(path)
                        .and_then(|bytes| superspec_geometry::heightmap::decode(&bytes))
                })
                .transpose()
                .at(source)?;
            let cols = raster
                .as_ref()
                .map(|r| r.width as f64)
                .unwrap_or(source.number("columns", 0.));
            let rows = raster
                .as_ref()
                .map(|r| r.height as f64)
                .unwrap_or(source.number("rows", 0.));
            if cols < 2.
                || rows < 2.
                || cols.fract() != 0.
                || rows.fract() != 0.
                || cols * rows > 1_048_576.
            {
                return Err(source.fail(
                    "AML_TERRAIN",
                    "Grid dimensions exceed the sample budget or have fewer than two rows/columns.",
                ));
            }
            if ((cols - 1.) / tile).ceil() * ((rows - 1.) / tile).ceil() > 16384. {
                return Err(source.fail("AML_COMPLEXITY", "Terrain tile budget exceeded."));
            }
            let (cols, rows) = (cols as usize, rows as usize);
            let cell = source.vector("cell-size", [1., 1.]);
            if cell.iter().any(|v| *v <= 0.) {
                return Err(source.fail("AML_TERRAIN", "Cell size must be positive."));
            }
            let heights = match source.attributes.get("heights") {
                Some(Value::Vector(values)) => values.as_slice(),
                _ => &[],
            };
            if raster.is_none() && heights.len() != cols * rows {
                return Err(
                    source.fail("AML_TERRAIN", "Grid dimensions must match height samples.")
                );
            }
            let min = source.number("min-height", 0.);
            let max = source.number("max-height", 1.);
            if max < min {
                return Err(source.fail(
                    "AML_TERRAIN",
                    "Maximum height must not be below minimum height.",
                ));
            }
            let mut points = vec![];
            for y in 0..rows {
                for x in 0..cols {
                    let z = if let Some(r) = &raster {
                        let row = if source.boolean("flip-y", false) {
                            rows - 1 - y
                        } else {
                            y
                        };
                        min + (max - min) * r.samples[row * cols + x] as f64 / r.maximum
                    } else {
                        heights[y * cols + x]
                    };
                    points.push([x as f64 * cell[0], y as f64 * cell[1], z]);
                }
            }
            let mut triangles = vec![];
            for y in 0..rows - 1 {
                for x in 0..cols - 1 {
                    let a = y * cols + x;
                    triangles.extend([[a, a + 1, a + cols + 1], [a, a + cols + 1, a + cols]]);
                }
            }
            Profile {
                points,
                triangles,
                loops: vec![],
            }
        } else {
            Profile {
                points: source.points("points"),
                triangles: source.triangles(),
                loops: vec![],
            }
        };
        if source.text("orientation") == Some("down") && !sources[3] {
            for t in &mut profile.triangles {
                t.swap(0, 2);
            }
        }
        validate(&Geometry {
            points: profile.points.clone(),
            triangles: profile.triangles.clone(),
        })
        .at(source)?;
        let grades = self
            .resolution
            .nodes
            .iter()
            .enumerate()
            .filter(|(_, n)| n.source.kind == "grade")
            .filter_map(|(i, n)| {
                self.resolution
                    .reference(i, n.source.text("host").unwrap())
                    .ok()
                    .filter(|h| *h == index)
                    .map(|_| n)
            })
            .collect::<Vec<_>>();
        if !grades.is_empty() {
            if sources[3] {
                return Err(source.fail(
                    "AML_GRADE",
                    "Grading requires a grid, heightmap or constrained height profile.",
                ));
            }
            profile = self.grade_terrain(&profile, &grades)?;
        }
        let world = profile.transformed(node.matrix);
        validate(&Geometry {
            points: world.points.clone(),
            triangles: world.triangles.clone(),
        })
        .at(source)?;
        Ok(world)
    }
}

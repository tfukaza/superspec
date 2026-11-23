//! The lowest graphics level's shop interior: the room is furnished out of sight exactly as
//! at any level (every seeded draw is made), and what it holds is then drawn as blocks, one
//! to a cell of the room, each the tight bounds of the goods in its cell in their own mean
//! colours. Shelves, counters, heaps and hung goods keep their places, massing and colour
//! variation; lights, wall finishes and posters are passed through as they were drawn.
use super::super::{
    fixture::Fixture,
    graphics_detail::GraphicsDetail,
    mesh::{Mesh, Mode},
    pen::{Pen, Tint},
};
use std::collections::BTreeMap;
use superspec_geometry::{
    Matrix, V3,
    math::{add, cross, length, scale, sub},
};

/// A cell of the room, in meters: about a shelf bay wide and two shelves high.
const CELL: V3 = [0.75, 0.75, 0.56];
/// Nothing is drawn thinner than this at a reduced level.
const THINNEST: f64 = 0.025;

/// Surfaces drawn as they are: emitters, wall and floor finishes, printed sheets and glass.
fn passed(slot: &str, part: &str) -> bool {
    matches!(
        slot,
        "tube-light"
            | "interior"
            | "concrete"
            | "shop-floor"
            | "posters"
            | "restaurant-tiles"
            | "restaurant-surfaces"
            | "display-glass"
            | "interior-lit"
            | "lettering"
            | "glass"
    ) || part.contains("tube-emitter")
        || part.contains("bulb-emitter")
}

#[derive(Clone, Copy)]
struct Block {
    low: V3,
    high: V3,
    /// Area-weighted colour sums below and above the cell's middle, and their areas.
    bands: [(V3, f64); 2],
}

fn clip(polygon: &[V3], axis: usize, bound: f64, above: bool) -> Vec<V3> {
    let inside = |p: &V3| {
        if above {
            p[axis] >= bound
        } else {
            p[axis] <= bound
        }
    };
    let mut output = Vec::with_capacity(polygon.len() + 2);
    for (i, a) in polygon.iter().enumerate() {
        let b = &polygon[(i + 1) % polygon.len()];
        if inside(a) {
            output.push(*a);
        }
        if inside(a) != inside(b) {
            let t = (bound - a[axis]) / (b[axis] - a[axis]);
            let mut cut = add(*a, scale(sub(*b, *a), t));
            cut[axis] = bound;
            output.push(cut);
        }
    }
    output
}

fn area(polygon: &[V3]) -> f64 {
    (1..polygon.len().saturating_sub(1))
        .map(|i| {
            length(cross(
                sub(polygon[i], polygon[0]),
                sub(polygon[i + 1], polygon[0]),
            )) / 2.
        })
        .sum()
}

/// Furnishes a room through `furnish` and draws it massed into `d`, whose frame is the
/// room's (see the module).
pub(super) fn room(
    d: &mut Fixture<'_, '_>,
    furnish: impl FnOnce(&mut Fixture<'_, '_>) -> Result<(), String>,
) -> Result<(), String> {
    let mut scratch = Mesh::new(Mode::Render, 400_000, &|| false)?;
    scratch.set_graphics_detail(GraphicsDetail::VeryLow);
    let mut pen = Pen::new(Matrix::default(), &d.pen.part)?;
    pen.mirrored = d.pen.mirrored;
    furnish(&mut Fixture {
        pen,
        mesh: &mut scratch,
        repeats: d.repeats,
    })?;
    let mut blocks: BTreeMap<(bool, [i64; 3]), Block> = BTreeMap::new();
    for (index, corners) in scratch.indices.chunks_exact(3).enumerate() {
        if index.is_multiple_of(256) {
            d.mesh.check_work(0)?;
        }
        let surface = &scratch.surfaces[scratch.triangle_surfaces[index]];
        let vertices = [0, 1, 2].map(|k| scratch.vertices[corners[k] as usize]);
        let points = vertices.map(|v| v.position);
        if passed(&surface.slot, &surface.part) {
            let mut pen = d.pen.clone();
            pen.part.clone_from(&surface.part);
            let colors = vertices.map(|v| v.color);
            let color = |p: V3| -> Result<V3, String> {
                points
                    .iter()
                    .position(|v| *v == p)
                    .map(|i| colors[i])
                    .ok_or_else(|| "Missing passed surface tint.".into())
            };
            let normals = vertices.map(|v| v.normal);
            pen.face_raw(
                d.mesh,
                &surface.slot,
                &points,
                add(add(normals[0], normals[1]), normals[2]),
                &Tint::Field(&color),
                1.,
                Some(&vertices.map(|v| v.uv)),
                Some(&normals),
            )?;
            continue;
        }
        let color: V3 =
            std::array::from_fn(|axis| vertices.iter().map(|v| v.color[axis]).sum::<f64>() / 3.);
        // Lamp lenses and bulbs mass into unshaded blocks of their own, so a shop's lights
        // still read as lights; dimmer unshaded goods (glossy fruit, glass) join the rest.
        let unshaded = surface.slot == "pane" && (color[0] + color[1] + color[2]) / 3. > 0.75;
        let range: [[i64; 2]; 3] = std::array::from_fn(|axis| {
            let low = points.iter().map(|p| p[axis]).fold(f64::INFINITY, f64::min);
            let high = points
                .iter()
                .map(|p| p[axis])
                .fold(f64::NEG_INFINITY, f64::max);
            [
                (low / CELL[axis]).floor() as i64,
                (high / CELL[axis]).floor() as i64,
            ]
        });
        if (0..3).any(|axis| range[axis][1] - range[axis][0] > 256) {
            return Err("Shop massing exceeds its bounded room.".into());
        }
        for x in range[0][0]..=range[0][1] {
            for y in range[1][0]..=range[1][1] {
                for z in range[2][0]..=range[2][1] {
                    let cell = [x, y, z];
                    let mut polygon = points.to_vec();
                    for axis in 0..3 {
                        let low = cell[axis] as f64 * CELL[axis];
                        polygon = clip(&polygon, axis, low, true);
                        polygon = clip(&polygon, axis, low + CELL[axis], false);
                    }
                    if polygon.is_empty() {
                        continue;
                    }
                    let block = blocks.entry((unshaded, cell)).or_insert(Block {
                        low: [f64::INFINITY; 3],
                        high: [f64::NEG_INFINITY; 3],
                        bands: [([0.; 3], 0.); 2],
                    });
                    let mut middle = 0.;
                    for p in &polygon {
                        for axis in 0..3 {
                            block.low[axis] = block.low[axis].min(p[axis]);
                            block.high[axis] = block.high[axis].max(p[axis]);
                        }
                        middle += p[2] / polygon.len() as f64;
                    }
                    // A sliver still counts for its colour.
                    let weight = area(&polygon) + 1e-9;
                    let band = usize::from(middle > (z as f64 + 0.5) * CELL[2]);
                    block.bands[band].0 = add(block.bands[band].0, scale(color, weight));
                    block.bands[band].1 += weight;
                }
            }
        }
    }
    for ((unshaded, _), block) in blocks {
        d.mesh.check_work(0)?;
        let Block {
            mut low,
            mut high,
            bands,
        } = block;
        for axis in 0..3 {
            if high[axis] - low[axis] < THINNEST {
                let centre = (low[axis] + high[axis]) / 2.;
                low[axis] = centre - THINNEST / 2.;
                high[axis] = centre + THINNEST / 2.;
            }
        }
        let mean = |band: usize| {
            let (sum, weight) = bands[band];
            (weight > 0.).then(|| scale(sum, 1. / weight))
        };
        let lower = mean(0).or(mean(1)).unwrap_or([0.5; 3]);
        let upper = mean(1).unwrap_or(lower);
        let middle = (low[2] + high[2]) / 2.;
        let color = |p: V3| -> Result<V3, String> { Ok(if p[2] < middle { lower } else { upper }) };
        let slot = if unshaded { "pane" } else { "paint" };
        let repeat = d.repeat(slot);
        let corner = |x: bool, y: bool, z: bool| {
            [
                if x { high[0] } else { low[0] },
                if y { high[1] } else { low[1] },
                if z { high[2] } else { low[2] },
            ]
        };
        let mut faces = vec![
            (
                [0., -1., 0.],
                [
                    corner(false, false, false),
                    corner(true, false, false),
                    corner(true, false, true),
                    corner(false, false, true),
                ],
            ),
            (
                [0., 1., 0.],
                [
                    corner(false, true, false),
                    corner(true, true, false),
                    corner(true, true, true),
                    corner(false, true, true),
                ],
            ),
            (
                [-1., 0., 0.],
                [
                    corner(false, false, false),
                    corner(false, true, false),
                    corner(false, true, true),
                    corner(false, false, true),
                ],
            ),
            (
                [1., 0., 0.],
                [
                    corner(true, false, false),
                    corner(true, true, false),
                    corner(true, true, true),
                    corner(true, false, true),
                ],
            ),
            (
                [0., 0., 1.],
                [
                    corner(false, false, true),
                    corner(true, false, true),
                    corner(true, true, true),
                    corner(false, true, true),
                ],
            ),
        ];
        // What hangs clear of the floor shows its underside.
        if low[2] > 0.5 {
            faces.push((
                [0., 0., -1.],
                [
                    corner(false, false, false),
                    corner(true, false, false),
                    corner(true, true, false),
                    corner(false, true, false),
                ],
            ));
        }
        for (out, points) in faces {
            d.pen.face_raw(
                d.mesh,
                slot,
                &points,
                out,
                &Tint::Field(&color),
                repeat,
                None,
                None,
            )?;
        }
    }
    Ok(())
}

//! Ground geometry and finite raster receivers. Inputs remain separate from authored collision.
use super::{
    context::Wall,
    coverage, field,
    layout::Layout,
    plan::{Patch, Pigment, Planner, Settings},
    raster,
    surface::{self, Finish, Source},
    terrain,
};
use crate::worldspec::themes::kowloon::{
    bake_jobs::{Job, Recipe},
    mesh::{Mesh, Vertex},
    weathered,
};
use std::collections::BTreeMap;
use superspec_geometry::obstructions::Budget;
use superspec_geometry::{
    Matrix, V3,
    math::{cross, dot, sub},
};
use superspec_types::rng::SeededRng;

pub struct Assets {
    pub sources: BTreeMap<Finish, Source>,
    pub tile_bed: Source,
    pub edge: Source,
    pub water: Source,
    pub paper: Source,
    pub brick: Source,
    pub can: Source,
}
pub struct Site<'a> {
    pub host: &'a str,
    /// Visual part emitted inside the owning host; roof terraces use `roof`.
    pub part: &'a str,
    pub seed: &'a str,
    pub layout: &'a Layout,
    /// Resolved world-space horizontal triangles and finite contacts.
    pub faces: &'a [Vec<V3>],
    pub seams: &'a [[V3; 2]],
    pub walls: &'a [Wall],
    pub settings: &'a Settings,
    pub grime: f64,
    pub wear: f64,
    pub sampling: f64,
    pub tile_size: f64,
    pub tile_aspect: Option<f64>,
    pub tile_wear: Option<f64>,
    pub tile_grime: Option<f64>,
    pub density: Option<f64>,
    pub asphalt_density: Option<f64>,
    pub fine_marks: bool,
    pub flush: bool,
}
pub use superspec_model::theme::ground::{Plan, Region};
fn unit(v: V3) -> V3 {
    let length = dot(v, v).sqrt();
    v.map(|v| v / length)
}
#[allow(clippy::too_many_arguments)]
fn emit(
    mesh: &mut Mesh<'_>,
    world: Matrix,
    part: &str,
    source: &Source,
    points: [V3; 3],
    patch: &Patch,
    colors: [V3; 3],
    chart: bool,
) -> Result<(), String> {
    mesh.spend_work(1)?;
    let normal = cross(sub(points[1], points[0]), sub(points[2], points[0]));
    if dot(normal, normal) < 1e-24 {
        return Ok(());
    }
    let vertices = points.map(|p| world.point(p));
    if mesh.surveying() {
        return mesh.survey_triangle(vertices);
    }
    let normal = unit(world.direction(unit(normal)));
    let cosine = superspec_geometry::math::cos(patch.angle);
    let sine = superspec_geometry::math::sin(patch.angle);
    let origin = world.point([0., 0., points[0][2]]);
    let u = unit(world.direction([1., 0., 0.]));
    let v = unit(world.direction([0., 1., 0.]));
    mesh.triangle_precise(
        part,
        &source.id,
        std::array::from_fn(|i| Vertex {
            position: vertices[i],
            normal,
            color: colors[i],
            uv: if chart {
                let p = sub(vertices[i], origin);
                [dot(p, u), dot(p, v)]
            } else {
                [
                    (points[i][0] * cosine - points[i][1] * sine) * source.repeat + patch.phase,
                    (points[i][0] * sine + points[i][1] * cosine) * source.repeat
                        + patch.phase * 0.37,
                ]
            },
        }),
    )
}
fn boundary_edges(faces: &[Vec<V3>]) -> Vec<[V3; 2]> {
    // Insertion order matters for source-identical edge selection. The map stores only indices.
    let mut lookup: BTreeMap<[[i64; 3]; 2], usize> = BTreeMap::new();
    let mut edges: Vec<([V3; 2], usize)> = Vec::new();
    for face in faces {
        for i in 0..3 {
            let points = [face[i], face[(i + 1) % 3]];
            let mut key = points.map(|p| p.map(|v| (v * 1e6 + 0.5).floor() as i64));
            key.sort();
            if let Some(&index) = lookup.get(&key) {
                edges[index].1 += 1;
            } else {
                lookup.insert(key, edges.len());
                edges.push((points, 1));
            }
        }
    }
    edges
        .into_iter()
        .filter_map(|(points, n)| (n == 1).then_some(points))
        .collect()
}
fn drawn(p: V3, elevation: f64, flush: bool) -> V3 {
    if flush { [p[0], p[1], elevation] } else { p }
}
/// Draws floors whose source PBR does not displace geometry; the shared uneven bed is retained.
/// Callers must route authored height-map floors through geometric surface refinement instead.
pub fn draw_preserved(
    mesh: &mut Mesh<'_>,
    site: &Site<'_>,
    assets: &Assets,
    budget: &mut Budget<'_>,
) -> Result<Vec<Region>, String> {
    site.settings.validate()?;
    budget.spend(site.faces.len() + site.seams.len() + site.walls.len())?;
    if site.faces.is_empty() {
        return Ok(vec![]);
    }
    if site.host.is_empty()
        || site.part.is_empty()
        || site.part.len() > 2048
        || site.host.len() > 2048
        || site.seed.len() > 2048
        || site.faces.len() > 65_536
        || site.seams.len() > 20_000
        || site.walls.len() > 20_000
        || site
            .faces
            .iter()
            .any(|f| f.len() != 3 || f.iter().any(|p| !field::valid_point(*p)))
        || site.seams.iter().flatten().any(|p| !field::valid_point(*p))
    {
        return Err("Ground rendering requires bounded identified receiving geometry.".into());
    }
    let world = site.layout.world.checked()?;
    let inverse = world.inverse()?;
    if world.0[0][2].abs() > 1e-10
        || world.0[1][2].abs() > 1e-10
        || world.0[2] != [0., 0., 1.]
        || (dot(world.0[0], world.0[0]) - 1.).abs() > 1e-10
        || (dot(world.0[1], world.0[1]) - 1.).abs() > 1e-10
        || dot(world.0[0], world.0[1]).abs() > 1e-10
        || (world.determinant() - 1.).abs() > 1e-10
    {
        return Err("Ground layout must be a rigid Z-up floor frame.".into());
    }
    let faces: Vec<Vec<_>> = site
        .faces
        .iter()
        .map(|f| f.iter().map(|p| inverse.point(*p)).collect())
        .collect();
    let elevation = faces[0][0][2];
    if faces
        .iter()
        .flatten()
        .any(|p| (p[2] - elevation).abs() > 0.001)
    {
        return Err("Ground receivers must share one horizontal elevation.".into());
    }
    let seams: Vec<_> = site
        .seams
        .iter()
        .map(|e| e.map(|p| inverse.point(p)))
        .collect();
    let walls: Vec<_> = site
        .walls
        .iter()
        .map(|w| Wall {
            host: w.host.clone(),
            seam: w.seam.map(|p| inverse.point(p)),
        })
        .collect();
    let seed = SeededRng::from_text(site.seed).integer(1, 2147483647) as u32;
    let planner = Planner {
        layout: site.layout,
        settings: site.settings,
        seed,
        walls: &walls,
    };
    let patches = planner.patches(&faces, budget)?;
    let boundaries = boundary_edges(&faces);
    let asphalt: Vec<_> = patches.iter().filter_map(|p| p.asphalt.clone()).collect();
    let mut regions = Vec::new();
    for patch in &patches {
        budget.spend(1)?;
        let mut receiving = Vec::new();
        let mut footprint = Vec::new();
        for face in &faces {
            budget.spend(face.len() * 4)?;
            let clipped = field::intersect(face, &patch.polygon);
            if clipped.is_empty() {
                continue;
            }
            receiving.push(face.clone());
            footprint.push(clipped.iter().map(|p| world.point(*p)).collect());
        }
        if receiving.is_empty() {
            continue;
        }
        regions.push(Region {
            id: patch.id.clone(),
            kind: if patch.asphalt.is_some() {
                Finish::Asphalt
            } else {
                patch.kind
            },
            bounds: patch
                .polygon
                .iter()
                .map(|p| world.point([p[0], p[1], elevation]))
                .collect(),
            footprint,
        });
        let pigment = Pigment {
            patch,
            settings: site.settings,
            seed,
            grime: site.grime,
            wear: site.wear,
            tile_size: site.tile_size,
        };
        let terrain = terrain::build(
            site.layout,
            patch,
            &pigment,
            &receiving,
            site.sampling,
            site.fine_marks,
            budget,
        )?;
        let local_asphalt: Vec<_> = asphalt
            .iter()
            .filter(|p| {
                coverage::overlaps(coverage::bounds(p), coverage::bounds(&patch.polygon), 0.22)
            })
            .cloned()
            .collect();
        let mut edges = boundaries.clone();
        if let Some(m) = &patch.manhole {
            edges.extend(
                m.outer
                    .iter()
                    .enumerate()
                    .map(|(i, &p)| [p, m.outer[(i + 1) % m.outer.len()]]),
            );
        }
        let mut sources = assets.sources.clone();
        if patch.kind == Finish::Tile {
            sources.insert(Finish::Substrate, assets.tile_bed.clone());
        }
        let field_recipe = surface::Recipe {
            seed,
            kind: patch.kind,
            polygon: patch.polygon.clone(),
            asphalt: local_asphalt,
            deposits: terrain.paints.clone(),
            tile_size: site.tile_size,
            tile_aspect: site.tile_aspect,
            tile_wear: Some(site.tile_wear.unwrap_or(0.9)),
            tile_grime: Some(site.tile_grime.unwrap_or(site.grime)),
            worn_edges: patch.worn_edges.clone(),
            angle: patch.angle,
            phase: patch.phase,
            relief: !site.flush && site.settings.relief > 0.,
            receiving_edges: edges,
            sources,
        };
        // Admission also establishes all static finish bindings before any of this patch is emitted.
        surface::Field::new(&field_recipe, budget)?;
        let source = |f: &Finish| {
            assets
                .sources
                .get(f)
                .ok_or_else(|| format!("Missing ground material {f:?}."))
        };
        let mut opaque = Vec::new();
        for piece in &terrain.pieces {
            if matches!(
                piece.finish,
                Finish::ManholeRim | Finish::ManholeCover | Finish::ManholeSlot
            ) {
                for i in 1..piece.points.len() - 1 {
                    let face = [piece.points[0], piece.points[i], piece.points[i + 1]];
                    emit(
                        mesh,
                        world,
                        site.part,
                        source(&piece.finish)?,
                        face.map(|p| drawn(p, elevation, site.flush)),
                        patch,
                        face.map(|p| pigment.color(p, piece.finish)),
                        false,
                    )?;
                }
            } else {
                opaque.push(piece);
            }
        }
        if !opaque.is_empty() {
            let bounds = coverage::bounds(
                &opaque
                    .iter()
                    .flat_map(|p| p.points.iter().copied())
                    .collect::<Vec<_>>(),
            );
            let default_density = if patch.kind == Finish::Tile {
                256_f64
            } else {
                16_f64
            };
            let default_density = if field_recipe.asphalt.is_empty() {
                default_density
            } else {
                default_density.max(site.asphalt_density.unwrap_or(default_density))
            };
            let density = site.density.unwrap_or(default_density);
            let first = mesh.triangle_count();
            for piece in opaque {
                for i in 1..piece.points.len() - 1 {
                    emit(
                        mesh,
                        world,
                        site.part,
                        source(&piece.finish)?,
                        [piece.points[0], piece.points[i], piece.points[i + 1]]
                            .map(|p| drawn(p, elevation, site.flush)),
                        patch,
                        [[1.; 3]; 3],
                        true,
                    )?;
                }
            }
            if !mesh.surveying() && mesh.triangle_count() > first {
                let recipe = raster::Recipe {
                    bounds,
                    surface: field_recipe,
                    patch: patch.clone(),
                    settings: site.settings.clone(),
                    grime: site.grime,
                    wear: site.wear,
                    tile_size: site.tile_size,
                    elevation,
                    density,
                };
                weathered::receive_surface(
                    mesh,
                    first,
                    Job {
                        recipe: Recipe::Pavement(Box::new(recipe)),
                        density,
                    },
                    format!("{}/{}/{}", site.host, site.seed, patch.id),
                    None,
                )?;
            }
        }
        if patch.wet && site.settings.relief > 0. && !site.flush {
            let water = elevation + site.settings.relief * (0.13 + site.settings.wetness * 0.16);
            for piece in &terrain.pieces {
                if matches!(
                    piece.finish,
                    Finish::ManholeRim | Finish::ManholeCover | Finish::ManholeSlot
                ) {
                    continue;
                }
                budget.spend(piece.points.len())?;
                let points = field::clip(&piece.points, |p| {
                    water
                        - (elevation + (p[2] - elevation).clamp(0., site.settings.relief))
                        - 0.0015
                });
                for i in 1..points.len().saturating_sub(1) {
                    emit(
                        mesh,
                        world,
                        site.part,
                        &assets.water,
                        [points[0], points[i], points[i + 1]].map(|p| [p[0], p[1], water]),
                        patch,
                        [[1.; 3]; 3],
                        false,
                    )?;
                }
            }
        }
        if !site.flush {
            close_edges(
                mesh,
                world,
                site.part,
                &assets.edge,
                patch,
                &terrain.patch_faces,
                &boundaries,
                elevation,
                site.settings,
                budget,
            )?;
        }
        debris(
            mesh,
            site,
            assets,
            patch,
            &receiving,
            &terrain.patch_faces,
            &seams,
            elevation,
            seed,
            budget,
        )?;
    }
    Ok(regions)
}
#[allow(clippy::too_many_arguments)]
fn close_edges(
    mesh: &mut Mesh<'_>,
    world: Matrix,
    host: &str,
    source: &Source,
    patch: &Patch,
    faces: &[Vec<V3>],
    boundaries: &[[V3; 2]],
    elevation: f64,
    settings: &Settings,
    budget: &mut Budget<'_>,
) -> Result<(), String> {
    let water = elevation + settings.relief * (0.13 + settings.wetness * 0.16);
    for polygon in faces {
        for (i, &a) in polygon.iter().enumerate() {
            let b = polygon[(i + 1) % polygon.len()];
            budget.spend(boundaries.len())?;
            if !boundaries.iter().any(|e| {
                field::edge_distance(a, e[0], e[1]) < 1e-7
                    && field::edge_distance(b, e[0], e[1]) < 1e-7
            }) {
                continue;
            }
            let mut points = vec![a, b];
            if patch.wet && (a[2] - water) * (b[2] - water) < 0. {
                let t = (water - a[2]) / (b[2] - a[2]);
                points.insert(1, std::array::from_fn(|i| a[i] + (b[i] - a[i]) * t));
            }
            for edge in points.windows(2) {
                let top = [edge[0], edge[1]]
                    .map(|p| [p[0], p[1], if patch.wet { p[2].max(water) } else { p[2] }]);
                let bottom = top.map(|p| [p[0], p[1], elevation]);
                for face in [[top[0], bottom[0], bottom[1]], [top[0], bottom[1], top[1]]] {
                    emit(mesh, world, host, source, face, patch, [[0.8; 3]; 3], false)?;
                }
            }
        }
    }
    Ok(())
}
#[allow(clippy::too_many_arguments)]
fn debris(
    mesh: &mut Mesh<'_>,
    site: &Site<'_>,
    assets: &Assets,
    patch: &Patch,
    receiving: &[Vec<V3>],
    patch_faces: &[Vec<V3>],
    seams: &[[V3; 2]],
    elevation: f64,
    seed: u32,
    budget: &mut Budget<'_>,
) -> Result<(), String> {
    if site.settings.litter == 0. {
        return Ok(());
    }
    let mut rng = SeededRng::from_text(&format!("{seed}/{}/litter", patch.id));
    let size = site.settings.patch_size;
    for i in 0..9 {
        budget.spend(seams.len() + 1)?;
        let p = [
            patch.center[0] + rng.float(-0.45, 0.45) * size,
            patch.center[1] + rng.float(-0.45, 0.45) * size,
            elevation,
        ];
        if patch.manhole.as_ref().is_some_and(|m| {
            superspec_geometry::math::hypot(p[0] - m.center[0], p[1] - m.center[1]) < 0.65
        }) {
            continue;
        }
        let edge = seams
            .iter()
            .map(|e| seam_distance(p, *e))
            .fold(f64::INFINITY, f64::min);
        if rng.next_f64() > site.settings.litter * if edge < 0.8 { 0.8 } else { 0.17 }
            || edge < 0.12
        {
            continue;
        }
        let width = rng.float(0.055, 0.2);
        let depth = rng.float(0.06, 0.21);
        let angle = rng.float(0., std::f64::consts::PI * 2.);
        let (s, c) = (
            superspec_geometry::math::sin(angle),
            superspec_geometry::math::cos(angle),
        );
        let mut transform = Matrix([[c, s, 0.], [-s, c, 0.], [0., 0., 1.], [p[0], p[1], 0.]]);
        let footprint: Vec<_> = field::rectangle(-width / 2., -depth / 2., width, depth, 0.)
            .into_iter()
            .map(|p| transform.point(p))
            .collect();
        if !super::plan::supported(&footprint, &receiving.iter().collect::<Vec<_>>(), budget)?
            || (field::area(&field::intersect(&footprint, &patch.polygon))
                - field::area(&footprint))
            .abs()
                > 1e-7
        {
            continue;
        }
        let mut highest = f64::NEG_INFINITY;
        for face in patch_faces {
            budget.spend(face.len() * footprint.len())?;
            for p in field::intersect(face, &footprint) {
                highest = highest.max(drawn(p, elevation, site.flush)[2]);
            }
        }
        let base = highest + 0.001;
        let kind = rng.next_f64();
        if kind < 0.6 {
            let points: Vec<_> = footprint
                .iter()
                .enumerate()
                .map(|(n, p)| {
                    [
                        p[0],
                        p[1],
                        base + if n == 1 { rng.float(0.004, 0.02) } else { 0. },
                    ]
                })
                .collect();
            let center = [p[0], p[1], base + rng.float(0.004, 0.012)];
            for n in 0..4 {
                emit(
                    mesh,
                    site.layout.world,
                    site.part,
                    &assets.paper,
                    [center, points[n], points[(n + 1) % 4]],
                    patch,
                    [[1.; 3], [0.8; 3], [1.; 3]],
                    false,
                )?;
            }
        } else {
            transform.0[3] = [p[0], p[1], base];
            let height = rng.float(0.018, 0.045);
            let source = if kind < 0.85 {
                &assets.brick
            } else {
                &assets.can
            };
            let name = format!("ground/litter/{}/{i}", patch.id.replace('/', "-"));
            let phase = SeededRng::from_text(&format!("{}/{name}", site.host)).float(0., 100.);
            litter_box(
                mesh,
                site.layout.world.then(transform),
                &format!("{}/$dressing/{name}", site.part),
                source,
                width,
                depth,
                height,
                phase,
            )?;
        }
    }
    Ok(())
}
fn seam_distance(p: V3, seam: [V3; 2]) -> f64 {
    let d = sub(seam[1], seam[0]);
    let t = (dot(sub(p, seam[0]), d) / dot(d, d).max(1e-12)).clamp(0., 1.);
    let offset = std::array::from_fn(|i| p[i] - seam[0][i] - d[i] * t);
    dot(offset, offset).sqrt()
}
#[allow(clippy::too_many_arguments)]
fn litter_box(
    mesh: &mut Mesh<'_>,
    world: Matrix,
    part: &str,
    source: &Source,
    width: f64,
    depth: f64,
    height: f64,
    phase: f64,
) -> Result<(), String> {
    // Preserve the source BoxGeometry Float32 construction, translation and normal matrix.
    let faces = [
        (
            [[1., 1., 1.], [1., 1., 0.], [1., 0., 1.], [1., 0., 0.]],
            [1., 0., 0.],
        ),
        (
            [[0., 1., 0.], [0., 1., 1.], [0., 0., 0.], [0., 0., 1.]],
            [-1., 0., 0.],
        ),
        (
            [[0., 1., 0.], [1., 1., 0.], [0., 1., 1.], [1., 1., 1.]],
            [0., 1., 0.],
        ),
        (
            [[0., 0., 1.], [1., 0., 1.], [0., 0., 0.], [1., 0., 0.]],
            [0., -1., 0.],
        ),
        (
            [[0., 1., 1.], [1., 1., 1.], [0., 0., 1.], [1., 0., 1.]],
            [0., 0., 1.],
        ),
        (
            [[1., 1., 0.], [0., 1., 0.], [1., 0., 0.], [0., 0., 0.]],
            [0., 0., -1.],
        ),
    ];
    for (points, normal) in faces {
        let local = points.map(|p| {
            [
                ((p[0] - 0.5) * width) as f32 as f64,
                ((p[1] - 0.5) * depth) as f32 as f64,
                (((p[2] - 0.5) * height) as f32 as f64 + height / 2.) as f32 as f64,
            ]
        });
        let points = local.map(|p| world.point(p).map(|v| v as f32 as f64));
        let inverse = world.inverse()?;
        let n = unit([
            dot(inverse.0[0], normal),
            dot(inverse.0[1], normal),
            dot(inverse.0[2], normal),
        ])
        .map(|v| v as f32 as f64);
        let axes = if normal[2].abs() > 0.8 {
            [0, 1]
        } else if normal[1].abs() > 0.8 {
            [0, 2]
        } else {
            [1, 2]
        };
        let orientation = if phase.floor() as i64 % 2 != 0 {
            -1.
        } else {
            1.
        };
        for indices in [[0, 2, 1], [2, 3, 1]] {
            if mesh.surveying() {
                mesh.survey_triangle(indices.map(|i| points[i]))?;
            } else {
                mesh.triangle_precise(
                    part,
                    &source.id,
                    indices.map(|i| Vertex {
                        position: points[i],
                        normal: n,
                        uv: [
                            local[i][axes[0]] * source.repeat * orientation + phase,
                            local[i][axes[1]] * source.repeat * orientation + phase * 0.37,
                        ],
                        color: [0.8; 3],
                    }),
                )?;
            }
        }
    }
    Ok(())
}

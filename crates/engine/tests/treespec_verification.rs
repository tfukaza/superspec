//! Writes each TreeSpec fixture as a GLB for looking at, and prints timings. Both tests are
//! ignored: run them with `-- --ignored --nocapture`. Output goes to `TREESPEC_OUTPUT`, or
//! `generated/verification/treespec` in the repository. Vertex colours stand in for the
//! painted materials that do not exist yet: a root-to-tip gradient, the colour variation
//! value and soft light on the shade normal.
use serde_json::json;
use std::{path::PathBuf, time::Instant};
use superspec_engine::treespec::{
    FoliageMesh, HEIGHT_RATIO, InstanceBatch, RANDOM, SHADE_NORMAL, VARIATION,
    grass::{self, DensityMask, GrassPatch, GrassSite, GroundMesh},
    tree::{self, GeneratedTree},
};

type V3 = [f64; 3];
const SUN: V3 = [-0.303, -0.505, 0.808];

fn output() -> PathBuf {
    let path = std::env::var_os("TREESPEC_OUTPUT").map_or_else(
        || PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../generated/verification/treespec"),
        PathBuf::from,
    );
    std::fs::create_dir_all(&path).unwrap();
    path
}
fn dot(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn unit(a: V3) -> V3 {
    let length = dot(a, a).sqrt().max(1e-12);
    a.map(|v| v / length)
}
fn mix(a: V3, b: V3, t: f64) -> V3 {
    std::array::from_fn(|i| a[i] + (b[i] - a[i]) * t)
}
fn apply(transform: &[V3; 4], p: V3, translate: bool) -> V3 {
    std::array::from_fn(|axis| {
        transform[0][axis] * p[0]
            + transform[1][axis] * p[1]
            + transform[2][axis] * p[2]
            + if translate { transform[3][axis] } else { 0. }
    })
}

/// One drawable mesh of a GLB: positions, normals and linear vertex colours, Z up.
#[derive(Default)]
struct Part {
    name: String,
    positions: Vec<V3>,
    normals: Vec<V3>,
    colors: Vec<V3>,
    indices: Vec<u32>,
    double_sided: bool,
}
impl Part {
    fn new(name: &str, double_sided: bool) -> Self {
        Self {
            name: name.into(),
            double_sided,
            ..Self::default()
        }
    }
    /// Adds a mesh under a transform; `paint` gives a vertex its normal and colour.
    fn add(
        &mut self,
        mesh: &FoliageMesh,
        transform: &[V3; 4],
        paint: &dyn Fn(usize, V3) -> (V3, V3),
    ) {
        let first = self.positions.len() as u32;
        for vertex in 0..mesh.positions.len() {
            let world = apply(transform, mesh.positions[vertex], true);
            let (normal, color) = paint(vertex, world);
            self.positions.push(world);
            self.normals.push(normal);
            self.colors.push(color);
        }
        self.indices.extend(mesh.indices.iter().map(|i| first + i));
    }
    /// Triangles as written: a two-sided part is written with both faces.
    fn triangles(&self) -> usize {
        self.indices.len() / 3 * if self.double_sided { 2 } else { 1 }
    }
}

fn glb(parts: &[Part]) -> Vec<u8> {
    let mut binary = Vec::<u8>::new();
    let (mut views, mut accessors, mut meshes, mut nodes, mut materials) =
        (vec![], vec![], vec![], vec![], vec![]);
    let mut view = |bytes: Vec<u8>, target: u32, views: &mut Vec<serde_json::Value>| {
        views.push(json!({"buffer": 0, "byteOffset": binary.len(), "byteLength": bytes.len(), "target": target}));
        binary.extend(bytes);
        while binary.len() % 4 != 0 {
            binary.push(0);
        }
        views.len() - 1
    };
    // glTF is Y up: (x, y, z) becomes (x, z, -y).
    let upright = |values: &[V3]| -> Vec<u8> {
        values
            .iter()
            .flat_map(|p| [p[0] as f32, p[2] as f32, -p[1] as f32])
            .flat_map(f32::to_le_bytes)
            .collect()
    };
    for part in parts.iter().filter(|part| !part.indices.is_empty()) {
        let count = part.positions.len();
        let mut low = [f32::INFINITY; 3];
        let mut high = [f32::NEG_INFINITY; 3];
        for p in &part.positions {
            for (axis, value) in [p[0] as f32, p[2] as f32, -p[1] as f32]
                .into_iter()
                .enumerate()
            {
                low[axis] = low[axis].min(value);
                high[axis] = high[axis].max(value);
            }
        }
        let base = accessors.len();
        let position = view(upright(&part.positions), 34962, &mut views);
        accessors.push(json!({"bufferView": position, "componentType": 5126, "count": count, "type": "VEC3", "min": low, "max": high}));
        let normal = view(upright(&part.normals), 34962, &mut views);
        accessors.push(
            json!({"bufferView": normal, "componentType": 5126, "count": count, "type": "VEC3"}),
        );
        let color = view(
            part.colors
                .iter()
                .flatten()
                .flat_map(|v| (v.clamp(0., 1.) as f32).to_le_bytes())
                .collect(),
            34962,
            &mut views,
        );
        accessors.push(
            json!({"bufferView": color, "componentType": 5126, "count": count, "type": "VEC3"}),
        );
        let uv = view(vec![0; count * 8], 34962, &mut views);
        accessors
            .push(json!({"bufferView": uv, "componentType": 5126, "count": count, "type": "VEC2"}));
        // A two-sided renderer turns the normal over on back faces, which would turn a
        // shade normal into the canopy. Both faces are written instead, sharing normals.
        let mut faces = part.indices.clone();
        if part.double_sided {
            faces.extend(
                part.indices
                    .chunks_exact(3)
                    .flat_map(|t| [t[0], t[2], t[1]]),
            );
        }
        let indices = view(
            faces.iter().flat_map(|i| i.to_le_bytes()).collect(),
            34963,
            &mut views,
        );
        accessors.push(json!({"bufferView": indices, "componentType": 5125, "count": faces.len(), "type": "SCALAR"}));
        materials.push(json!({"name": part.name, "doubleSided": false,
            "pbrMetallicRoughness": {"baseColorFactor": [1, 1, 1, 1], "metallicFactor": 0, "roughnessFactor": 0.9}}));
        meshes.push(json!({"name": part.name, "primitives": [{
            "attributes": {"POSITION": base, "NORMAL": base + 1, "COLOR_0": base + 2, "TEXCOORD_0": base + 3},
            "indices": base + 4, "material": materials.len() - 1}]}));
        nodes.push(json!({"name": part.name, "mesh": meshes.len() - 1}));
    }
    let document = json!({
        "asset": {"version": "2.0", "generator": "superspec treespec verification"},
        "scene": 0, "scenes": [{"nodes": (0..nodes.len()).collect::<Vec<_>>()}],
        "nodes": nodes, "meshes": meshes, "materials": materials,
        "buffers": [{"byteLength": binary.len()}], "bufferViews": views, "accessors": accessors,
    });
    let mut text = serde_json::to_vec(&document).unwrap();
    while text.len() % 4 != 0 {
        text.push(b' ');
    }
    let mut out = Vec::new();
    out.extend(0x4654_6c67u32.to_le_bytes());
    out.extend(2u32.to_le_bytes());
    out.extend(((12 + 8 + text.len() + 8 + binary.len()) as u32).to_le_bytes());
    out.extend((text.len() as u32).to_le_bytes());
    out.extend(0x4e4f_534au32.to_le_bytes());
    out.extend(text);
    out.extend((binary.len() as u32).to_le_bytes());
    out.extend(0x004e_4942u32.to_le_bytes());
    out.extend(binary);
    out
}

fn moved(offset: V3) -> [V3; 4] {
    [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.], offset]
}
fn then(outer: &[V3; 4], inner: &[V3; 4]) -> [V3; 4] {
    [
        apply(outer, inner[0], false),
        apply(outer, inner[1], false),
        apply(outer, inner[2], false),
        apply(outer, inner[3], true),
    ]
}
fn light(normal: V3, floor: f64) -> f64 {
    floor + (1. - floor) * dot(normal, unit(SUN)).max(0.)
}

/// Rolling ground: a grid over a rectangle, `cells` a side.
fn hills(low: [f64; 2], high: [f64; 2], cells: usize, relief: f64) -> GroundMesh {
    let mut ground = GroundMesh {
        positions: vec![],
        indices: vec![],
    };
    for row in 0..=cells {
        for column in 0..=cells {
            let x = low[0] + (high[0] - low[0]) * column as f64 / cells as f64;
            let y = low[1] + (high[1] - low[1]) * row as f64 / cells as f64;
            ground.positions.push([
                x,
                y,
                relief * ((x * 1.9).sin() * (y * 1.4 + 0.6).cos() + 0.4 * (x * 0.7 + y).sin()),
            ]);
        }
    }
    let width = cells as u32 + 1;
    for row in 0..cells as u32 {
        for column in 0..cells as u32 {
            let a = row * width + column;
            ground
                .indices
                .extend([a, a + 1, a + width + 1, a, a + width + 1, a + width]);
        }
    }
    ground
}
fn soil(ground: &GroundMesh, offset: V3) -> Part {
    let mut part = Part::new("ground", false);
    part.positions = ground
        .positions
        .iter()
        .map(|p| [p[0] + offset[0], p[1] + offset[1], p[2] + offset[2]])
        .collect();
    part.normals = vec![[0., 0., 1.]; ground.positions.len()];
    part.colors = vec![[0.12, 0.16, 0.06]; ground.positions.len()];
    part.indices = ground.indices.clone();
    part
}

/// One level of a patch as a single mesh, in the colours a painted material would give.
fn grass_part(patch: &GrassPatch, level: usize, offset: V3) -> Part {
    let mut part = Part::new(&format!("grass-lod{level}"), true);
    let lod = &patch.lods[level];
    for batch in patch.batches() {
        let mesh = &patch.meshes[lod.meshes[batch.mesh]];
        for instance in 0..grass::GrassTile::drawn(batch, lod.keep) {
            let transform = then(&moved(offset), &batch.transforms[instance]);
            let variation = batch.scalars[VARIATION][instance];
            part.add(mesh, &transform, &|vertex, _| {
                let shade = unit(apply(&transform, mesh.vectors[SHADE_NORMAL][vertex], false));
                let t = mesh.scalars[HEIGHT_RATIO][vertex];
                let base = mix(
                    [0.05, 0.13, 0.03],
                    mix([0.3, 0.5, 0.1], [0.5, 0.6, 0.16], variation),
                    t,
                );
                let tone = (0.82 + 0.3 * mesh.scalars[RANDOM][vertex]) * light(shade, 0.65);
                (shade, base.map(|v| v * tone))
            });
        }
    }
    part
}

/// One level of a plant as bark and leaf meshes.
fn tree_parts(tree: &GeneratedTree, level: usize, offset: V3, palette: [V3; 2]) -> Vec<Part> {
    let lod = &tree.lods[level];
    let place = moved(offset);
    let mut bark = Part::new(&format!("{}-bark-lod{level}", tree.id), false);
    for mesh in [Some(lod.trunk), lod.branches]
        .into_iter()
        .flatten()
        .map(|i| &tree.meshes[i])
    {
        bark.add(mesh, &place, &|vertex, _| {
            let tone =
                (0.75 + 0.35 * mesh.scalars[RANDOM][vertex]) * light(mesh.normals[vertex], 0.6);
            (mesh.normals[vertex], [0.26, 0.19, 0.13].map(|v| v * tone))
        });
    }
    let mut leaves = Part::new(&format!("{}-leaves-lod{level}", tree.id), true);
    let mesh = &tree.meshes[lod.leaf];
    let batch: &InstanceBatch = &tree.leaves;
    for instance in 0..lod.leaf_instances {
        let transform = then(&place, &batch.transforms[instance]);
        let shade = batch.vectors[SHADE_NORMAL][instance];
        let variation = batch.scalars[VARIATION][instance];
        leaves.add(mesh, &transform, &|vertex, _| {
            let t = mesh.scalars[HEIGHT_RATIO][vertex];
            let tone =
                (0.75 + 0.25 * t) * (0.9 + 0.25 * mesh.scalars[RANDOM][vertex]) * light(shade, 0.5);
            (
                shade,
                mix(palette[0], palette[1], variation).map(|v| v * tone),
            )
        });
    }
    let mut parts = vec![bark, leaves];
    // The canopy hull: a shell behind the leaves, or the far level in their place.
    if let Some(hull) = lod.hull_mesh {
        let mut shell = Part::new(&format!("{}-hull-lod{level}", tree.id), false);
        let mesh = &tree.meshes[hull];
        shell.add(mesh, &place, &|vertex, _| {
            let tone = light(mesh.normals[vertex], 0.5);
            (
                mesh.normals[vertex],
                mix(palette[0], palette[1], 0.5).map(|v| v * tone),
            )
        });
        parts.push(shell);
    }
    parts
}

fn palette(id: &str) -> [V3; 2] {
    match id {
        "conifer" => [[0.06, 0.24, 0.16], [0.16, 0.42, 0.2]],
        "shrub" => [[0.16, 0.42, 0.1], [0.5, 0.66, 0.16]],
        _ => [[0.14, 0.38, 0.08], [0.42, 0.62, 0.14]],
    }
}
fn trees() -> Vec<GeneratedTree> {
    [
        (
            "shrub",
            include_str!("../src/treespec/fixtures/shrub.tree.json"),
        ),
        (
            "broadleaf",
            include_str!("../src/treespec/fixtures/broadleaf.tree.json"),
        ),
        (
            "conifer",
            include_str!("../src/treespec/fixtures/conifer.tree.json"),
        ),
    ]
    .into_iter()
    .map(|(name, text)| {
        tree::generate(
            &tree::parse(&format!("{name}.tree.json"), text).unwrap(),
            &|| false,
        )
        .unwrap()
    })
    .collect()
}
fn meadow() -> grass::GrassDefinition {
    grass::parse(
        "meadow.grass.json",
        include_str!("../src/treespec/fixtures/meadow.grass.json"),
    )
    .unwrap()
}
fn disc(radius: f64) -> GroundMesh {
    let mut ground = GroundMesh {
        positions: vec![[0.; 3]],
        indices: vec![],
    };
    for step in 0..32u32 {
        let turn = step as f64 / 32. * std::f64::consts::TAU;
        ground
            .positions
            .push([radius * turn.cos(), radius * turn.sin(), 0.]);
        ground.indices.extend([0, step + 1, (step + 1) % 32 + 1]);
    }
    ground
}

#[test]
#[ignore = "writes GLB files for visual review"]
fn write_fixture_glbs() {
    let out = output();
    let write = |name: &str, parts: &[Part]| {
        let triangles: usize = parts.iter().map(Part::triangles).sum();
        assert!(
            triangles <= 250_000,
            "{name} has {triangles} triangles, more than a GLB item may"
        );
        std::fs::write(out.join(name), glb(parts)).unwrap();
        println!("{name}: {triangles} triangles");
    };
    let definition = meadow();
    // A 2.6 m by 2.6 m rolling patch at the most detailed level.
    let ground = hills([0., 0.], [2.6, 2.6], 16, 0.12);
    let patch = grass::generate(
        &definition,
        &GrassSite {
            ground: ground.clone(),
            mask: DensityMask::Uniform,
        },
        &|| false,
    )
    .unwrap();
    write(
        "grass.glb",
        &[soil(&ground, [0.; 3]), grass_part(&patch, 0, [0.; 3])],
    );
    // The three levels side by side, each on its own copy of a 2 m patch.
    let ground = hills([0., 0.], [2., 2.], 10, 0.08);
    let patch = grass::generate(
        &definition,
        &GrassSite {
            ground: ground.clone(),
            mask: DensityMask::Uniform,
        },
        &|| false,
    )
    .unwrap();
    let mut parts = vec![];
    for level in 0..3 {
        let offset = [level as f64 * 2.3, 0., 0.];
        parts.push(soil(&ground, offset));
        parts.push(grass_part(&patch, level, offset));
    }
    write("grass-lods.glb", &parts);

    for tree in trees() {
        let reach =
            (tree.bounds[1][0] - tree.bounds[0][0]).max(tree.bounds[1][1] - tree.bounds[0][1]);
        let mut parts = vec![soil(&disc(0.6 * reach), [0.; 3])];
        parts.extend(tree_parts(&tree, 0, [0.; 3], palette(&tree.id)));
        write(&format!("{}.glb", tree.id), &parts);
        let mut parts = vec![];
        for level in 0..3 {
            parts.extend(tree_parts(
                &tree,
                level,
                [level as f64 * reach * 1.15, 0., 0.],
                palette(&tree.id),
            ));
        }
        write(&format!("{}-lods.glb", tree.id), &parts);
    }

    // Beside the washroom slab, which spans x from -0.55 to 2.45 and y from -2.45 to 0.4
    // with its top at z = 0: a strip of ground with grass, and one shrub on it.
    let ground = hills([2.45, -2.45], [4.65, 0.4], 12, 0.03);
    let shrub_at = [3.6, -1.0];
    // Grass everywhere on the strip but under the shrub: a field mask that is zero there.
    let (columns, rows, cell) = (45, 58, 0.05);
    let values = (0..rows)
        .flat_map(|row| (0..columns).map(move |column| (column, row)))
        .map(|(column, row)| {
            let (x, y) = (2.45 + column as f64 * cell, -2.45 + row as f64 * cell);
            let away = ((x - shrub_at[0]).powi(2) + (y - shrub_at[1]).powi(2)).sqrt();
            ((away - 0.2) / 0.25).clamp(0., 1.)
        })
        .collect();
    let site = GrassSite {
        ground: ground.clone(),
        mask: DensityMask::Field {
            origin: [2.45, -2.45],
            cell,
            columns,
            rows,
            values,
        },
    };
    let patch = grass::generate(&definition, &site, &|| false).unwrap();
    write(
        "washroom-grass.glb",
        &[soil(&ground, [0.; 3]), grass_part(&patch, 0, [0.; 3])],
    );
    let shrub = &trees()[0];
    write(
        "washroom-shrub.glb",
        &tree_parts(shrub, 0, [shrub_at[0], shrub_at[1], 0.], palette("shrub")),
    );
}

#[test]
#[ignore = "prints timings; run in release"]
fn timings() {
    let definition = meadow();
    let time = |label: &str, work: &mut dyn FnMut() -> String| {
        let mut best = f64::INFINITY;
        let mut note = String::new();
        for _ in 0..5 {
            let start = Instant::now();
            note = work();
            best = best.min(start.elapsed().as_secs_f64());
        }
        println!("{label}: {:.2} ms ({note})", best * 1e3);
        best
    };
    time("grass clump meshes (4 variants, 3 levels)", &mut || {
        let (meshes, _) = definition.clump_meshes().unwrap();
        format!("{} meshes", meshes.len())
    });
    for side in [10f64.sqrt(), 10., 30.] {
        let site = GrassSite {
            ground: hills([0., 0.], [side, side], 32, 0.1),
            mask: DensityMask::Uniform,
        };
        let mut blades = 0;
        let seconds = time(&format!("grass patch {:.0} m2", side * side), &mut || {
            let patch = grass::generate(&definition, &site, &|| false).unwrap();
            blades = patch.stats.blades;
            format!(
                "{} clumps, {} blades, {} / {} / {} triangles",
                patch.stats.instances,
                patch.stats.blades,
                patch.lods[0].triangles,
                patch.lods[1].triangles,
                patch.lods[2].triangles
            )
        });
        println!(
            "  {:.1} million blades placed per second",
            blades as f64 / seconds / 1e6
        );
    }
    for (name, text) in [
        (
            "shrub",
            include_str!("../src/treespec/fixtures/shrub.tree.json"),
        ),
        (
            "broadleaf",
            include_str!("../src/treespec/fixtures/broadleaf.tree.json"),
        ),
        (
            "conifer",
            include_str!("../src/treespec/fixtures/conifer.tree.json"),
        ),
    ] {
        let definition = tree::parse(name, text).unwrap();
        time(name, &mut || {
            let tree = tree::generate(&definition, &|| false).unwrap();
            format!(
                "{} branches, {} leaf clusters, {} / {} / {} triangles",
                tree.skeleton.len(),
                tree.leaves.len(),
                tree.lods[0].triangles,
                tree.lods[1].triangles,
                tree.lods[2].triangles
            )
        });
    }
}

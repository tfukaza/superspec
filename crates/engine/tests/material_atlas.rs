//! Receiver atlases: the two triangles of a quad share one chart, charts are sized by their faces
//! in meters at one texel density, and a half-turned triangle encodes relief like its partner.
use superspec_engine::authoring::{Output, Receiver, Request, Session};

/// An axis-aligned box with flat-shaded outward faces, split per face as exporters write it.
fn cuboid(min: [f64; 3], max: [f64; 3]) -> Receiver {
    let corner = |x: usize, y: usize, z: usize| {
        [
            [min[0], max[0]][x],
            [min[1], max[1]][y],
            [min[2], max[2]][z],
        ]
    };
    let faces: [([[usize; 3]; 4], [f64; 3]); 6] = [
        ([[0, 0, 1], [1, 0, 1], [1, 1, 1], [0, 1, 1]], [0., 0., 1.]),
        ([[0, 1, 0], [1, 1, 0], [1, 0, 0], [0, 0, 0]], [0., 0., -1.]),
        ([[0, 0, 0], [1, 0, 0], [1, 0, 1], [0, 0, 1]], [0., -1., 0.]),
        ([[1, 1, 0], [0, 1, 0], [0, 1, 1], [1, 1, 1]], [0., 1., 0.]),
        ([[0, 1, 0], [0, 0, 0], [0, 0, 1], [0, 1, 1]], [-1., 0., 0.]),
        ([[1, 0, 0], [1, 1, 0], [1, 1, 1], [1, 0, 1]], [1., 0., 0.]),
    ];
    let (mut positions, mut normals, mut uvs, mut indices) = (vec![], vec![], vec![], vec![]);
    for (quad, normal) in faces {
        let base = (positions.len() / 3) as u32;
        for (index, [x, y, z]) in quad.into_iter().enumerate() {
            positions.extend(corner(x, y, z));
            normals.extend(normal);
            uvs.extend([[0., 0.], [1., 0.], [1., 1.], [0., 1.]][index]);
        }
        indices.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
    }
    Receiver {
        id: "box".into(),
        object_id: "box".into(),
        positions,
        normals,
        uvs,
        indices,
        object_to_world: None,
        contacts: Vec::new(),
    }
}

fn bake(source: &str, receiver: Receiver, resolution: usize) -> Output {
    let request = Request {
        entry: "main.ms".into(),
        source: source.into(),
        resolution,
        receivers: vec![receiver],
        mapping: superspec_engine::authoring::Mapping::World,
        ..Request::default()
    };
    Session::default()
        .build(
            &request,
            1,
            &|name| Err(format!("no asset {name}")),
            &|| false,
        )
        .unwrap_or_else(|e| panic!("{e:?}"))
}

fn mesh(output: &Output) -> &superspec_model::instances::LocalMesh {
    output.model.meshes.values().next().expect("one baked mesh")
}

/// Decoded RGBA texels of one of the material's maps, with the image width.
fn image(output: &Output, normal: bool) -> (usize, Vec<[u8; 4]>) {
    let material = output.model.materials.values().next().unwrap();
    let id = if normal {
        &material.normal_texture
    } else {
        &material.texture
    };
    let decoded =
        superspec_geometry::image::decode(&output.model.images[id.as_ref().unwrap()].bytes)
            .unwrap();
    (
        decoded.width,
        decoded
            .rgba
            .chunks_exact(4)
            .map(|t| [t[0], t[1], t[2], t[3]])
            .collect(),
    )
}

/// Atlas texel rectangle covered by a triangle's UVs.
fn uv_box(
    mesh: &superspec_model::instances::LocalMesh,
    triangle: usize,
    width: f64,
    height: f64,
) -> [f64; 4] {
    let corners = &mesh.indices[triangle * 3..triangle * 3 + 3];
    let xs = corners.iter().map(|i| mesh.uvs[*i as usize][0] * width);
    let ys = corners
        .iter()
        .map(|i| (1. - mesh.uvs[*i as usize][1]) * height);
    [
        xs.clone().fold(f64::INFINITY, f64::min),
        ys.clone().fold(f64::INFINITY, f64::min),
        xs.fold(f64::NEG_INFINITY, f64::max),
        ys.fold(f64::NEG_INFINITY, f64::max),
    ]
}

#[test]
fn quads_share_charts_sized_by_their_faces_at_one_density() {
    // A thin 8 x 0.22 x 3.2 m wall: its big faces get far more texels than its narrow ends.
    let output = bake(
        "material 1; output base_color = vec3(0.5);",
        cuboid([0., 0., 0.], [8., 0.22, 3.2]),
        512,
    );
    let mesh = mesh(&output);
    let (width, texels) = image(&output, false);
    let height = texels.len() / width;
    let boxes: Vec<[f64; 4]> = (0..mesh.indices.len() / 3)
        .map(|triangle| uv_box(mesh, triangle, width as f64, height as f64))
        .collect();
    // Each face's two triangles cover the same rectangle.
    for face in 0..6 {
        let [a, b] = [boxes[face * 2], boxes[face * 2 + 1]];
        for k in 0..4 {
            assert!((a[k] - b[k]).abs() < 1e-9, "face {face}: {a:?} {b:?}");
        }
    }
    // Texels per meter match on the front face (8 m wide) and an end face (0.22 m wide).
    // A chart's legs may lie along either atlas axis, so compare sorted extents with sorted sides.
    let density = |face: usize, meters: [f64; 2]| {
        let [x0, y0, x1, y1] = boxes[face * 2];
        let mut spans = [x1 - x0, y1 - y0];
        spans.sort_by(f64::total_cmp);
        let mut sides = meters;
        sides.sort_by(f64::total_cmp);
        [spans[0] / sides[0], spans[1] / sides[1]]
    };
    let front = density(2, [8., 3.2]);
    let end = density(5, [0.22, 3.2]);
    for value in front.into_iter().chain(end) {
        assert!(
            (value - front[0]).abs() / front[0] < 0.2,
            "{front:?} {end:?}"
        );
    }
    // The faces fill the atlas: the front face's 8 m side spans most of it.
    let [x0, y0, x1, y1] = boxes[4];
    assert!(
        (x1 - x0).max(y1 - y0) > 0.8 * width as f64,
        "{:?}",
        boxes[4]
    );
}

#[test]
fn both_halves_of_a_quad_encode_the_same_relief() {
    // A planar slope rising 2 cm per meter along x has one normal everywhere on a face.
    let output = bake(
        "material 1; output base_color = vec3(0.5); output height = world.x * 0.02;",
        cuboid([0., 0., 0.], [2., 1., 0.1]),
        256,
    );
    let mesh = mesh(&output);
    let (width, normals) = image(&output, true);
    let height = normals.len() / width;
    // Inner texels of the top face (+z): both triangles, away from the gutters.
    let [x0, y0, x1, y1] = uv_box(mesh, 0, width as f64, height as f64);
    let mut seen = Vec::new();
    for y in (y0 as usize + 3)..(y1 as usize - 3) {
        for x in (x0 as usize + 3)..(x1 as usize - 3) {
            seen.push(normals[y * width + x]);
        }
    }
    let mean =
        |channel: usize| seen.iter().map(|t| t[channel] as f64).sum::<f64>() / seen.len() as f64;
    // Dithering moves each channel by at most one step; a mirrored half would differ by far more.
    for texel in &seen {
        for channel in 0..3 {
            assert!(
                (texel[channel] as f64 - mean(channel)).abs() <= 1.01,
                "{texel:?} vs {}",
                mean(channel)
            );
        }
    }
    // The slope is visible: x or y tilts by about 0.02 * 127 ~ 2.5 steps from flat.
    let tilt = (mean(0) - 127.5).abs().max((mean(1) - 127.5).abs());
    assert!((1.5..4.0).contains(&tilt), "tilt {tilt}");
}

#[test]
fn dithering_removes_contour_plateaus_from_gentle_relief() {
    // A slow 2 mm swell across a 2 m face: without dithering, rows of identical normals form
    // plateaus a dozen texels wide whose edges read as stripes. With it, neighbors vary.
    let output = bake(
        "material 1; output base_color = vec3(0.5); output height = sin(world.x * 1.5) * 0.002;",
        cuboid([0., 0., 0.], [2., 2., 0.1]),
        256,
    );
    let mesh = mesh(&output);
    let (width, normals) = image(&output, true);
    let height = normals.len() / width;
    let [x0, y0, x1, _] = uv_box(mesh, 0, width as f64, height as f64);
    let y = y0 as usize + 6;
    let row: Vec<u8> = ((x0 as usize + 3)..(x1 as usize - 3))
        .map(|x| normals[y * width + x][0])
        .collect();
    // Rounding alone gives plateaus over a hundred texels wide (nearly every neighbor equal); a
    // half-step white-noise offset makes neighbors differ about half the time and breaks every
    // plateau up within a few dozen texels.
    let longest = row.chunk_by(|a, b| a == b).map(<[u8]>::len).max().unwrap();
    let equal = row.windows(2).filter(|w| w[0] == w[1]).count() as f64 / (row.len() - 1) as f64;
    assert!(
        longest <= 24 && equal < 0.75,
        "plateau of {longest}, {equal:.2} equal neighbors in {row:?}"
    );
}

#[test]
fn every_chart_orientation_decodes_to_the_world_relief_normal() {
    // A planar height field h = g . world. On each face the shaded normal is the face normal
    // tilted against the in-plane part of g. Unequal sides make charts of both orientations, and
    // each face's two triangles cover the unturned and half-turned cases.
    let gradient = [0.03, -0.05, 0.04];
    let output = bake(
        "material 1; output base_color = vec3(0.5); output height = world.x * 0.03 - world.y * 0.05 + world.z * 0.04;",
        cuboid([0., 0., 0.], [2., 0.6, 1.1]),
        256,
    );
    let mesh = mesh(&output);
    let (width, normals) = image(&output, true);
    let height = normals.len() / width;
    let sub = |a: [f64; 3], b: [f64; 3]| [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
    let dot = |a: [f64; 3], b: [f64; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let unit = |a: [f64; 3]| {
        let l = dot(a, a).sqrt();
        a.map(|v| v / l)
    };
    let cross = |a: [f64; 3], b: [f64; 3]| {
        [
            a[1] * b[2] - a[2] * b[1],
            a[2] * b[0] - a[0] * b[2],
            a[0] * b[1] - a[1] * b[0],
        ]
    };
    for triangle in 0..mesh.indices.len() / 3 {
        let ids: Vec<usize> = mesh.indices[triangle * 3..triangle * 3 + 3]
            .iter()
            .map(|i| *i as usize)
            .collect();
        let p = ids.iter().map(|i| mesh.positions[*i]).collect::<Vec<_>>();
        let uv = ids.iter().map(|i| mesh.uvs[*i]).collect::<Vec<_>>();
        let n = unit(cross(sub(p[1], p[0]), sub(p[2], p[0])));
        // The renderer's frame: world directions of increasing texture u and v.
        let (e1, e2) = (sub(p[1], p[0]), sub(p[2], p[0]));
        let (du1, dv1, du2, dv2) = (
            uv[1][0] - uv[0][0],
            uv[1][1] - uv[0][1],
            uv[2][0] - uv[0][0],
            uv[2][1] - uv[0][1],
        );
        let r = 1. / (du1 * dv2 - du2 * dv1);
        let tangent = unit(std::array::from_fn(|i| (e1[i] * dv2 - e2[i] * dv1) * r));
        let bitangent = unit(std::array::from_fn(|i| (e2[i] * du1 - e1[i] * du2) * r));
        // A texel near the triangle's centroid, pulled toward its corner to stay inside.
        let centroid = [
            (uv[0][0] * 2. + uv[1][0] + uv[2][0]) / 4.,
            (uv[0][1] * 2. + uv[1][1] + uv[2][1]) / 4.,
        ];
        let x = (centroid[0] * width as f64) as usize;
        let y = ((1. - centroid[1]) * height as f64) as usize;
        let texel = normals[y * width + x];
        let local: [f64; 3] = std::array::from_fn(|i| texel[i] as f64 / 255. * 2. - 1.);
        let decoded = unit(std::array::from_fn(|i| {
            tangent[i] * local[0] + bitangent[i] * local[1] + n[i] * local[2]
        }));
        let along = dot(gradient, n);
        let in_plane: [f64; 3] = std::array::from_fn(|i| gradient[i] - n[i] * along);
        let expected = unit(sub(n, in_plane));
        let degrees = dot(decoded, expected).clamp(-1., 1.).acos().to_degrees();
        assert!(
            degrees < 1.5,
            "triangle {triangle}: {decoded:?} vs {expected:?} ({degrees:.2} deg)"
        );
    }
}

/// Flat quads in the XY plane facing +Z, each given by its low corner and size in meters.
fn quads(list: &[([f64; 2], [f64; 2])]) -> Receiver {
    let (mut positions, mut normals, mut uvs, mut indices) = (vec![], vec![], vec![], vec![]);
    for ([x, y], [w, h]) in list {
        let base = (positions.len() / 3) as u32;
        for [dx, dy] in [[0., 0.], [1., 0.], [1., 1.], [0., 1.]] {
            positions.extend([x + dx * w, y + dy * h, 0.]);
            normals.extend([0., 0., 1.]);
            uvs.extend([dx, dy]);
        }
        indices.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
    }
    Receiver {
        id: "quads".into(),
        object_id: "quads".into(),
        positions,
        normals,
        uvs,
        indices,
        object_to_world: None,
        contacts: Vec::new(),
    }
}

fn bake_at(receiver: Receiver, source: &str, density: Option<f64>) -> Result<Output, String> {
    let request = Request {
        entry: "main.ms".into(),
        source: source.into(),
        resolution: 512,
        receivers: vec![receiver],
        mapping: superspec_engine::authoring::Mapping::World,
        texel_density: density,
        ..Request::default()
    };
    Session::default()
        .build(
            &request,
            1,
            &|name| Err(format!("no asset {name}")),
            &|| false,
        )
        .map_err(|errors| format!("{errors:?}"))
}

#[test]
fn a_texel_density_gives_small_receivers_small_textures() {
    let plain = "material 1; output base_color = vec3(0.5);";
    let atlas = |output: &Output| {
        let atlas = &output.metadata["atlases"][0];
        (
            atlas["width"].as_f64().unwrap(),
            atlas["height"].as_f64().unwrap(),
            atlas["texelsPerMeter"].as_f64().unwrap(),
        )
    };
    // A 20 x 10 x 10 cm block at 100 texels per meter: four 24 x 14 charts and two 14 x 14
    // charts (2-texel gutters included) first fit a 64-texel square, in three shelves.
    let block = || cuboid([0., 0., 0.], [0.2, 0.1, 0.1]);
    let sized = bake_at(block(), plain, Some(100.)).unwrap();
    assert_eq!(atlas(&sized), (64., 42., 100.));
    assert_eq!(image(&sized, false).0, 64);
    // Without a density the same block fills the requested resolution.
    let filled = bake_at(block(), plain, None).unwrap();
    assert_eq!(atlas(&filled).0, 512.);
    assert!(atlas(&filled).2 > 1000., "{:?}", atlas(&filled));
    // A wall that cannot reach the density keeps the resolution, exactly as without one.
    let wall = || cuboid([0., 0., 0.], [8., 0.22, 3.2]);
    let capped = bake_at(wall(), plain, Some(1000.)).unwrap();
    assert_eq!(
        atlas(&capped),
        atlas(&bake_at(wall(), plain, None).unwrap())
    );
    assert_eq!(
        image(&capped, false),
        image(&bake_at(wall(), plain, None).unwrap(), false)
    );
    assert!(atlas(&capped).2 < 100.);
    assert!(
        bake_at(block(), plain, Some(0.5))
            .err()
            .expect("the density is rejected")
            .contains("Texel density")
    );
}

#[test]
fn charts_below_the_density_repeat_their_samples() {
    // A 1 m quad sets the receiver's density near 500 texels per meter; the 1 mm quads beside
    // it are far smaller than a texel, so each is padded to the minimum chart and sampled once.
    let mut list = vec![([0., 0.], [1., 1.])];
    list.extend((0..6).map(|i| ([1.2 + f64::from(i) * 0.0137, 0.], [0.001, 0.001])));
    // Color changes several times across each small quad, so finer sampling would show.
    let output = bake_at(
        quads(&list),
        "material 1; output base_color = vec3(fract(world.x * 3100.0), fract(world.y * 2900.0), 0.5);",
        None,
    )
    .unwrap();
    let mesh = mesh(&output);
    let (width, texels) = image(&output, false);
    let height = texels.len() / width;
    let mut colors = std::collections::BTreeSet::new();
    for quad in 1..list.len() {
        let [x0, y0, x1, y1] = uv_box(mesh, quad * 2, width as f64, height as f64);
        // The whole padded chart, gutters included, holds one color.
        let (x0, y0, x1, y1) = (
            x0 as usize - 2,
            y0 as usize - 2,
            x1 as usize + 2,
            y1 as usize + 2,
        );
        assert_eq!((x1 - x0, y1 - y0), (8, 8));
        let first = texels[y0 * width + x0];
        for y in y0..y1 {
            for x in x0..x1 {
                assert_eq!(texels[y * width + x], first, "quad {quad} at {x},{y}");
            }
        }
        colors.insert(first);
    }
    assert!(colors.len() > 1, "each small quad has its own sample");
    // The large quad still varies texel by texel.
    let [x0, y0, ..] = uv_box(mesh, 0, width as f64, height as f64);
    let at = |dx: usize| texels[(y0 as usize + 3) * width + x0 as usize + dx];
    assert_ne!(at(3), at(4));
}

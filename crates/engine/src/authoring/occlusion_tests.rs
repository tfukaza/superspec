//! The occluder service against expectations derived apart from it: the shadow of a square under
//! a sun with a disk (closed form in the plane of the sun's directions), the ambient occlusion
//! and sky exposure of a floor beside a wall (the area of a circle's segment), and the contract
//! around them: keys, threads, cancellation, the ray allowance, texels without a value. The
//! washroom tests are ignored by default; they need `generated/washroom`.
use super::*;
use crate::authoring::Mapping;
use std::cell::Cell;
use std::f64::consts::PI;
use superspec_types::lighting::Lighting;

fn quad(id: &str, corners: [V3; 4]) -> Receiver {
    let normal = unit(cross(
        sub(corners[1], corners[0]),
        sub(corners[3], corners[0]),
    ))
    .unwrap();
    Receiver {
        id: id.into(),
        object_id: id.into(),
        positions: corners.iter().flatten().copied().collect(),
        normals: [normal; 4].iter().flatten().copied().collect(),
        uvs: vec![0., 0., 1., 0., 1., 1., 0., 1.],
        indices: vec![0, 1, 2, 0, 2, 3],
        object_to_world: None,
        contacts: Vec::new(),
    }
}

fn mesh(receiver: &Receiver) -> OccluderMesh {
    OccluderMesh {
        id: receiver.id.clone(),
        positions: receiver.positions.clone(),
        indices: receiver.indices.clone(),
        object_to_world: None,
    }
}

fn scene(receivers: &[&Receiver]) -> Occluders {
    Occluders::new(&receivers.iter().map(|r| mesh(r)).collect::<Vec<_>>()).unwrap()
}

/// A level floor of `size` meters a side about the origin, facing up.
fn floor(size: f64) -> Receiver {
    let h = size / 2.;
    quad(
        "floor",
        [[-h, -h, 0.], [h, -h, 0.], [h, h, 0.], [-h, h, 0.]],
    )
}

fn request(resolution: usize) -> Request {
    Request {
        resolution,
        ..Request::default()
    }
}

fn sun(direction: V3) -> Lighting {
    let mut lighting = Lighting::default();
    lighting.lights[0].direction = unit(direction).unwrap();
    lighting
}

fn bake(
    occluders: &Occluders,
    receivers: &[Receiver],
    request: &Request,
    lighting: &Lighting,
    settings: &OcclusionSettings,
    names: &[&str],
) -> Vec<ReceiverOcclusion> {
    let job = OcclusionJob {
        lighting,
        settings,
        names,
        held: &|_| false,
    };
    occlusion_planes(occluders, receivers, request, &job, &|| false).unwrap()
}

/// The values of an encoded plane, read from the documented layout rather than the decoder.
fn values(plane: &OcclusionPlane) -> Vec<f32> {
    let bytes = plane.bytes.as_ref().expect("the plane was baked");
    assert_eq!(&bytes[..4], b"SPL1");
    let word = |at: usize| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap()) as usize;
    assert_eq!((word(4), word(8), word(12)), (plane.width, plane.height, 1));
    let values: Vec<f32> = bytes[16..]
        .chunks_exact(4)
        .map(|chunk| f32::from_le_bytes(chunk.try_into().unwrap()))
        .collect();
    assert_eq!(values.len(), plane.width * plane.height);
    values
}

fn plane<'a>(baked: &'a [ReceiverOcclusion], id: &str, name: &str) -> &'a OcclusionPlane {
    baked
        .iter()
        .find(|receiver| receiver.id == id)
        .and_then(|receiver| receiver.planes.iter().find(|plane| plane.name == name))
        .unwrap_or_else(|| panic!("no plane {name} of {id}"))
}

/// Every inner texel of a receiver's atlas: its index, world position and footprint.
fn inner_texels(receiver: &Receiver, request: &Request) -> (Atlas, Vec<(usize, V3, f64)>) {
    let atlas = Atlas::for_request(receiver.clone(), request).unwrap();
    let mut texels = Vec::new();
    for chart in &atlas.charts {
        for y in GUTTER..chart.height - GUTTER {
            for x in GUTTER..chart.width - GUTTER {
                let (triangle, u, v) = atlas.texel(chart, x, y);
                let sample = atlas.sample(triangle, u, v).unwrap();
                texels.push((
                    (chart.y + y) * atlas.width + chart.x + x,
                    sample.world,
                    sample.footprint,
                ));
            }
        }
    }
    (atlas, texels)
}

/// The share of a unit disk on the far side of a line at the signed distance `offset` from its
/// center, on the side the distance is measured toward: 0 at -1, a half at 0, 1 at 1.
fn disk_share(offset: f64) -> f64 {
    if offset <= -1. {
        0.
    } else if offset >= 1. {
        1.
    } else {
        1. - (offset.acos() - offset * (1. - offset * offset).sqrt()) / PI
    }
}

#[test]
fn a_square_casts_the_shadow_and_penumbra_geometry_gives() {
    // A 1 m square 1 m above a 4 m floor; the sun stands 30 degrees from the zenith toward +x
    // and has an angular radius of 0.05.
    let (height, zenith, radius) = (1.0f64, 30f64.to_radians(), 0.05f64);
    let ground = floor(4.);
    let square = quad(
        "square",
        [
            [-0.5, -0.5, height],
            [0.5, -0.5, height],
            [0.5, 0.5, height],
            [-0.5, 0.5, height],
        ],
    );
    let toward = [zenith.sin(), 0., zenith.cos()];
    let settings = OcclusionSettings {
        sun_radius: radius,
        shadow_samples: 64,
        ..OcclusionSettings::default()
    };
    let request = request(256);
    let baked = bake(
        &scene(&[&ground, &square]),
        std::slice::from_ref(&ground),
        &request,
        &sun(toward),
        &settings,
        &[SHADOW],
    );
    let shadow = values(plane(&baked, "floor", SHADOW));
    let (_, texels) = inner_texels(&ground, &request);
    // Seen from a floor point, the rays that pass an edge of the square (a line along y at
    // x = edge) lie in the plane through the point and the edge. In the plane of directions one
    // unit toward the sun that plane is a line, at the tangent of its angle to the sun's center,
    // and the sun's disk has the radius tan(radius): the lit share is a circle's segment.
    let expected = |point: V3, lift: f64| {
        let rise = height - lift;
        let share = |edge: f64| {
            let run = edge - point[0];
            let sine = (toward[0] * rise - toward[2] * run) / run.hypot(rise);
            disk_share(sine.asin().tan() / radius.tan())
        };
        // Rays strike the square's plane beyond its low edge and short of its high edge.
        share(-0.5) - share(0.5)
    };
    let (mut checked, mut error, mut worst) = (0, 0., 0f64);
    let (mut lit, mut dark) = (0, 0);
    for &(texel, point, footprint) in &texels {
        // Away from the square's other two edges, whose penumbra is not in this expectation.
        if point[1].abs() > 0.3 {
            continue;
        }
        let want = expected(point, settings.bias * footprint);
        let got = f64::from(shadow[texel]);
        if want == 0. {
            assert_eq!(got, 0., "lit at {point:?}");
            lit += 1;
        } else if want == 1. {
            assert_eq!(got, 1., "in the umbra at {point:?}");
            dark += 1;
        }
        worst = worst.max((got - want).abs());
        error += (got - want).abs();
        checked += 1;
    }
    assert!(
        checked > 5000 && lit > 1000 && dark > 500,
        "{checked} {lit} {dark}"
    );
    // 64 directions resolve the disk's segment to a few of them.
    assert!(worst <= 0.07, "worst error {worst}");
    assert!(
        error / (checked as f64) < 0.004,
        "mean error {}",
        error / checked as f64
    );

    // The penumbra of the square's +x edge along the row nearest y = 0: centered on the edge's
    // shadow and as wide as the sun's disk makes it.
    let footprint = texels[0].2;
    let rise = height - settings.bias * footprint;
    let center = 0.5 - rise * zenith.tan();
    let width = rise * ((zenith + radius).tan() - (zenith - radius).tan());
    let row = texels
        .iter()
        .map(|texel| texel.1[1])
        .min_by(|a, b| a.abs().total_cmp(&b.abs()))
        .unwrap();
    let mut partial: Vec<(f64, f64)> = texels
        .iter()
        .filter(|texel| (texel.1[1] - row).abs() < 1e-9 && texel.1[0] > center - 0.4)
        .map(|texel| (texel.1[0], f64::from(shadow[texel.0])))
        .filter(|(_, value)| *value > 0. && *value < 1.)
        .collect();
    partial.sort_by(|a, b| a.0.total_cmp(&b.0));
    let measured = partial.len() as f64 * footprint;
    assert!(
        (measured - width).abs() <= 2. * footprint,
        "penumbra {measured} m, expected {width} m"
    );
    let middle = (partial[0].0 + partial[partial.len() - 1].0) / 2.;
    assert!(
        (middle - center).abs() <= footprint,
        "penumbra about {middle}, expected {center}"
    );
    assert!(
        partial.windows(2).all(|pair| pair[0].1 >= pair[1].1),
        "the shadow fades monotonically across the penumbra"
    );
}

/// A 2 m floor and a wall standing on its +x edge, far longer and taller than any reach used.
fn corner() -> (Receiver, Receiver) {
    (
        floor(2.),
        quad(
            "wall",
            [[1., 4., 0.], [1., -4., 0.], [1., -4., 4.], [1., 4., 4.]],
        ),
    )
}

#[test]
fn a_concave_corner_is_occluded_by_the_segment_of_the_hemisphere_the_wall_fills() {
    let (ground, wall) = corner();
    let reach = 0.5;
    let settings = OcclusionSettings {
        occlusion_samples: 64,
        occlusion_distance: reach,
        exposure_samples: 64,
        exposure_distance: reach,
        ..OcclusionSettings::default()
    };
    let request = request(128);
    let baked = bake(
        &scene(&[&ground, &wall]),
        std::slice::from_ref(&ground),
        &request,
        &Lighting::default(),
        &settings,
        &[OCCLUSION, SUN_EXPOSURE],
    );
    let occlusion = values(plane(&baked, "floor", OCCLUSION));
    let exposure = values(plane(&baked, "floor", SUN_EXPOSURE));
    let (_, texels) = inner_texels(&ground, &request);
    // A direction reaches the wall within `reach` when its component toward the wall is at
    // least distance / reach. Cosine-weighted directions are even over the unit disk they
    // project to, so the occluded share is the disk's segment beyond that line. The floor
    // faces up, so its sky exposure within the same reach is the rest.
    let expected = |distance: f64| 1. - disk_share(distance / reach);
    let mut columns: BTreeMap<i64, (f64, f64, f64, usize)> = BTreeMap::new();
    for &(texel, point, _) in &texels {
        let distance = 1. - point[0];
        let want = expected(distance);
        let got = f64::from(occlusion[texel]);
        let open = f64::from(exposure[texel]);
        if distance >= reach + 1e-9 {
            assert_eq!((got, open), (0., 1.), "beyond the reach at {point:?}");
        }
        assert!(
            (got - want).abs() <= 0.08,
            "occlusion {got} for {want} at {point:?}"
        );
        assert!(
            (open - (1. - want)).abs() <= 0.08,
            "exposure {open} for {} at {point:?}",
            1. - want
        );
        let column = columns.entry((distance * 1e6).round() as i64).or_default();
        *column = (distance, column.1 + got, column.2 + open, column.3 + 1);
    }
    // Each texel turns the pattern its own way, so a column's mean is closer than any texel.
    let mut nearest = f64::INFINITY;
    for (distance, occluded, open, count) in columns.into_values() {
        assert!(count > 100);
        let want = expected(distance);
        let (occluded, open) = (occluded / count as f64, open / count as f64);
        assert!(
            (occluded - want).abs() < 0.012,
            "{occluded} for {want} at {distance}"
        );
        assert!((open - (1. - want)).abs() < 0.012, "{open} at {distance}");
        nearest = nearest.min(distance);
    }
    // At the wall's foot nearly half of the hemisphere is wall.
    assert!(expected(nearest) > 0.48);
}

#[test]
fn sky_exposure_is_what_falls_onto_the_outer_side() {
    let settings = OcclusionSettings {
        exposure_samples: 64,
        ..OcclusionSettings::default()
    };
    let request = request(64);
    let exposure = |receiver: Receiver, others: &[&Receiver]| {
        let mut all = vec![&receiver];
        all.extend(others);
        let baked = bake(
            &scene(&all),
            std::slice::from_ref(&receiver),
            &request,
            &Lighting::default(),
            &settings,
            &[SUN_EXPOSURE],
        );
        let found = values(plane(&baked, &receiver.id, SUN_EXPOSURE));
        let (_, texels) = inner_texels(&receiver, &request);
        texels
            .iter()
            .map(|texel| f64::from(found[texel.0]))
            .collect::<Vec<_>>()
    };
    let mean = |values: &[f64]| values.iter().sum::<f64>() / values.len() as f64;
    // Open level ground receives everything, a ceiling nothing.
    assert!(exposure(floor(1.), &[]).iter().all(|v| *v == 1.));
    let ceiling = quad(
        "ceiling",
        [[-1., -1., 2.], [-1., 1., 2.], [1., 1., 2.], [1., -1., 2.]],
    );
    assert!(exposure(ceiling, &[]).iter().all(|v| *v == 0.));
    // An open wall receives the half of the sky on its side.
    let wall = quad(
        "wall",
        [[0., 1., 0.], [0., -1., 0.], [0., -1., 2.], [0., 1., 2.]],
    );
    let open = exposure(wall, &[]);
    assert!(open.iter().all(|v| (v - 0.5).abs() <= 0.08));
    assert!((mean(&open) - 0.5).abs() < 0.01, "{}", mean(&open));
    // Under a wide roof 3 m up, only directions that travel the 20 m of reach before they
    // climb to it stay open: those lower than 3/20, the rim of the disk beyond the radius
    // sqrt(1 - (3/20)^2), which is (3/20)^2 of its area.
    let roof = quad(
        "roof",
        [
            [-500., -500., 3.],
            [-500., 500., 3.],
            [500., 500., 3.],
            [500., -500., 3.],
        ],
    );
    assert_eq!(settings.exposure_distance, 20.);
    let sheltered = exposure(floor(1.), &[&roof]);
    let rim = (3f64 / 20.).powi(2);
    assert!(
        sheltered.iter().all(|v| (v - rim).abs() <= 1. / 64.),
        "{sheltered:?}"
    );
}

/// A box of unequal sides: its charts leave part of the atlas empty.
fn block() -> Vec<Receiver> {
    let (x, y, z) = (0.9, 0.5, 0.3);
    let mut positions = Vec::new();
    let mut normals = Vec::new();
    let mut uvs = Vec::new();
    let mut indices = Vec::new();
    let faces: [(V3, [V3; 4]); 6] = [
        (
            [0., 0., 1.],
            [[0., 0., z], [x, 0., z], [x, y, z], [0., y, z]],
        ),
        (
            [0., 0., -1.],
            [[0., 0., 0.], [0., y, 0.], [x, y, 0.], [x, 0., 0.]],
        ),
        (
            [0., -1., 0.],
            [[0., 0., 0.], [x, 0., 0.], [x, 0., z], [0., 0., z]],
        ),
        (
            [0., 1., 0.],
            [[0., y, 0.], [0., y, z], [x, y, z], [x, y, 0.]],
        ),
        (
            [-1., 0., 0.],
            [[0., 0., 0.], [0., 0., z], [0., y, z], [0., y, 0.]],
        ),
        (
            [1., 0., 0.],
            [[x, 0., 0.], [x, y, 0.], [x, y, z], [x, 0., z]],
        ),
    ];
    for (normal, corners) in faces {
        let start = (positions.len() / 3) as u32;
        for corner in corners {
            // The block stands on the floor, off its center.
            positions.extend([corner[0] - 0.2, corner[1] - 0.1, corner[2]]);
            normals.extend(normal);
        }
        uvs.extend([0., 0., 1., 0., 1., 1., 0., 1.]);
        indices.extend([start, start + 1, start + 2, start, start + 2, start + 3]);
    }
    vec![
        floor(3.),
        Receiver {
            id: "block".into(),
            object_id: "block".into(),
            positions,
            normals,
            uvs,
            indices,
            object_to_world: None,
            contacts: Vec::new(),
        },
    ]
}

fn block_scene() -> (Occluders, Vec<Receiver>) {
    let receivers = block();
    (scene(&receivers.iter().collect::<Vec<_>>()), receivers)
}

#[test]
fn texels_outside_every_chart_hold_no_value_and_gutters_repeat_their_chart() {
    let (occluders, receivers) = block_scene();
    let request = request(64);
    // The reach covers the block from anywhere inside it.
    let settings = OcclusionSettings {
        occlusion_distance: 2.,
        ..OcclusionSettings::default()
    };
    let baked = bake(
        &occluders,
        &receivers,
        &request,
        &Lighting::default(),
        &settings,
        &PLANES,
    );
    let atlas = Atlas::for_request(receivers[1].clone(), &request).unwrap();
    let mut covered = vec![false; atlas.width * atlas.height];
    for chart in &atlas.charts {
        for y in chart.y..chart.y + chart.height {
            for x in chart.x..chart.x + chart.width {
                covered[y * atlas.width + x] = true;
            }
        }
    }
    assert!(
        covered.contains(&false),
        "the block's atlas has empty texels"
    );
    for name in PLANES {
        let found = values(plane(&baked, "block", name));
        for (texel, value) in found.iter().enumerate() {
            assert_eq!(value.is_nan(), !covered[texel], "{name} at {texel}");
            assert!(value.is_nan() || (0. ..=1.).contains(value));
        }
        for chart in &atlas.charts {
            for y in 0..chart.height {
                for x in 0..chart.width {
                    let nearest = (
                        x.clamp(GUTTER, chart.width - GUTTER - 1),
                        y.clamp(GUTTER, chart.height - GUTTER - 1),
                    );
                    let at = |(x, y): (usize, usize)| {
                        found[(chart.y + y) * atlas.width + chart.x + x].to_bits()
                    };
                    assert_eq!(at((x, y)), at(nearest), "{name} gutter");
                }
            }
        }
        // The planes decode as the external inputs a bake of the same request reads.
        let bytes = plane(&baked, "block", name).bytes.clone().unwrap();
        let decoded = Plane::decode_external(name, &bytes, atlas.width, atlas.height).unwrap();
        assert_eq!(decoded.stored(), covered.iter().filter(|c| **c).count());
    }
    // The floor under the block is closed, sheltered and in shadow; the block's top is open.
    let (mut under, mut top) = (0, 0);
    for (index, id, level, want) in [
        (0, "floor", 0., (1., 0., 1.)),
        (1, "block", 0.3, (0., 1., 0.)),
    ] {
        let [shadow, occlusion, exposure] = PLANES.map(|name| values(plane(&baked, id, name)));
        let (_, texels) = inner_texels(&receivers[index], &request);
        for (texel, point, _) in texels {
            let inside = (-0.15..0.65).contains(&point[0]) && (-0.05..0.35).contains(&point[1]);
            if point[2] == level && inside {
                assert_eq!(
                    (occlusion[texel], exposure[texel], shadow[texel]),
                    want,
                    "{id} at {point:?}"
                );
                *(if index == 0 { &mut under } else { &mut top }) += 1;
            }
        }
    }
    assert!(under > 20 && top > 20, "{under} {top}");
}

#[test]
fn any_number_of_threads_gives_the_same_bytes() {
    let (occluders, receivers) = block_scene();
    let request = request(96);
    let with = |threads: usize| {
        let settings = OcclusionSettings {
            threads,
            ..OcclusionSettings::default()
        };
        bake(
            &occluders,
            &receivers,
            &request,
            &Lighting::default(),
            &settings,
            &PLANES,
        )
    };
    let one = with(1);
    for threads in [2, 3, 7] {
        let many = with(threads);
        for (single, several) in one.iter().zip(&many) {
            assert_eq!(single.id, several.id);
            for (a, b) in single.planes.iter().zip(&several.planes) {
                assert_eq!((&a.name, &a.key, a.rays), (&b.name, &b.key, b.rays));
                assert_eq!(
                    a.bytes, b.bytes,
                    "{} of {}, {threads} threads",
                    a.name, single.id
                );
            }
        }
    }
    assert!(one.iter().flat_map(|r| &r.planes).all(|p| p.rays > 0));
}

fn keys(baked: &[ReceiverOcclusion], id: &str) -> [String; 3] {
    PLANES.map(|name| plane(baked, id, name).key.clone())
}

#[test]
fn a_key_follows_exactly_what_its_plane_reads() {
    let (ground, wall) = corner();
    let request = request(64);
    let settings = OcclusionSettings::default();
    let toward = [0.4, -0.2, 0.9];
    let run = |others: &[&Receiver], lighting: &Lighting, settings: &OcclusionSettings| {
        let mut all = vec![&ground, &wall];
        all.extend(others);
        keys(
            &bake(
                &scene(&all),
                std::slice::from_ref(&ground),
                &request,
                lighting,
                settings,
                &PLANES,
            ),
            "floor",
        )
    };
    let base = run(&[], &sun(toward), &settings);
    assert_eq!(base, run(&[], &sun(toward), &settings));
    let changed = |other: &[String; 3]| [0, 1, 2].map(|i| base[i] != other[i]);
    // The sun's direction is read by the shadow alone.
    let moved = run(&[], &sun([-0.3, 0.5, 0.8]), &settings);
    assert_eq!(changed(&moved), [true, false, false]);
    // No plane reads the light's look, strength or the scene's rendering settings.
    let mut styled = sun(toward);
    styled.lights[0].intensity = 9.;
    styled.lights[0].color = [0.2, 0.3, 0.4];
    styled.lights[0].shadow.softness = 0.5;
    styled.lights[0].shadow.noise.seed = 99;
    styled.ambient_occlusion.radius = 3.;
    styled.exposure = 2.;
    assert_eq!(base, run(&[], &styled, &settings));
    // A sun that casts no shadow, and no sun at all, are one constant plane.
    let mut dull = sun(toward);
    dull.lights[0].casts_shadow = false;
    let mut none = sun(toward);
    none.lights.clear();
    let unlit = run(&[], &dull, &settings);
    assert_eq!(changed(&unlit), [true, false, false]);
    assert_eq!(unlit, run(&[], &none, &settings));
    // Geometry no ray can meet (below and behind, far away) changes nothing.
    let far = quad(
        "far",
        [
            [-60., -1., -50.],
            [-58., -1., -50.],
            [-58., 1., -50.],
            [-60., 1., -50.],
        ],
    );
    assert_eq!(base, run(&[&far], &sun(toward), &settings));
    // A sheet 30 m up the sun's direction is beyond the sky exposure's reach and shades.
    let at = mul(unit(toward).unwrap(), 30.);
    let sheet = quad(
        "sheet",
        [
            add(at, [-1., -1., 0.]),
            add(at, [1., -1., 0.]),
            add(at, [1., 1., 0.]),
            add(at, [-1., 1., 0.]),
        ],
    );
    assert_eq!(
        changed(&run(&[&sheet], &sun(toward), &settings)),
        [true, false, false]
    );
    // A pebble on the floor is within every reach.
    let pebble = quad(
        "pebble",
        [
            [0., 0., 0.05],
            [0.1, 0., 0.05],
            [0.1, 0.1, 0.05],
            [0., 0.1, 0.05],
        ],
    );
    assert_eq!(
        changed(&run(&[&pebble], &sun(toward), &settings)),
        [true, true, true]
    );
    // Each quality setting belongs to one plane; the bias to all; threads and the allowance to none.
    let set = |change: &dyn Fn(&mut OcclusionSettings)| {
        let mut settings = settings.clone();
        change(&mut settings);
        changed(&run(&[], &sun(toward), &settings))
    };
    assert_eq!(set(&|s| s.shadow_samples = 8), [true, false, false]);
    assert_eq!(set(&|s| s.sun_radius = 0.01), [true, false, false]);
    assert_eq!(set(&|s| s.shadow_distance = 50.), [true, false, false]);
    assert_eq!(set(&|s| s.occlusion_samples = 8), [false, true, false]);
    assert_eq!(set(&|s| s.occlusion_distance = 0.25), [false, true, false]);
    assert_eq!(set(&|s| s.exposure_samples = 8), [false, false, true]);
    assert_eq!(set(&|s| s.exposure_distance = 5.), [false, false, true]);
    assert_eq!(set(&|s| s.bias = 0.25), [true, true, true]);
    assert_eq!(set(&|s| s.threads = 3), [false, false, false]);
    assert_eq!(set(&|s| s.max_rays = 1 << 40), [false, false, false]);
    // The layout is part of every key.
    let finer = keys(
        &bake(
            &scene(&[&ground, &wall]),
            std::slice::from_ref(&ground),
            &self::request(128),
            &sun(toward),
            &settings,
            &PLANES,
        ),
        "floor",
    );
    assert_eq!(changed(&finer), [true, true, true]);
}

#[test]
fn a_moved_sun_rebakes_the_shadow_alone() {
    let (occluders, receivers) = block_scene();
    let request = request(64);
    let settings = OcclusionSettings::default();
    let first = bake(
        &occluders,
        &receivers,
        &request,
        &sun([0.3, 0.2, 0.9]),
        &settings,
        &PLANES,
    );
    let have: BTreeSet<String> = first
        .iter()
        .flat_map(|receiver| receiver.planes.iter().map(|plane| plane.key.clone()))
        .collect();
    let lighting = sun([-0.5, 0.1, 0.7]);
    let job = OcclusionJob {
        lighting: &lighting,
        settings: &settings,
        names: &PLANES,
        held: &|key| have.contains(key),
    };
    let second = occlusion_planes(&occluders, &receivers, &request, &job, &|| false).unwrap();
    for (before, after) in first.iter().zip(&second) {
        for (before, after) in before.planes.iter().zip(&after.planes) {
            let rebaked = after.name == SHADOW;
            assert_eq!(after.bytes.is_some(), rebaked, "{}", after.name);
            assert_eq!(after.rays > 0, rebaked);
            assert_eq!(before.key != after.key, rebaked);
            assert_eq!(before.occluders > 0, after.occluders > 0);
        }
    }
    // The shadow moved with the sun.
    assert_ne!(
        plane(&first, "floor", SHADOW).bytes,
        plane(&second, "floor", SHADOW).bytes
    );
}

#[test]
fn a_cancelled_bake_returns_nothing() {
    let (occluders, receivers) = block_scene();
    let request = request(96);
    let lighting = Lighting::default();
    for threads in [1, 4] {
        let settings = OcclusionSettings {
            threads,
            ..OcclusionSettings::default()
        };
        let job = OcclusionJob {
            lighting: &lighting,
            settings: &settings,
            names: &PLANES,
            held: &|_| false,
        };
        // Before anything is laid out, and once rows are being cast.
        for after in [0, 4] {
            let asked = Cell::new(0);
            let cancelled = || {
                asked.set(asked.get() + 1);
                asked.get() > after
            };
            let error = occlusion_planes(&occluders, &receivers, &request, &job, &cancelled)
                .expect_err("a cancelled bake has no result");
            assert_eq!(error, "Occlusion bake cancelled.", "{threads} threads");
            assert!(asked.get() > after);
        }
    }
}

#[test]
fn rays_beyond_the_allowance_are_refused_before_any_is_cast() {
    let (occluders, receivers) = block_scene();
    let request = request(64);
    let lighting = Lighting::default();
    let settings = OcclusionSettings {
        max_rays: 1000,
        ..OcclusionSettings::default()
    };
    let asked = Cell::new(0);
    let run = |settings: &OcclusionSettings, held: &dyn Fn(&str) -> bool| {
        let job = OcclusionJob {
            lighting: &lighting,
            settings,
            names: &PLANES,
            held,
        };
        occlusion_planes(&occluders, &receivers, &request, &job, &|| {
            asked.set(asked.get() + 1);
            false
        })
    };
    let error = run(&settings, &|_| false).unwrap_err();
    assert!(
        error.contains("rays, above the allowance of 1000 (maxRays)"),
        "{error}"
    );
    // The bound is every casting texel times its plane's samples, whatever the geometry.
    let inner: usize = receivers
        .iter()
        .map(|receiver| inner_texels(receiver, &request).1.len())
        .sum();
    let defaults = OcclusionSettings::default();
    let bound =
        inner * (defaults.shadow_samples + defaults.occlusion_samples + defaults.exposure_samples);
    assert!(error.contains(&format!("up to {bound} rays")), "{error}");
    assert_eq!(
        asked.get(),
        receivers.len(),
        "refused after the layouts, before any row"
    );
    // Exactly the bound is admitted, and planes the caller holds need no rays.
    let exact = OcclusionSettings {
        max_rays: bound as u64,
        ..defaults.clone()
    };
    let baked = run(&exact, &|_| false).unwrap();
    let cast: u64 = baked.iter().flat_map(|r| &r.planes).map(|p| p.rays).sum();
    assert!(cast > 0 && cast <= bound as u64);
    let none = OcclusionSettings {
        max_rays: 0,
        ..defaults
    };
    assert!(run(&none, &|_| false).is_err());
    let kept = run(&none, &|_| true).unwrap();
    assert!(
        kept.iter()
            .flat_map(|r| &r.planes)
            .all(|p| p.bytes.is_none())
    );
    // Settings and names outside what the service offers are named.
    let job = |settings: &OcclusionSettings, names: &[&str]| {
        let job = OcclusionJob {
            lighting: &lighting,
            settings,
            names,
            held: &|_| false,
        };
        occlusion_planes(&occluders, &receivers, &request, &job, &|| false).unwrap_err()
    };
    let wide = OcclusionSettings {
        occlusion_samples: 5000,
        ..OcclusionSettings::default()
    };
    assert!(job(&wide, &PLANES).contains("occlusionSamples"));
    assert!(job(&OcclusionSettings::default(), &["shade"]).contains("'shade'"));
}

/// The bytes are part of the contract between targets: a Wasm build that runs this test must
/// reach the digest the native build pinned. A change that alters values bumps
/// [`OCCLUSION_VERSION`] and this digest together.
#[test]
fn the_planes_are_the_same_bytes_on_every_target() {
    let (occluders, receivers) = block_scene();
    let baked = bake(
        &occluders,
        &receivers,
        &request(64),
        &Lighting::default(),
        &OcclusionSettings::default(),
        &PLANES,
    );
    let mut hash = Sha256::new();
    for plane in baked.iter().flat_map(|receiver| &receiver.planes) {
        hash.update(plane.key.as_bytes());
        hash.update(plane.bytes.as_ref().unwrap());
    }
    assert_eq!(
        format!("{:x}", hash.finalize()),
        // Version 2: the keys also name a chart's lattice and face membership; the values are
        // version 1's.
        "495544656820a5e31595fe4aa8a1dcdc71182a1aa055e96022a89067e2619726"
    );
}

const SHADE: &str = "material 1;
output base_color = vec3(external(\"shadow\", 0.5));
output roughness = 1 - external(\"occlusion\", 0.5);
output metalness = 0;";

#[test]
fn bound_planes_reach_a_material_bake_of_the_same_request() {
    let ground = floor(4.);
    let square = quad(
        "square",
        [[-1., -1., 1.], [1., -1., 1.], [1., 1., 1.], [-1., 1., 1.]],
    );
    let mut request = Request {
        entry: "shade.ms".into(),
        source: SHADE.into(),
        resolution: 64,
        mapping: Mapping::World,
        receivers: vec![ground.clone()],
        ..Request::default()
    };
    let lighting = sun([0., 0., 1.]);
    let baked = bake(
        &scene(&[&ground, &square]),
        &request.receivers,
        &request,
        &lighting,
        &OcclusionSettings::default(),
        &[SHADOW, OCCLUSION],
    );
    let assets = bind_occlusion(&mut request, &baked);
    assert_eq!(assets.len(), 2);
    assert_eq!(
        request.planes["floor"][SHADOW],
        PlaneSource::Asset(plane(&baked, "floor", SHADOW).asset())
    );
    let output = Session::default()
        .build(
            &request,
            1,
            &|name| {
                assets
                    .get(name)
                    .cloned()
                    .ok_or_else(|| format!("{name} is unavailable"))
            },
            &|| false,
        )
        .unwrap_or_else(|e| panic!("{e:?}"));
    let material = output.model.materials.values().next().unwrap();
    let color = superspec_geometry::image::decode(
        &output.model.images[material.texture.as_ref().unwrap()].bytes,
    )
    .unwrap();
    let shadow = values(plane(&baked, "floor", SHADOW));
    assert_eq!(
        (color.width, color.height),
        (baked[0].planes[0].width, baked[0].planes[0].height)
    );
    let (mut lit, mut dark) = (0, 0);
    for (texel, value) in shadow.iter().enumerate() {
        let pixel = color.rgba[texel * 4];
        if *value == 0. {
            assert_eq!(pixel, 0, "lit texel {texel}");
            lit += 1;
        } else if *value == 1. {
            assert_eq!(pixel, 255, "shadowed texel {texel}");
            dark += 1;
        }
    }
    assert!(lit > 500 && dark > 200, "{lit} {dark}");
}

// ---- The washroom ------------------------------------------------------------------------

mod washroom {
    use super::*;
    use crate::authoring::project::{material_request, receivers};
    use std::path::PathBuf;
    use std::time::Instant;

    fn root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
    }

    fn asset(name: &str) -> Result<Arc<[u8]>, String> {
        let path = match name.strip_prefix("superspec-material:") {
            Some(catalog) => root().join("assets/materials").join(catalog),
            None if name == "washroom.glb" => root().join("generated/washroom/washroom.glb"),
            None => root()
                .join("crates/engine/src/materialspec/program/examples")
                .join(name),
        };
        std::fs::read(&path)
            .map(Arc::from)
            .map_err(|e| format!("{name}: {e}"))
    }

    /// The washroom project `tests/studio/washroom/build.mjs` wrote, at the preview size the
    /// browser test uses (`RESOLUTION`, 256 by default).
    fn project() -> Project {
        let path = root().join("generated/washroom/washroom.studio.json");
        let text = std::fs::read_to_string(&path).unwrap_or_else(|e| {
            panic!(
                "{}: {e}; run node tests/studio/washroom/build.mjs",
                path.display()
            )
        });
        let mut value: serde_json::Value = serde_json::from_str(&text).unwrap();
        let fields = value.as_object_mut().unwrap();
        fields.remove("assets");
        fields.remove("lighting");
        let mut project: Project = serde_json::from_value(value).unwrap();
        project.resolution = std::env::var("RESOLUTION")
            .ok()
            .and_then(|text| text.parse().ok())
            .unwrap_or(256);
        project
    }

    struct Washroom {
        project: Project,
        occluders: Occluders,
        /// The slab and the tall wall, with the bake request that sizes each one's atlas.
        receivers: Vec<(&'static str, Receiver, Request)>,
    }

    fn washroom() -> Washroom {
        let project = project();
        let mut session = Session::default();
        let prepared = session
            .prepare_project(&project, 1, &asset, &|| false)
            .unwrap_or_else(|e| panic!("{e:?}"));
        let scene = prepared.scene;
        let transforms = prepared.transforms;
        let occluders = Occluders::from_scene(&scene).unwrap();
        let mut found = Vec::new();
        for (name, material) in [("slab", "concrete-slab"), ("wall-tall", "concrete-walls")] {
            let assignment = project
                .materials
                .iter()
                .find(|assignment| assignment.id == material)
                .unwrap();
            let ids = BTreeSet::from([assignment.meshes[0].clone()]);
            let receiver = receivers(&scene, &ids, &transforms).pop().unwrap();
            let request = material_request(&project, assignment, "", Vec::new());
            found.push((name, receiver, request));
        }
        Washroom {
            project,
            occluders,
            receivers: found,
        }
    }

    fn out() -> PathBuf {
        let out = root().join("generated/verification/occluder");
        std::fs::create_dir_all(&out).unwrap();
        out
    }

    /// A plane as a gray picture, 0 black to 1 white; texels without a value are dark violet.
    fn picture(plane: &OcclusionPlane, path: &std::path::Path) {
        let rgba: Vec<u8> = values(plane)
            .iter()
            .flat_map(|value| {
                if value.is_nan() {
                    [48, 0, 72, 255]
                } else {
                    let gray = (value.clamp(0., 1.) * 255.).round() as u8;
                    [gray, gray, gray, 255]
                }
            })
            .collect();
        enlarged(plane.width, plane.height, &rgba, path);
    }

    /// Writes an atlas as a PNG of whole texels, enlarged so a preview-sized one can be read.
    fn enlarged(width: usize, height: usize, texels: &[u8], path: &std::path::Path) {
        let scale = (768 / width).max(1);
        let mut rgba = Vec::with_capacity(width * height * scale * scale * 4);
        for y in 0..height * scale {
            for x in 0..width * scale {
                let at = (y / scale * width + x / scale) * 4;
                rgba.extend(&texels[at..at + 4]);
            }
        }
        let image = superspec_geometry::image::Image {
            width: width * scale,
            height: height * scale,
            rgba,
            mime: "image/png",
        };
        std::fs::write(path, superspec_geometry::image::encode(&image).unwrap()).unwrap();
    }

    fn range(times: &[f64]) -> String {
        let low = times.iter().copied().fold(f64::INFINITY, f64::min);
        let high = times.iter().copied().fold(0., f64::max);
        format!("{low:.1}-{high:.1} ms")
    }

    #[test]
    #[ignore = "needs generated/washroom: node tests/studio/washroom/build.mjs"]
    fn washroom_planes_show_the_scene_and_report_their_cost() {
        let scene = washroom();
        let lighting = Lighting::default();
        println!(
            "washroom at {} px, {} texels per meter: {} occluder triangles",
            scene.project.resolution,
            scene.project.texel_density.unwrap_or(0.),
            scene.occluders.triangles()
        );
        let whole: Vec<f64> = (0..5)
            .map(|_| {
                let triangles = scene.occluders.triangles.clone();
                let start = Instant::now();
                let bvh = Bvh::new(triangles);
                let time = start.elapsed().as_secs_f64() * 1e3;
                assert!(!bvh.nodes.is_empty());
                time
            })
            .collect();
        println!("BVH over the whole scene: {}", range(&whole));
        let runs = 3;
        for (name, receiver, request) in &scene.receivers {
            let mut pictures = Vec::new();
            for threads in [0, 1] {
                let settings = OcclusionSettings {
                    threads,
                    ..OcclusionSettings::default()
                };
                for plane_name in PLANES {
                    let mut times = Vec::new();
                    let mut last = None;
                    for _ in 0..runs {
                        let job = OcclusionJob {
                            lighting: &lighting,
                            settings: &settings,
                            names: &[plane_name],
                            held: &|_| false,
                        };
                        let start = Instant::now();
                        let mut baked = occlusion_planes(
                            &scene.occluders,
                            std::slice::from_ref(receiver),
                            request,
                            &job,
                            &|| false,
                        )
                        .unwrap();
                        times.push(start.elapsed().as_secs_f64() * 1e3);
                        last = baked.pop().unwrap().planes.pop();
                    }
                    let plane = last.unwrap();
                    let fastest = times.iter().copied().fold(f64::INFINITY, f64::min);
                    let slowest = times.iter().copied().fold(0., f64::max);
                    let covered = values(&plane).iter().filter(|v| !v.is_nan()).count();
                    println!(
                        "{name} {plane_name}: {}x{} ({covered} texels), {} triangles in reach, {} rays, {} on {} thread(s), {:.1}-{:.1} M rays/s, key {}",
                        plane.width,
                        plane.height,
                        plane.occluders,
                        plane.rays,
                        range(&times),
                        settings.threads(),
                        plane.rays as f64 / slowest / 1e3,
                        plane.rays as f64 / fastest / 1e3,
                        &plane.key[..12],
                    );
                    if threads == 0 {
                        pictures.push(plane);
                    } else {
                        let other = pictures.iter().find(|p| p.name == plane.name).unwrap();
                        assert_eq!(other.bytes, plane.bytes, "threads change nothing");
                    }
                }
            }
            // The cost of what a key needs alone: layout, reach and hash.
            let settings = OcclusionSettings::default();
            let keyed: Vec<f64> = (0..runs)
                .map(|_| {
                    let job = OcclusionJob {
                        lighting: &lighting,
                        settings: &settings,
                        names: &PLANES,
                        held: &|_| true,
                    };
                    let start = Instant::now();
                    occlusion_planes(
                        &scene.occluders,
                        std::slice::from_ref(receiver),
                        request,
                        &job,
                        &|| false,
                    )
                    .unwrap();
                    start.elapsed().as_secs_f64() * 1e3
                })
                .collect();
            println!(
                "{name} keys of all three planes, nothing baked: {}",
                range(&keyed)
            );
            for plane in &pictures {
                let path = out().join(format!("{name}-{}.png", plane.name));
                picture(plane, &path);
                println!("wrote {}", path.display());
                let found = values(plane);
                let covered: Vec<f32> = found.iter().copied().filter(|v| !v.is_nan()).collect();
                let mean = covered.iter().sum::<f32>() / covered.len() as f32;
                let low = covered.iter().filter(|v| **v == 0.).count();
                let high = covered.iter().filter(|v| **v == 1.).count();
                println!(
                    "{name} {}: mean {mean:.3}, {low} texels at 0, {high} at 1 of {}",
                    plane.name,
                    covered.len()
                );
                // Each plane distinguishes parts of the receiver.
                assert!(mean > 0.01 && mean < 0.99);
            }
            // A sun elsewhere leaves occlusion and sky exposure as they are.
            let mut moved = lighting.clone();
            moved.lights[0].direction = unit([4., -2., 5.]).unwrap();
            let have: BTreeSet<String> = pictures.iter().map(|plane| plane.key.clone()).collect();
            let job = OcclusionJob {
                lighting: &moved,
                settings: &settings,
                names: &PLANES,
                held: &|key| have.contains(key),
            };
            let again = occlusion_planes(
                &scene.occluders,
                std::slice::from_ref(receiver),
                request,
                &job,
                &|| false,
            )
            .unwrap();
            let rebaked: Vec<&str> = again[0]
                .planes
                .iter()
                .filter(|plane| plane.bytes.is_some())
                .map(|plane| plane.name.as_str())
                .collect();
            assert_eq!(rebaked, [SHADOW]);
            let path = out().join(format!("{name}-shadow-moved-sun.png"));
            picture(&again[0].planes[0], &path);
            println!(
                "{name}: a moved sun rebaked {rebaked:?}; wrote {}",
                path.display()
            );
        }
    }

    /// Moss where the surface is closed in or sheltered, on plain concrete.
    const MOSS: &str = "material 1;
let closed = external(\"occlusion\", 0);
let open = external(\"sunExposure\", 1);
let damp = clamp(closed * 1.6 + (1 - open) * 0.5, 0, 1);
let grain = noise(surface * 9, seed(\"moss\"));
let patch = noise(surface * 2.5, seed(\"patch\"));
let moss = smoothstep(0.35, 0.6, damp * (0.75 + 0.5 * patch) + (grain - 0.5) * 0.18);
output base_color = mix(vec3(0.52, 0.5, 0.47) * (0.9 + 0.2 * grain), vec3(0.1, 0.22, 0.05) * (0.7 + 0.6 * grain), moss);
output roughness = mix(0.75, 0.95, moss);
output metalness = 0;";

    #[test]
    #[ignore = "needs generated/washroom: node tests/studio/washroom/build.mjs"]
    fn washroom_moss_grows_where_the_planes_say() {
        let scene = washroom();
        let lighting = Lighting::default();
        let settings = OcclusionSettings::default();
        for (name, receiver, sizing) in &scene.receivers {
            let job = OcclusionJob {
                lighting: &lighting,
                settings: &settings,
                names: &[OCCLUSION, SUN_EXPOSURE],
                held: &|_| false,
            };
            let baked = occlusion_planes(
                &scene.occluders,
                std::slice::from_ref(receiver),
                sizing,
                &job,
                &|| false,
            )
            .unwrap();
            let mut request = Request {
                entry: "moss.ms".into(),
                source: MOSS.into(),
                modules: BTreeMap::new(),
                parameters: BTreeMap::new(),
                receivers: vec![receiver.clone()],
                ..sizing.clone()
            };
            let assets = bind_occlusion(&mut request, &baked);
            let lookup = |name: &str| {
                assets
                    .get(name)
                    .cloned()
                    .ok_or_else(|| format!("{name} is unavailable"))
            };
            let mut session = Session::default();
            let start = Instant::now();
            let output = session
                .build(&request, 1, &lookup, &|| false)
                .unwrap_or_else(|e| panic!("{e:?}"));
            let time = start.elapsed().as_secs_f64() * 1e3;
            let material = output.model.materials.values().next().unwrap();
            let bytes = &output.model.images[material.texture.as_ref().unwrap()].bytes;
            let path = out().join(format!("{name}-moss.png"));
            let write = |bytes: &[u8], path: &std::path::Path| {
                let image = superspec_geometry::image::decode(bytes).unwrap();
                enlarged(image.width, image.height, &image.rgba, path);
            };
            write(bytes, &path);
            println!(
                "{name}: moss baked in {time:.0} ms; wrote {}",
                path.display()
            );
            // Without the planes the program reads its fallbacks: open, bare concrete.
            let bare = Request {
                planes: BTreeMap::new(),
                ..request.clone()
            };
            let plain = Session::default()
                .build(&bare, 1, &lookup, &|| false)
                .unwrap_or_else(|e| panic!("{e:?}"));
            let plain_material = plain.model.materials.values().next().unwrap();
            let plain_bytes = &plain.model.images[plain_material.texture.as_ref().unwrap()].bytes;
            write(plain_bytes, &out().join(format!("{name}-moss-unbound.png")));
            let green = |bytes: &[u8]| {
                let image = superspec_geometry::image::decode(bytes).unwrap();
                image
                    .rgba
                    .chunks_exact(4)
                    .filter(|p| p[3] > 0 && p[1] > p[0].saturating_add(12))
                    .count()
            };
            let (mossy, none) = (green(bytes), green(plain_bytes));
            println!("{name}: {mossy} green texels with the planes, {none} without");
            assert!(mossy > 200 && none == 0);
        }
    }
}

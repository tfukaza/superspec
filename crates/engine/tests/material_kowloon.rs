//! The ported Kowloon concrete and rusted-steel recipes bake on real receivers through the
//! authoring session, with the studies' characteristic structure and repeatable output.
//! Expectations are properties of the studies (darker joints, chipped and intact finishes, bare
//! and oxidized metal), not frozen pixels.
use std::{collections::BTreeMap, path::PathBuf, sync::Arc, time::Instant};
use superspec_engine::authoring::{Output, Profile, Receiver, Request, Session};

fn examples() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/materialspec/program/examples")
}

fn read(name: &str) -> String {
    std::fs::read_to_string(examples().join(name)).unwrap()
}

/// Companion images beside the recipes, and catalog images from the shipped materials folder.
fn asset(name: &str) -> Result<Arc<[u8]>, String> {
    let path = match name.strip_prefix("superspec-material:") {
        Some(catalog) => PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../assets/materials")
            .join(catalog),
        None => examples().join(name),
    };
    std::fs::read(&path)
        .map(Arc::from)
        .map_err(|e| format!("{name}: {e}"))
}

/// An axis-aligned box from `min` to `max` with flat-shaded outward faces, split per face.
fn cuboid(id: &str, min: [f64; 3], max: [f64; 3]) -> Receiver {
    let corner = |x: usize, y: usize, z: usize| {
        [
            [min[0], max[0]][x],
            [min[1], max[1]][y],
            [min[2], max[2]][z],
        ]
    };
    // Each face as four corners counterclockwise seen from outside, with its normal.
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
        id: id.into(),
        object_id: id.into(),
        positions,
        normals,
        uvs,
        indices,
        object_to_world: None,
        contacts: Vec::new(),
    }
}

fn bake(entry: &str, receiver: Receiver, parameters: serde_json::Value) -> (Output, f64) {
    bake_with(entry, receiver, parameters, Profile::Preview, 128)
}

fn bake_with(
    entry: &str,
    receiver: Receiver,
    parameters: serde_json::Value,
    profile: Profile,
    resolution: usize,
) -> (Output, f64) {
    let modules: BTreeMap<String, String> = [
        "kowloon-noise.material.ts",
        "kowloon-concrete.material.ts",
        "kowloon-rusted-steel.material.ts",
    ]
    .into_iter()
    .map(|name| (name.to_owned(), read(name)))
    .collect();
    let request = Request {
        kind: "material".into(),
        entry: entry.into(),
        source: modules[entry].clone(),
        modules,
        parameters: serde_json::from_value(parameters).unwrap(),
        seed: 7,
        resolution,
        profile,
        receivers: vec![receiver],
        ..Request::default()
    };
    let mut session = Session::default();
    let started = Instant::now();
    let output = session
        .build(&request, 1, &asset, &|| false)
        .unwrap_or_else(|e| panic!("{e:?}"));
    (output, started.elapsed().as_secs_f64())
}

/// Decoded RGBA texels of the material's base color, normal and properties maps.
fn maps(output: &Output) -> [Vec<[u8; 4]>; 3] {
    let material = output
        .model
        .materials
        .values()
        .next()
        .expect("one baked material");
    let image = |id: &Option<String>| {
        let bytes = &output.model.images[id.as_ref().expect("map present")].bytes;
        let decoded = superspec_geometry::image::decode(bytes).unwrap();
        decoded
            .rgba
            .chunks_exact(4)
            .filter(|t| t[3] > 0)
            .map(|t| [t[0], t[1], t[2], t[3]])
            .collect::<Vec<_>>()
    };
    [
        image(&material.texture),
        image(&material.normal_texture),
        image(&material.metallic_roughness_texture),
    ]
}

/// Separates image decoding from evaluation: `cargo test --release ... -- --ignored --nocapture`.
#[test]
#[ignore]
fn bench_decode_and_evaluation() {
    use superspec_engine::materialspec::program::{
        self, BakeRequest, Domain, Image, Inputs, Sources,
    };
    let mut images = BTreeMap::new();
    let started = Instant::now();
    for name in [
        "textures/concrete-plain.png",
        "textures/concrete-fine-sand.png",
        "superspec-material:building-plaster-plain.png",
    ] {
        let decoded = superspec_geometry::image::decode(&asset(name).unwrap()).unwrap();
        images.insert(
            name.to_owned(),
            Image {
                width: decoded.width,
                height: decoded.height,
                rgba: decoded.rgba,
            }
            .into(),
        );
    }
    eprintln!(
        "decode three images: {:.3}s",
        started.elapsed().as_secs_f64()
    );
    let modules: BTreeMap<String, String> =
        ["kowloon-noise.material.ts", "kowloon-concrete.material.ts"]
            .into_iter()
            .map(|name| (name.to_owned(), read(name)))
            .collect();
    let compiled = program::compile(&Sources {
        entry: "kowloon-concrete.material.ts".into(),
        modules,
    })
    .unwrap();
    let inputs = Inputs {
        images,
        ..Inputs::default()
    };
    // The smallest operation budget one sample needs is its step count.
    let steps = |height_only: bool| {
        let (mut low, mut high) = (1usize, 200_000usize);
        while low < high {
            let middle = (low + high) / 2;
            let mut evaluator = compiled.evaluator_with_budget(&inputs, middle).unwrap();
            let sample = superspec_engine::materialspec::program::Sample {
                world: [0.3, 0.2, 0.],
                ..Default::default()
            };
            let fits = if height_only {
                evaluator.evaluate_height(&sample).is_ok()
            } else {
                evaluator.evaluate_channels(&sample, None).is_ok()
            };
            if fits {
                high = middle
            } else {
                low = middle + 1
            }
        }
        low
    };
    eprintln!(
        "steps per evaluation: channels {}, height {}",
        steps(false),
        steps(true)
    );
    let mut evaluator = compiled.evaluator(&inputs).unwrap();
    for height_only in [false, true] {
        let started = Instant::now();
        for i in 0..4000 {
            evaluator.replenish(64_000_000);
            let sample = superspec_engine::materialspec::program::Sample {
                world: [(i % 64) as f64 * 0.013, (i / 64) as f64 * 0.017, 0.],
                footprint: 0.004,
                ..Default::default()
            };
            if height_only {
                evaluator.evaluate_height(&sample).unwrap();
            } else {
                evaluator.evaluate_channels(&sample, None).unwrap();
            }
        }
        let each = started.elapsed().as_secs_f64() / 4000. * 1e6;
        eprintln!(
            "steps per evaluation: {} takes {each:.1} us",
            if height_only { "height" } else { "channels" }
        );
    }
    for size in [32usize, 64, 128] {
        let started = Instant::now();
        compiled
            .bake(
                &BakeRequest {
                    width: size,
                    height: size,
                    domain: Domain::default(),
                    inspection: None,
                },
                &inputs,
            )
            .unwrap();
        let seconds = started.elapsed().as_secs_f64();
        eprintln!(
            "concrete swatch {size}x{size}: {seconds:.3}s, {:.1} us per texel",
            seconds * 1e6 / (size * size) as f64
        );
    }
}

fn luminance(t: &[u8; 4]) -> f64 {
    let linear = |b: u8| superspec_types::color::LINEAR[b as usize];
    0.2126 * linear(t[0]) + 0.7152 * linear(t[1]) + 0.0722 * linear(t[2])
}

#[test]
fn concrete_bakes_varied_joints_chips_and_creases_on_a_wall() {
    // A 1.2 m wide, 1.1 m tall, 22 cm thick wall: a construction joint 1 m from its center line,
    // form ties on the 1 m grid, and texels of about 4 cm at this resolution.
    let wall = cuboid("wall", [-0.6, -0.11, 0.], [0.6, 0.11, 1.1]);
    let (output, seconds) = bake(
        "kowloon-concrete.material.ts",
        wall.clone(),
        serde_json::json!({}),
    );
    eprintln!("concrete wall bake: {seconds:.2}s");
    let [color, normal, properties] = maps(&output);
    let mean = color.iter().map(luminance).sum::<f64>() / color.len() as f64;
    // The study's sources average about 0.36 linear before darkening, grime and joints.
    assert!((0.12..0.45).contains(&mean), "mean luminance {mean}");
    assert!(
        color.iter().any(|t| luminance(t) < mean * 0.6),
        "joints, grime and edges are darker"
    );
    // Intact concrete is the study's 0.30 roughness; chips are 0.92.
    let roughness: Vec<u8> = properties.iter().map(|t| t[1]).collect();
    assert!(roughness.iter().any(|r| *r <= 90), "intact finish present");
    // At about 4 cm per texel the study's 22 % chips are filtered to partial coverage.
    assert!(
        roughness.iter().any(|r| *r >= 110),
        "chips raise roughness above the intact finish"
    );
    assert!(
        properties.iter().all(|t| t[2] == 0),
        "concrete is not metallic"
    );
    // Relief and the crease round-over bend the normal map away from flat.
    assert!(
        normal
            .iter()
            .any(|t| (t[0] as i32 - 128).abs() > 30 || (t[1] as i32 - 128).abs() > 30)
    );
    // Seeds make the bake repeatable.
    let (again, _) = bake("kowloon-concrete.material.ts", wall, serde_json::json!({}));
    assert_eq!(maps(&again), maps(&output));
}

#[test]
fn concrete_toggles_and_the_ground_slab_change_only_what_they_name() {
    let slab = cuboid("slab", [-0.5, -0.5, 0.], [0.5, 0.5, 0.12]);
    let (plain, _) = bake(
        "kowloon-concrete.material.ts",
        slab.clone(),
        serde_json::json!({ "ground": true, "chipAmount": 0, "creaseWear": false }),
    );
    // Frequent chips reach full coverage and the study's 0.92 chipped roughness.
    let (chipped, _) = bake(
        "kowloon-concrete.material.ts",
        slab,
        serde_json::json!({ "ground": true, "chipAmount": 0.7 }),
    );
    let [plain_color, _, plain_properties] = maps(&plain);
    let [_, _, chipped_properties] = maps(&chipped);
    // Without chips every texel keeps the intact roughness.
    assert!(
        plain_properties
            .iter()
            .all(|t| (t[1] as i32 - 77).abs() <= 1),
        "intact 0.30 roughness everywhere"
    );
    assert!(chipped_properties.iter().any(|t| t[1] >= 220));
    // A slab carries no joints, cutouts or grime: no texel is darkened toward a seam.
    let mean = plain_color.iter().map(luminance).sum::<f64>() / plain_color.len() as f64;
    assert!(
        plain_color.iter().all(|t| luminance(t) > mean * 0.4),
        "no joint darkening on the slab"
    );
}

#[test]
fn concrete_top_runoff_leaves_slab_caps_and_soffits_dry() {
    let mut slab = cuboid("slab", [-0.8, -0.8, 0.], [0.8, 0.8, 0.18]);
    // The first two quads are the upper and lower faces. Keep wall mode enabled
    // so only the receiver's slope can turn off top runoff.
    slab.positions.truncate(24);
    slab.normals.truncate(24);
    slab.uvs.truncate(16);
    slab.indices.truncate(12);
    let recipe = "kowloon-concrete.material.ts";
    let (with_runoff, _) = bake(recipe, slab.clone(), serde_json::json!({ "grime": true }));
    let (without_runoff, _) = bake(recipe, slab, serde_json::json!({ "grime": false }));
    assert_eq!(maps(&with_runoff)[0], maps(&without_runoff)[0]);
}

#[test]
fn rusted_steel_bakes_bare_metal_and_oxide_with_rusted_edges() {
    let block = cuboid("block", [0., 0., 0.], [0.4, 0.3, 0.25]);
    let (output, seconds) = bake(
        "kowloon-rusted-steel.material.ts",
        block.clone(),
        serde_json::json!({}),
    );
    eprintln!("rusted steel bake: {seconds:.2}s");
    let [color, _, properties] = maps(&output);
    // Bare steel keeps metalness 0.92; full oxide has none.
    let metal: Vec<u8> = properties.iter().map(|t| t[2]).collect();
    // Coarse texels average the millimeter raindrops (6.5 % coverage), so bare steel reads just below 0.92.
    assert!(metal.iter().any(|m| *m >= 200), "bare steel present");
    assert!(metal.iter().any(|m| *m <= 25), "rusted steel present");
    // Oxide is brown (red over blue); steel is blue gray (blue at least red).
    assert!(color.iter().any(|t| t[0] as i32 > t[2] as i32 + 20));
    assert!(color.iter().any(|t| t[2] >= t[0]));
    let rusted = metal.iter().filter(|m| **m <= 25).count() as f64 / metal.len() as f64;
    // Turning every pattern off leaves only bare steel.
    let (clean, _) = bake(
        "kowloon-rusted-steel.material.ts",
        block,
        serde_json::json!({ "edgeRust": 0, "surfaceRust": 0, "rainRust": 0 }),
    );
    let [_, _, clean_properties] = maps(&clean);
    assert!(
        clean_properties.iter().all(|t| t[2] >= 230),
        "no oxide without rust patterns"
    );
    assert!(
        rusted > 0.01 && rusted < 0.9,
        "partial rust coverage {rusted}"
    );
    // The baked material carries the live operations: arguments that never vary become
    // constants, and per-sample ones become texture channels decoding to their value range.
    let material = output.model.materials.values().next().unwrap();
    let ids: Vec<_> = material.live.iter().map(|op| op.id.as_str()).collect();
    assert_eq!(
        ids,
        [
            "modulateDiffuse",
            "grazingFalloff",
            "scaleSpecular",
            "filterEnvironment",
            "bandLuminance",
            "edgeGlint"
        ]
    );
    use superspec_types::material::LiveArgument;
    let argument = |op: usize, name: &str| material.live[op].arguments[name].clone();
    assert_eq!(argument(1, "from"), LiveArgument::Constant { value: 0.3 });
    assert_eq!(
        argument(4, "spacing"),
        LiveArgument::Constant { value: 1. / 5.5 }
    );
    assert_eq!(argument(1, "to"), LiveArgument::Constant { value: 0.06 });
    assert!(
        matches!(argument(2, "factor"), LiveArgument::Texture { .. }),
        "rims vary"
    );
    let LiveArgument::Texture {
        texture,
        scale,
        offset,
        ..
    } = argument(0, "factor")
    else {
        panic!("diffuse grain varies over the surface");
    };
    assert!(output.model.images.contains_key(&texture));
    // The grain factor is 1 + (noise - 0.5) * 0.04: within [0.98, 1.02].
    assert!(
        offset >= 0.98 - 1e-9 && offset + scale <= 1.02 + 1e-9,
        "{offset} {scale}"
    );
    // Live lighting travels with the material as metadata.
    let live = output.metadata["live"]
        .as_array()
        .expect("live operations recorded");
    let names: Vec<_> = live
        .iter()
        .map(|op| op["id"].as_str().unwrap_or_default())
        .collect();
    assert_eq!(
        names,
        [
            "modulateDiffuse",
            "grazingFalloff",
            "scaleSpecular",
            "filterEnvironment",
            "bandLuminance",
            "edgeGlint"
        ],
        "{live:?}"
    );
}

#[test]
fn production_bakes_rows_on_threads_with_the_preview_bytes() {
    // Rusted steel exercises features, live texture channels and the normal offset together.
    let block = cuboid("block", [0., 0., 0.], [0.4, 0.3, 0.25]);
    let entry = "kowloon-rusted-steel.material.ts";
    let (preview, serial) = bake_with(
        entry,
        block.clone(),
        serde_json::json!({}),
        Profile::Preview,
        96,
    );
    let (production, threaded) =
        bake_with(entry, block, serde_json::json!({}), Profile::Production, 96);
    eprintln!("rusted steel at 96: preview {serial:.2}s, production {threaded:.2}s");
    // Production compresses its PNGs harder, so the files (and their content-hash identities)
    // differ, but every map and live channel decodes to the same pixels.
    let decoded = |output: &Output, id: &str| {
        superspec_geometry::image::decode(&output.model.images[id].bytes)
            .unwrap()
            .rgba
    };
    let maps = |output: &Output| {
        let material = output.model.materials.values().next().unwrap();
        [
            &material.texture,
            &material.normal_texture,
            &material.metallic_roughness_texture,
        ]
        .map(|id| decoded(output, id.as_ref().unwrap()))
    };
    assert_eq!(maps(&production), maps(&preview));
    use superspec_types::material::LiveArgument;
    let live = |output: &Output| {
        let material = output.model.materials.values().next().unwrap();
        material
            .live
            .iter()
            .flat_map(|operation| {
                operation.arguments.iter().map(|(name, argument)| {
                    let value = match argument {
                        LiveArgument::Constant { value } => format!("{value}"),
                        LiveArgument::Texture {
                            texture,
                            channel,
                            scale,
                            offset,
                        } => format!(
                            "{channel} {scale} {offset} {:x}",
                            decoded(output, texture)
                                .iter()
                                .fold(0u64, |h, b| h.wrapping_mul(31).wrapping_add(*b as u64))
                        ),
                    };
                    (operation.id.clone(), name.clone(), value)
                })
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(live(&production), live(&preview));
    let size = |output: &Output| {
        output
            .model
            .images
            .values()
            .map(|image| image.bytes.len())
            .sum::<usize>()
    };
    assert!(
        size(&production) < size(&preview),
        "production {} bytes, preview {}",
        size(&production),
        size(&preview)
    );
}

#[test]
fn only_the_production_profile_admits_large_textures() {
    let request = |profile, resolution| Request {
        entry: "flat.ms".into(),
        source: "material 1;\noutput base_color = vec3(0.5, 0.4, 0.3);\n".into(),
        resolution,
        profile,
        receivers: vec![cuboid("plate", [0., 0., 0.], [1., 1., 0.01])],
        ..Request::default()
    };
    let build = |profile, resolution| {
        Session::default().build(&request(profile, resolution), 1, &asset, &|| false)
    };
    let error = build(Profile::Preview, 1024)
        .err()
        .expect("preview stops at 512");
    assert!(format!("{error:?}").contains("16–512"), "{error:?}");
    build(Profile::Production, 1024).unwrap_or_else(|e| panic!("{e:?}"));
    let error = build(Profile::Production, 4096)
        .err()
        .expect("production stops at 2048");
    assert!(format!("{error:?}").contains("16–2048"), "{error:?}");
}

/// A host that distributes a bake across several sessions gets the bytes of an undistributed one.
#[test]
fn rows_baked_by_other_sessions_give_the_same_material() {
    for (entry, receiver) in [
        (
            "kowloon-concrete.material.ts",
            cuboid("wall", [-0.6, -0.11, 0.], [0.6, 0.11, 1.1]),
        ),
        (
            "kowloon-rusted-steel.material.ts",
            cuboid("block", [0., 0., 0.], [0.4, 0.3, 0.25]),
        ),
    ] {
        let (whole, _) = bake(entry, receiver.clone(), serde_json::json!({}));
        let modules: BTreeMap<String, String> = [
            "kowloon-noise.material.ts",
            "kowloon-concrete.material.ts",
            "kowloon-rusted-steel.material.ts",
        ]
        .into_iter()
        .map(|name| (name.to_owned(), read(name)))
        .collect();
        let request = Request {
            kind: "material".into(),
            entry: entry.into(),
            source: modules[entry].clone(),
            modules,
            seed: 7,
            resolution: 128,
            receivers: vec![receiver],
            ..Request::default()
        };
        let mut session = Session::default();
        session.distribute_bakes(true);
        let stopped = session
            .build(&request, 1, &asset, &|| false)
            .err()
            .expect("a large uncached bake is reported, not evaluated");
        assert_eq!(stopped[0].code, "BAKE_PENDING");
        let pending = session.take_pending_bake().expect("the pending bake");
        assert_eq!(pending.keys.len(), 1);
        // The description survives the JSON trip between workers.
        let pending: superspec_engine::authoring::PendingBake =
            serde_json::from_str(&serde_json::to_string(&pending).unwrap()).unwrap();
        for index in 0..3 {
            let share = Session::default()
                .bake_share(&pending, index, 3, &asset, &|| false)
                .unwrap_or_else(|e| panic!("{e:?}"));
            session.provide_bake_share(&share).unwrap();
        }
        let distributed = session
            .build(&request, 1, &asset, &|| false)
            .unwrap_or_else(|e| panic!("{e:?}"));
        assert!(session.take_pending_bake().is_none());
        assert_eq!(
            distributed.model.materials.keys().collect::<Vec<_>>(),
            whole.model.materials.keys().collect::<Vec<_>>(),
            "{entry}: material identities hash every map and live argument"
        );
        // The finished bake is cached: building again neither stops nor needs rows.
        session
            .build(&request, 2, &asset, &|| false)
            .unwrap_or_else(|e| panic!("{e:?}"));
        // An incomplete set of rows is reported again rather than baked with holes.
        let mut partial = Session::default();
        partial.distribute_bakes(true);
        assert!(partial.build(&request, 1, &asset, &|| false).is_err());
        let pending = partial.take_pending_bake().unwrap();
        let share = Session::default()
            .bake_share(&pending, 0, 3, &asset, &|| false)
            .unwrap_or_else(|e| panic!("{e:?}"));
        partial.provide_bake_share(&share).unwrap();
        assert!(partial.build(&request, 1, &asset, &|| false).is_err());
        assert!(partial.take_pending_bake().is_some());
        // Turning distribution off bakes here.
        partial.distribute_bakes(false);
        let local = partial
            .build(&request, 1, &asset, &|| false)
            .unwrap_or_else(|e| panic!("{e:?}"));
        assert_eq!(
            local.model.materials.keys().collect::<Vec<_>>(),
            whole.model.materials.keys().collect::<Vec<_>>()
        );
    }
}

/// Serial bake time and an output fingerprint for both recipes, for checking that an evaluator
/// optimization changes speed but no byte: `cargo test --release ... bench_fingerprint -- --ignored
/// --nocapture`. Material identities hash every encoded map and live argument.
#[test]
#[ignore]
fn bench_fingerprint() {
    let resolution = std::env::var("BENCH_RESOLUTION")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(256);
    for (entry, receiver) in [
        (
            "kowloon-concrete.material.ts",
            cuboid("wall", [-0.6, -0.11, 0.], [0.6, 0.11, 1.1]),
        ),
        (
            "kowloon-rusted-steel.material.ts",
            cuboid("block", [0., 0., 0.], [0.4, 0.3, 0.25]),
        ),
    ] {
        // BENCH_PARAMETERS (JSON) overrides parameters, to time one part of a recipe.
        let parameters = std::env::var("BENCH_PARAMETERS")
            .ok()
            .filter(|_| entry.contains("concrete"))
            .map_or(serde_json::json!({}), |v| serde_json::from_str(&v).unwrap());
        let (output, seconds) =
            bake_with(entry, receiver, parameters, Profile::Preview, resolution);
        let identity: Vec<&String> = output.model.materials.keys().collect();
        eprintln!("{entry} at {resolution}: {seconds:.2}s {identity:?}");
    }
}

/// Serial and threaded wall-clock for one larger bake: `cargo test --release ... bench_profiles -- --ignored --nocapture`.
#[test]
#[ignore]
fn bench_profiles() {
    let wall = cuboid("wall", [-0.6, -0.11, 0.], [0.6, 0.11, 1.1]);
    let entry = "kowloon-concrete.material.ts";
    let (_, serial) = bake_with(
        entry,
        wall.clone(),
        serde_json::json!({}),
        Profile::Preview,
        384,
    );
    let (_, threaded) = bake_with(entry, wall, serde_json::json!({}), Profile::Production, 384);
    eprintln!("concrete wall at 384: preview {serial:.2}s, production {threaded:.2}s");
}

//! Bake-speed measurement and output-identity matrix for the example material programs.
//!
//! `cargo test --release -p superspec-engine --test material_bake_profile -- --ignored --nocapture`
//! bakes the Kowloon concrete, rusted steel, a noise study and the tiled courtyard at several
//! resolutions: a cold build (compile, decode, layout, bake, encode) and parameter edits on the
//! same session (the work a slider drag repeats). Every line is a JSON object on stderr; with
//! `BENCH_OUT=<file>` the lines are also written there. `material_identity` hashes every encoded
//! map and live argument, so two runs of the matrix are byte-identical exactly when their identity
//! columns are equal.
//!
//! Environment: `BENCH_RESOLUTIONS` (comma list, default `64,256,512`), `BENCH_PROGRAMS`
//! (comma list of case names, default all), `BENCH_EDITS=0` to skip parameter edits.
use std::{collections::BTreeMap, io::Write, path::PathBuf, sync::Arc, time::Instant};
use superspec_engine::authoring::{Output, Profile, Receiver, Request, Session};
use superspec_engine::materialspec::program::Edge;

fn examples() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/materialspec/program/examples")
}

fn read(name: &str) -> String {
    std::fs::read_to_string(examples().join(name)).unwrap()
}

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

const NOISE_STUDY: &str = r#"
import { defineMaterial, param, vec2, vec3, add, sub, mul, mix, abs, surface, seed } from "@superspec/material/v1";
import { fractal, fracture, domainWarp, cellRandom, contour } from "./kowloon-noise.material";

const frequency = param.scalar({ default: 6, range: [1, 40], label: "Frequency" });
const jaggedness = param.scalar({ default: 0.8, range: [0, 2], label: "Jaggedness" });
const tint = param.vec3({ default: vec3(0.5, 0.45, 0.4), range: [0, 1], label: "Tint" });
const gloss = param.scalar({ default: 0.4, range: [0, 1], label: "Gloss" });
const relief = param.scalar({ default: 0.004, range: [0, 0.01], label: "Relief" });

const p = mul(surface, frequency);
const broad = fracture(p, jaggedness, seed("broad"));
const warped = domainWarp(add(p, vec2(5.1, 2.3)), seed("warp"));
const cells = cellRandom(add(vec2(0.5, 0.5), mul(p, 0.5)), seed("cells"));
const grain = fractal(mul(p, 9), seed("grain"));
const mask = contour(broad, 0.04);

export default defineMaterial({
  name: "Noise study",
  base: {
    baseColor: mix(mul(tint, add(0.6, mul(0.4, grain))), vec3(cells.x, cells.y, cells.z), mul(0.35, mask)),
    roughness: add(gloss, mul(0.4, abs(sub(warped.x, 0.1)))),
    metalness: 0,
    height: mul(add(mul(broad, 0.7), mul(grain, 0.3)), relief),
  },
});
"#;

struct Case {
    name: &'static str,
    entry: &'static str,
    modules: Vec<(&'static str, String)>,
    receiver: Receiver,
    domain: [f64; 2],
    edges: Vec<Edge>,
    /// Edits applied one at a time to the session after the cold build: (label, parameters).
    edits: Vec<(&'static str, serde_json::Value)>,
}

fn cases() -> Vec<Case> {
    let noise = read("kowloon-noise.material.ts");
    let swatch = |size: f64| Receiver {
        id: "swatch".into(),
        object_id: "swatch".into(),
        positions: vec![0., 0., 0., size, 0., 0., size, 0., size, 0., 0., size],
        normals: vec![0., -1., 0., 0., -1., 0., 0., -1., 0., 0., -1., 0.],
        uvs: vec![0., 0., 1., 0., 1., 1., 0., 1.],
        indices: vec![0, 1, 2, 0, 2, 3],
        object_to_world: None,
        contacts: Vec::new(),
    };
    vec![
        Case {
            name: "kowloon-concrete",
            entry: "kowloon-concrete.material.ts",
            modules: vec![
                ("kowloon-noise.material.ts", noise.clone()),
                (
                    "kowloon-concrete.material.ts",
                    read("kowloon-concrete.material.ts"),
                ),
            ],
            receiver: cuboid("wall", [-0.6, -0.11, 0.], [0.6, 0.11, 1.1]),
            domain: [1., 1.],
            edges: vec![],
            edits: vec![
                ("roughness", serde_json::json!({ "intactRoughness": 0.35 })),
                ("color", serde_json::json!({ "hueAmount": 0.07 })),
                ("height", serde_json::json!({ "chipRelief": 0.004 })),
                ("structure", serde_json::json!({ "chipScale": 0.3 })),
            ],
        },
        Case {
            name: "kowloon-rusted-steel",
            entry: "kowloon-rusted-steel.material.ts",
            modules: vec![
                ("kowloon-noise.material.ts", noise.clone()),
                (
                    "kowloon-rusted-steel.material.ts",
                    read("kowloon-rusted-steel.material.ts"),
                ),
            ],
            receiver: cuboid("block", [0., 0., 0.], [0.4, 0.3, 0.25]),
            domain: [1., 1.],
            edges: vec![],
            edits: vec![
                ("metalness", serde_json::json!({ "metalness": 0.8 })),
                (
                    "color",
                    serde_json::json!({ "substrate": [0.12, 0.13, 0.14] }),
                ),
                ("roughness", serde_json::json!({ "roughness": 0.3 })),
                ("structure", serde_json::json!({ "rustRepeat": 3 })),
            ],
        },
        Case {
            name: "noise-study",
            entry: "noise-study.material.ts",
            modules: vec![
                ("kowloon-noise.material.ts", noise),
                ("noise-study.material.ts", NOISE_STUDY.to_owned()),
            ],
            receiver: swatch(1.),
            domain: [1., 1.],
            edges: vec![],
            edits: vec![
                ("roughness", serde_json::json!({ "gloss": 0.5 })),
                ("color", serde_json::json!({ "tint": [0.6, 0.5, 0.4] })),
                ("height", serde_json::json!({ "relief": 0.006 })),
                ("structure", serde_json::json!({ "frequency": 8 })),
            ],
        },
        Case {
            name: "tiled-courtyard",
            entry: "tiled-courtyard.material.ts",
            modules: vec![(
                "tiled-courtyard.material.ts",
                read("tiled-courtyard.material.ts"),
            )],
            receiver: swatch(2.),
            domain: [2., 2.],
            edges: vec![
                Edge {
                    start: [0., 0., 2.],
                    end: [2., 0., 2.],
                },
                Edge {
                    start: [0.5, 0., 1.2],
                    end: [1.4, 0., 1.2],
                },
            ],
            edits: vec![
                ("roughness", serde_json::json!({ "glazeRoughness": 0.3 })),
                ("color", serde_json::json!({ "glaze": [0.7, 0.72, 0.7] })),
                ("structure", serde_json::json!({ "stagger": 0.3 })),
            ],
        },
    ]
}

fn request(case: &Case, parameters: serde_json::Value, resolution: usize) -> Request {
    let modules: BTreeMap<String, String> = case
        .modules
        .iter()
        .map(|(name, text)| ((*name).to_owned(), text.clone()))
        .collect();
    Request {
        kind: "material".into(),
        entry: case.entry.into(),
        source: modules[case.entry].clone(),
        modules,
        parameters: serde_json::from_value(parameters).unwrap(),
        seed: 7,
        resolution,
        profile: if resolution > 512 {
            Profile::Production
        } else {
            Profile::Preview
        },
        receivers: vec![case.receiver.clone()],
        domain: case.domain,
        edges: case.edges.clone(),
        ..Request::default()
    }
}

fn identity(output: &Output) -> String {
    output
        .model
        .materials
        .keys()
        .cloned()
        .collect::<Vec<_>>()
        .join(",")
}

fn texels(output: &Output) -> u64 {
    output.metadata["atlases"]
        .as_array()
        .map(|atlases| {
            atlases
                .iter()
                .map(|a| {
                    (a["width"].as_f64().unwrap_or(0.) * a["height"].as_f64().unwrap_or(0.)) as u64
                })
                .sum()
        })
        .unwrap_or(0)
}

#[test]
#[ignore]
fn bench_bake_profile() {
    let resolutions: Vec<usize> = std::env::var("BENCH_RESOLUTIONS")
        .unwrap_or_else(|_| "64,256,512".into())
        .split(',')
        .filter_map(|v| v.trim().parse().ok())
        .collect();
    let wanted: Option<Vec<String>> = std::env::var("BENCH_PROGRAMS")
        .ok()
        .map(|v| v.split(',').map(|s| s.trim().to_owned()).collect());
    let edits = std::env::var("BENCH_EDITS").map_or(true, |v| v != "0");
    let mut out = std::env::var("BENCH_OUT")
        .ok()
        .map(|path| std::fs::File::create(path).unwrap());
    let mut emit = |line: serde_json::Value| {
        let text = line.to_string();
        eprintln!("{text}");
        if let Some(file) = out.as_mut() {
            writeln!(file, "{text}").unwrap();
        }
    };
    for case in cases() {
        if wanted
            .as_ref()
            .is_some_and(|w| !w.iter().any(|n| n == case.name))
        {
            continue;
        }
        for &resolution in &resolutions {
            let mut session = Session::default();
            let mut revision = 1;
            let mut run = |session: &mut Session, label: &str, parameters: serde_json::Value| {
                let request = request(&case, parameters.clone(), resolution);
                revision += 1;
                let started = Instant::now();
                let output = session
                    .build(&request, revision, &asset, &|| false)
                    .unwrap_or_else(|e| panic!("{} {label}: {e:?}", case.name));
                let seconds = started.elapsed().as_secs_f64();
                (output, seconds)
            };
            let (output, seconds) = run(&mut session, "cold", serde_json::json!({}));
            let atlas_texels = texels(&output);
            emit(serde_json::json!({
                "case": case.name, "resolution": resolution, "step": "cold",
                "seconds": seconds, "atlas_texels": atlas_texels,
                "ns_per_texel": seconds * 1e9 / atlas_texels.max(1) as f64,
                "material_identity": identity(&output),
            }));
            if edits {
                for (label, parameters) in case.edits.clone() {
                    // Edits apply to the base parameters, one at a time.
                    let (output, seconds) = run(&mut session, label, parameters);
                    emit(serde_json::json!({
                        "case": case.name, "resolution": resolution, "step": format!("edit:{label}"),
                        "seconds": seconds, "atlas_texels": atlas_texels,
                        "ns_per_texel": seconds * 1e9 / atlas_texels.max(1) as f64,
                        "material_identity": identity(&output),
                    }));
                }
            }
        }
    }
}

/// Writes the requests of a native-versus-Wasm check and the packet hashes a native session builds
/// for them: `PARITY_OUT=<file> cargo test ... write_parity_fixture -- --ignored`. A Wasm session
/// that builds the same requests in the same order must produce the same packets
/// (`tests/material-bake-parity.mjs`), which covers row evaluation, per-map retention and the
/// inspection preview on the Wasm target.
#[test]
#[ignore]
fn write_parity_fixture() {
    use sha2::{Digest, Sha256};
    let path = std::env::var("PARITY_OUT").expect("PARITY_OUT names the fixture file");
    let mut sequences = Vec::new();
    let mut assets = serde_json::Map::new();
    let resolution = 80;
    for case in cases() {
        let mut requests = vec![("base", request(&case, serde_json::json!({}), resolution))];
        for (label, parameters) in &case.edits {
            requests.push((label, request(&case, parameters.clone(), resolution)));
        }
        let mut inspected = request(&case, serde_json::json!({}), 48);
        inspected.inspection = Some("height".into());
        requests.push(("inspection", inspected));
        // The base again, from retained maps, then another seed.
        requests.push((
            "base again",
            request(&case, serde_json::json!({}), resolution),
        ));
        let mut reseeded = request(&case, serde_json::json!({}), resolution);
        reseeded.seed = 11;
        requests.push(("seed", reseeded));
        // The production encoder too: its bytes must be the same on every target.
        let mut production = request(&case, serde_json::json!({}), 96);
        production.profile = Profile::Production;
        requests.push(("production", production));
        let mut session = Session::default();
        let mut hashes = Vec::new();
        for (i, (label, request)) in requests.iter().enumerate() {
            let revision = i as u64 + 1;
            let output = session
                .build(request, revision, &asset, &|| false)
                .unwrap_or_else(|e| panic!("{} {label}: {e:?}", case.name));
            let packet = output.scene(revision as u32).unwrap().packet();
            hashes.push(format!("{:x}", Sha256::digest(&packet)));
            for name in session.library_assets(request) {
                assets.insert(
                    name.clone(),
                    serde_json::json!(format!(
                        "assets/materials/{}",
                        &name["superspec-material:".len()..]
                    )),
                );
            }
        }
        for image in [
            "textures/concrete-plain.png",
            "textures/concrete-fine-sand.png",
        ] {
            assets.insert(
                image.into(),
                serde_json::json!(format!(
                    "crates/engine/src/materialspec/program/examples/{image}"
                )),
            );
        }
        sequences.push(serde_json::json!({
            "name": case.name,
            "requests": requests.iter().map(|(_, r)| serde_json::to_string(r).unwrap()).collect::<Vec<_>>(),
            "labels": requests.iter().map(|(l, _)| l).collect::<Vec<_>>(),
            "packetSha256": hashes,
        }));
    }
    std::fs::write(
        path,
        serde_json::to_string_pretty(
            &serde_json::json!({ "sequences": sequences, "assets": assets }),
        )
        .unwrap(),
    )
    .unwrap();
}

/// How much of a production bake the map encoding is: each map of a 2048 px bake is decoded and
/// encoded again alone (`encode_small`, as production does), serially and all at once.
#[test]
#[ignore]
fn bench_encode_share() {
    for case in cases() {
        let started = Instant::now();
        let output = Session::default()
            .build(
                &request(&case, serde_json::json!({}), 2048),
                1,
                &asset,
                &|| false,
            )
            .unwrap_or_else(|e| panic!("{e:?}"));
        let wall = started.elapsed().as_secs_f64();
        let maps: Vec<_> = output
            .model
            .images
            .values()
            .map(|image| superspec_geometry::image::decode(&image.bytes).unwrap())
            .collect();
        let encode = |image: &superspec_geometry::image::Image| {
            let started = Instant::now();
            superspec_geometry::image::encode_small_cancellable(
                &superspec_geometry::image::Image {
                    width: image.width,
                    height: image.height,
                    rgba: image.rgba.clone(),
                    mime: "image/png",
                },
                &|| false,
            )
            .unwrap();
            started.elapsed().as_secs_f64()
        };
        let each: Vec<f64> = maps.iter().map(encode).collect();
        let together = Instant::now();
        std::thread::scope(|scope| {
            let handles: Vec<_> = maps
                .iter()
                .map(|m| scope.spawn(move || encode(m)))
                .collect();
            for handle in handles {
                handle.join().unwrap();
            }
        });
        eprintln!(
            "{}: build {wall:.2}s; encode serial {:.2}s (maps {:?}), all at once {:.2}s",
            case.name,
            each.iter().sum::<f64>(),
            each.iter()
                .map(|s| (s * 100.).round() / 100.)
                .collect::<Vec<_>>(),
            together.elapsed().as_secs_f64()
        );
    }
}

/// The decoded pixels and encoded sizes of every map of a production bake of each example at
/// 256, 512 and 2048 px, one JSON line per build (`MAPS_OUT=<file>` also writes them there). Two
/// encoders that keep pixels give equal `pixels` lists; only `bytes` and `seconds` may differ.
#[test]
#[ignore]
fn dump_production_maps() {
    use sha2::{Digest, Sha256};
    let mut out = std::env::var("MAPS_OUT")
        .ok()
        .map(|path| std::fs::File::create(path).unwrap());
    for case in cases() {
        for resolution in [256, 512, 2048] {
            let mut production = request(&case, serde_json::json!({}), resolution);
            production.profile = Profile::Production;
            let started = Instant::now();
            let output = Session::default()
                .build(&production, 1, &asset, &|| false)
                .unwrap_or_else(|e| panic!("{e:?}"));
            let seconds = started.elapsed().as_secs_f64();
            let mut pixels: Vec<String> = output
                .model
                .images
                .values()
                .map(|image| {
                    let decoded = superspec_geometry::image::decode(&image.bytes).unwrap();
                    format!(
                        "{}x{}:{:x}",
                        decoded.width,
                        decoded.height,
                        Sha256::digest(&decoded.rgba)
                    )
                })
                .collect();
            pixels.sort();
            let line = serde_json::json!({
                "case": case.name, "resolution": resolution, "seconds": seconds,
                "bytes": output.model.images.values().map(|image| image.bytes.len()).sum::<usize>(),
                "maps": pixels.len(), "pixels": pixels, "material_identity": identity(&output),
            })
            .to_string();
            eprintln!("{line}");
            if let Some(file) = out.as_mut() {
                writeln!(file, "{line}").unwrap();
            }
        }
    }
}

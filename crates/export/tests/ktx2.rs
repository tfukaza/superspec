//! KTX2 texture export: KHR_texture_basisu textures with a codec per role, decoding close to
//! their sources, deterministic, and readable again by the GLB importer.
use serde_json::Value;
use std::sync::Arc;
use superspec_engine::authoring::{Request, Session};
use superspec_engine::compile_with_assets;
use superspec_export::{Options, Textures, glb, instance_glb};
use superspec_geometry::image::{self, Image, encode};
use superspec_geometry::ktx2;

/// A smooth 64 x 64 pattern per map, distinct per name.
fn pattern(seed: u8) -> Vec<u8> {
    (0..64 * 64)
        .flat_map(|i| {
            let (x, y) = ((i % 64) as u8, (i / 64) as u8);
            [
                x.wrapping_mul(3).wrapping_add(seed),
                y.wrapping_mul(2),
                128 + seed / 2,
                255,
            ]
        })
        .collect()
}

fn source(name: &str) -> Result<Vec<u8>, String> {
    let seed = match name {
        "color.png" => 10,
        "normal.png" => 40,
        "data.png" => 90,
        _ => return Err(format!("unexpected asset {name}")),
    };
    encode(&Image {
        width: 64,
        height: 64,
        rgba: pattern(seed),
        mime: "image/png",
    })
}

fn scene() -> superspec_engine::Scene {
    compile_with_assets(
        r##"<architecture version="1" id="ktx2">
        <material id="finish" color="#808080" texture="color.png" normal-texture="normal.png" metallic-roughness-texture="data.png" emissive="#204080" emissive-strength="3"/>
        <box id="solid" width="2" depth="3" height="4" material="finish"/>
        </architecture>"##,
        1,
        &|path| source(path),
    )
    .unwrap()
}

fn parse(bytes: &[u8]) -> (Value, &[u8]) {
    let number = |at: usize| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap()) as usize;
    let length = number(12);
    let json = serde_json::from_slice(&bytes[20..20 + length]).unwrap();
    (json, &bytes[28 + length..])
}

/// The bytes of the image a material texture slot uses.
fn image_bytes<'a>(doc: &Value, binary: &'a [u8], slot: &Value) -> (&'a [u8], String, Value) {
    let texture = &doc["textures"][slot["index"].as_u64().unwrap() as usize];
    let source = texture["extensions"]["KHR_texture_basisu"]["source"]
        .as_u64()
        .or(texture["source"].as_u64())
        .unwrap() as usize;
    let image = &doc["images"][source];
    let view = &doc["bufferViews"][image["bufferView"].as_u64().unwrap() as usize];
    let start = view["byteOffset"].as_u64().unwrap() as usize;
    let length = view["byteLength"].as_u64().unwrap() as usize;
    (
        &binary[start..start + length],
        image["name"].as_str().unwrap().to_owned(),
        texture.clone(),
    )
}

fn supercompression(file: &[u8]) -> u32 {
    u32::from_le_bytes(file[44..48].try_into().unwrap())
}

#[test]
fn ktx2_export_compresses_each_texture_by_role() {
    let scene = scene();
    let options = Options {
        textures: Textures::Ktx2,
        ..Options::default()
    };
    let bytes = glb(&scene, &options).unwrap();
    let (doc, binary) = parse(&bytes);
    assert_eq!(
        doc["extensionsRequired"],
        serde_json::json!(["KHR_texture_basisu"])
    );
    let used: Vec<&str> = doc["extensionsUsed"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert!(
        used.contains(&"KHR_texture_basisu") && used.contains(&"KHR_materials_emissive_strength")
    );
    let finish = &doc["materials"][0];
    // Color is ETC1S (BasisLZ, scheme 1); normals and metallic-roughness are UASTC + Zstandard (2).
    for (slot, name, scheme) in [
        (
            &finish["pbrMetallicRoughness"]["baseColorTexture"],
            "color.png",
            1,
        ),
        (&finish["normalTexture"], "normal.png", 2),
        (
            &finish["pbrMetallicRoughness"]["metallicRoughnessTexture"],
            "data.png",
            2,
        ),
    ] {
        let (file, image_name, texture) = image_bytes(&doc, binary, slot);
        assert_eq!(image_name, name);
        assert!(
            texture.get("source").is_none(),
            "no PNG fallback for {name}"
        );
        assert_eq!(
            doc["images"][texture["extensions"]["KHR_texture_basisu"]["source"]
                .as_u64()
                .unwrap() as usize]["mimeType"],
            "image/ktx2"
        );
        assert_eq!(supercompression(file), scheme, "{name}");
        let decoded = ktx2::decode(file).unwrap();
        let original = image::decode(&source(name).unwrap()).unwrap();
        let error = decoded
            .rgba
            .iter()
            .zip(&original.rgba)
            .map(|(a, b)| (*a as f64 - *b as f64).abs())
            .sum::<f64>()
            / original.rgba.len() as f64;
        assert!(error < 6., "{name}: mean error {error}");
    }
    // Deterministic, and far smaller than an uncompressed upload would be.
    assert_eq!(bytes, glb(&scene, &options).unwrap());
    // The library default stays PNG.
    let (png, _) = parse(&glb(&scene, &Options::default()).unwrap());
    assert!(
        png["images"]
            .as_array()
            .unwrap()
            .iter()
            .all(|image| image["mimeType"] == "image/png")
    );
    assert!(png.get("extensionsRequired").is_none());
}

/// Textures encoded on threads inside `glb` give the same file as encoding each one separately,
/// one at a time, and handing the results in; a cancelled export stops instead of finishing.
#[test]
fn threaded_ktx2_export_matches_one_at_a_time_and_cancels() {
    let scene = scene();
    let options = Options {
        textures: Textures::Ktx2,
        ..Options::default()
    };
    let jobs = superspec_export::texture_jobs(&scene, &options).unwrap();
    assert_eq!(
        jobs.iter().map(|job| job.id).collect::<Vec<_>>(),
        ["color.png", "data.png", "normal.png"]
    );
    let encoded = jobs
        .iter()
        .map(|job| {
            (
                job.id.to_owned(),
                superspec_export::encode_texture(job.source, job.kind).unwrap(),
            )
        })
        .collect();
    let supplied = glb(
        &scene,
        &Options {
            encoded: Some(&encoded),
            ..options
        },
    )
    .unwrap();
    assert_eq!(glb(&scene, &options).unwrap(), supplied);

    // Cancelling after more and more of the export's checks, before, during and after texture
    // encoding, stops it; only an export that is never cancelled returns a file, the same file.
    let polls = std::sync::atomic::AtomicUsize::new(0);
    let mut stopped = 0;
    loop {
        polls.store(0, std::sync::atomic::Ordering::Relaxed);
        let cancelled = || polls.fetch_add(1, std::sync::atomic::Ordering::Relaxed) >= stopped;
        let result = glb(
            &scene,
            &Options {
                textures: Textures::Ktx2,
                cancelled: &cancelled,
                ..Options::default()
            },
        );
        match result {
            Ok(bytes) => {
                assert_eq!(bytes, supplied);
                break;
            }
            Err(error) => assert_eq!(error, "Export cancelled.", "after {stopped} polls"),
        }
        stopped = (stopped * 5 / 4).max(stopped + 1);
        assert!(stopped < 1 << 40, "the export never finished");
    }
    assert!(stopped >= 3, "only {stopped} cancellation points");
}

#[test]
fn ktx2_glbs_import_and_convert_back_to_png() {
    let exported = glb(
        &scene(),
        &Options {
            textures: Textures::Ktx2,
            ..Options::default()
        },
    )
    .unwrap();
    let asset: Arc<[u8]> = Arc::from(exported);
    let output = Session::default()
        .build(
            &Request {
                kind: "import-glb".into(),
                entry: "scene.glb".into(),
                ..Request::default()
            },
            1,
            &|name| {
                if name == "scene.glb" {
                    Ok(asset.clone())
                } else {
                    Err(format!("unexpected asset {name}"))
                }
            },
            &|| false,
        )
        .unwrap_or_else(|e| panic!("{e:?}"));
    // Imported textures stay compressed.
    assert!(
        output
            .model
            .images
            .values()
            .all(|image| image.mime == "image/ktx2"),
        "{:?}",
        output
            .model
            .images
            .values()
            .map(|image| image.mime)
            .collect::<Vec<_>>()
    );
    // Re-exported as KTX2 they pass through unchanged; as PNG they are transcoded.
    let again = instance_glb(
        &output.model,
        &Options {
            textures: Textures::Ktx2,
            ..Options::default()
        },
    )
    .unwrap();
    let (doc, binary) = parse(&again);
    let (file, _, _) = image_bytes(&doc, binary, &doc["materials"][0]["normalTexture"]);
    assert!(
        output
            .model
            .images
            .values()
            .any(|image| image.bytes.as_ref() == file)
    );
    let png = instance_glb(&output.model, &Options::default()).unwrap();
    let (doc, binary) = parse(&png);
    let (file, _, _) = image_bytes(&doc, binary, &doc["materials"][0]["normalTexture"]);
    let decoded = image::decode(file).unwrap();
    assert_eq!(
        (decoded.mime, decoded.width, decoded.height),
        ("image/png", 64, 64)
    );
}

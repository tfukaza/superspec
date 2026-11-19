use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{cell::RefCell, collections::BTreeMap, fs, path::Path, process::Command};
use superspec_engine::worldspec::themes::kowloon::authoring::{Inputs, atlas, kit::lettering};

fn fixture(name: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/offline")
        .join(name)
}
fn run(arguments: &[&std::ffi::OsStr]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_superspec-materials"))
        .args(arguments)
        .output()
        .unwrap()
}
fn expected(name: &str, bytes: &[u8]) {
    let hashes: BTreeMap<String, String> =
        serde_json::from_str(include_str!("fixtures/offline/expected.json")).unwrap();
    assert_eq!(
        format!("{:x}", Sha256::digest(bytes)),
        hashes[name],
        "{name}"
    );
}
fn names(directory: &Path) -> Vec<String> {
    let mut names = fs::read_dir(directory)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect::<Vec<_>>();
    names.sort();
    names
}

#[test]
fn intrinsic_maps_preserve_frozen_bytes_and_refuse_occupied_output() {
    let temp = tempfile::tempdir().unwrap();
    let occupied = temp.path().join("occupied");
    fs::create_dir(&occupied).unwrap();
    fs::write(occupied.join("sentinel"), b"retained").unwrap();
    assert!(
        !run(&["brushed-metal".as_ref(), occupied.as_os_str()])
            .status
            .success()
    );
    assert_eq!(names(&occupied), ["sentinel"]);
    assert_eq!(fs::read(occupied.join("sentinel")).unwrap(), b"retained");
    let output = temp.path().join("fresh");
    let result = run(&["brushed-metal".as_ref(), output.as_os_str()]);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(
        names(&output),
        [
            "brushed-metal-normal.png",
            "brushed-metal-properties.png",
            "brushed-metal.png"
        ]
    );
    for name in names(&output) {
        expected(
            &format!("brushed/{name}"),
            &fs::read(output.join(name)).unwrap(),
        );
    }
}

#[test]
fn theme_catalog_preserves_bytes_and_exclusive_publication_after_input_failures() {
    let temp = tempfile::tempdir().unwrap();
    let source = fixture("bags.json");
    let output = temp.path().join("bags.json");
    let missing = temp.path().join("missing.json");
    assert!(
        !run(&[
            "bag-clusters".as_ref(),
            missing.as_os_str(),
            output.as_os_str()
        ])
        .status
        .success()
    );
    assert!(!output.exists());
    let invalid = temp.path().join("invalid.json");
    fs::write(&invalid, b"{}").unwrap();
    assert!(
        !run(&[
            "bag-clusters".as_ref(),
            invalid.as_os_str(),
            output.as_os_str()
        ])
        .status
        .success()
    );
    assert!(!output.exists());
    let occupied = temp.path().join("occupied.json");
    fs::write(&occupied, b"retained").unwrap();
    assert!(
        !run(&[
            "bag-clusters".as_ref(),
            source.as_os_str(),
            occupied.as_os_str()
        ])
        .status
        .success()
    );
    assert_eq!(fs::read(&occupied).unwrap(), b"retained");
    let result = run(&[
        "bag-clusters".as_ref(),
        source.as_os_str(),
        output.as_os_str(),
    ]);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    expected("bags.json", &fs::read(output).unwrap());
}

#[test]
fn theme_atlas_accepts_memory_sources_in_the_original_order_and_recovers_from_corruption() {
    let directory = Path::new("virtual");
    let source_names = [
        "catalog.json",
        "chicken-color",
        "chicken-properties",
        "slices-color",
    ];
    let sources = RefCell::new(
        source_names
            .into_iter()
            .map(|name| {
                (
                    directory.join(name),
                    fs::read(fixture(&format!("food/{name}"))).unwrap(),
                )
            })
            .collect::<BTreeMap<_, _>>(),
    );
    let reads = RefCell::new(Vec::new());
    let read = |path: &Path| {
        reads
            .borrow_mut()
            .push(path.file_name().unwrap().to_str().unwrap().to_owned());
        sources
            .borrow()
            .get(path)
            .cloned()
            .ok_or_else(|| "missing fixture source".to_owned())
    };
    let exists = |path: &Path| sources.borrow().contains_key(path);
    let catalog = |_: &str| panic!("this recipe must use its caller's source catalog");
    let inputs = Inputs {
        read: &read,
        exists: &exists,
        catalog: &catalog,
    };
    let files = atlas::pack(&inputs, directory, true).unwrap();
    assert_eq!(
        *reads.borrow(),
        [
            "catalog.json",
            "chicken-color",
            "chicken-properties",
            "chicken-color",
            "chicken-properties",
            "chicken-color",
            "chicken-properties",
            "slices-color"
        ]
    );
    assert_eq!(
        files
            .iter()
            .map(|(name, _)| name.as_str())
            .collect::<Vec<_>>(),
        [
            "restaurant-food.png",
            "restaurant-food-normal.png",
            "restaurant-food-properties.png",
            "restaurant-food.json"
        ]
    );
    for (name, bytes) in &files {
        expected(&format!("food/{name}"), bytes);
    }
    let original = sources
        .borrow_mut()
        .insert(directory.join("chicken-color"), b"invalid image".to_vec())
        .unwrap();
    assert!(atlas::pack(&inputs, directory, true).is_err());
    sources
        .borrow_mut()
        .insert(directory.join("chicken-color"), original);
    assert_eq!(atlas::pack(&inputs, directory, true).unwrap(), files);

    let temp = tempfile::tempdir().unwrap();
    let output = temp.path().join("food");
    let result = run(&[
        "pack-restaurant-food".as_ref(),
        fixture("food").as_os_str(),
        output.as_os_str(),
    ]);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    for (name, bytes) in files {
        assert_eq!(fs::read(output.join(name)).unwrap(), bytes);
    }
}

#[test]
fn lettering_checks_capacity_before_acquiring_fonts_and_propagates_loader_failure() {
    let extra: String = (0x3400..0x3800)
        .map(|value| char::from_u32(value).unwrap())
        .collect();
    assert!(
        lettering::bake(&extra, &mut |_| panic!(
            "oversized glyph set must fail before font acquisition"
        ))
        .is_err()
    );
    let mut seen = Vec::new();
    let error = lettering::bake("", &mut |spec| {
        seen.push((spec.filename, spec.weight));
        Err("fixture font unavailable".into())
    })
    .unwrap_err();
    assert_eq!(error, "fixture font unavailable");
    assert_eq!(seen, [("NotoSansHK[wght].ttf", Some(900.))]);
}

#[test]
fn lettering_extension_rejects_another_atlas_before_acquiring_fonts() {
    let other = superspec_geometry::image::Image {
        width: 2,
        height: 2,
        rgba: vec![255; 16],
        mime: "image/png",
    };
    let error = lettering::extend("修", &other, &mut |_| {
        panic!("an image of another layout must fail before font acquisition")
    })
    .unwrap_err();
    assert!(error.contains("cannot be extended"), "{error}");
}

fn embedded_maps(invalid_properties: bool) -> Vec<u8> {
    let document: Value = json!({
        "materials":[{"pbrMetallicRoughness":{"baseColorTexture":{"index":0},"metallicRoughnessTexture":{"index":1}}}],
        "textures":[{"source":0},{"source":1}],
        "images":[{"bufferView":0},{"bufferView":1}],
        "bufferViews":[{"buffer":0,"byteOffset":0,"byteLength":4},{"buffer":0,"byteOffset":if invalid_properties {100} else {4},"byteLength":4}]
    });
    let mut json = serde_json::to_vec(&document).unwrap();
    while !json.len().is_multiple_of(4) {
        json.push(b' ');
    }
    let mut bytes = b"glTF".to_vec();
    for number in [2_u32, (28 + json.len() + 8) as u32, json.len() as u32] {
        bytes.extend(number.to_le_bytes());
    }
    bytes.extend(b"JSON");
    bytes.extend(json);
    bytes.extend(8_u32.to_le_bytes());
    bytes.extend(b"BIN\0new-pbr!");
    bytes
}

#[test]
fn embedded_image_failure_preserves_original_channel_publication_order() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("source.glb");
    let output = temp.path().join("maps");
    fs::create_dir(&output).unwrap();
    for name in ["color", "normal", "properties"] {
        fs::write(output.join(format!("item-{name}")), b"old").unwrap();
    }
    fs::write(&source, embedded_maps(true)).unwrap();
    let arguments = [
        "extract-glb-maps".as_ref(),
        source.as_os_str(),
        "item".as_ref(),
        output.as_os_str(),
    ];
    assert!(!run(&arguments).status.success());
    assert_eq!(fs::read(output.join("item-color")).unwrap(), b"new-");
    assert!(!output.join("item-normal").exists());
    assert_eq!(fs::read(output.join("item-properties")).unwrap(), b"old");
    fs::write(&source, embedded_maps(false)).unwrap();
    let result = run(&arguments);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(fs::read(output.join("item-properties")).unwrap(), b"pbr!");
}

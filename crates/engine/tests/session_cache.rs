use superspec_engine::{Session, compile_with_assets};
use superspec_geometry::image::{Image, encode};
const SOURCE: &str = r##"<architecture version="1" id="cache"><material id="finish" color="#ffffff" texture="finish.png"/><box id="box" material="finish" width="2" depth="1" height="1"/></architecture>"##;
fn png(color: [u8; 4]) -> Vec<u8> {
    encode(&Image {
        width: 1,
        height: 1,
        rgba: color.to_vec(),
        mime: "image/png",
    })
    .unwrap()
}

#[test]
fn edit_reuse_matches_fresh_compilation_and_never_substitutes_missing_or_changed_bytes() {
    let cache = Session::default();
    let original = png([12, 34, 56, 255]);
    let changed = png([56, 34, 12, 255]);
    let compile = |source: &str, bytes: &[u8], revision| {
        let read = |_: &str| Ok(bytes.to_vec());
        let actual = cache.compile(source, revision, &read).unwrap().packet();
        assert_eq!(
            actual,
            compile_with_assets(source, revision, &read)
                .unwrap()
                .packet()
        );
        actual
    };
    let first = compile(SOURCE, &original, 0);
    let edited = SOURCE.replace("width=\"2\"", "width=\"3\"");
    let next = compile(&edited, &original, 1);
    assert_ne!(first, next);
    assert_eq!(cache.stats().hits, 1);
    assert_eq!(cache.stats().misses, 1);
    compile(&edited, &changed, 2);
    assert_eq!(cache.stats().misses, 2);
    assert!(
        cache
            .compile(SOURCE, 3, &|_| Err("Removed companion".into()))
            .is_err()
    );
    let mut corrupt = original.clone();
    corrupt[20] ^= 1;
    assert!(cache.compile(SOURCE, 4, &|_| Ok(corrupt.clone())).is_err());
    assert_eq!(cache.stats().entries, 2);
    compile(SOURCE, &original, 5);
    assert_eq!(cache.stats().hits, 2);
    // Renaming a companion can reuse identical validated bytes, but still reads the new identity.
    let renamed = SOURCE.replace("finish.png", "other.png");
    cache
        .compile(&renamed, 6, &|name| {
            assert_eq!(name, "other.png");
            Ok(original.clone())
        })
        .unwrap();
    assert_eq!(cache.stats().hits, 3);
    cache.clear();
    assert_eq!(cache.stats().entries, 0);
    assert_eq!(cache.stats().encoded_bytes, 0);
    compile(SOURCE, &original, 7);
    assert_eq!(cache.stats().misses, 1);
}

#[test]
fn cache_is_bounded_owned_and_optional() {
    let first = png([1, 2, 3, 255]);
    let second = png([4, 5, 6, 255]);
    let limit = first.len().max(second.len());
    let cache = Session::new(limit).unwrap();
    for bytes in [&first, &second, &first] {
        cache.compile(SOURCE, 0, &|_| Ok(bytes.clone())).unwrap();
        assert_eq!(cache.stats().entries, 1);
        assert!(cache.stats().encoded_bytes <= limit);
    }
    assert_eq!(cache.stats().misses, 3);
    let disabled = Session::new(0).unwrap();
    for _ in 0..2 {
        disabled.compile(SOURCE, 0, &|_| Ok(first.clone())).unwrap();
    }
    assert_eq!(disabled.stats().entries, 0);
    assert_eq!(disabled.stats().hits, 0);
    assert_eq!(disabled.stats().misses, 2);
    assert!(Session::new(128 * 1024 * 1024 + 1).is_err());
    let oversized = Session::new(first.len() - 1).unwrap();
    oversized
        .compile(SOURCE, 0, &|_| Ok(first.clone()))
        .unwrap();
    assert_eq!(oversized.stats().encoded_bytes, 0);
    let entries = Session::new(1024 * 1024).unwrap();
    for i in 0u16..257 {
        let bytes = png([i as u8, (i >> 8) as u8, 123, 255]);
        entries.compile(SOURCE, 0, &|_| Ok(bytes.clone())).unwrap();
        assert!(entries.stats().entries <= 256 && entries.stats().encoded_bytes <= 1024 * 1024);
    }
    assert_eq!(entries.stats().entries, 256);
    entries
        .compile(SOURCE, 0, &|_| Ok(png([0, 1, 123, 255])))
        .unwrap();
    assert_eq!(entries.stats().hits, 1);
    entries
        .compile(SOURCE, 0, &|_| Ok(png([0, 0, 123, 255])))
        .unwrap();
    assert_eq!(entries.stats().misses, 258);
}

#[test]
fn cached_admission_still_counts_every_used_image_against_bundle_limits() {
    let cache = Session::default();
    let bytes = encode(&Image {
        width: 2048,
        height: 2048,
        rgba: vec![255; 2048 * 2048 * 4],
        mime: "image/png",
    })
    .unwrap();
    let source = |count| {
        let mut body = String::from("<architecture version=\"1\" id=\"budget\">");
        for i in 0..count {
            body.push_str(&format!("<material id=\"m{i}\" texture=\"{i}.png\"/><box id=\"b{i}\" material=\"m{i}\" position=\"{} 0 0\" width=\"1\" depth=\"1\" height=\"1\"/>",i*2));
        }
        body + "</architecture>"
    };
    // As many distinct images as the scene pixel allowance admits, then one more.
    let fits = superspec_model::MAX_SCENE_IMAGE_PIXELS / (2048 * 2048);
    // Independently of the constant: the allowance is 512 Mi pixels, 128 images of 2048 by 2048.
    assert_eq!(fits, 128);
    cache
        .compile(&source(fits), 0, &|_| Ok(bytes.clone()))
        .unwrap();
    assert_eq!(cache.stats().misses, 1);
    assert_eq!(cache.stats().hits, fits as u64 - 1);
    let error = cache
        .compile(&source(fits + 1), 1, &|_| Ok(bytes.clone()))
        .err()
        .expect("one more image identity exceeds the scene pixel allowance");
    assert!(error.message.contains("pixel budget"));
    cache
        .compile(&source(1), 2, &|_| Ok(bytes.clone()))
        .unwrap();
    cache
        .compile_collision(SOURCE, 3, &|_| {
            panic!("Visual image requested by collision-only compilation")
        })
        .unwrap();
}

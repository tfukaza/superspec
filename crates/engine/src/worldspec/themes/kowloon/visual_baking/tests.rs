use super::*;
use crate::worldspec::themes::kowloon::{
    bake::BakedMaterial,
    cache::Cache,
    fixture::Fixture,
    mesh::{Mesh as Drawing, Mode},
    metal::Binding,
    painted,
    pen::Pen,
    tile,
};
use std::{
    sync::Mutex,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};
use superspec_geometry::{
    Matrix,
    image::{Image, encode},
};

fn png(value: u8) -> Arc<[u8]> {
    encode(&Image {
        width: 2,
        height: 2,
        rgba: [value, 128, 255, 255].repeat(4),
        mime: "image/png",
    })
    .unwrap()
    .into()
}

fn materials() -> BTreeMap<String, Material> {
    ["a", "b", "unused"]
        .into_iter()
        .map(|id| {
            (
                id.into(),
                Material {
                    id: id.into(),
                    color: "#ffffff".into(),
                    texture: Some(format!("{id}.png")),
                    ..Default::default()
                },
            )
        })
        .collect()
}

fn sources() -> BTreeMap<String, Arc<[u8]>> {
    BTreeMap::from([("a.png".into(), png(64)), ("b.png".into(), png(192))])
}

fn painted_drawing() -> Drawing<'static> {
    painted_slots(&["a", "b", "a"])
}

fn painted_slots(slots: &[&str]) -> Drawing<'static> {
    let mut mesh = Drawing::new(Mode::Render, 32, &|| false).unwrap();
    for (i, &slot) in slots.iter().enumerate() {
        let mut fixture = Fixture {
            pen: Pen::new(Matrix::default(), &format!("part-{i}"))
                .unwrap()
                .moved([i as f64 * 2., 0., 0.])
                .unwrap(),
            mesh: &mut mesh,
            repeats: &|_| 1.,
        };
        painted::panel(
            &mut fixture,
            slot,
            [0., 0., 1., 1.],
            0.,
            [1.; 3],
            Some(&painted::Surface {
                recipe: painted::Recipe {
                    finish: painted::Settings {
                        bounds: [0., 0., 1., 1.],
                        paint: [1.; 3],
                        substrate: [0.4; 3],
                        seed: 12.,
                        wear: 0.3,
                        grime: 0.4,
                        handle: None,
                        panels: vec![],
                    },
                    binding: Binding {
                        material: slot.into(),
                        repeat: 1.,
                    },
                },
                group: format!("panel-{i}"),
                density: 8.,
            }),
        )
        .unwrap();
    }
    mesh
}

fn tiled_drawing() -> Drawing<'static> {
    let mut mesh = Drawing::new(Mode::Render, 32, &|| false).unwrap();
    let mut fixture = Fixture {
        pen: Pen::new(Matrix::default(), "tile").unwrap(),
        mesh: &mut mesh,
        repeats: &|_| 1.,
    };
    tile::panel(
        &mut fixture,
        "a",
        &tile::Recipe {
            bounds: [0., 0., 1., 1.],
            tiles: tile::Settings {
                seed: 7,
                share: 1.,
                wear: 0.3,
                grime: 0.6,
                size: Some(0.2),
                aspect: Some(1.),
            },
            materials: tile::Materials {
                ceramic: "b".into(),
                bed: "a".into(),
                grime: "coat".into(),
                repeat: 1.,
                bed_repeat: 1.,
                bed_roughness: 0.8,
                roughness: 0.4,
            },
            bed_tint: [1.; 3],
            density: 16.,
        },
        "tile",
    )
    .unwrap();
    mesh
}

fn run(
    cache: Option<&Mutex<Cache>>,
    materials: &BTreeMap<String, Material>,
    sources: &BTreeMap<String, Arc<[u8]>>,
    draw: fn() -> Drawing<'static>,
    cancelled: &(dyn Fn() -> bool + Sync),
) -> Result<(Vec<BakedMaterial>, Drawing<'static>), Diagnostic> {
    run_drawing(cache, materials, sources, draw(), cancelled)
}

fn run_drawing(
    cache: Option<&Mutex<Cache>>,
    materials: &BTreeMap<String, Material>,
    sources: &BTreeMap<String, Arc<[u8]>>,
    mut drawing: Drawing<'static>,
    cancelled: &(dyn Fn() -> bool + Sync),
) -> Result<(Vec<BakedMaterial>, Drawing<'static>), Diagnostic> {
    let plan = weathered::finish(&mut drawing, &|| false).unwrap();
    let rule = superspec_aml::architecture::parse("<architecture version=\"1\" id=\"source\"/>")
        .unwrap()
        .root;
    let bakes = VisualBaking {
        cache,
        workers: 4,
        cancelled,
        observe: &|_| {},
        asset: &|path| {
            sources
                .get(path)
                .cloned()
                .ok_or_else(|| format!("Missing source {path}."))
        },
    }
    .bake(plan, "host", &rule, materials)?;
    Ok((bakes, drawing))
}

fn equivalent(
    actual: &(Vec<BakedMaterial>, Drawing<'_>),
    expected: &(Vec<BakedMaterial>, Drawing<'_>),
) {
    assert_eq!(actual.0.len(), expected.0.len());
    for (a, b) in actual.0.iter().zip(&expected.0) {
        assert_eq!(a.material(), b.material());
        assert_eq!(a.images, b.images);
        assert_eq!(
            serde_json::to_value(a.layout()).unwrap(),
            serde_json::to_value(b.layout()).unwrap()
        );
    }
    let vertices = |drawing: &Drawing<'_>| {
        drawing
            .vertices
            .iter()
            .map(|v| (v.position, v.normal, v.uv, v.color))
            .collect::<Vec<_>>()
    };
    let surfaces = |drawing: &Drawing<'_>| {
        drawing
            .surfaces
            .iter()
            .map(|s| (&s.part, &s.slot))
            .map(|(p, s)| (p.clone(), s.clone()))
            .collect::<Vec<_>>()
    };
    assert_eq!(vertices(&actual.1), vertices(&expected.1));
    assert_eq!(surfaces(&actual.1), surfaces(&expected.1));
    assert_eq!(actual.1.indices, expected.1.indices);
    assert_eq!(actual.1.triangle_surfaces, expected.1.triangle_surfaces);
}

fn checked(
    cache: &Mutex<Cache>,
    materials: &BTreeMap<String, Material>,
    sources: &BTreeMap<String, Arc<[u8]>>,
    draw: fn() -> Drawing<'static>,
) -> (Vec<BakedMaterial>, Drawing<'static>) {
    let actual = run(Some(cache), materials, sources, draw, &|| false).unwrap();
    equivalent(
        &actual,
        &run(None, materials, sources, draw, &|| false).unwrap(),
    );
    actual
}

#[test]
fn production_keys_retain_unrelated_rasters_and_decode_only_missing_sources() {
    let cache = Mutex::new(Cache::default());
    let mut materials = materials();
    let mut sources = sources();
    let cold = checked(&cache, &materials, &sources, painted_drawing);
    let stats = cache_stats(&cache);
    assert_eq!(
        (
            stats.baked_jobs,
            stats.coalesced_jobs,
            stats.source_decodes,
            stats.source_decoded_pixels
        ),
        (2, 1, 2, 8)
    );
    assert!(Arc::ptr_eq(&cold.0[0].images[0].1, &cold.0[2].images[0].1));
    let warm = checked(&cache, &materials, &sources, painted_drawing);
    equivalent(&warm, &cold);
    assert_eq!(cache_stats(&cache).source_decodes, 2);
    assert_eq!(cache_stats(&cache).binding_hits, 1);

    materials.get_mut("unused").unwrap().color = "#ff0000".into();
    checked(&cache, &materials, &sources, painted_drawing);
    assert_eq!(
        (
            cache_stats(&cache).baked_jobs,
            cache_stats(&cache).source_decodes
        ),
        (2, 2)
    );

    materials.get_mut("a").unwrap().emissive = Some("#336699".into());
    let definition_edit = checked(&cache, &materials, &sources, painted_drawing);
    assert_eq!(
        definition_edit.0[0].material().emissive.as_deref(),
        Some("#336699")
    );
    assert_eq!(
        (
            cache_stats(&cache).baked_jobs,
            cache_stats(&cache).source_decodes
        ),
        (3, 3)
    );
    assert!(Arc::ptr_eq(
        &cold.0[1].images[0].1,
        &definition_edit.0[1].images[0].1
    ));

    sources.insert("a.png".into(), png(13));
    let image_edit = checked(&cache, &materials, &sources, painted_drawing);
    assert_ne!(definition_edit.0[0].images, image_edit.0[0].images);
    assert_eq!(
        (
            cache_stats(&cache).baked_jobs,
            cache_stats(&cache).source_decodes
        ),
        (4, 4)
    );
    assert!(Arc::ptr_eq(
        &cold.0[1].images[0].1,
        &image_edit.0[1].images[0].1
    ));
}

#[test]
fn shared_companions_decode_once_and_invalidate_every_dependent_job() {
    let cache = Mutex::new(Cache::default());
    let mut materials = materials();
    materials.get_mut("b").unwrap().texture = Some("a.png".into());
    let mut sources = sources();
    checked(&cache, &materials, &sources, painted_drawing);
    assert_eq!(
        (
            cache_stats(&cache).baked_jobs,
            cache_stats(&cache).source_decodes
        ),
        (2, 1)
    );
    sources.insert("a.png".into(), png(1));
    checked(&cache, &materials, &sources, painted_drawing);
    assert_eq!(
        (
            cache_stats(&cache).baked_jobs,
            cache_stats(&cache).source_decodes
        ),
        (4, 2)
    );
}

#[test]
fn all_companions_and_secondary_zero_coverage_coatings_participate() {
    let mut materials = materials();
    let mut sources = sources();
    let a = materials.get_mut("a").unwrap();
    a.normal_texture = Some("normal.png".into());
    a.metallic_roughness_texture = Some("properties.png".into());
    a.height_texture = Some("height.png".into());
    a.height_scale = Some(0.01);
    for (path, value) in [
        ("normal.png", 128),
        ("properties.png", 200),
        ("height.png", 99),
    ] {
        sources.insert(path.into(), png(value));
    }
    let cache = Mutex::new(Cache::default());
    checked(&cache, &materials, &sources, painted_drawing);
    assert_eq!(cache_stats(&cache).source_decodes, 5);
    for (i, path) in ["a.png", "normal.png", "properties.png", "height.png"]
        .into_iter()
        .enumerate()
    {
        sources.insert(path.into(), png(20 + i as u8));
        checked(&cache, &materials, &sources, painted_drawing);
        assert_eq!(cache_stats(&cache).baked_jobs, 3 + i as u64);
        assert_eq!(cache_stats(&cache).source_decodes, 9 + i as u64 * 4);
    }

    let cache = Mutex::new(Cache::default());
    materials.insert(
        "coat".into(),
        Material {
            id: "coat".into(),
            texture: Some("coat.png".into()),
            ..Default::default()
        },
    );
    sources.insert("coat.png".into(), png(42));
    checked(&cache, &materials, &sources, tiled_drawing);
    // The structural ceramic grime coating has zero opacity but must remain a dependency.
    for path in ["b.png", "coat.png"] {
        let previous = cache_stats(&cache).baked_jobs;
        sources.insert(path.into(), png(77));
        checked(&cache, &materials, &sources, tiled_drawing);
        assert_eq!(cache_stats(&cache).baked_jobs, previous + 1);
    }
}

#[test]
fn warm_required_sources_fail_closed_and_cancelled_work_recovers() {
    let cache = Mutex::new(Cache::default());
    let materials = materials();
    let mut sources = sources();
    checked(&cache, &materials, &sources, painted_drawing);
    let old = sources.remove("b.png").unwrap();
    assert!(
        run(Some(&cache), &materials, &sources, painted_drawing, &|| {
            false
        })
        .err()
        .unwrap()
        .message
        .contains("Missing source")
    );
    sources.insert("b.png".into(), Arc::from(&b"corrupt png"[..]));
    assert!(
        run(Some(&cache), &materials, &sources, painted_drawing, &|| {
            false
        })
        .is_err()
    );
    assert_eq!(cache_stats(&cache).baked_jobs, 2);
    sources.insert("b.png".into(), old);
    checked(&cache, &materials, &sources, painted_drawing);

    let cache = Mutex::new(Cache::default());
    let caller = std::thread::current().id();
    let observed = AtomicBool::new(false);
    let cancelled = || {
        if std::thread::current().id() != caller {
            observed.store(true, Ordering::SeqCst);
        }
        observed.load(Ordering::SeqCst)
    };
    assert!(
        run(
            Some(&cache),
            &materials,
            &sources,
            painted_drawing,
            &cancelled
        )
        .is_err()
    );
    assert!(observed.load(Ordering::SeqCst));
    assert_eq!(
        (
            cache_stats(&cache).baked_jobs,
            cache_stats(&cache).entries,
            cache_stats(&cache).binding_entries
        ),
        (0, 0, 0)
    );
    checked(&cache, &materials, &sources, painted_drawing);
    assert_eq!(cache_stats(&cache).baked_jobs, 2);
}

#[test]
fn combined_warm_sources_still_obey_the_total_host_pixel_allowance() {
    let cache = Mutex::new(Cache::default());
    let bytes: Arc<[u8]> = encode(&Image {
        width: 4096,
        height: 4096,
        rgba: vec![255; 4096 * 4096 * 4],
        mime: "image/png",
    })
    .unwrap()
    .into();
    let names: Vec<_> = (0..7).map(|i| format!("material-{i}")).collect();
    let materials = names
        .iter()
        .map(|id| {
            (
                id.clone(),
                Material {
                    id: id.clone(),
                    texture: Some(format!("{id}.png")),
                    ..Default::default()
                },
            )
        })
        .collect();
    let sources = names
        .iter()
        .map(|id| (format!("{id}.png"), bytes.clone()))
        .collect();
    // Prime one source at a time so the fixture never retains seven decoded images together.
    for name in &names {
        run_drawing(
            Some(&cache),
            &materials,
            &sources,
            painted_slots(&[name]),
            &|| false,
        )
        .unwrap();
    }
    let before = cache_stats(&cache);
    assert_eq!((before.baked_jobs, before.source_decodes), (7, 7));
    let six: Vec<_> = names[..6].iter().map(String::as_str).collect();
    run_drawing(
        Some(&cache),
        &materials,
        &sources,
        painted_slots(&six),
        &|| false,
    )
    .unwrap();
    let after_six = cache_stats(&cache);
    assert_eq!((after_six.baked_jobs, after_six.source_decodes), (7, 7));
    let seven: Vec<_> = names.iter().map(String::as_str).collect();
    let error = run_drawing(
        Some(&cache),
        &materials,
        &sources,
        painted_slots(&seven),
        &|| false,
    )
    .err()
    .unwrap();
    assert_eq!(error.code, "AML_COMPLEXITY");
    assert!(error.message.contains("pixel allowance"));
    let failed = cache_stats(&cache);
    assert_eq!(
        (
            failed.baked_jobs,
            failed.source_decodes,
            failed.binding_entries
        ),
        (7, 7, after_six.binding_entries)
    );
    run_drawing(
        Some(&cache),
        &materials,
        &sources,
        painted_slots(&six),
        &|| false,
    )
    .unwrap();
    assert_eq!(cache_stats(&cache).binding_hits, after_six.binding_hits + 1);
}

#[test]
fn evicted_source_admission_metadata_redecodes_and_recovers_exact_output() {
    let cache = Mutex::new(Cache::default());
    let materials = materials();
    let sources = sources();
    let cold = checked(&cache, &materials, &sources, painted_drawing);
    for value in 0u16..256 {
        cache
            .lock()
            .expect("bake cache")
            .image(&value.to_le_bytes());
    }
    let recovered = checked(&cache, &materials, &sources, painted_drawing);
    equivalent(&cold, &recovered);
    assert_eq!(
        (
            cache_stats(&cache).baked_jobs,
            cache_stats(&cache).source_decodes
        ),
        (4, 4)
    );
    checked(&cache, &materials, &sources, painted_drawing);
    assert_eq!(
        (
            cache_stats(&cache).baked_jobs,
            cache_stats(&cache).source_decodes
        ),
        (4, 4)
    );
}

fn cache_stats(cache: &Mutex<Cache>) -> crate::BakeStats {
    cache.lock().expect("bake cache").stats()
}

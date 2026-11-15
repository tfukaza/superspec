use super::*;
use crate::worldspec::themes::kowloon::{
    cache::Cache,
    mesh::{Mode, Vertex},
};
use std::{collections::BTreeMap, sync::Arc, sync::Mutex};
use superspec_aml::material::Material;

fn job(seed: f64) -> Job {
    Job {
        density: 8.,
        recipe: Recipe::Metal {
            settings: Box::new(metal::Settings {
                kind: metal::Kind::Frame,
                bounds: [0., 0., 1., 0.6],
                paint: [0.3, 0.5, 0.7],
                seed,
                wear: 0.6,
                grime: 0.7,
                rust: 0.4,
                paint_coverage: None,
                edge_axes: None,
                contacts: None,
                metric: None,
                folds: vec![],
                wrap_u: None,
                substrate: metal::Binding {
                    material: "steel".into(),
                    repeat: 3.,
                },
                coating: metal::Binding {
                    material: "paint".into(),
                    repeat: 5.,
                },
            }),
            runoff: None,
        },
    }
}

fn materials() -> BTreeMap<String, Material> {
    [
        Material {
            id: "steel".into(),
            color: "#9fb1c3".into(),
            roughness: 0.7,
            metalness: 0.9,
            ..Default::default()
        },
        Material {
            id: "paint".into(),
            color: "#ee8b42".into(),
            roughness: 0.35,
            ..Default::default()
        },
    ]
    .into_iter()
    .map(|m| (m.id.clone(), m))
    .collect()
}

fn plan<'mesh, 'cancel>(
    mesh: &'mesh mut Mesh<'cancel>,
    jobs: Vec<Job>,
    offset: f64,
) -> Plan<'mesh, 'cancel> {
    let receivers = (0..jobs.len())
        .map(|i| {
            let vertices = [[0., 0.], [1., 0.], [0., 0.6]].map(|uv| Vertex {
                position: [offset + 2. * i as f64 + uv[0], uv[1], 0.],
                normal: [0., 0., 1.],
                uv,
                color: [1.; 3],
            });
            mesh.triangle(&format!("part-{i}"), "unbaked", vertices, [0, 1, 2], None)
                .unwrap();
            ReceiverRange {
                first_triangle: i,
                triangle_count: 1,
                job: i,
            }
        })
        .collect();
    Plan {
        jobs,
        receivers,
        mesh,
        edge_vertices: Arc::from([]),
        edge_index: None,
    }
}

fn mesh() -> Mesh<'static> {
    Mesh::new(Mode::Render, 32, &|| false).unwrap()
}

fn assert_equivalent(
    actual: &[BakedMaterial],
    drawing: &Mesh<'_>,
    expected: &[BakedMaterial],
    clean: &Mesh<'_>,
) {
    assert_eq!(actual.len(), expected.len());
    for (actual, expected) in actual.iter().zip(expected) {
        assert_eq!(actual.material, expected.material);
        assert_eq!(actual.images, expected.images);
        assert_eq!(actual.pixels, expected.pixels);
        assert_eq!(
            serde_json::to_value(actual.layout()).unwrap(),
            serde_json::to_value(expected.layout()).unwrap()
        );
    }
    let vertices = |m: &Mesh<'_>| {
        m.vertices
            .iter()
            .map(|v| (v.position, v.normal, v.uv, v.color))
            .collect::<Vec<_>>()
    };
    let surfaces = |m: &Mesh<'_>| {
        m.surfaces
            .iter()
            .map(|s| (s.part.clone(), s.slot.clone()))
            .collect::<Vec<_>>()
    };
    assert_eq!(vertices(drawing), vertices(clean));
    assert_eq!(surfaces(drawing), surfaces(clean));
    assert_eq!(drawing.indices, clean.indices);
    assert_eq!(drawing.triangle_surfaces, clean.triangle_surfaces);
}

fn assert_shared(a: &BakedMaterial, b: &BakedMaterial) {
    assert_eq!(a.images.len(), b.images.len());
    for ((_, a), (_, b)) in a.images.iter().zip(&b.images) {
        assert!(Arc::ptr_eq(a, b));
    }
}

#[test]
fn repeated_rasters_share_work_across_slots_hosts_and_reordered_edits() {
    let materials = materials();
    let inputs = Inputs {
        materials: &materials,
        images: &BTreeMap::new(),
    };
    for workers in [1, 4] {
        let cache = Mutex::new(Cache::default());
        let mut first_mesh = mesh();
        let first = plan(&mut first_mesh, vec![job(1.), job(1.), job(2.)], 0.)
            .bake_cached_with_workers(&inputs, "first-host", b"inputs", &cache, workers, &|| false)
            .unwrap();
        let mut clean = mesh();
        let expected = plan(&mut clean, vec![job(1.), job(1.), job(2.)], 0.)
            .bake(&inputs, "first-host", &|| false)
            .unwrap();
        assert_equivalent(&first, &first_mesh, &expected, &clean);
        assert_shared(&first[0], &first[1]);
        assert_ne!(first[0].material.id, first[1].material.id);
        let stats = cache_stats(&cache);
        assert_eq!(stats.baked_jobs, 2);
        assert_eq!(stats.coalesced_jobs, 1);
        assert_eq!(
            stats.baked_channel_pixels,
            (first[0].pixels + first[2].pixels) as u64
        );

        let mut second_mesh = mesh();
        let second = plan(
            &mut second_mesh,
            vec![job(2.), job(3.), job(1.), job(3.)],
            10.,
        )
        .bake_cached_with_workers(&inputs, "second-host", b"inputs", &cache, workers, &|| {
            false
        })
        .unwrap();
        let mut clean = mesh();
        let expected = plan(&mut clean, vec![job(2.), job(3.), job(1.), job(3.)], 10.)
            .bake(&inputs, "second-host", &|| false)
            .unwrap();
        assert_equivalent(&second, &second_mesh, &expected, &clean);
        assert_shared(&second[1], &second[3]);
        assert_shared(&second[0], &first[2]);
        assert_shared(&second[2], &first[0]);
        assert_eq!(cache_stats(&cache).baked_jobs, 3);
        assert_eq!(cache_stats(&cache).coalesced_jobs, 2);

        let mut third_mesh = mesh();
        let third = plan(&mut third_mesh, vec![job(3.), job(2.), job(1.)], -10.);
        let warm = third
            .cached_bakes("third-host", b"inputs", &cache)
            .unwrap()
            .unwrap();
        let warm = third
            .bind_cached(warm, b"inputs", &cache, &|| false)
            .unwrap();
        let mut clean = mesh();
        let expected = plan(&mut clean, vec![job(3.), job(2.), job(1.)], -10.)
            .bake(&inputs, "third-host", &|| false)
            .unwrap();
        assert_equivalent(&warm, &third_mesh, &expected, &clean);
        assert_shared(&warm[0], &second[1]);
        assert_eq!(cache_stats(&cache).baked_jobs, 3);
    }
}

#[test]
fn recipe_resolution_and_spatial_inputs_keep_distinct_rasters() {
    let materials = materials();
    let inputs = Inputs {
        materials: &materials,
        images: &BTreeMap::new(),
    };
    let cache = Mutex::new(Cache::default());
    let mut variants = vec![job(1.), job(2.)];
    let mut denser = job(1.);
    denser.density = 16.;
    variants.push(denser);
    for change in 0..4 {
        let mut variant = job(1.);
        let Recipe::Metal { settings, .. } = &mut variant.recipe else {
            unreachable!()
        };
        match change {
            0 => settings.contacts = Some(vec![[0.2, 0.3]]),
            1 => {
                settings.metric = Some(metal::Metric {
                    scale: [1., 1.],
                    gravity: [1., 0.],
                })
            }
            2 => {
                settings.folds = vec![metal::Fold {
                    axis: 0,
                    at: 0.3,
                    concave: true,
                }]
            }
            _ => settings.bounds[2] = 1.5,
        }
        variants.push(variant);
    }
    for (i, variant) in variants.into_iter().enumerate() {
        let fresh_job = Job {
            recipe: variant.recipe.clone(),
            density: variant.density,
        };
        let mut drawing = mesh();
        let actual = plan(&mut drawing, vec![variant], 0.)
            .bake_cached(&inputs, "host", b"inputs", &cache, &|| false)
            .unwrap();
        let mut clean = mesh();
        let expected = plan(&mut clean, vec![fresh_job], 0.)
            .bake(&inputs, "host", &|| false)
            .unwrap();
        assert_equivalent(&actual, &drawing, &expected, &clean);
        assert_eq!(cache_stats(&cache).baked_jobs, (i + 1) as u64);
    }
}

#[test]
fn source_bytes_and_material_definition_changes_rebuild() {
    use superspec_geometry::image::{Image, encode};
    let cache = Mutex::new(Cache::default());
    let mut materials = materials();
    materials.get_mut("steel").unwrap().texture = Some("steel.png".into());
    let mut prior_images = None;
    for (i, color) in [[10, 20, 30, 255], [70, 80, 90, 255], [70, 80, 90, 255]]
        .into_iter()
        .enumerate()
    {
        if i == 2 {
            materials.get_mut("steel").unwrap().emissive = Some("#110000".into());
        }
        let image = Image {
            width: 1,
            height: 1,
            rgba: color.to_vec(),
            mime: "image/png",
        };
        let identity = cache
            .lock()
            .expect("bake cache")
            .image(&encode(&image).unwrap());
        let prefix = serde_json::to_vec(&(&materials, identity)).unwrap();
        let images = BTreeMap::from([("steel.png".into(), image)]);
        let inputs = Inputs {
            materials: &materials,
            images: &images,
        };
        let mut drawing = mesh();
        let actual = plan(&mut drawing, vec![job(1.)], 0.)
            .bake_cached(&inputs, "host", &prefix, &cache, &|| false)
            .unwrap();
        let mut clean = mesh();
        let expected = plan(&mut clean, vec![job(1.)], 0.)
            .bake(&inputs, "host", &|| false)
            .unwrap();
        assert_equivalent(&actual, &drawing, &expected, &clean);
        assert_eq!(cache_stats(&cache).baked_jobs, (i + 1) as u64);
        if i == 1 {
            assert_ne!(prior_images.as_ref().unwrap(), &actual[0].images);
        }
        prior_images = Some(actual[0].images.clone());
    }
}

#[test]
#[cfg(not(target_arch = "wasm32"))]
fn cancelled_raster_work_keeps_drawings_and_cache_unpublished_then_recovers() {
    use std::sync::atomic::{AtomicBool, Ordering};
    let materials = materials();
    let inputs = Inputs {
        materials: &materials,
        images: &BTreeMap::new(),
    };
    let cache = Mutex::new(Cache::default());
    let mut drawing = mesh();
    let batch = plan(&mut drawing, vec![job(1.), job(1.), job(2.)], 0.);
    let main = std::thread::current().id();
    let stopped = AtomicBool::new(false);
    let cancelled = || {
        if std::thread::current().id() != main {
            stopped.store(true, Ordering::Relaxed);
        }
        stopped.load(Ordering::Relaxed)
    };
    let error = batch
        .bake_cached_with_workers(&inputs, "host", b"inputs", &cache, 4, &cancelled)
        .err()
        .unwrap();
    assert!(error.to_lowercase().contains("cancelled"));
    assert!(stopped.load(Ordering::Relaxed));
    assert!(drawing.surfaces.iter().all(|s| s.slot == "unbaked"));
    assert_eq!(cache_stats(&cache).entries, 0);
    assert_eq!(cache_stats(&cache).binding_entries, 0);
    assert_eq!(cache_stats(&cache).baked_jobs, 0);
    let mut recovered = mesh();
    let actual = plan(&mut recovered, vec![job(1.), job(1.), job(2.)], 0.)
        .bake_cached(&inputs, "host", b"inputs", &cache, &|| false)
        .unwrap();
    let mut clean = mesh();
    let expected = plan(&mut clean, vec![job(1.), job(1.), job(2.)], 0.)
        .bake(&inputs, "host", &|| false)
        .unwrap();
    assert_equivalent(&actual, &recovered, &expected, &clean);
    assert_eq!(cache_stats(&cache).baked_jobs, 2);
    assert_eq!(cache_stats(&cache).coalesced_jobs, 1);
}

fn cache_stats(cache: &Mutex<Cache>) -> crate::BakeStats {
    cache.lock().expect("bake cache").stats()
}

#[test]
fn warm_binding_releases_cache_before_drawing_cancellation_callbacks() {
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    let cache = Mutex::new(Cache::default());
    let materials = materials();
    let inputs = Inputs {
        materials: &materials,
        images: &BTreeMap::new(),
    };
    let mut cold_mesh = mesh();
    let cold = plan(&mut cold_mesh, vec![job(1.)], 0.)
        .bake_cached(&inputs, "host", b"inputs", &cache, &|| false)
        .unwrap();
    let calls = AtomicUsize::new(0);
    let locked = AtomicBool::new(false);
    let check = || {
        calls.fetch_add(1, Ordering::Relaxed);
        if cache.try_lock().is_err() {
            locked.store(true, Ordering::Relaxed);
        }
        false
    };
    let mut warm_mesh = Mesh::new(Mode::Render, 32, &check).unwrap();
    let warm_plan = plan(&mut warm_mesh, vec![job(1.)], 0.);
    let before = calls.load(Ordering::Relaxed);
    let warm = warm_plan
        .bake_cached(&inputs, "host", b"inputs", &cache, &|| false)
        .unwrap();
    assert_eq!(cache_stats(&cache).binding_hits, 1);
    assert!(
        calls.load(Ordering::Relaxed) > before,
        "restoring retained geometry still checks cancellation"
    );
    assert!(
        !locked.load(Ordering::Relaxed),
        "host cancellation may read cache statistics during binding restore"
    );
    assert_equivalent(&warm, &warm_mesh, &cold, &cold_mesh);
}

#![cfg(not(target_arch = "wasm32"))]
use std::{
    collections::BTreeMap,
    sync::atomic::{AtomicUsize, Ordering},
};
use superspec_aml::material::Material;
use superspec_engine::{
    compile,
    materialspec::surface::Inputs,
    worldspec::themes::kowloon::{
        mesh::{Mesh, Mode},
        metal::{Binding, Kind},
        pen::{Pen, Tint},
        weathered::{self, ParallelBaking, Settings},
    },
};
use superspec_geometry::Matrix;

fn drawing() -> Mesh<'static> {
    drawing_with_missing(false)
}
fn drawing_with_missing(missing: bool) -> Mesh<'static> {
    let mut mesh = Mesh::new(Mode::Render, 100, &|| false).unwrap();
    let settings = Settings {
        seed: "parallel".into(),
        wear: 0.4,
        grime: 0.5,
        rust: 0.3,
        texture_density: 128.,
        metal: Binding {
            material: "metal".into(),
            repeat: 2.,
        },
        sheet: Binding {
            material: "metal".into(),
            repeat: 2.,
        },
        paint: Binding {
            material: "paint".into(),
            repeat: 3.,
        },
    };
    let pen = Pen::new(Matrix::default(), "panels")
        .unwrap()
        .with_weathering(
            &mut mesh,
            settings.clone(),
            Kind::Sheet,
            None,
            vec!["metal".into()],
        )
        .unwrap();
    for i in 0..4 {
        let selected = if i == 3 && missing {
            let mut changed = settings.clone();
            changed.paint.material = "missing-later-paint".into();
            pen.with_weathering(&mut mesh, changed, Kind::Sheet, None, vec!["metal".into()])
                .unwrap()
        } else {
            pen.clone()
        };
        selected
            .moved([i as f64, 0., 0.])
            .unwrap()
            .panel(
                &mut mesh,
                "metal",
                [0., 0.25, 0., 0.25],
                0.,
                &Tint::Color([0.2 + i as f64 * 0.1, 0.5, 0.7]),
                2.,
            )
            .unwrap();
    }
    mesh
}
fn materials() -> BTreeMap<String, Material> {
    ["metal", "paint"]
        .into_iter()
        .map(|id| {
            (
                id.into(),
                Material {
                    id: id.into(),
                    color: "#a0b0c0".into(),
                    roughness: 0.7,
                    ..Material::default()
                },
            )
        })
        .collect()
}
fn packet(workers: usize) -> Vec<u8> {
    let mut mesh = drawing();
    let plan = weathered::finish(&mut mesh, &|| false).unwrap();
    let bakes = plan
        .bake_parallel(
            &Inputs {
                materials: &materials(),
                images: &BTreeMap::new(),
            },
            "parallel",
            &ParallelBaking {
                workers,
                ..Default::default()
            },
            &|| false,
        )
        .unwrap();
    let mut scene=compile(r#"<architecture version="1" id="parallel"><box id="host" width="4" depth=".1" height="1" visible="false"/></architecture>"#,0).unwrap();
    let bindings = bakes
        .iter()
        .map(|b| (b.material().id.clone(), b.material().id.clone()))
        .collect();
    scene
        .append_baked_visual("host", &mesh, &bindings, &bakes)
        .unwrap();
    scene.packet()
}
fn state(mesh: &Mesh<'_>) -> String {
    format!(
        "{:?}{:?}{:?}{:?}",
        mesh.vertices, mesh.indices, mesh.surfaces, mesh.triangle_surfaces
    )
}

#[test]
fn native_worker_counts_keep_identical_complete_packets() {
    let serial = packet(1);
    for workers in [2, 4, 16] {
        assert_eq!(packet(workers), serial, "workers={workers}");
    }
    let mut mesh = drawing();
    let plan = weathered::finish(&mut mesh, &|| false).unwrap();
    assert!(
        plan.worker_count(&ParallelBaking {
            workers: 0,
            ..Default::default()
        })
        .is_err()
    );
    assert!(
        plan.worker_count(&ParallelBaking {
            workers: 17,
            ..Default::default()
        })
        .is_err()
    );
    assert!(
        plan.worker_count(&ParallelBaking {
            workers: 4,
            working_bytes: 1
        })
        .is_err()
    );
    assert!(
        plan.worker_count(&ParallelBaking {
            workers: 4,
            working_bytes: 257 * 1024 * 1024
        })
        .is_err()
    );
    // Find the smallest accepted per-job allowance; adding workers cannot exceed that memory cap.
    let mut low = 1;
    let mut high = 128 * 1024 * 1024;
    while low < high {
        let mid = (low + high) / 2;
        if plan
            .worker_count(&ParallelBaking {
                workers: 4,
                working_bytes: mid,
            })
            .is_ok()
        {
            high = mid;
        } else {
            low = mid + 1;
        }
    }
    assert_eq!(
        plan.worker_count(&ParallelBaking {
            workers: 4,
            working_bytes: low
        })
        .unwrap(),
        1
    );
    let cpus = std::thread::available_parallelism().map_or(1, usize::from);
    assert_eq!(
        plan.worker_count(&ParallelBaking {
            workers: 4,
            working_bytes: low * 2
        })
        .unwrap(),
        2.min(cpus)
    );
}

#[test]
fn cancelled_and_failed_workers_publish_no_binding_and_fresh_work_recovers() {
    for missing in [false, true] {
        let mut mesh = drawing_with_missing(missing);
        let before = state(&mesh);
        let plan = weathered::finish(&mut mesh, &|| false).unwrap();
        let materials = materials();
        let checkpoints = AtomicUsize::new(0);
        let result = plan.bake_parallel(
            &Inputs {
                materials: &materials,
                images: &BTreeMap::new(),
            },
            "failure",
            &ParallelBaking::default(),
            &|| !missing && checkpoints.fetch_add(1, Ordering::Relaxed) > 10,
        );
        let error = result.err().unwrap();
        assert!(
            error.contains(if missing {
                "Unknown surface material"
            } else {
                "cancelled"
            }),
            "{error}"
        );
        assert_eq!(state(&mesh), before);
        assert_eq!(packet(4), packet(1));
    }
}

#[test]
fn multiple_workers_overlap_and_panics_join_before_returning() {
    use std::{
        collections::HashSet,
        sync::{Condvar, Mutex, atomic::AtomicBool},
        time::Duration,
    };
    if std::thread::available_parallelism().map_or(1, usize::from) < 2 {
        eprintln!("Single-CPU host: actual multithread overlap/panic coverage unavailable.");
        return;
    }
    let mut mesh = drawing();
    let plan = weathered::finish(&mut mesh, &|| false).unwrap();
    let options = ParallelBaking {
        workers: 2,
        ..Default::default()
    };
    assert_eq!(plan.worker_count(&options).unwrap(), 2);
    let owner = std::thread::current().id();
    let threads = Mutex::new(HashSet::new());
    let ready = Condvar::new();
    plan.bake_parallel(
        &Inputs {
            materials: &materials(),
            images: &BTreeMap::new(),
        },
        "overlap",
        &options,
        &|| {
            let thread = std::thread::current().id();
            if thread == owner {
                return false;
            }
            let mut seen = threads.lock().unwrap();
            if seen.insert(thread) {
                ready.notify_all();
                let (seen, timeout) = ready
                    .wait_timeout_while(seen, Duration::from_secs(5), |s| s.len() < 2)
                    .unwrap();
                return timeout.timed_out() && seen.len() < 2;
            }
            false
        },
    )
    .unwrap();
    assert_eq!(threads.into_inner().unwrap().len(), 2);

    struct Activity<'a>(&'a AtomicUsize);
    impl Drop for Activity<'_> {
        fn drop(&mut self) {
            self.0.fetch_sub(1, Ordering::SeqCst);
        }
    }
    let mut mesh = drawing();
    let before = state(&mesh);
    let plan = weathered::finish(&mut mesh, &|| false).unwrap();
    let once = AtomicBool::new(false);
    let active = AtomicUsize::new(0);
    let error = plan
        .bake_parallel(
            &Inputs {
                materials: &materials(),
                images: &BTreeMap::new(),
            },
            "panic",
            &options,
            &|| {
                active.fetch_add(1, Ordering::SeqCst);
                let _active = Activity(&active);
                assert!(once.swap(true, Ordering::SeqCst), "Injected worker failure");
                false
            },
        )
        .err()
        .unwrap();
    assert!(error.contains("worker panicked"));
    assert_eq!(active.load(Ordering::SeqCst), 0);
    assert_eq!(state(&mesh), before);
    assert_eq!(packet(2), packet(1));
}

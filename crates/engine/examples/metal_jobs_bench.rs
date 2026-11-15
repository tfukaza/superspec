//! Isolated raster job experiment. This is not the default courtyard or an edit-cache benchmark.
use std::{collections::BTreeMap, time::Instant};
use superspec_aml::material::Material;
use superspec_engine::{
    compile,
    materialspec::surface::Inputs,
    worldspec::themes::kowloon::{
        mesh::{Mesh, Mode},
        metal::{Binding, Kind},
        pen::{Pen, Tint},
        weathered::{self, Settings},
    },
};
use superspec_geometry::Matrix;

fn run(workers: usize) -> (Vec<u8>, serde_json::Value) {
    let start = Instant::now();
    let mut scene=compile(r#"<architecture version="1" id="metal-jobs"><box id="host" width="9" depth=".1" height="2" visible="false"/></architecture>"#,0).unwrap();
    let materials = BTreeMap::from([
        (
            "metal".into(),
            Material {
                id: "metal".into(),
                color: "#b0bac0".into(),
                roughness: 0.5,
                metalness: 0.9,
                ..Material::default()
            },
        ),
        (
            "paint".into(),
            Material {
                id: "paint".into(),
                color: "#ffffff".into(),
                roughness: 0.8,
                ..Material::default()
            },
        ),
    ]);
    let mut mesh = Mesh::new(Mode::Render, 100, &|| false).unwrap();
    let settings = Settings {
        seed: "jobs".into(),
        wear: 0.4,
        grime: 0.5,
        rust: 0.3,
        texture_density: 256.,
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
        .with_weathering(&mut mesh, settings, Kind::Sheet, None, vec!["metal".into()])
        .unwrap();
    for i in 0..4 {
        pen.moved([i as f64 * 2.2, 0., 0.])
            .unwrap()
            .panel(
                &mut mesh,
                "metal",
                [0., 2., 0., 2.],
                0.,
                &Tint::Color([0.2 + i as f64 * 0.08, 0.5, 0.7]),
                2.,
            )
            .unwrap();
    }
    let plan = weathered::finish(&mut mesh, &|| false).unwrap();
    let jobs = plan.jobs().len();
    let options = weathered::ParallelBaking {
        workers,
        ..Default::default()
    };
    let effective_workers = plan.worker_count(&options).unwrap();
    let planning = start.elapsed().as_secs_f64() * 1000.;
    let bake_start = Instant::now();
    let bakes = plan
        .bake_parallel(
            &Inputs {
                materials: &materials,
                images: &BTreeMap::new(),
            },
            "bench",
            &options,
            &|| false,
        )
        .unwrap();
    let baking = bake_start.elapsed().as_secs_f64() * 1000.;
    let bindings = bakes
        .iter()
        .map(|b| (b.material().id.clone(), b.material().id.clone()))
        .collect();
    scene
        .append_baked_visual("host", &mesh, &bindings, &bakes)
        .unwrap();
    let packet = scene.packet();
    let total = start.elapsed().as_secs_f64() * 1000.;
    let report = serde_json::json!({"jobs":jobs,"requestedWorkers":workers,"effectiveWorkers":effective_workers,"planningMs":planning,"bakingAndBindingMs":baking,"totalMs":total,"packetBytes":packet.len()});
    (packet, report)
}
fn main() {
    let samples = std::env::args()
        .nth(1)
        .map(|s| s.parse::<usize>().unwrap())
        .unwrap_or(4);
    assert!((1..=32).contains(&samples));
    let mut reference = None;
    let mut reports = Vec::new();
    for _ in 0..samples {
        for workers in [1, 4, 4, 1] {
            let (packet, report) = run(workers);
            if let Some(reference) = &reference {
                assert_eq!(&packet, reference);
            } else {
                reference = Some(packet);
            }
            reports.push(report);
        }
    }
    println!(
        "{}",
        serde_json::json!({"scope":"four separate two-meter metal charts; no texture input or result cache; not courtyard", "outputParity":true,"samples":reports})
    );
}

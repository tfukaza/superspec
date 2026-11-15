//! Shared painted leaves and metal trim through one Rust job plan.
use std::collections::BTreeMap;
use superspec_engine::{
    Scene, compile,
    materialspec::surface::Inputs,
    worldspec::themes::kowloon::{
        fixture::Fixture,
        mesh::{Mesh, Mode},
        metal::{Binding, Kind},
        painted::{self, Recipe, Settings, Surface},
        pen::Pen,
        weathered,
    },
};
use superspec_geometry::Matrix;
#[path = "pipe_scene.rs"]
#[allow(dead_code)]
mod pipe;
pub fn scene(parallel: bool) -> Scene {
    let mut scene=compile(r#"<architecture version="1" id="rust-paint"><box id="host" width="3.5" depth=".1" height="2.2" visible="false"/></architecture>"#,0).unwrap();
    let materials = pipe::materials();
    let mut mesh = Mesh::new(Mode::Render, 50000, &|| false).unwrap();
    let root = Pen::new(
        Matrix([[1., 0., 0.], [0., -1., 0.], [0., 0., 1.], [0.; 3]]),
        "painted-leaf",
    )
    .unwrap();
    for (i, wear) in [0., 0.8, 0.8].into_iter().enumerate() {
        let pen = root
            .moved([i as f64 * 1.15, 0., 0.])
            .unwrap()
            .into_part(&format!("leaf-{i}"))
            .unwrap();
        let s = Surface {
            recipe: Recipe {
                finish: Settings {
                    bounds: [0., 0., 1., 2.],
                    paint: [0.12, 0.31, 0.24],
                    substrate: [0.17, 0.12, 0.08],
                    seed: 37.,
                    wear,
                    grime: wear,
                    handle: Some([0.88, 1.]),
                    panels: vec![[0.12, 0.2, 0.88, 0.85], [0.12, 1.15, 0.88, 1.85]],
                },
                binding: Binding {
                    material: "paint".into(),
                    repeat: 1.,
                },
            },
            density: 128.,
            group: "shared-leaf".into(),
        };
        let mut f = Fixture {
            pen: pen.clone(),
            mesh: &mut mesh,
            repeats: &|_| 1.,
        };
        painted::panel(
            &mut f,
            "paint",
            [0., 0., 1., 2.],
            0.,
            s.recipe.finish.paint,
            Some(&s),
        )
        .unwrap();
        for bounds in &s.recipe.finish.panels {
            let [a, low, b, high] = *bounds;
            let outer = [
                [a, 0.04, low],
                [b, 0.04, low],
                [b, 0.04, high],
                [a, 0.04, high],
            ];
            let inner = [
                [a + 0.025, 0.01, low + 0.025],
                [b - 0.025, 0.01, low + 0.025],
                [b - 0.025, 0.01, high - 0.025],
                [a + 0.025, 0.01, high - 0.025],
            ];
            for j in 0..4 {
                let n = (j + 1) % 4;
                painted::face(
                    &mut f,
                    "paint",
                    &[outer[j], outer[n], inner[n], inner[j]],
                    s.recipe.finish.paint,
                    Some(&s),
                )
                .unwrap();
            }
            painted::face(&mut f, "paint", &inner, s.recipe.finish.paint, Some(&s)).unwrap();
        }
        let mut options = pipe::options().weathering.unwrap();
        options.texture_density = 64.;
        let metal = pen
            .with_weathering(f.mesh, options, Kind::Frame, None, vec!["metal".into()])
            .unwrap();
        let mut f = Fixture {
            pen: metal,
            mesh: f.mesh,
            repeats: &|_| 1.,
        };
        // Broad plates exercise both recipe kinds; the small pull retains compact paint.
        f.cuboid(
            "metal",
            [0.82, 0.042, 0.94],
            [0.94, 0.055, 1.13],
            [0.45, 0.5, 0.48],
            "b",
        )
        .unwrap();
        f.bar(
            "metal",
            [0.88, 0.08, 0.98],
            [0.88, 0.08, 1.1],
            0.02,
            [0.45, 0.5, 0.48],
        )
        .unwrap();
    }
    let before = mesh
        .vertices
        .iter()
        .map(|v| (v.position, v.normal, v.color))
        .collect::<Vec<_>>();
    let plan = weathered::finish(&mut mesh, &|| false).unwrap();
    let inputs = Inputs {
        materials: &materials,
        images: &BTreeMap::new(),
    };
    #[cfg(not(target_arch = "wasm32"))]
    let bakes = if parallel {
        plan.bake_parallel(
            &inputs,
            "painted",
            &weathered::ParallelBaking::default(),
            &|| false,
        )
        .unwrap()
    } else {
        plan.bake(&inputs, "painted", &|| false).unwrap()
    };
    #[cfg(target_arch = "wasm32")]
    let bakes = {
        let _ = parallel;
        plan.bake(&inputs, "painted", &|| false).unwrap()
    };
    assert_eq!(
        before,
        mesh.vertices
            .iter()
            .map(|v| (v.position, v.normal, v.color))
            .collect::<Vec<_>>()
    );
    let mut bindings: BTreeMap<_, _> = materials
        .keys()
        .map(|id| (id.clone(), id.clone()))
        .collect();
    bindings.extend(
        bakes
            .iter()
            .map(|b| (b.material().id.clone(), b.material().id.clone())),
    );
    scene.manifest.materials.extend(materials.into_values());
    scene
        .append_baked_visual("host", &mesh, &bindings, &bakes)
        .unwrap();
    scene
}

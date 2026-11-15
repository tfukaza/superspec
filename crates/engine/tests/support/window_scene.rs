//! Production geometry assembled into a small visual acceptance scene, not the courtyard benchmark.
use std::collections::BTreeMap;
use superspec_aml::{material::Material, rng::SeededRng};
use superspec_engine::{
    Scene, compile,
    worldspec::themes::kowloon::{
        fixture::Fixture,
        mesh::{Mesh, Mode},
        pen::Pen,
        windows::{Guard, Kind, Look, security_bars, window},
    },
};
use superspec_geometry::Matrix;

pub fn scene() -> Scene {
    windows_scene(
        &[Kind::Casement, Kind::Slider, Kind::Louver],
        "window-fixtures",
        false,
    )
}
pub fn timber_scene() -> Scene {
    windows_scene(&[Kind::Timber], "timber-window-fixture", false)
}
pub fn weathered_scene() -> Scene {
    windows_scene(&[Kind::Casement], "weathered-window-fixture", true)
}
fn windows_scene(kinds: &[Kind], id: &str, weathered: bool) -> Scene {
    let slots = [
        "concrete",
        "paint",
        "metal",
        "timber",
        "pane",
        "interior-lit",
        "glass",
        "shop-timber",
    ];
    let mut scene=compile(&format!(r##"<architecture version="1" id="{id}"><box id="host" width="8" depth="0.1" height="3" visible="false"/></architecture>"##),75).unwrap();
    scene
        .manifest
        .materials
        .extend(slots.iter().map(|slot| Material {
            id: slot.to_string(),
            color: "#ffffff".into(),
            opacity: if *slot == "glass" { Some(0.28) } else { None },
            roughness: if *slot == "glass" { 0.18 } else { 0.8 },
            ..Material::default()
        }));
    if kinds.contains(&Kind::Timber) {
        let mut material =
            superspec_aml::material_catalog::material("builtin:shop-timber").unwrap();
        material.id = "shop-timber".into();
        *scene
            .manifest
            .materials
            .iter_mut()
            .find(|m| m.id == "shop-timber")
            .unwrap() = material;
        for (name, bytes) in [
            (
                "shop-timber.png",
                include_bytes!("../../../../assets/materials/shop-timber.png").as_slice(),
            ),
            (
                "shop-timber-normal.png",
                include_bytes!("../../../../assets/materials/shop-timber-normal.png").as_slice(),
            ),
            (
                "shop-timber-properties.png",
                include_bytes!("../../../../assets/materials/shop-timber-properties.png")
                    .as_slice(),
            ),
        ] {
            scene.manifest.assets.push(superspec_engine::Asset {
                id: format!("superspec-material:{name}"),
                mime: "image/png",
                byte_start: scene.asset_bytes.len(),
                byte_length: bytes.len(),
            });
            scene.asset_bytes.extend_from_slice(bytes);
        }
        scene.manifest.asset_byte_count = scene.asset_bytes.len();
    }
    let mut mesh = Mesh::new(Mode::Render, 20000, &|| false).unwrap();
    for (i, kind) in kinds.iter().copied().enumerate() {
        let pen = Pen::new(
            Matrix([
                [1., 0., 0.],
                [0., -1., 0.],
                [0., 0., 1.],
                [i as f64 * 2.8, 0., 0.],
            ]),
            &format!("window-{i}"),
        )
        .unwrap();
        let mut drawing = Fixture {
            pen,
            mesh: &mut mesh,
            repeats: &|_| 1.,
        };
        let look = Look {
            weathering: weathered.then(|| {
                superspec_engine::worldspec::themes::kowloon::weathered::Settings {
                    seed: "window/metal".into(),
                    wear: 0.6,
                    grime: 0.7,
                    rust: 0.4,
                    texture_density: 128.,
                    metal: superspec_engine::worldspec::themes::kowloon::metal::Binding {
                        material: "metal".into(),
                        repeat: 1.,
                    },
                    sheet: superspec_engine::worldspec::themes::kowloon::metal::Binding {
                        material: "metal".into(),
                        repeat: 1.,
                    },
                    paint: superspec_engine::worldspec::themes::kowloon::metal::Binding {
                        material: "paint".into(),
                        repeat: 1.,
                    },
                }
            }),
            kind,
            frame: [0.2, 0.5, 0.8],
            trim: [0.7, 0.6, 0.5],
            curtain: [0.3, 0.8, 0.2],
            reach: 1.5,
            hood: true,
            lit: false,
            boarded: false,
            fixed: false,
            recess: 0.3,
            transom_ratio: None,
            lower_pane_bars: None,
            timber_textured: true,
        };
        window(
            &mut drawing,
            [0.2, 2.2, 0.4, 2.1],
            &look,
            &mut SeededRng::from_text("window-fixture/0"),
        )
        .unwrap();
        if i == 2 {
            security_bars(
                &mut drawing,
                [0.2, 2.2, 0.4, 2.1],
                Guard::Bars,
                [0.1, 0.2, 0.12],
                None,
            )
            .unwrap();
        }
    }
    let mut bindings = BTreeMap::from_iter(
        slots
            .iter()
            .map(|slot| (slot.to_string(), slot.to_string())),
    );
    if weathered {
        let excluded = unmapped(&mesh);
        let materials = scene
            .manifest
            .materials
            .iter()
            .map(|m| (m.id.clone(), m.clone()))
            .collect();
        let plan =
            superspec_engine::worldspec::themes::kowloon::weathered::finish(&mut mesh, &|| false)
                .unwrap();
        let bakes = plan
            .bake(
                &superspec_engine::materialspec::surface::Inputs {
                    materials: &materials,
                    images: &BTreeMap::new(),
                },
                "window",
                &|| false,
            )
            .unwrap();
        assert_eq!(
            unmapped(&mesh),
            excluded,
            "Baking preserves glass, interior and concrete attributes"
        );
        bindings.extend(
            bakes
                .iter()
                .map(|b| (b.material().id.clone(), b.material().id.clone())),
        );
        scene
            .append_baked_visual("host", &mesh, &bindings, &bakes)
            .unwrap();
    } else {
        scene.append_visual("host", &mesh, &bindings).unwrap();
    }
    scene
}

fn unmapped(mesh: &Mesh<'_>) -> Vec<(String, Vec<[u64; 11]>)> {
    mesh.indices
        .as_chunks::<3>()
        .0
        .iter()
        .enumerate()
        .filter_map(|(t, triangle)| {
            let slot = &mesh.surfaces[mesh.triangle_surfaces[t]].slot;
            ["glass", "pane", "concrete", "shop-timber"]
                .contains(&slot.as_str())
                .then(|| {
                    (
                        slot.clone(),
                        triangle
                            .iter()
                            .map(|i| {
                                let v = mesh.vertices[*i as usize];
                                let mut values = [0u64; 11];
                                for (at, value) in v
                                    .position
                                    .into_iter()
                                    .chain(v.normal)
                                    .chain(v.uv)
                                    .chain(v.color)
                                    .enumerate()
                                {
                                    values[at] = value.to_bits();
                                }
                                values
                            })
                            .collect(),
                    )
                })
        })
        .collect()
}

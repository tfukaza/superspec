//! Complete household bays with wall receivers and shipped atlases.
use super::balcony_props_scene as props;
use std::collections::BTreeMap;
use superspec_aml::{material::Material, rng::SeededRng};
use superspec_engine::{
    Asset, Scene,
    materialspec::surface::Inputs,
    worldspec::themes::kowloon::{
        facade_cells::{self, Cell, Kind, Role},
        facade_material,
        fixture::Fixture,
        kit::Kit,
        massing::Rect,
        mesh::{Mesh, Mode},
        metal::Binding,
        palette,
        pen::Pen,
        skin, wall, weathered,
    },
};
use superspec_geometry::Matrix;
pub fn assets() -> (Scene, BTreeMap<String, Material>) {
    let (mut scene, mut materials) = props::atlas_scene();
    for id in [
        "wall",
        "concrete",
        "paint",
        "metal",
        "sheet",
        "pane",
        "interior-lit",
        "timber",
        "glass",
    ] {
        materials.insert(
            id.into(),
            Material {
                id: id.into(),
                color: "#ffffff".into(),
                repeat: 1.,
                roughness: 0.75,
                ..Material::default()
            },
        );
    }
    for id in ["shop-timber", "aircon", "tarp"] {
        let mut m = superspec_aml::material_catalog::material(&format!("builtin:{id}")).unwrap();
        m.id = id.into();
        materials.insert(id.into(), m);
    }
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
            include_bytes!("../../../../assets/materials/shop-timber-properties.png").as_slice(),
        ),
        (
            "aircon.png",
            include_bytes!("../../../../assets/materials/aircon.png").as_slice(),
        ),
        (
            "tarp.png",
            include_bytes!("../../../../assets/materials/tarp.png").as_slice(),
        ),
        (
            "tarp-normal.png",
            include_bytes!("../../../../assets/materials/tarp-normal.png").as_slice(),
        ),
        (
            "tarp-properties.png",
            include_bytes!("../../../../assets/materials/tarp-properties.png").as_slice(),
        ),
    ] {
        scene.manifest.assets.push(Asset {
            id: format!("superspec-material:{name}"),
            mime: "image/png",
            byte_start: scene.asset_bytes.len(),
            byte_length: bytes.len(),
        });
        scene.asset_bytes.extend_from_slice(bytes);
    }
    scene.manifest.asset_byte_count = scene.asset_bytes.len();
    scene.manifest.materials = materials.values().cloned().collect();
    (scene, materials)
}
pub fn scene() -> Scene {
    let (mut scene, materials) = assets();
    let mut mesh = Mesh::new(Mode::Render, 150000, &|| false).unwrap();
    for (i, (kind, group, v)) in [
        (Kind::Window, 0, 0),
        (Kind::Band, 1, 0),
        (Kind::Stair, 3, 0),
        (Kind::Balcony, 22, 4),
        (Kind::Balcony, 27, 5),
        (Kind::Balcony, 32, 6),
    ]
    .into_iter()
    .enumerate()
    {
        let style = palette::lot_style(&mut SeededRng::from_text(&format!("cell-style/{v}")));
        let mut flat = palette::flat(&style, &mut SeededRng::from_text(&format!("cell-flat/{v}")));
        if v >= 4 {
            for n in 0..1000 {
                flat = palette::flat(
                    &style,
                    &mut SeededRng::from_text(&format!("cell-flat-extra/{n}")),
                );
                if match v {
                    4 => !flat.vacant && flat.enclosure == "caged",
                    5 => !flat.vacant && flat.enclosure == "glazed",
                    _ => flat.vacant,
                } {
                    break;
                }
            }
        }
        let settings=serde_json::from_value(serde_json::json!({"weathering":{"seed":format!("cell/{group}"),"wear":0.25,"grime":0.6,"rust":0.4,"textureDensity":64,"metal":{"material":"metal","repeat":1},"sheet":{"material":"sheet","repeat":1},"paint":{"material":"paint","repeat":1}},"lettering":true,"timber":{"textured":true,"timberRepeat":1,"atlasRepeat":1}})).unwrap();
        let kit = Kit::new(settings, 2).unwrap();
        let key = format!("cell/{group}");
        let lot_key = format!("lot-{v}");
        let c = Cell {
            kind,
            role: Role::Front,
            story: 0,
            width: 3.6,
            height: 3.,
            reach: 1.3,
            balcony: 0.85,
            ground: 3.4,
            story_height: 3.,
            top: true,
            lot_key: &lot_key,
            key: &key,
            wash: style.wall,
            style: &style,
            flat: &flat,
            kit: &kit,
            plants: true,
            cloth: true,
        };
        let mut matrix = Matrix::default();
        matrix.0[3] = [(i % 3) as f64 * 4., 0., (i / 3) as f64 * 3.5];
        let d = &mut Fixture {
            pen: Pen::new(matrix, &format!("bay-{i}")).unwrap(),
            mesh: &mut mesh,
            repeats: &|s| materials[s].repeat,
        };
        let opening = c.opening().unwrap();
        let openings = opening
            .map(|o| Rect {
                u0: o.x,
                u1: o.x + o.w,
                v0: o.z,
                v1: o.z + o.h,
            })
            .into_iter()
            .collect();
        skin::draw(
            d,
            "wall",
            &skin::Settings {
                rects: vec![Rect {
                    u0: 0.,
                    u1: 3.6,
                    v0: 0.,
                    v1: 3.,
                }],
                openings,
                u_marks: opening.map(|o| vec![o.x, o.x + o.w]).unwrap_or_default(),
                z_marks: opening.map(|o| vec![o.z, o.z + o.h]).unwrap_or_default(),
                finish: facade_material::Settings {
                    concrete: false,
                    coat: facade_material::Coat {
                        material: "wall".into(),
                        repeat: 1.,
                        tint: style.wall,
                    },
                    backing: Binding {
                        material: "concrete".into(),
                        repeat: 1.,
                    },
                    wear: 0.25,
                    density: 64.,
                    variant: 1,
                },
                condition: wall::Settings {
                    base: None,
                    ledges: vec![wall::Ledge {
                        u0: 0.,
                        u1: 3.6,
                        z: 3.,
                        flow: 1.,
                    }],
                    openings: vec![],
                    grime: 0.6,
                    wear: 0.25,
                    seed: 42.,
                },
                washes: vec![],
                tiles: None,
            },
            &key,
        )
        .unwrap();
        if let Some(o) = opening {
            for (points, normal) in [
                (
                    vec![
                        [o.x, 0., o.z],
                        [o.x, -o.depth, o.z],
                        [o.x, -o.depth, o.z + o.h],
                        [o.x, 0., o.z + o.h],
                    ],
                    [1., 0., 0.],
                ),
                (
                    vec![
                        [o.x + o.w, 0., o.z],
                        [o.x + o.w, -o.depth, o.z],
                        [o.x + o.w, -o.depth, o.z + o.h],
                        [o.x + o.w, 0., o.z + o.h],
                    ],
                    [-1., 0., 0.],
                ),
                (
                    vec![
                        [o.x, 0., o.z],
                        [o.x + o.w, 0., o.z],
                        [o.x + o.w, -o.depth, o.z],
                        [o.x, -o.depth, o.z],
                    ],
                    [0., 0., 1.],
                ),
                (
                    vec![
                        [o.x, 0., o.z + o.h],
                        [o.x + o.w, 0., o.z + o.h],
                        [o.x + o.w, -o.depth, o.z + o.h],
                        [o.x, -o.depth, o.z + o.h],
                    ],
                    [0., 0., -1.],
                ),
            ] {
                d.face("wall", &points, normal, palette::shade(style.wall, 0.82))
                    .unwrap();
            }
            d.panel(
                "paint",
                [o.x, o.x + o.w, o.z, o.z + o.h],
                -o.depth,
                palette::srgb("#2a2927"),
            )
            .unwrap();
        }
        facade_cells::draw(d, &c).unwrap();
    }
    let plan = weathered::finish(&mut mesh, &|| false).unwrap();
    let inputs = Inputs {
        materials: &materials,
        images: &BTreeMap::new(),
    };
    #[cfg(not(target_arch = "wasm32"))]
    let bakes = plan
        .bake_parallel(
            &inputs,
            "cells",
            &weathered::ParallelBaking::default(),
            &|| false,
        )
        .unwrap();
    #[cfg(target_arch = "wasm32")]
    let bakes = plan.bake(&inputs, "cells", &|| false).unwrap();
    let mut bindings: BTreeMap<_, _> = materials
        .keys()
        .map(|id| (id.clone(), id.clone()))
        .collect();
    bindings.extend(
        bakes
            .iter()
            .map(|b| (b.material().id.clone(), b.material().id.clone())),
    );
    scene
        .append_baked_visual("host", &mesh, &bindings, &bakes)
        .unwrap();
    scene
}

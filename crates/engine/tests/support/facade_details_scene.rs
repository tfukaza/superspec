//! Planned trim and runoff on a finished facade, with thresholds and window openings.
use std::collections::BTreeMap;
use superspec_aml::material::Material;
use superspec_engine::{
    Scene, compile,
    materialspec::surface::Inputs,
    worldspec::themes::kowloon::{
        facade_details::{self, Request, Settings},
        facade_material,
        fixture::Fixture,
        massing::Rect,
        mesh::{Mesh, Mode},
        palette::srgb,
        pen::Pen,
        skin, wall, weathered,
    },
};
use superspec_geometry::Matrix;
pub fn scene() -> Scene {
    let mut scene=compile(r#"<architecture version="1" id="rust-facade-details"><box id="host" width="6" depth=".1" height="9.2" visible="false"/></architecture>"#,0).unwrap();
    let materials: BTreeMap<String, Material> = [
        ("wall", 1.3),
        ("concrete", 2.7),
        ("metal", 1.5),
        ("sheet", 1.5),
        ("paint", 1.),
    ]
    .into_iter()
    .map(|(id, repeat)| {
        (
            id.into(),
            Material {
                id: id.into(),
                color: "#ffffff".into(),
                repeat,
                roughness: 0.8,
                metalness: if id == "metal" { 0.3 } else { 0. },
                ..Material::default()
            },
        )
    })
    .collect();
    let settings:Settings=serde_json::from_value(serde_json::json!({"weathering":{"seed":"facade/0","wear":0.25,"grime":0.6,"rust":0.4,"textureDensity":64,"metal":{"material":"metal","repeat":1.5},"sheet":{"material":"sheet","repeat":1.5},"paint":{"material":"paint","repeat":1}},"concrete":{"material":"concrete","repeat":2.7},"detail":1,"trim":[0.8,0.77,0.65]})).unwrap();
    let request = Request {
        key: "lot/front".into(),
        ground: 2.8,
        roof: 8.8,
        top: 9.2,
        stories: 3,
        story_height: 3.,
        body_span: [0., 6.],
        walls: vec![Rect {
            u0: 0.,
            u1: 6.,
            v0: 2.8,
            v1: 9.2,
        }],
        openings: vec![
            Rect {
                u0: 1.,
                u1: 2.3,
                v0: 3.4,
                v1: 4.8,
            },
            Rect {
                u0: 3.5,
                u1: 4.8,
                v0: 6.1,
                v1: 7.8,
            },
        ],
    };
    let reach = |_: Rect, limit: f64| Ok(limit);
    let miters = |_: Rect| Ok([None; 2]);
    let plan =
        facade_details::plan(&request, &settings.weathering.seed, &reach, &|| false).unwrap();
    let mut mesh = Mesh::new(Mode::Render, 150000, &|| false).unwrap();
    let mut d = Fixture {
        pen: Pen::new(Matrix::default(), "facade").unwrap(),
        mesh: &mut mesh,
        repeats: &|s| materials[s].repeat,
    };
    let mut openings = request.openings.clone();
    openings.push(Rect {
        u0: 2.5,
        u1: 3.5,
        v0: 0.,
        v1: 2.2,
    });
    let mut ledges = plan.ledges.clone();
    ledges.push(wall::Ledge {
        u0: 0.,
        u1: 6.,
        z: 9.2,
        flow: 0.8,
    });
    skin::draw(
        &mut d,
        "wall",
        &skin::Settings {
            rects: vec![Rect {
                u0: 0.,
                u1: 6.,
                v0: 0.,
                v1: 9.2,
            }],
            openings: openings.clone(),
            u_marks: openings.iter().flat_map(|r| [r.u0, r.u1]).collect(),
            z_marks: openings.iter().flat_map(|r| [r.v0, r.v1]).collect(),
            finish: facade_material::Settings {
                concrete: false,
                coat: facade_material::Coat {
                    material: "wall".into(),
                    repeat: 1.3,
                    tint: srgb("#d6e4d4"),
                },
                backing: settings.concrete.clone(),
                wear: 0.25,
                density: 64.,
                variant: 1,
            },
            condition: wall::Settings {
                base: Some(0.),
                ledges,
                openings: openings
                    .iter()
                    .map(|r| wall::Opening {
                        u0: r.u0,
                        u1: r.u1,
                        z0: r.v0,
                        z1: r.v1,
                    })
                    .collect(),
                grime: 0.6,
                wear: 0.25,
                seed: 8173.,
            },
            washes: vec![],
            tiles: None,
        },
        "envelope",
    )
    .unwrap();
    facade_details::draw(&mut d, &plan, &settings, &request.key, &miters).unwrap();
    facade_details::plinth(
        &mut d,
        &[Rect {
            u0: 0.,
            u1: 6.,
            v0: 0.,
            v1: 2.8,
        }],
        &[[2.5, 3.5]],
        2.8,
        &settings,
        &request.key,
        &reach,
        &miters,
    )
    .unwrap();
    let plan = weathered::finish(&mut mesh, &|| false).unwrap();
    let inputs = Inputs {
        materials: &materials,
        images: &BTreeMap::new(),
    };
    #[cfg(not(target_arch = "wasm32"))]
    let bakes = plan
        .bake_parallel(
            &inputs,
            "facade-details",
            &weathered::ParallelBaking::default(),
            &|| false,
        )
        .unwrap();
    #[cfg(target_arch = "wasm32")]
    let bakes = plan.bake(&inputs, "facade-details", &|| false).unwrap();
    scene.manifest.materials = materials.values().cloned().collect();
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

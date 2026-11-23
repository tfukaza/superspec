use serde_json::json;
use std::{collections::BTreeMap, sync::Arc};
use superspec_engine::Scene;
use superspec_engine::{
    authoring::{Project, Session},
    worldspec::authored::{Assignment, Placement, World},
};

fn assets(name: &str) -> Result<Arc<[u8]>, String> {
    Err(format!("Unexpected asset {name}"))
}
fn transform(x: f64) -> [[f64; 3]; 4] {
    [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.], [x, 0., 0.2]]
}
fn project() -> Project {
    let item = json!({"format":"itemspec","version":1,"id":"saved-pipe","parameters":{
        "length":{"type":"number","role":"geometry","default":1.,"min":0.1,"max":10.},
        "paint":{"type":"color","role":"finish","default":[0.,1.,0.]},
        "roughness":{"type":"number","role":"finish","default":0.2,"min":0.,"max":1.}
    },"generator":{"kind":"pipe","length":{"parameter":"length"},"radius":0.1,"wallThickness":0.02,"segments":8,"capped":true},
    "slots":{"surface":{"entry":"paint.ms","parameters":{"tint":{"parameter":"paint"},"roughness":{"parameter":"roughness"}}}},"collision":"mesh"});
    let modules=BTreeMap::from([
        ("scene.aml".into(),"<architecture version=\"1\" id=\"authored-world\"><box id=\"slab\" width=\"4\" depth=\"4\" height=\"0.2\"/></architecture>".into()),
        ("pipe.item.json".into(),serde_json::to_string_pretty(&item).unwrap()),
        ("paint.ms".into(),"material 1; param tint: vec3 = vec3(1,1,1); param roughness: scalar = 0.5; output base_color = tint; output roughness = roughness;".into()),
    ]);
    let world = World {
        items: vec![
            Placement {
                id: "left".into(),
                entry: "pipe.item.json".into(),
                parameters: BTreeMap::from([
                    ("length".into(), json!(3.)),
                    ("roughness".into(), json!(0.6)),
                ]),
                slots: BTreeMap::from([(
                    "surface".into(),
                    superspec_engine::materialspec::program::MaterialRequest {
                        entry: "paint.ms".into(),
                        parameters: BTreeMap::from([
                            ("tint".into(), json!([1., 0., 0.])),
                            ("roughness".into(), json!(0.9)),
                        ]),
                        seed: 0,
                    },
                )]),
                transform: transform(-1.),
            },
            Placement {
                id: "right".into(),
                entry: "pipe.item.json".into(),
                parameters: BTreeMap::new(),
                slots: BTreeMap::new(),
                transform: transform(1.),
            },
        ],
        materials: vec![Assignment {
            objects: vec!["slab".into()],
            entry: "paint.ms".into(),
            parameters: BTreeMap::from([
                ("tint".into(), json!([0.5, 0.5, 0.5])),
                ("roughness".into(), json!(0.8)),
            ]),
            seed: 0,
            mapping: Default::default(),
        }],
        theme_presets: BTreeMap::from([(
            "pipe.item.json".into(),
            BTreeMap::from([
                ("length".into(), json!(1.5)),
                ("paint".into(), json!([0., 1., 0.])),
                ("roughness".into(), json!(0.3)),
            ]),
        )]),
        project_presets: BTreeMap::from([(
            "pipe.item.json".into(),
            BTreeMap::from([
                ("length".into(), json!(2.)),
                ("paint".into(), json!([0., 0., 1.])),
                ("roughness".into(), json!(0.4)),
            ]),
        )]),
        ..Default::default()
    };
    Project {
        kind: "world".into(),
        entry: "scene.aml".into(),
        modules,
        resolution: 16,
        world: Some(world),
        ..Default::default()
    }
}

#[test]
fn world_graphics_labels_follow_object_identity_into_visual_exports() {
    let mut project = project();
    project.world.as_mut().unwrap().render_visibility =
        serde_json::from_value(json!({"item/left": "low", "slab": "medium"})).unwrap();
    let built = Session::default()
        .build_project(&project, 1, &assets, &|| false)
        .unwrap();
    let scene = built.model.scene(1).unwrap();
    assert!(scene.manifest.colliders.iter().any(|id| id.starts_with("item/left/")));
    let glb = superspec_export::glb(&scene, &Default::default()).unwrap();
    let length = u32::from_le_bytes(glb[12..16].try_into().unwrap()) as usize;
    let document: serde_json::Value = serde_json::from_slice(&glb[20..20 + length]).unwrap();
    let nodes = document["nodes"].as_array().unwrap();
    assert!(nodes.iter().any(|node| node["extras"]["objectId"] == "item/left"
        && node["extras"]["worldSpecMinGraphics"] == "low"));
    assert!(nodes.iter().any(|node| node["extras"]["objectId"] == "item/right"
        && node["extras"]["worldSpecMinGraphics"].is_null()));
}
fn physical(scene: &Scene) -> BTreeMap<String, Vec<[f32; 3]>> {
    scene
        .manifest
        .colliders
        .iter()
        .map(|id| {
            let mesh = scene
                .manifest
                .meshes
                .iter()
                .find(|mesh| &mesh.id == id)
                .unwrap();
            let points = scene.indices[mesh.index_start..mesh.index_start + mesh.index_count]
                .iter()
                .map(|index| {
                    let start = (mesh.vertex_start + *index as usize) * 3;
                    scene.positions[start..start + 3].try_into().unwrap()
                })
                .collect();
            (id.clone(), points)
        })
        .collect()
}
fn texture(scene: &Scene, object: &str, properties: bool) -> Vec<u8> {
    let mesh = scene
        .manifest
        .meshes
        .iter()
        .find(|mesh| mesh.visible && mesh.object_id == object)
        .unwrap();
    let material = scene
        .manifest
        .materials
        .iter()
        .find(|material| Some(&material.id) == mesh.material.as_ref())
        .unwrap();
    let name = if properties {
        material.metallic_roughness_texture.as_ref().unwrap()
    } else {
        material.texture.as_ref().unwrap()
    };
    let asset = scene
        .manifest
        .assets
        .iter()
        .find(|asset| &asset.id == name)
        .unwrap();
    superspec_geometry::image::decode(
        &scene.asset_bytes[asset.byte_start..asset.byte_start + asset.byte_length],
    )
    .unwrap()
    .rgba
}
fn bounds(scene: &Scene, object: &str) -> [[f64; 3]; 2] {
    let mut bounds = [[f64::INFINITY; 3], [f64::NEG_INFINITY; 3]];
    for mesh in scene
        .manifest
        .meshes
        .iter()
        .filter(|mesh| mesh.visible && mesh.object_id == object)
    {
        for axis in 0..3 {
            bounds[0][axis] = bounds[0][axis].min(mesh.bounds[0][axis]);
            bounds[1][axis] = bounds[1][axis].max(mesh.bounds[1][axis]);
        }
    }
    bounds
}

#[test]
fn saved_items_presets_finishes_collision_and_export_share_the_world_model() {
    let mut project = project();
    let original_modules = project.modules.clone();
    let mut session = Session::default();
    let built = session
        .build_project(&project, 7, &assets, &|| false)
        .unwrap();
    let delivered = built.scene(7).unwrap();
    let scene = &delivered;
    for (id, length, x) in [("item/left", 3., -1.), ("item/right", 2., 1.)] {
        let bounds = bounds(scene, id);
        assert!((bounds[1][2] - (0.2 + length)).abs() < 1e-6);
        assert!((bounds[0][0] - (x - 0.1)).abs() < 1e-6);
        assert!(
            scene
                .manifest
                .architecture
                .as_ref()
                .unwrap()
                .objects
                .iter()
                .any(|object| object.id == id)
        );
        assert!(
            scene
                .manifest
                .colliders
                .iter()
                .any(|collider| collider.starts_with(&format!("{id}/")))
        );
    }
    assert!(
        texture(scene, "item/left", false)
            .chunks_exact(4)
            .any(|p| p == [255, 0, 0, 255])
    );
    assert!(
        texture(scene, "item/right", false)
            .chunks_exact(4)
            .any(|p| p == [0, 0, 255, 255])
    );
    assert!(
        texture(scene, "item/left", true)
            .chunks_exact(4)
            .any(|p| p[1] == 230)
    );
    assert!(
        texture(scene, "item/right", true)
            .chunks_exact(4)
            .any(|p| p[1] == 102)
    );
    assert!(
        texture(scene, "slab", false)
            .chunks_exact(4)
            .any(|p| p == [188, 188, 188, 255])
    );
    let structural = superspec_engine::compile(&project.modules["scene.aml"], 7).unwrap();
    let physical_before = physical(scene);
    let structural_physics = physical(&structural);
    assert!(!structural_physics.is_empty());
    for (id, triangles) in structural_physics {
        assert_eq!(physical_before.get(&id), Some(&triangles));
    }
    let unchanged = scene.packet();
    let exported = superspec_export::glb(scene, &Default::default()).unwrap();
    assert_eq!(&exported[..4], b"glTF");
    assert_eq!(scene.packet(), unchanged);
    assert_eq!(project.modules, original_modules);
    let world = project.world.as_mut().unwrap();
    world.items[1]
        .parameters
        .insert("paint".into(), json!([1., 1., 0.]));
    world.materials[0]
        .parameters
        .insert("tint".into(), json!([0.2, 0.3, 0.4]));
    let edited = session
        .build_project(&project, 8, &assets, &|| false)
        .unwrap();
    let edited_scene = edited.scene(8).unwrap();
    assert_eq!(physical(&edited_scene), physical_before);
    assert!(
        texture(&edited_scene, "slab", false)
            .chunks_exact(4)
            .any(|p| p == [124, 149, 170, 255])
    );
    assert!(
        texture(&edited_scene, "item/right", false)
            .chunks_exact(4)
            .any(|p| p == [255, 255, 0, 255])
    );
    let fresh = Session::default()
        .build_project(&project, 8, &assets, &|| false)
        .unwrap();
    assert_eq!(edited_scene.packet(), fresh.scene(8).unwrap().packet());
    assert_eq!(project.modules, original_modules);
}

#[test]
fn bad_world_overrides_and_slot_replacements_fail_then_recover() {
    let mut project = project();
    let mut session = Session::default();
    project.world.as_mut().unwrap().items[1]
        .parameters
        .insert("length".into(), json!(-1.));
    let errors = session
        .build_project(&project, 1, &assets, &|| false)
        .err()
        .unwrap();
    assert_eq!(errors[0].source_name, "pipe.item.json");
    assert_eq!(errors[0].code, "ITEM_PARAMETER");
    assert!(
        errors[0].line > 1,
        "Saved item diagnostics must point into the original authored source."
    );
    project.world.as_mut().unwrap().items[1].parameters.clear();
    let binding = project.world.as_ref().unwrap().items[0].slots["surface"].clone();
    project.world.as_mut().unwrap().items[1]
        .slots
        .insert("unknown".into(), binding);
    let errors = session
        .build_project(&project, 2, &assets, &|| false)
        .err()
        .unwrap();
    assert_eq!(errors[0].code, "ITEM_MATERIAL");
    project.world.as_mut().unwrap().items[1].slots.clear();
    assert!(
        session
            .build_project(&project, 3, &assets, &|| false)
            .is_ok()
    );
}

fn premade(normal: [u8; 4]) -> Arc<[u8]> {
    let png = superspec_geometry::image::encode(&superspec_geometry::image::Image {
        width: 1,
        height: 1,
        rgba: normal.to_vec(),
        mime: "image/png",
    })
    .unwrap();
    let mut bytes = Vec::new();
    for value in [0f32, 0., 0., 1., 0., 0., 0., 1., 0., 0., 0., 1., 0., 0., 1.] {
        bytes.extend(value.to_le_bytes());
    }
    let image_start = bytes.len();
    bytes.extend(png);
    let mut document=serde_json::to_vec(&json!({"asset":{"version":"2.0"},"buffers":[{"byteLength":bytes.len()}],
        "bufferViews":[{"buffer":0,"byteOffset":0,"byteLength":36},{"buffer":0,"byteOffset":36,"byteLength":24},{"buffer":0,"byteOffset":image_start,"byteLength":bytes.len()-image_start}],
        "accessors":[{"bufferView":0,"componentType":5126,"count":3,"type":"VEC3"},{"bufferView":1,"componentType":5126,"count":3,"type":"VEC2"}],
        "images":[{"bufferView":2,"mimeType":"image/png"}],"textures":[{"source":0}],
        "materials":[{"pbrMetallicRoughness":{"baseColorTexture":{"index":0}},"normalTexture":{"index":0}}],
        "meshes":[{"primitives":[{"attributes":{"POSITION":0,"TEXCOORD_0":1},"material":0}]}],"nodes":[{"mesh":0}],"scenes":[{"nodes":[0]}]})).unwrap();
    while !document.len().is_multiple_of(4) {
        document.push(b' ');
    }
    while !bytes.len().is_multiple_of(4) {
        bytes.push(0);
    }
    let mut glb = Vec::new();
    glb.extend(b"glTF");
    glb.extend(2u32.to_le_bytes());
    glb.extend(((28 + document.len() + bytes.len()) as u32).to_le_bytes());
    glb.extend((document.len() as u32).to_le_bytes());
    glb.extend(b"JSON");
    glb.extend(document);
    glb.extend((bytes.len() as u32).to_le_bytes());
    glb.extend(b"BIN\0");
    glb.extend(bytes);
    glb.into()
}

#[test]
fn premade_resource_names_are_independent_and_repeated_images_count_once() {
    let mut project = project();
    for name in ["a", "b"] {
        project.modules.insert(format!("{name}.item.json"),json!({"format":"itemspec","version":1,"id":name,"generator":{"kind":"glb","asset":format!("{name}.glb")}}).to_string());
    }
    project.world = Some(World {
        items: [("a", "a", -1.), ("b", "b", 1.), ("again", "a", 2.)]
            .into_iter()
            .map(|(id, entry, x)| Placement {
                id: id.into(),
                entry: format!("{entry}.item.json"),
                parameters: BTreeMap::new(),
                slots: BTreeMap::new(),
                transform: transform(x),
            })
            .collect(),
        ..Default::default()
    });
    let bytes = BTreeMap::from([
        ("a.glb", premade([128, 255, 128, 255])),
        ("b.glb", premade([255, 128, 128, 255])),
    ]);
    let built = Session::default()
        .build_project(
            &project,
            1,
            &|name| {
                bytes
                    .get(name)
                    .cloned()
                    .ok_or_else(|| format!("Missing {name}"))
            },
            &|| false,
        )
        .unwrap();
    let layers = built.model.layers();
    let (
        superspec_model::composition::Layer::Append(first),
        superspec_model::composition::Layer::Append(repeated),
    ) = (&layers[0], &layers[2])
    else {
        panic!("Expected retained item placements")
    };
    let first_instance = &first.instances[0];
    let repeated_instance = &repeated.instances[0];
    assert!(
        Arc::ptr_eq(
            &first.meshes[&first_instance.mesh],
            &repeated.meshes[&repeated_instance.mesh]
        ),
        "Repeated imported items retain one immutable local mesh across world assembly."
    );
    assert_ne!(first_instance.transform, repeated_instance.transform);
    assert_ne!(first_instance.object_id, repeated_instance.object_id);
    let local_positions = first.meshes[&first_instance.mesh].positions.clone();
    let delivered = built.scene(1).unwrap();
    let scene = &delivered;
    assert_eq!(scene.manifest.assets.len(), 2);
    assert_eq!(scene.image_pixels(), 2);
    let binding = |object: &str| {
        scene
            .manifest
            .meshes
            .iter()
            .find(|mesh| mesh.object_id == object)
            .unwrap()
            .material
            .clone()
    };
    assert_eq!(binding("item/a"), binding("item/again"));
    assert_ne!(binding("item/a"), binding("item/b"));
    assert_eq!(scene.manifest.mesh_tangents.len(), 3);
    for values in scene.manifest.mesh_tangents.values() {
        assert_eq!(values, &vec![[1., 0., 0., 1.]; 3]);
    }
    let before = scene.packet();
    let exported = superspec_export::glb(scene, &Default::default()).unwrap();
    let length = u32::from_le_bytes(exported[12..16].try_into().unwrap()) as usize;
    let gltf: serde_json::Value = serde_json::from_slice(&exported[20..20 + length]).unwrap();
    assert_eq!(
        gltf["meshes"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|m| m["primitives"].as_array().unwrap())
            .filter(|p| p["attributes"]["TANGENT"].is_number())
            .count(),
        3
    );
    assert_eq!(scene.packet(), before);
    assert_eq!(
        superspec_export::world_glb(&built.model, 1, &Default::default()).unwrap(),
        exported
    );
    assert_eq!(
        first.meshes[&first_instance.mesh].positions,
        local_positions
    );
    let mut independent = built.scene(1).unwrap();
    independent.positions[0] = 123.;
    assert_eq!(built.scene(1).unwrap().packet(), before);
    assert!(
        built
            .model
            .scene_cancellable(1, &|| true)
            .unwrap_err()
            .contains("cancelled")
    );
    assert_eq!(built.scene(1).unwrap().packet(), before);
}

#[test]
fn procedural_world_keeps_shared_local_meshes_and_separate_placement_collision() {
    let mut project = project();
    project.modules.insert("pipe.item.json".into(), json!({"format":"itemspec","version":1,"id":"pipe","generator":{"kind":"pipe","length":1.,"radius":0.1,"segments":8},"collision":"mesh"}).to_string());
    let world = project.world.as_mut().unwrap();
    world.theme_presets.clear();
    world.project_presets.clear();
    world.materials.clear();
    for item in &mut world.items {
        item.parameters.clear();
        item.slots.clear();
    }
    let output = Session::default()
        .build_project(&project, 2, &assets, &|| false)
        .unwrap();
    let layers = output.model.layers();
    let (
        superspec_model::composition::Layer::Append(left),
        superspec_model::composition::Layer::Append(right),
    ) = (&layers[0], &layers[1])
    else {
        panic!("Expected retained procedural instances")
    };
    let a = &left.instances[0];
    let b = &right.instances[0];
    assert!(Arc::ptr_eq(&left.meshes[&a.mesh], &right.meshes[&b.mesh]));
    assert!(a.collidable && b.collidable);
    assert_ne!(a.transform, b.transform);
    let base = output.model.base_scene().unwrap();
    assert_eq!(
        base.manifest
            .colliders
            .iter()
            .filter(|id| id.starts_with("item/"))
            .count(),
        0
    );
    let scene = output.scene(2).unwrap();
    for id in ["item/left", "item/right"] {
        assert_eq!(
            scene
                .manifest
                .colliders
                .iter()
                .filter(|collider| collider.starts_with(&format!("{id}/")))
                .count(),
            1
        );
        let extent = bounds(&scene, id);
        assert!((extent[1][2] - extent[0][2] - 1.).abs() < 1e-6);
    }
    assert_eq!(scene.manifest.geometry_validation, "not-run");
    assert_eq!(scene.manifest.navigation_validation, "not-run");
    assert_eq!(
        superspec_export::world_glb(&output.model, 2, &Default::default()).unwrap(),
        superspec_export::glb(&scene, &Default::default()).unwrap()
    );
}

#[test]
fn cancellation_during_physical_source_loading_prevents_world_publication_and_recovers() {
    let mut project = project();
    project.modules.insert("scene.aml".into(),r#"<architecture version="1" id="cancel-world"><material id="paint" texture="color.png"/><box id="box" width="1" depth="1" height="1" material="paint"/></architecture>"#.into());
    project.world = Some(World::default());
    let bytes: Arc<[u8]> = superspec_geometry::image::encode(&superspec_geometry::image::Image {
        width: 1,
        height: 1,
        rgba: vec![10, 20, 30, 255],
        mime: "image/png",
    })
    .unwrap()
    .into();
    let cancelled = std::cell::Cell::new(false);
    let mut session = Session::default();
    let error = session
        .build_project(
            &project,
            1,
            &|name| {
                assert_eq!(name, "color.png");
                cancelled.set(true);
                Ok(bytes.clone())
            },
            &|| cancelled.get(),
        )
        .err()
        .expect("Cancelled AML-only world must not publish");
    assert!(
        cancelled.get(),
        "The actual source reader must have run: {error:?}"
    );
    assert!(error[0].message.contains("cancelled"), "{error:?}");
    cancelled.set(false);
    let recovered = session
        .build_project(&project, 2, &|_| Ok(bytes.clone()), &|| cancelled.get())
        .unwrap();
    let fresh = Session::default()
        .build_project(&project, 2, &|_| Ok(bytes.clone()), &|| false)
        .unwrap();
    assert_eq!(
        recovered.scene(2).unwrap().packet(),
        fresh.scene(2).unwrap().packet()
    );
}

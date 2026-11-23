use serde_json::{Value, json};
use std::{cell::Cell, collections::BTreeMap, sync::Arc};
use superspec_engine::itemspec::definition::{
    self as item, Collision, CollisionIntent, CollisionShape, Values,
};
use superspec_geometry::{image, math::unit};

fn absent(name: &str) -> Result<Arc<[u8]>, String> {
    Err(format!("Missing {name}"))
}
fn pipe() -> Value {
    json!({"format":"itemspec","version":1,"id":"pipe","parameters":{
    "length":{"type":"number","role":"geometry","default":2.,"min":0.01,"max":100.},
    "roughness":{"type":"number","role":"finish","default":0.4,"min":0.,"max":1.}
},"generator":{"kind":"pipe","length":{"parameter":"length"},"radius":0.5,"wallThickness":0.1,"segments":16,"capped":true},
"slots":{"surface":{"entry":"materials/metal.ms","parameters":{"roughness":{"parameter":"roughness"}},"seed":7}},
"attachments":[{"name":"bracket","position":[1.,0.,{"parameter":"length"}],"direction":[1.,0.,0.]}],"collision":"mesh"})
}
fn definition(value: &Value) -> item::ItemDefinition {
    item::parse("test.item.json", &value.to_string()).unwrap()
}
fn approximate(a: [f64; 3], b: [f64; 3]) {
    for axis in 0..3 {
        assert!((a[axis] - b[axis]).abs() < 1e-6, "{a:?} != {b:?}");
    }
}

#[test]
fn procedural_controls_separate_finish_identity_and_refresh_metadata() {
    let mut value = pipe();
    value["transform"] = json!([[1., 0., 0.], [0., 1., 0.], [0., 0., 1.], [3., 4., 5.]]);
    let definition = definition(&value);
    let cold = item::generate(&definition, &Values::new(), &absent, &|| false).unwrap();
    assert_eq!(cold.geometry.bounds, [[2.5, 3.5, 5.], [3.5, 4.5, 7.]]);
    assert!(matches!(
        cold.collision,
        Collision::Intent(CollisionIntent::Mesh)
    ));
    assert_eq!(
        cold.attachments
            .iter()
            .find(|a| a.name == "end")
            .unwrap()
            .position,
        [3., 4., 7.]
    );
    assert_eq!(
        cold.attachments
            .iter()
            .find(|a| a.name == "bracket")
            .unwrap()
            .position,
        [4., 4., 7.]
    );
    let mesh = &cold.geometry.meshes[0];
    assert!(mesh.uvs.iter().any(|uv| uv[0] > 3.));
    assert!(mesh.normals.iter().any(|n| n[0] > 0.3 && n[1] > 0.3));
    let parameters = BTreeMap::from([("roughness".into(), json!(0.9))]);
    let prepared = item::prepare(&definition, &parameters, &absent, &|| false).unwrap();
    assert_eq!(prepared.geometry_key, cold.geometry_key);
    let warm = prepared.reuse(&cold).unwrap();
    assert!(Arc::ptr_eq(&cold.geometry, &warm.geometry));
    assert_eq!(warm.materials["surface"].parameters["roughness"], 0.9);
    assert_eq!(warm.provenance.parameters["roughness"], 0.9);
    let fresh = prepared.build(&|| false).unwrap();
    assert_eq!(
        serde_json::to_value(warm).unwrap(),
        serde_json::to_value(fresh).unwrap()
    );
    let longer = item::prepare(
        &definition,
        &BTreeMap::from([("length".into(), json!(3.))]),
        &absent,
        &|| false,
    )
    .unwrap();
    assert_ne!(longer.geometry_key, cold.geometry_key);
    assert_eq!(longer.reuse(&cold).unwrap_err().code, "ITEM_IDENTITY");
    let mut assignment = cold.materials.clone();
    assignment.get_mut("surface").unwrap().seed = 42;
    let rebound = cold.with_material_overrides(&assignment).unwrap();
    assert!(Arc::ptr_eq(&rebound.geometry, &cold.geometry));
    assignment.insert("missing".into(), assignment["surface"].clone());
    assert_eq!(
        cold.with_material_overrides(&assignment).unwrap_err().code,
        "ITEM_MATERIAL"
    );
}

#[test]
fn duct_collision_controls_validation_and_cancel_recovery() {
    let mut value = pipe();
    value["generator"] = json!({"kind":"duct","length":2.,"width":4.,"height":1.,"wallThickness":0.1,"capped":false});
    value["collision"] = json!({"kind":"box","center":[0.,0.,1.],"size":[4.,1.,2.]});
    let definition = definition(&value);
    let generated = item::generate(&definition, &Values::new(), &absent, &|| false).unwrap();
    assert_eq!(generated.geometry.bounds, [[-2., -0.5, 0.], [2., 0.5, 2.]]);
    assert_eq!(generated.geometry.meshes[0].indices.len() / 3, 16);
    assert!(matches!(
        generated.collision,
        Collision::Shape(CollisionShape::Box {
            size: [4., 1., 2.],
            ..
        })
    ));
    assert_eq!(
        item::generate(&definition, &Values::new(), &absent, &|| true)
            .unwrap_err()
            .code,
        "ITEM_CANCELLED"
    );
    let prepared = item::prepare(&definition, &Values::new(), &absent, &|| false).unwrap();
    assert_eq!(prepared.build(&|| true).unwrap_err().code, "ITEM_CANCELLED");
    assert_eq!(
        prepared.build(&|| false).unwrap().geometry.bounds,
        generated.geometry.bounds
    );
    value["generator"]["length"] = json!({"parameter":"roughness"});
    assert_eq!(
        item::generate(&self::definition(&value), &Values::new(), &absent, &|| {
            false
        })
        .unwrap_err()
        .code,
        "ITEM_PARAMETER"
    );
    value["generator"]["length"] = json!(-1.);
    assert_eq!(
        item::generate(&self::definition(&value), &Values::new(), &absent, &|| {
            false
        })
        .unwrap_err()
        .code,
        "ITEM_GEOMETRY"
    );
    value["generator"]["length"] = json!(2.);
    value["generator"]["wallThickness"] = json!(0.5);
    assert!(
        item::generate(&self::definition(&value), &Values::new(), &absent, &|| {
            false
        })
        .is_err()
    );
}

fn view(document: &mut Value, bytes: &mut Vec<u8>, data: &[u8]) -> usize {
    while !bytes.len().is_multiple_of(4) {
        bytes.push(0);
    }
    let start = bytes.len();
    bytes.extend(data);
    let views = document["bufferViews"].as_array_mut().unwrap();
    let index = views.len();
    views.push(json!({"buffer":0,"byteOffset":start,"byteLength":data.len()}));
    index
}
fn floats(values: &[f32]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_le_bytes()).collect()
}
fn fixture() -> (Value, Vec<u8>, Vec<u8>) {
    let mut document = json!({"asset":{"version":"2.0"},"bufferViews":[],"buffers":[{"byteLength":0}],
        "accessors":[
            {"bufferView":0,"byteOffset":0,"componentType":5126,"count":3,"type":"VEC3"},
            {"bufferView":0,"byteOffset":12,"componentType":5126,"count":3,"type":"VEC3"},
            {"bufferView":0,"byteOffset":24,"componentType":5123,"normalized":true,"count":3,"type":"VEC2"},
            {"bufferView":1,"componentType":5126,"count":3,"type":"VEC4"},
            {"bufferView":2,"componentType":5121,"normalized":true,"count":3,"type":"VEC4"},
            {"bufferView":3,"componentType":5123,"count":3,"type":"SCALAR"}
        ],"meshes":[{"primitives":[{"attributes":{"POSITION":0,"NORMAL":1,"TEXCOORD_0":2,"TEXCOORD_1":2,"TANGENT":3,"COLOR_0":4},"indices":5,"material":0}]}],
        "nodes":[{"name":"parent","translation":[3.,4.,5.],"scale":[2.,3.,4.],"children":[1],"extras":{"attachment":"wall"}}, {"mesh":0,"translation":[1.,0.,0.],"scale":[-1.,1.,1.]},{"mesh":0}],"scenes":[{"nodes":[0,2]}],"scene":0,
        "images":[{"bufferView":4,"mimeType":"image/png"}],"textures":[{"source":0}],
        "materials":[{"name":"finish","pbrMetallicRoughness":{"baseColorFactor":[0.12345,0.23456,0.34567,0.4],"metallicFactor":0.6,"roughnessFactor":0.7,"baseColorTexture":{"index":0},"metallicRoughnessTexture":{"index":0}},"normalTexture":{"index":0,"scale":0.8},"occlusionTexture":{"index":0,"strength":0.2},"emissiveTexture":{"index":0},"emissiveFactor":[0.,0.,0.],"alphaMode":"MASK","alphaCutoff":0.35,"doubleSided":true}]
    });
    let mut interleaved = Vec::new();
    for (position, uv) in [
        ([0., 0., 0.], [0u16, 0]),
        ([1., 0., 0.], [65535, 0]),
        ([0., 1., 0.], [0, 65535]),
    ] {
        interleaved.extend(floats(&position));
        interleaved.extend(floats(&[1., 1., 1.]));
        for v in uv {
            interleaved.extend(v.to_le_bytes());
        }
        interleaved.extend([0; 4]);
    }
    let mut bytes = Vec::new();
    view(&mut document, &mut bytes, &interleaved);
    document["bufferViews"][0]["byteStride"] = json!(32);
    view(
        &mut document,
        &mut bytes,
        &floats(&[1., 0., 0., 1., 1., 0., 0., 1., 1., 0., 0., 1.]),
    );
    view(
        &mut document,
        &mut bytes,
        &[255, 128, 0, 255, 0, 255, 128, 255, 128, 0, 255, 255],
    );
    view(&mut document, &mut bytes, &[0, 0, 1, 0, 2, 0]);
    let png = image::encode(&image::Image {
        width: 1,
        height: 1,
        rgba: vec![10, 20, 30, 40],
        mime: "image/png",
    })
    .unwrap();
    view(&mut document, &mut bytes, &png);
    (document, bytes, png)
}
fn glb(mut document: Value, mut bytes: Vec<u8>) -> Arc<[u8]> {
    document["buffers"][0]["byteLength"] = json!(bytes.len());
    while !bytes.len().is_multiple_of(4) {
        bytes.push(0);
    }
    let mut json = serde_json::to_vec(&document).unwrap();
    while !json.len().is_multiple_of(4) {
        json.push(b' ');
    }
    let mut result = Vec::new();
    result.extend(b"glTF");
    result.extend(2u32.to_le_bytes());
    result.extend(((28 + json.len() + bytes.len()) as u32).to_le_bytes());
    result.extend((json.len() as u32).to_le_bytes());
    result.extend(b"JSON");
    result.extend(json);
    result.extend((bytes.len() as u32).to_le_bytes());
    result.extend(b"BIN\0");
    result.extend(bytes);
    result.into()
}
fn imported(bytes: Arc<[u8]>) -> Result<item::GeneratedItem, item::ItemDiagnostic> {
    item::generate(
        &definition(
            &json!({"format":"itemspec","version":1,"id":"fixture","generator":{"kind":"glb","asset":"models/input.glb"},"collision":"bounds"}),
        ),
        &Values::new(),
        &|name| {
            if name == "models/input.glb" {
                Ok(bytes.clone())
            } else {
                absent(name)
            }
        },
        &|| false,
    )
}

#[test]
fn nested_glb_converts_uv_origin_once_when_adopted() {
    let (document, bytes, _) = fixture();
    let glb = glb(document, bytes);
    let paint = json!({"format":"itemspec","version":1,"id":"paint","generator":{"kind":"glb","asset":"paint.glb"}});
    let assembly = |entry| {
        json!({"format":"itemspec","version":2,"id":"assembly","parts":[
            {"id":"inner","generator":{"kind":"item","entry":entry}}
        ]})
    };
    let generated = item::generate(
        &definition(&assembly("nested.item.json")),
        &Values::new(),
        &|name| match name {
            "nested.item.json" => Ok(Arc::from(
                assembly("paint.item.json").to_string().into_bytes(),
            )),
            "paint.item.json" => Ok(Arc::from(paint.to_string().into_bytes())),
            "paint.glb" => Ok(glb.clone()),
            _ => absent(name),
        },
        &|| false,
    )
    .unwrap();
    for mesh in &generated.geometry.meshes {
        assert_eq!(mesh.uvs, [[0., 1.], [1., 1.], [0., 0.]]);
        assert_eq!(mesh.additional_uvs[&1], mesh.uvs);
    }
}

#[test]
fn glb_retains_shared_local_meshes_hierarchy_channels_and_affine_normals() {
    let (document, bytes, png) = fixture();
    let generated = imported(glb(document, bytes)).unwrap();
    assert_eq!(generated.geometry.meshes.len(), 1);
    assert_eq!(generated.geometry.nodes.len(), 3);
    assert_eq!(generated.geometry.nodes[1].parent, Some(0));
    assert_eq!(generated.geometry.nodes[2].parent, None);
    assert_eq!(
        generated.geometry.nodes[0].extras,
        Some(json!({"attachment":"wall"}))
    );
    let local = &generated.geometry.meshes[0];
    assert_eq!(local.positions[2], [0., 0., 1.]);
    assert_eq!(local.uvs, vec![[0., 0.], [1., 0.], [0., 1.]]);
    assert_eq!(local.additional_uvs[&1], local.uvs);
    assert_eq!(local.colors[0], [1., 128. / 255., 0., 1.]);
    assert_eq!(local.material_slot, "material/0");
    assert_eq!(&*generated.geometry.images[0].bytes, png);
    let material = &generated.geometry.imported_materials["material/0"];
    assert_eq!(material.textures.len(), 5);
    material.ensure_scene_compatible().unwrap();
    let metadata = material.material();
    metadata.validate().unwrap();
    assert_eq!(
        metadata.base_color_factor,
        Some([0.12345, 0.23456, 0.34567])
    );
    assert_eq!(metadata.emissive_factor, Some([0.; 3]));
    assert_eq!(metadata.opacity, Some(0.4));
    assert_eq!(metadata.occlusion_strength, Some(0.2));
    assert_eq!(metadata.normal_scale, Some(0.8));
    let flattened = generated.flattened().unwrap();
    assert_eq!(flattened.len(), 2);
    approximate(flattened[0].positions[0], [5., -5., 4.]);
    approximate(flattened[0].positions[1], [3., -5., 4.]);
    approximate(
        flattened[0].normals[0],
        unit([-0.5, -0.25, 1. / 3.]).unwrap(),
    );
    assert_eq!(flattened[0].indices, [0, 2, 1]);
    assert_eq!(flattened[0].tangents[0], [-1., 0., 0., -1.]);
    assert_eq!(flattened[1].indices, [0, 1, 2]);
    assert_eq!(generated.geometry.bounds, [[0., -5., 0.], [5., 0., 7.]]);
}

#[test]
fn glb_sparse_positions_and_external_bytes_are_identity_inputs() {
    let (mut document, mut bytes, png) = fixture();
    let indices = view(&mut document, &mut bytes, &[1, 2]);
    let values = view(
        &mut document,
        &mut bytes,
        &floats(&[1., 0., 0., 0., 1., 0.]),
    );
    document["accessors"][0] = json!({"componentType":5126,"count":3,"type":"VEC3","sparse":{"count":2,"indices":{"bufferView":indices,"componentType":5121},"values":{"bufferView":values}}});
    document["images"][0] = json!({"uri":"../textures/finish.png"});
    let bytes = glb(document, bytes);
    let definition = definition(
        &json!({"format":"itemspec","version":1,"id":"external","generator":{"kind":"glb","asset":"models/input.glb"}}),
    );
    let png: Arc<[u8]> = png.into();
    let reads = Cell::new(0);
    let reader = |name: &str| match name {
        "models/input.glb" => Ok(bytes.clone()),
        "textures/finish.png" => {
            reads.set(reads.get() + 1);
            Ok(png.clone())
        }
        _ => absent(name),
    };
    let generated = item::generate(&definition, &Values::new(), &reader, &|| false).unwrap();
    assert_eq!(reads.get(), 1);
    assert_eq!(
        generated.geometry.meshes[0].positions,
        vec![[0., 0., 0.], [1., 0., 0.], [0., 0., 1.]]
    );
    assert!(
        generated
            .provenance
            .assets
            .contains_key("textures/finish.png")
    );
    let changed: Arc<[u8]> = image::encode(&image::Image {
        width: 1,
        height: 1,
        rgba: vec![9, 8, 7, 6],
        mime: "image/png",
    })
    .unwrap()
    .into();
    let prepared = item::prepare(
        &definition,
        &Values::new(),
        &|name| {
            if name == "models/input.glb" {
                Ok(bytes.clone())
            } else {
                Ok(changed.clone())
            }
        },
        &|| false,
    )
    .unwrap();
    assert_ne!(prepared.geometry_key, generated.geometry_key);
    assert!(
        item::prepare(
            &definition,
            &Values::new(),
            &|name| if name == "models/input.glb" {
                Ok(bytes.clone())
            } else {
                absent(name)
            },
            &|| false
        )
        .is_err()
    );
    assert_eq!(
        item::generate(&definition, &Values::new(), &reader, &|| false)
            .unwrap()
            .geometry_key,
        generated.geometry_key
    );
}

#[test]
fn malformed_or_unsupported_glb_fails_without_poisoning_next_import() {
    let (document, bytes, _) = fixture();
    let changes: Vec<(&str, Value)> = vec![
        ("/accessors/0/count", json!(500001)),
        ("/accessors/0/byteOffset", json!(999999)),
        ("/accessors/5/componentType", json!(5126)),
        ("/nodes/1/children", json!([0])),
        ("/nodes/1/scale", json!([0., 1., 1.])),
        ("/meshes/0/primitives/0/material", json!(999)),
        ("/materials/0/normalTexture/texCoord", json!(7)),
        ("/skins", json!([{}])),
        ("/extensionsRequired", json!(["KHR_draco_mesh_compression"])),
        ("/meshes/0/primitives/0/attributes/COLOR_1", json!(4)),
        ("/images/0/uri", json!("../../escape.png")),
    ];
    for (pointer, value) in changes {
        let mut malformed = document.clone();
        let (parent, key) = pointer.rsplit_once('/').unwrap();
        malformed
            .pointer_mut(parent)
            .unwrap()
            .as_object_mut()
            .unwrap()
            .insert(key.into(), value);
        let error = imported(glb(malformed, bytes.clone())).unwrap_err();
        assert!(error.code.starts_with("ITEM_GLB"), "{pointer}: {error}");
        assert_eq!(error.source, "models/input.glb");
    }
    let mut truncated = glb(document.clone(), bytes.clone()).to_vec();
    truncated.pop();
    assert!(imported(truncated.into()).is_err());
    assert_eq!(
        imported(glb(document, bytes))
            .unwrap()
            .geometry
            .meshes
            .len(),
        1
    );
}

#[test]
fn imported_sampler_and_uv_state_is_retained_until_delivery_admission() {
    let (mut document, bytes, _) = fixture();
    document["materials"][0]["normalTexture"]["texCoord"] = json!(1);
    document["materials"][0]["normalTexture"]["extensions"] =
        json!({"KHR_texture_transform":{"offset":[0.25,0.5],"scale":[2.,3.],"rotation":0.1}});
    document["samplers"] = json!([{"wrapS":33071,"minFilter":9728}]);
    document["textures"][0]["sampler"] = json!(0);
    let generated = imported(glb(document, bytes)).unwrap();
    let material = &generated.geometry.imported_materials["material/0"];
    assert_eq!(material.textures["normal"].tex_coord, 1);
    assert_eq!(
        material.textures["normal"].transform.as_ref().unwrap()["offset"],
        json!([0.25, 0.5])
    );
    assert_eq!(material.textures["normal"].sampler["wrapS"], 33071);
    assert!(material.ensure_scene_compatible().is_err());
}

#[test]
fn semantic_diagnostics_locate_escaped_keys_and_utf16_ranges() {
    let text = "{\n  \"format\":\"itemspec\",\"version\":1,\"id\":\"pipe\",\n  \"parameters\":{\"finish/tint\":{\"type\":\"choice\",\"role\":\"finish\",\"default\":\"港🧱\",\"choices\":[\"港🧱\"]}},\n  \"generator\":{\"kind\":\"pipe\",\"length\":-2,\"radius\":0.1}\n}";
    let definition = item::parse("unicode.item.json", text).unwrap();
    let mut error = item::generate(&definition, &Values::new(), &absent, &|| false).unwrap_err();
    item::locate_diagnostic(text, &mut error);
    let start = text.find("-2").unwrap();
    assert_eq!(error.offset, Some(text[..start].encode_utf16().count()));
    assert_eq!(error.end, Some(text[..start + 2].encode_utf16().count()));
    assert_eq!(error.line, Some(4));
    let mut error = definition
        .resolve_parameters(&BTreeMap::from([("finish/tint".into(), json!("bad"))]))
        .unwrap_err();
    assert_eq!(error.pointer, "/parameters/finish~1tint");
    item::locate_diagnostic(text, &mut error);
    let start = text.find("{\"type\"").unwrap();
    assert_eq!(error.offset, Some(text[..start].encode_utf16().count()));
    let broken = text.replace("\"radius\":0.1", "\"radius\":wat");
    let syntax = item::parse("bad.item.json", &broken).unwrap_err();
    assert_eq!(syntax.code, "ITEM_JSON");
    assert!(syntax.offset.is_some());
    assert!(syntax.end > syntax.offset);
}

#[test]
fn instanced_unused_vertices_cannot_bypass_placed_geometry_allowance() {
    let mut positions = vec![0f32; 10_000 * 3];
    positions[3] = 1.;
    positions[7] = 1.;
    let mut bytes = floats(&positions);
    bytes.extend([0, 0, 1, 0, 2, 0]);
    bytes.extend([0, 0]);
    bytes.extend(floats(&[0., 0., 1.].repeat(10_000)));
    // One placement of the 10,000-vertex source past the placed allowance, of which only
    // three vertices each are referenced.
    let placements = superspec_types::limits::MAX_MODEL_VERTICES / 10_000 + 1;
    assert_eq!(superspec_types::limits::MAX_MODEL_VERTICES % 10_000, 0);
    let document = json!({"asset":{"version":"2.0"},"buffers":[{"byteLength":bytes.len()}],
        "bufferViews":[{"buffer":0,"byteOffset":0,"byteLength":120_000},{"buffer":0,"byteOffset":120_000,"byteLength":6},{"buffer":0,"byteOffset":120_008,"byteLength":120_000}],
        "accessors":[{"bufferView":0,"componentType":5126,"count":10_000,"type":"VEC3"},{"bufferView":1,"componentType":5123,"count":3,"type":"SCALAR"},{"bufferView":2,"componentType":5126,"count":10_000,"type":"VEC3"}],
        "meshes":[{"primitives":[{"attributes":{"POSITION":0,"NORMAL":2},"indices":1}]}],
        "nodes":(0..placements).map(|_|json!({"mesh":0})).collect::<Vec<_>>(),
        "scenes":[{"nodes":(0..placements).collect::<Vec<_>>()}]});
    let error = imported(glb(document, bytes)).unwrap_err();
    assert!(
        error.message.contains("Placed item vertex allowance"),
        "{error}"
    );
    let (document, bytes, _) = fixture();
    assert!(imported(glb(document, bytes)).is_ok());
}

#[test]
fn normal_map_delivery_keeps_original_uv_basis_and_authored_handedness() {
    let (mut document, mut bytes, _) = fixture();
    document["nodes"] = json!([{"mesh":0}]);
    document["scenes"] = json!([{"nodes":[0]}]);
    document["meshes"][0]["primitives"][0]["attributes"]
        .as_object_mut()
        .unwrap()
        .remove("TANGENT");
    for vertex in 0..3 {
        bytes[vertex * 32 + 12..vertex * 32 + 24].copy_from_slice(&floats(&[0., 0., 1.]));
    }
    let normal = image::encode(&image::Image {
        width: 1,
        height: 1,
        rgba: vec![128, 255, 128, 255],
        mime: "image/png",
    })
    .unwrap();
    let image_view = view(&mut document, &mut bytes, &normal);
    document["images"][0]["bufferView"] = json!(image_view);
    let generated = imported(glb(document, bytes)).unwrap();
    let mesh = &generated.geometry.meshes[0];
    assert_eq!(&*generated.geometry.images[0].bytes, normal);
    let original = item::delivery_tangents(mesh).unwrap();
    assert_eq!(original, vec![[1., 0., 0., 1.]; 3]);
    let mut flipped = mesh.clone();
    for uv in &mut flipped.uvs {
        uv[1] = 1. - uv[1];
    }
    let recomputed = item::delivery_tangents(&flipped).unwrap();
    assert_eq!(recomputed, vec![[1., 0., 0., -1.]; 3]);
    flipped.tangents = original.clone();
    assert_eq!(item::delivery_tangents(&flipped).unwrap(), original);
    // A normal-map +Y sample must remain +Z in this converted X/Z face.
    let tangent = original[0];
    let normal = mesh.normals[0];
    assert_eq!(
        superspec_geometry::math::scale(
            superspec_geometry::math::cross(normal, [tangent[0], tangent[1], tangent[2]]),
            tangent[3]
        ),
        [0., 0., 1.]
    );
    flipped.tangents.clear();
    flipped.uvs.fill([0.; 2]);
    let fallback = item::delivery_tangents(&flipped).unwrap();
    for tangent in fallback {
        assert!(tangent.iter().all(|v| v.is_finite()));
        assert_eq!(tangent[1], 0.);
    }
}

#[test]
fn exposed_hex_color_resolves_to_a_valid_linear_material_parameter() {
    use superspec_engine::materialspec::program;
    let mut value = pipe();
    value["parameters"]["paint"] = json!({"type":"color","role":"finish","default":"#8040ff"});
    value["slots"]["surface"]["parameters"]["tint"] = json!({"parameter":"paint"});
    let definition = definition(&value);
    let generated = item::generate(&definition, &Values::new(), &absent, &|| false).unwrap();
    let request = &generated.materials["surface"];
    let source = "material 1; param tint: vec3 = vec3(1,1,1); param roughness: scalar = 0.4; output base_color = tint; output roughness = roughness;";
    let program = program::compile(&program::Sources {
        entry: request.entry.clone(),
        modules: BTreeMap::from([(request.entry.clone(), source.into())]),
    })
    .unwrap();
    let output = program
        .evaluate(
            &program::Inputs {
                parameters: request.parameters.clone(),
                ..Default::default()
            },
            &program::Sample::default(),
        )
        .unwrap();
    assert_eq!(output.base_color, superspec_types::color::srgb("#8040ff"));
    let prepared = item::prepare(
        &definition,
        &BTreeMap::from([("paint".into(), json!([0.1, 0.2, 0.3]))]),
        &absent,
        &|| false,
    )
    .unwrap();
    assert_eq!(prepared.geometry_key, generated.geometry_key);
    assert_eq!(
        prepared.materials["surface"].parameters["tint"],
        json!([0.1, 0.2, 0.3])
    );
}

#[test]
fn missing_normals_produce_flat_faces_and_ignore_authored_tangents() {
    let mut bytes = floats(&[0., 0., 0., 1., 0., 0., 0., 1., 0., 0., 0., 1.]);
    bytes.extend(floats(&[1., 0., 0., 1.].repeat(4)));
    bytes.extend([0, 0, 1, 0, 2, 0, 0, 0, 3, 0, 1, 0]);
    let document = json!({"asset":{"version":"2.0"},"buffers":[{"byteLength":bytes.len()}],
        "bufferViews":[{"buffer":0,"byteOffset":0,"byteLength":48},{"buffer":0,"byteOffset":48,"byteLength":64},{"buffer":0,"byteOffset":112,"byteLength":12}],
        "accessors":[{"bufferView":0,"componentType":5126,"count":4,"type":"VEC3"},{"bufferView":1,"componentType":5126,"count":4,"type":"VEC4"},{"bufferView":2,"componentType":5123,"count":6,"type":"SCALAR"}],
        "meshes":[{"primitives":[{"attributes":{"POSITION":0,"TANGENT":1},"indices":2}]}],"nodes":[{"mesh":0}],"scenes":[{"nodes":[0]}]});
    let generated = imported(glb(document, bytes)).unwrap();
    let mesh = &generated.geometry.meshes[0];
    assert_eq!(mesh.positions.len(), 6);
    assert_eq!(mesh.indices, [0, 1, 2, 3, 4, 5]);
    assert_eq!(&mesh.normals[..3], &[[0., -1., 0.]; 3]);
    assert_eq!(&mesh.normals[3..], &[[0., 0., 1.]; 3]);
    assert!(mesh.tangents.is_empty());
    assert_eq!(mesh.positions[0], mesh.positions[3]);
}

#[test]
fn retained_item_charge_covers_extra_attributes_and_enforces_the_store_limit() {
    use superspec_engine::runtime::dependencies::{Node, Plan, Runtime};
    let mut rich =
        item::generate(&definition(&pipe()), &Values::new(), &absent, &|| false).unwrap();
    let before = rich.retained_bytes();
    let geometry = Arc::make_mut(&mut rich.geometry);
    let mesh = &mut geometry.meshes[0];
    let count = mesh.positions.len();
    mesh.colors = vec![[1.; 4]; count];
    mesh.tangents = vec![[1., 0., 0., 1.]; count];
    for set in 1..=7 {
        mesh.additional_uvs.insert(set, vec![[0.25, 0.75]; count]);
    }
    let attributes = count * (2 * 4 + 7 * 2) * std::mem::size_of::<f64>();
    let after = rich.retained_bytes();
    assert!(after >= before + attributes);
    let colors = &mut Arc::make_mut(&mut rich.geometry).meshes[0].colors;
    colors.clear();
    assert_eq!(
        rich.retained_bytes(),
        after,
        "Clearing elements does not release their retained allocation."
    );
    let plan = Plan::new(
        vec![Node {
            id: "item".into(),
            implementation: "retention-test".into(),
            inputs: vec![1],
            dependencies: vec![],
            source: None,
        }],
        vec!["item".into()],
    )
    .unwrap();
    for (limit, expected_builds) in [(before + attributes / 2, 2), (after, 1)] {
        let runtime = Runtime::new(limit);
        let builds = Cell::new(0);
        for revision in 1..=2 {
            runtime
                .run(revision, &plan, &|| false, |_, _, _| {
                    builds.set(builds.get() + 1);
                    let value = rich.clone();
                    let bytes = value.retained_bytes();
                    Ok((value, bytes))
                })
                .unwrap();
        }
        assert_eq!(
            builds.get(),
            expected_builds,
            "Attribute-heavy artifacts must be delivered but not retained past the configured limit."
        );
    }
    let before = rich.retained_bytes();
    let payload = "retained metadata ".repeat(1024);
    let geometry = Arc::make_mut(&mut rich.geometry);
    geometry.nodes[0].extras = Some(json!({"nested":[payload.clone()]}));
    geometry
        .imported_materials
        .get_mut("surface")
        .unwrap()
        .extensions
        .insert("TEST_metadata".into(), json!({"payload":payload.clone()}));
    let encoded = image::encode(&image::Image {
        width: 32,
        height: 32,
        rgba: (0..4096).map(|i| (i * 37) as u8).collect(),
        mime: "image/png",
    })
    .unwrap();
    let encoded_bytes = encoded.len();
    geometry.images.push(item::ItemImage {
        id: "retained-image".into(),
        mime: "image/png".into(),
        bytes: encoded.into(),
    });
    rich.provenance
        .parameters
        .insert("record".into(), json!({"payload":payload.clone()}));
    assert!(rich.retained_bytes() >= before + payload.len() * 3 + encoded_bytes);
    assert!(rich.retained_bytes() > rich.geometry.retained_bytes());
}

#[test]
fn native_vertex_sharing_preserves_all_corner_attributes_and_unreferenced_bounds() {
    let mut mesh = item::ItemMesh {
        id: "seams".into(),
        material_slot: "metal".into(),
        positions: vec![[0., 0., 0.]; 8],
        normals: vec![[0., 0., 1.]; 8],
        uvs: vec![[0., 0.]; 8],
        colors: vec![[1.; 4]; 8],
        tangents: vec![[1., 0., 0., 1.]; 8],
        additional_uvs: BTreeMap::from([(2, vec![[0., 1.]; 8])]),
        indices: vec![0, 2, 3, 1, 4, 5, 1, 5, 6],
        roles: vec![],
    };
    mesh.normals[2] = [0., 1., 0.];
    let neighboring_uv = 1.0 + f64::EPSILON;
    mesh.uvs[3][0] = neighboring_uv;
    mesh.colors[4][2] = 0.5;
    mesh.tangents[5][3] = -1.;
    mesh.additional_uvs.get_mut(&2).unwrap()[6][1] = 0.;
    mesh.positions[7] = [9.; 3];
    mesh.share_vertices();
    assert_eq!(mesh.positions.len(), 7);
    assert_eq!(mesh.indices, [0, 1, 2, 0, 3, 4, 0, 4, 5]);
    assert_eq!(mesh.positions[6], [9.; 3]);
    assert_eq!(mesh.normals[1], [0., 1., 0.]);
    assert_eq!(mesh.uvs[2], [neighboring_uv, 0.]);
    assert_eq!(mesh.colors[3][2], 0.5);
    assert_eq!(mesh.tangents[4][3], -1.);
    assert_eq!(mesh.additional_uvs[&2][5], [0., 0.]);
    mesh.tangents.clear();
    mesh.additional_uvs.clear();
    mesh.share_vertices();
    assert_eq!(mesh.positions.len(), 5);
    assert_eq!(mesh.indices, [0, 1, 2, 0, 3, 0, 0, 0, 0]);
}

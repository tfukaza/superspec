use serde_json::Value;
use std::cell::Cell;
use superspec_engine::{Scene, compile, compile_with_assets};
use superspec_export::{Options, Purpose, glb};
use superspec_geometry::{
    image::{Image, encode},
    math::{cross, dot, sub},
};

fn model() -> Scene {
    compile_with_assets(r##"<architecture version="1" id="delivery">
    <material id="finish" color="#808080" texture="color.png" normal-texture="normal.png" normal-scale="0.35" metallic-roughness-texture="data.png" emissive="#204080" emissive-strength="3" alpha-cutoff="0.4"/>
    <material id="glass" color="#ffffff" opacity="0.6"/>
    <box id="solid" position="1 2 3" width="2" depth="3" height="4" material="finish"/>
    <box id="decoration" position="5 0 0" width="1" depth="1" height="2" collidable="false" material="glass"/>
    <box id="invisible" position="-2 0 0" width="1" depth="1" height="2" visible="false"/>
    </architecture>"##,4,&|path| {
        let pixel=match path {"color.png"=>[231,48,72,255],"normal.png"=>[112,145,242,255],"data.png"=>[0,73,198,255],_=>return Err("unexpected asset".into())};
        encode(&Image {width:1,height:1,rgba:pixel.to_vec(),mime:"image/png"})
    }).unwrap()
}
fn parse(bytes: &[u8]) -> (Value, &[u8]) {
    let number = |at: usize| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap()) as usize;
    assert_eq!(&bytes[..4], b"glTF");
    assert_eq!(number(4), 2);
    assert_eq!(number(8), bytes.len());
    assert_eq!(&bytes[16..20], b"JSON");
    let n = number(12);
    assert_eq!(n % 4, 0);
    let json = serde_json::from_slice(&bytes[20..20 + n]).unwrap();
    assert_eq!(&bytes[24 + n..28 + n], b"BIN\0");
    let binary = &bytes[28 + n..];
    assert_eq!(number(20 + n), binary.len());
    assert_eq!(binary.len() % 4, 0);
    (json, binary)
}
fn values(doc: &Value, binary: &[u8], index: &Value) -> Vec<f32> {
    let accessor = &doc["accessors"][index.as_u64().unwrap() as usize];
    assert_eq!(accessor["componentType"], 5126);
    let view = &doc["bufferViews"][accessor["bufferView"].as_u64().unwrap() as usize];
    let start = view["byteOffset"].as_u64().unwrap() as usize;
    let n = view["byteLength"].as_u64().unwrap() as usize;
    let components = match accessor["type"].as_str().unwrap() {
        "VEC2" => 2,
        "VEC3" => 3,
        "VEC4" => 4,
        _ => panic!("unexpected float accessor type"),
    };
    assert_eq!(
        n,
        accessor["count"].as_u64().unwrap() as usize * components * 4
    );
    binary[start..start + n]
        .as_chunks::<4>()
        .0
        .iter()
        .map(|v| f32::from_le_bytes(*v))
        .collect()
}

fn index_values(doc: &Value, binary: &[u8], index: &Value) -> Vec<u32> {
    let accessor = &doc["accessors"][index.as_u64().unwrap() as usize];
    let view = &doc["bufferViews"][accessor["bufferView"].as_u64().unwrap() as usize];
    let start = view["byteOffset"].as_u64().unwrap() as usize;
    let len = view["byteLength"].as_u64().unwrap() as usize;
    let bytes = &binary[start..start + len];
    let count = accessor["count"].as_u64().unwrap() as usize;
    match accessor["componentType"].as_u64().unwrap() {
        5123 => {
            assert_eq!(bytes.len(), count * 2);
            bytes
                .as_chunks::<2>()
                .0
                .iter()
                .map(|b| u32::from(u16::from_le_bytes(*b)))
                .collect()
        }
        5125 => {
            assert_eq!(bytes.len(), count * 4);
            bytes
                .as_chunks::<4>()
                .0
                .iter()
                .map(|b| u32::from_le_bytes(*b))
                .collect()
        }
        other => panic!("unexpected index component type {other}"),
    }
}

#[test]
fn glb_preserves_selected_geometry_pbr_images_sources_and_coordinate_contract() {
    let mut scene = model();
    for (i, color) in scene.colors.as_chunks_mut::<3>().0.iter_mut().enumerate() {
        *color = [0.25, 0.5, (i % 5) as f32 * 0.2];
    }
    let original = scene.packet();
    let bytes = glb(&scene, &Options::default()).unwrap();
    assert_eq!(bytes, glb(&scene, &Options::default()).unwrap());
    let (doc, binary) = parse(&bytes);
    assert_eq!(doc["meshes"].as_array().unwrap().len(), 2);
    assert_eq!(doc["images"].as_array().unwrap().len(), 3);
    assert_eq!(doc["extras"]["superspec"]["geometryValidation"], "not-run");
    assert_eq!(
        doc["extras"]["superspec"]["navigationValidation"],
        "not-run"
    );
    let root = doc["scenes"][0]["nodes"][0].as_u64().unwrap() as usize;
    let rotation = &doc["nodes"][root]["rotation"];
    assert_eq!(
        rotation[0].as_f64().unwrap(),
        -std::f64::consts::FRAC_1_SQRT_2
    );
    assert_eq!(
        rotation[3].as_f64().unwrap(),
        std::f64::consts::FRAC_1_SQRT_2
    );
    for view in doc["bufferViews"].as_array().unwrap() {
        assert_eq!(view["byteOffset"].as_u64().unwrap() % 4, 0);
        assert!(
            view["byteOffset"].as_u64().unwrap() + view["byteLength"].as_u64().unwrap()
                <= binary.len() as u64
        );
    }
    for mesh in doc["meshes"].as_array().unwrap() {
        let name = mesh["name"].as_str().unwrap();
        let input = scene.manifest.meshes.iter().find(|m| m.id == name).unwrap();
        assert!(input.visible);
        let primitive = &mesh["primitives"][0];
        let attributes = &primitive["attributes"];
        let positions = values(&doc, binary, &attributes["POSITION"]);
        let accessor = &doc["accessors"][attributes["POSITION"].as_u64().unwrap() as usize];
        for axis in 0..3 {
            let minimum = positions
                .iter()
                .skip(axis)
                .step_by(3)
                .copied()
                .fold(f32::INFINITY, f32::min);
            let maximum = positions
                .iter()
                .skip(axis)
                .step_by(3)
                .copied()
                .fold(f32::NEG_INFINITY, f32::max);
            assert_eq!(accessor["min"][axis].as_f64().unwrap(), f64::from(minimum));
            assert_eq!(accessor["max"][axis].as_f64().unwrap(), f64::from(maximum));
        }
        assert_eq!(
            positions,
            scene.positions[input.vertex_start * 3..(input.vertex_start + input.vertex_count) * 3]
        );
        let uv = values(&doc, binary, &attributes["TEXCOORD_0"]);
        assert_eq!(uv.len(), input.vertex_count * 2);
        for (i, value) in uv.iter().enumerate() {
            let source = scene.uvs[input.vertex_start * 2 + i];
            assert_eq!(*value, if i % 2 == 0 { source } else { 1. - source });
        }
        assert_eq!(
            values(&doc, binary, &attributes["COLOR_0"]),
            scene.colors[input.vertex_start * 3..(input.vertex_start + input.vertex_count) * 3]
        );
        let normals = values(&doc, binary, &attributes["NORMAL"]);
        let indices = index_values(&doc, binary, &primitive["indices"]);
        assert_eq!(
            indices,
            scene.indices[input.index_start..input.index_start + input.index_count]
        );
        for triangle in indices.as_chunks::<3>().0 {
            let [a, b, c] = triangle
                .map(|i| std::array::from_fn(|axis| f64::from(positions[i as usize * 3 + axis])));
            let normal =
                std::array::from_fn(|axis| f64::from(normals[triangle[0] as usize * 3 + axis]));
            assert!(dot(cross(sub(b, a), sub(c, a)), normal) > 0.);
        }
        if name == "solid" {
            let tangents = values(&doc, binary, &attributes["TANGENT"]);
            assert_eq!(tangents.len(), input.vertex_count * 4);
            for (t, n) in tangents
                .as_chunks::<4>()
                .0
                .iter()
                .zip(normals.as_chunks::<3>().0)
            {
                assert!((t[0] * n[0] + t[1] * n[1] + t[2] * n[2]).abs() < 1e-6);
                assert!((t[0] * t[0] + t[1] * t[1] + t[2] * t[2] - 1.).abs() < 1e-6);
                assert!(t[3] == 1. || t[3] == -1.);
            }
        } else {
            assert!(attributes["TANGENT"].is_null());
        }
        let node = doc["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|n| n["name"] == name)
            .unwrap();
        assert_eq!(
            node["extras"]["source"],
            serde_json::to_value(&input.source).unwrap()
        );
        assert_eq!(node["extras"]["meshId"], input.id);
        assert_eq!(node["extras"]["objectId"], input.object_id);
    }
    for image in doc["images"].as_array().unwrap() {
        let id = image["name"].as_str().unwrap();
        let asset = scene.manifest.assets.iter().find(|a| a.id == id).unwrap();
        let view = &doc["bufferViews"][image["bufferView"].as_u64().unwrap() as usize];
        let start = view["byteOffset"].as_u64().unwrap() as usize;
        assert_eq!(
            &binary[start..start + asset.byte_length],
            &scene.asset_bytes[asset.byte_start..asset.byte_start + asset.byte_length]
        );
    }
    let materials = doc["materials"].as_array().unwrap();
    let finish = materials.iter().find(|m| m["name"] == "finish").unwrap();
    for (texture, name) in [
        (
            &finish["pbrMetallicRoughness"]["baseColorTexture"],
            "color.png",
        ),
        (&finish["normalTexture"], "normal.png"),
        (
            &finish["pbrMetallicRoughness"]["metallicRoughnessTexture"],
            "data.png",
        ),
    ] {
        let index = texture["index"].as_u64().unwrap() as usize;
        let source = doc["textures"][index]["source"].as_u64().unwrap() as usize;
        assert_eq!(doc["images"][source]["name"], name);
    }
    assert_eq!(finish["alphaMode"], "MASK");
    assert_eq!(finish["alphaCutoff"], 0.4);
    assert_eq!(finish["normalTexture"]["scale"], 0.35);
    assert_eq!(
        finish["extensions"]["KHR_materials_emissive_strength"]["emissiveStrength"],
        3.
    );
    let factor = finish["pbrMetallicRoughness"]["baseColorFactor"][0]
        .as_f64()
        .unwrap();
    assert!((factor - 0.21586050011389926).abs() < 1e-14);
    let glass = materials.iter().find(|m| m["name"] == "glass").unwrap();
    assert_eq!(glass["alphaMode"], "BLEND");
    assert_eq!(glass["doubleSided"], false);
    assert_eq!(glass["pbrMetallicRoughness"]["baseColorFactor"][3], 0.6);
    assert_eq!(
        scene.packet(),
        original,
        "export must not mutate canonical scene"
    );
}

#[test]
fn exported_mesh_ownership_resolves_split_materials_collision_and_generated_parts() {
    use std::collections::BTreeMap;
    use superspec_engine::worldspec::themes::kowloon::{
        mesh::{Mesh, Mode},
        pen::{Pen, Tint},
    };
    use superspec_geometry::Matrix;
    let mut scene=compile(r##"<architecture version="1" id="owners"><material id="accent" color="#ff0000"/>
    <profile id="paint" points="1 0.5;3 0.5;3 1.5;1 1.5"/>
    <definition id="kit"><box id="body" width="4" depth="2" height="1"/>
    <face-material id="finish" host="body" face="top" profile="paint" projection="xy" material="accent"/></definition>
    <instance id="placed" ref="kit"/></architecture>"##,0).unwrap();
    let mut drawing = Mesh::new(Mode::Render, 10, &|| false).unwrap();
    Pen::new(Matrix::default(), "paint:detail")
        .unwrap()
        .panel(
            &mut drawing,
            "paint",
            [0., 1., 0., 1.],
            0.,
            &Tint::Color([1.; 3]),
            1.,
        )
        .unwrap();
    scene
        .append_visual(
            "placed/body",
            &drawing,
            &BTreeMap::from([("paint".into(), "accent".into())]),
        )
        .unwrap();
    assert!(
        scene
            .manifest
            .meshes
            .iter()
            .any(|m| m.id.contains("/$material/"))
    );
    for purpose in [Purpose::Visual, Purpose::Collision] {
        let bytes = glb(
            &scene,
            &Options {
                purpose,
                ..Default::default()
            },
        )
        .unwrap();
        let (doc, _) = parse(&bytes);
        let owners = doc["extras"]["superspec"]["architecture"]["objects"]
            .as_array()
            .unwrap();
        let expected: Vec<_> = scene
            .manifest
            .meshes
            .iter()
            .filter(|m| {
                if purpose == Purpose::Visual {
                    m.visible
                } else {
                    scene.manifest.colliders.contains(&m.id)
                }
            })
            .map(|m| m.id.as_str())
            .collect();
        let actual: Vec<_> = doc["meshes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|m| m["name"].as_str().unwrap())
            .collect();
        assert_eq!(actual, expected);
        for node in doc["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|n| !n["mesh"].is_null())
        {
            let owner = owners
                .iter()
                .find(|o| o["id"] == node["extras"]["objectId"])
                .expect("export owner must resolve directly");
            assert_eq!(owner["instances"], json_array("placed"));
            let mesh = node["extras"]["meshId"].as_str().unwrap();
            assert_eq!(
                owner["id"],
                if mesh.contains("$dressing") {
                    "placed/body/$dressing/paint:detail"
                } else {
                    "placed/body"
                }
            );
        }
    }
    fn json_array(value: &str) -> Value {
        serde_json::json!([value])
    }
}

#[test]
fn large_borrowed_metadata_obeys_output_budget_and_serialization_cancellation() {
    let label = "long metadata ".repeat(80000);
    let source = format!(
        r##"<architecture version="1" id="metadata"><box id="box"/><marker id="spawn" kind="spawn" position="0 0 0" label="{label}"/></architecture>"##
    );
    let scene = compile(&source, 0).unwrap();
    let calls = Cell::new(0);
    let count = || {
        calls.set(calls.get() + 1);
        false
    };
    assert!(
        glb(
            &scene,
            &Options {
                maximum_bytes: 8192,
                cancelled: &count,
                ..Default::default()
            }
        )
        .unwrap_err()
        .contains("budget")
    );
    assert!(
        calls.get() > 100,
        "budget must reach streaming metadata serialization"
    );
    let stopped = Cell::new(0);
    let cancel = || {
        stopped.set(stopped.get() + 1);
        stopped.get() > 100
    };
    assert!(
        glb(
            &scene,
            &Options {
                cancelled: &cancel,
                ..Default::default()
            }
        )
        .unwrap_err()
        .contains("cancelled")
    );
    let bytes = glb(&scene, &Options::default()).unwrap();
    let (doc, _) = parse(&bytes);
    assert_eq!(
        doc["extras"]["superspec"]["architecture"]["metadata"][0]["attributes"]["label"],
        label
    );
}

#[test]
fn collision_delivery_omits_decoration_and_appearance_even_when_assets_are_unavailable() {
    let mut scene = model();
    scene.asset_bytes.clear();
    scene.manifest.assets.clear();
    let bytes = glb(
        &scene,
        &Options {
            purpose: Purpose::Collision,
            ..Default::default()
        },
    )
    .unwrap();
    let (doc, binary) = parse(&bytes);
    let names: Vec<_> = doc["meshes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        names,
        scene
            .manifest
            .colliders
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>()
    );
    assert!(!names.contains(&"decoration"));
    assert!(names.iter().any(|n| n.contains("invisible")));
    assert!(doc["materials"].is_null());
    assert!(doc["images"].is_null());
    for mesh in doc["meshes"].as_array().unwrap() {
        assert!(mesh["primitives"][0]["attributes"]["TEXCOORD_0"].is_null());
        let source = scene
            .manifest
            .meshes
            .iter()
            .find(|m| m.id == mesh["name"])
            .unwrap();
        let primitive = &mesh["primitives"][0];
        assert_eq!(
            values(&doc, binary, &primitive["attributes"]["POSITION"]),
            scene.positions
                [source.vertex_start * 3..(source.vertex_start + source.vertex_count) * 3]
        );
        let actual = index_values(&doc, binary, &primitive["indices"]);
        assert_eq!(
            actual,
            scene.indices[source.index_start..source.index_start + source.index_count]
        );
    }
}

#[test]
fn collision_compilation_still_requires_geometric_heightmap_dependencies() {
    let source = r##"<architecture version="1" id="terrain"><material id="finish" texture="unavailable.png"/><terrain id="ground" material="finish" heightmap="height.png" min-height="1" max-height="4" cell-size="2 3"/></architecture>"##;
    let calls = Cell::new(0);
    let scene = superspec_engine::compile_collision_with_assets(source, 0, &|path| {
        assert_eq!(path, "height.png");
        calls.set(calls.get() + 1);
        Ok(include_bytes!("../../../tests/fixtures/aml/asymmetric-heightmap.png").to_vec())
    })
    .unwrap();
    assert!(calls.get() > 0);
    assert!(scene.manifest.materials.is_empty());
    assert!(scene.asset_bytes.is_empty());
    assert!(
        glb(
            &scene,
            &Options {
                purpose: Purpose::Collision,
                ..Default::default()
            }
        )
        .is_ok()
    );
    assert!(
        superspec_engine::compile_collision_with_assets(source, 0, &|_| Err(
            "missing heightmap".into()
        ))
        .is_err()
    );
}

#[test]
fn normal_map_basis_survives_v_flip_mirrored_u_and_world_axis_conversion() {
    use std::collections::BTreeMap;
    use superspec_engine::worldspec::themes::kowloon::{
        mesh::{Mesh, Mode},
        pen::{Pen, Tint},
    };
    use superspec_geometry::{
        Matrix,
        math::{add, scale, unit},
    };
    for mirrored in [false, true] {
        let mut scene = model();
        for mesh in &mut scene.manifest.meshes {
            mesh.visible = false;
        }
        let mut drawing = Mesh::new(Mode::Render, 10, &|| false).unwrap();
        let sign = if mirrored { -1. } else { 1. };
        Pen::new(Matrix::default(), "normal-test")
            .unwrap()
            .face(
                &mut drawing,
                "finish",
                &[[0., 0., 0.], [1., 0., 0.], [1., 1., 0.], [0., 1., 0.]],
                [0., 0., 1.],
                &Tint::Color([1.; 3]),
                1.,
                Some(&[[0., 0.], [sign, 0.], [sign, 1.], [0., 1.]]),
                None,
            )
            .unwrap();
        scene
            .append_visual(
                "solid",
                &drawing,
                &BTreeMap::from([("finish".into(), "finish".into())]),
            )
            .unwrap();
        let bytes = glb(&scene, &Options::default()).unwrap();
        let (doc, binary) = parse(&bytes);
        let attributes = &doc["meshes"][0]["primitives"][0]["attributes"];
        let tangents = values(&doc, binary, &attributes["TANGENT"]);
        let normals = values(&doc, binary, &attributes["NORMAL"]);
        assert_eq!(tangents.len(), 16);
        assert_eq!(normals.len(), 12);
        // Non-neutral source normal [112,145,242] with the declared .35 XY scale.
        let texel = [
            (112. / 255. * 2. - 1.) * 0.35,
            (145. / 255. * 2. - 1.) * 0.35,
            242. / 255. * 2. - 1.,
        ];
        let expected = unit([sign * texel[0], texel[2], -texel[1]]).unwrap();
        for (t, n) in tangents
            .as_chunks::<4>()
            .0
            .iter()
            .zip(normals.as_chunks::<3>().0)
        {
            assert_eq!(*t, [sign as f32, 0., 0., sign as f32]);
            let tangent = [f64::from(t[0]), f64::from(t[1]), f64::from(t[2])];
            let normal = n.map(f64::from);
            let bitangent = scale(cross(normal, tangent), f64::from(t[3]));
            assert_eq!(bitangent, [0., 1., 0.]);
            let source = unit(add(
                add(scale(tangent, texel[0]), scale(bitangent, texel[1])),
                scale(normal, texel[2]),
            ))
            .unwrap();
            let root = doc["scenes"][0]["nodes"][0].as_u64().unwrap() as usize;
            let rotation = doc["nodes"][root]["rotation"].as_array().unwrap();
            let q = [
                rotation[0].as_f64().unwrap(),
                rotation[1].as_f64().unwrap(),
                rotation[2].as_f64().unwrap(),
            ];
            let w = rotation[3].as_f64().unwrap();
            let world = add(
                source,
                scale(
                    add(scale(cross(q, source), w), cross(q, cross(q, source))),
                    2.,
                ),
            );
            for axis in 0..3 {
                assert!((world[axis] - expected[axis]).abs() < 1e-12);
            }
        }
    }
}

#[test]
fn export_rejects_invalid_ranges_hdr_missing_images_and_budgets_then_recovers() {
    let mut scene = model();
    let expected = glb(&scene, &Options::default()).unwrap();
    assert!(
        glb(
            &scene,
            &Options {
                maximum_bytes: 100,
                ..Default::default()
            }
        )
        .unwrap_err()
        .contains("budget")
    );
    let start = scene
        .manifest
        .meshes
        .iter()
        .find(|m| m.visible)
        .unwrap()
        .vertex_start;
    for invalid in [f32::NAN, f32::INFINITY, -0.1] {
        scene.colors[start * 3] = invalid;
        assert!(glb(&scene, &Options::default()).is_err());
    }
    scene.colors[start * 3] = 1.;
    let at = scene
        .manifest
        .meshes
        .iter()
        .find(|m| m.visible)
        .unwrap()
        .index_start;
    let index = scene.indices[at];
    scene.indices[at] = u32::MAX;
    assert!(glb(&scene, &Options::default()).is_err());
    scene.indices[at] = index;
    let images = std::mem::take(&mut scene.asset_bytes);
    assert!(
        glb(&scene, &Options::default())
            .unwrap_err()
            .contains("image range")
    );
    scene.asset_bytes = images;
    let cancelled = Cell::new(0);
    let check = || {
        let n = cancelled.get() + 1;
        cancelled.set(n);
        n > 12
    };
    assert!(
        glb(
            &scene,
            &Options {
                cancelled: &check,
                ..Default::default()
            }
        )
        .unwrap_err()
        .contains("cancelled")
    );
    assert_eq!(glb(&scene, &Options::default()).unwrap(), expected);
    let empty = compile("<architecture version=\"1\" id=\"empty\"/>", 0).unwrap();
    assert!(
        glb(&empty, &Options::default())
            .unwrap_err()
            .contains("empty")
    );
}

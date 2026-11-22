use serde_json::{Value, json};
use std::{collections::BTreeMap, sync::Arc};
use superspec_export::{Options, Purpose, Textures, glb, texture_jobs};
use superspec_model::instances::{IDENTITY, ImageResource, Instance, LocalMesh, Model};
use superspec_types::{
    diagnostic::SourceLocation,
    material::{Material, MaterialLod, Shading},
};

fn scene() -> superspec_model::Scene {
    let image = |id: &str, rgb: [u8; 3]| {
        (
            id.into(),
            ImageResource {
                mime: "image/png",
                pixels: 1,
                bytes: superspec_geometry::image::encode(&superspec_geometry::image::Image {
                    mime: "image/png",
                    width: 1,
                    height: 1,
                    rgba: vec![rgb[0], rgb[1], rgb[2], 255],
                })
                .unwrap()
                .into(),
            },
        )
    };
    let far = Material {
        id: "surface/far".into(),
        texture: Some("far-only.png".into()),
        roughness: 0.8,
        ..Default::default()
    };
    let near = Material {
        id: "surface".into(),
        texture: Some("near-only.png".into()),
        emissive: Some("#202020".into()),
        emissive_strength: Some(2.),
        shading: Some(Shading::Lit),
        lods: vec![MaterialLod {
            screen_coverage: 0.1,
            material: Box::new(far),
        }],
        ..Default::default()
    };
    Model {
        id: "material-lod".into(),
        meshes: BTreeMap::from([(
            "triangle".into(),
            Arc::new(LocalMesh {
                positions: vec![[0., 0., 0.], [1., 0., 0.], [0., 1., 0.]],
                normals: vec![[0., 0., 1.]; 3],
                uvs: vec![[0., 0.], [1., 0.], [0., 1.]],
                colors: vec![[1.; 3]; 3],
                indices: vec![0, 1, 2],
                tangents: vec![],
            }),
        )]),
        instances: vec![Instance {
            id: "triangle".into(),
            object_id: "triangle".into(),
            mesh: "triangle".into(),
            material: Some("surface".into()),
            transform: IDENTITY,
            source: SourceLocation {
                line: 1,
                column: 1,
                offset: 0,
                end: Some(1),
            },
            visible: true,
            collidable: true,
        }],
        materials: BTreeMap::from([("surface".into(), near)]),
        images: BTreeMap::from([
            image("near-only.png", [80, 100, 120]),
            image("far-only.png", [180, 190, 200]),
        ]),
        ..Default::default()
    }
    .scene(0)
    .unwrap()
}
fn document(bytes: &[u8]) -> Value {
    let length = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
    serde_json::from_slice(&bytes[20..20 + length]).unwrap()
}

#[test]
fn material_variants_are_optional_and_keep_core_binding_and_existing_extensions() {
    let scene = scene();
    let packet = scene.packet();
    let d = document(&glb(&scene, &Options::default()).unwrap());
    let bound = d["meshes"][0]["primitives"][0]["material"]
        .as_u64()
        .unwrap() as usize;
    let near = &d["materials"][bound];
    assert_eq!(near["name"], "surface");
    let far_index = near["extensions"]["MSFT_lod"]["ids"][0].as_u64().unwrap() as usize;
    assert_ne!(bound, far_index);
    assert_eq!(d["materials"][far_index]["name"], "surface/far");
    assert_eq!(near["extras"]["MSFT_screencoverage"], json!([0.1, 0.]));
    assert_eq!(
        near["extensions"]["KHR_materials_emissive_strength"]["emissiveStrength"],
        2.
    );
    assert_eq!(near["extras"]["materialSpec"]["shading"], "lit");
    assert!(d["materials"][far_index]["extensions"]["MSFT_lod"].is_null());
    assert_eq!(
        d["extensionsUsed"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|v| **v == json!("MSFT_lod"))
            .count(),
        1
    );
    assert!(
        !d["extensionsRequired"]
            .as_array()
            .is_some_and(|v| v.contains(&json!("MSFT_lod")))
    );
    assert!(
        d["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .all(|v| v["extensions"]["MSFT_lod"].is_null())
    );
    assert_eq!(d["images"].as_array().unwrap().len(), 2);
    assert_eq!(scene.packet(), packet);
}

#[test]
fn far_only_images_reach_shared_ktx2_jobs_and_missing_assets_fail_before_encoding() {
    let mut scene = scene();
    let options = Options {
        textures: Textures::Ktx2,
        ..Default::default()
    };
    let jobs = texture_jobs(&scene, &options).unwrap();
    assert_eq!(
        jobs.iter().map(|j| j.id).collect::<Vec<_>>(),
        vec!["far-only.png", "near-only.png"]
    );
    assert!(
        jobs.iter()
            .all(|job| job.kind == superspec_geometry::ktx2::Kind::Color)
    );
    scene.manifest.assets.retain(|a| a.id != "far-only.png");
    assert!(
        texture_jobs(&scene, &options)
            .err()
            .unwrap()
            .contains("far-only.png")
    );
}

#[test]
fn material_lod_does_not_change_collision_and_unused_chains_are_not_exported() {
    let mut scene = scene();
    let options = Options {
        purpose: Purpose::Collision,
        textures: Textures::Ktx2,
        ..Default::default()
    };
    assert!(texture_jobs(&scene, &options).unwrap().is_empty());
    let collision = glb(&scene, &options).unwrap();
    let d = document(&collision);
    assert_eq!(d["meshes"].as_array().unwrap().len(), 1);
    assert!(d["materials"].is_null());
    assert!(d["textures"].is_null());
    assert!(d["extensionsUsed"].is_null());
    scene.manifest.materials[0].lods.clear();
    assert_eq!(glb(&scene, &options).unwrap(), collision);
    scene.manifest.meshes[0].material = None;
    scene.manifest.materials[0].lods = vec![MaterialLod {
        screen_coverage: 0.1,
        material: Box::new(Material {
            id: "unused".into(),
            texture: Some("does-not-exist.png".into()),
            ..Default::default()
        }),
    }];
    let visual = document(&glb(&scene, &Options::default()).unwrap());
    assert!(visual["materials"].is_null());
    assert!(visual["extensionsUsed"].is_null());
}

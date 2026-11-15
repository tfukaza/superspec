#[path = "support/baked_scene.rs"]
mod baked_scene;
use std::collections::BTreeMap;
use superspec_aml::material::Material;
use superspec_engine::{
    compile,
    materialspec::{
        bake::BakedMaterial,
        surface::{Layout, Raster, RasterLayer, RasterSettings},
    },
    worldspec::themes::kowloon::{
        mesh::{Mesh, Mode},
        pen::{Pen, Tint},
    },
};
use superspec_geometry::{
    Matrix,
    image::{Image, decode},
};

fn raster() -> Raster {
    let layout = Layout::new(RasterSettings {
        bounds: [0., 0., 1., 1.],
        texels_per_meter: 4.,
        preserve: true,
        repeat: false,
        uv_coordinates: true,
        bump: true,
    })
    .unwrap();
    let image = |color: [u8; 4]| Image {
        width: layout.width,
        height: layout.height,
        rgba: color.repeat(layout.width * layout.height),
        mime: "image/png",
    };
    Raster {
        layers: vec![RasterLayer {
            source: "paint".into(),
            color: image([78, 124, 166, 255]),
            metallic_roughness: image([255, 173, 40, 255]),
            normal: Some(image([128, 128, 255, 255])),
        }],
        layout,
    }
}
fn source() -> Material {
    Material {
        id: "paint".into(),
        color: "#aa8844".into(),
        height_texture: Some("old.png".into()),
        height_scale: Some(0.1),
        height_offset: Some(-0.02),
        ..Material::default()
    }
}
fn bake() -> BakedMaterial {
    BakedMaterial::encode("baked", raster(), &source(), &|| false).unwrap()
}
fn drawing() -> Mesh<'static> {
    let mut mesh = Mesh::new(Mode::Render, 20, &|| false).unwrap();
    Pen::new(Matrix::default(), "face")
        .unwrap()
        .face(
            &mut mesh,
            "paint",
            &[[0., 0., 0.], [1., 0., 0.], [1., 0., 1.], [0., 0., 1.]],
            [0., 1., 0.],
            &Tint::Color([1.; 3]),
            1.,
            Some(&[[0., 0.], [1., 0.], [1., 1.], [0., 1.]]),
            None,
        )
        .unwrap();
    mesh
}
fn scene() -> superspec_engine::Scene {
    compile(r#"<architecture version="1" id="baked"><box id="host" width="1" depth="1" height="1"/></architecture>"#,0).unwrap()
}

#[test]
fn raster_channels_and_padded_uvs_reach_the_scene_without_changing_physics() {
    let bake = bake();
    let mut mesh = drawing();
    let before: Vec<_> = mesh
        .vertices
        .iter()
        .map(|v| (v.position, v.normal, v.color))
        .collect();
    bake.map_uvs(&mut mesh, "paint").unwrap();
    for vertex in &mesh.vertices {
        assert_eq!(
            vertex.uv,
            [
                0.25 + vertex.position[0] * 0.5,
                0.25 + vertex.position[2] * 0.5
            ]
        );
    }
    assert_eq!(
        before,
        mesh.vertices
            .iter()
            .map(|v| (v.position, v.normal, v.color))
            .collect::<Vec<_>>()
    );
    assert!(
        mesh.vertices
            .iter()
            .all(|v| v.uv.iter().all(|u| *u >= 0.25 && *u <= 0.75))
    );
    let mut scene = scene();
    let collision = scene.manifest.colliders.clone();
    let original_positions = scene.positions.clone();
    let bindings = BTreeMap::from([("paint".into(), bake.material().id.clone())]);
    scene
        .append_baked_visual("host", &mesh, &bindings, &[bake])
        .unwrap();
    assert_eq!(scene.manifest.colliders, collision);
    assert_eq!(
        scene.positions[..original_positions.len()],
        original_positions
    );
    let material = scene.manifest.materials.last().unwrap();
    assert_eq!(material.color, "#ffffff");
    assert_eq!(material.roughness, 1.);
    assert_eq!(material.metalness, 1.);
    assert_eq!(material.normal_scale, Some(1.));
    assert_eq!(material.height_texture, None);
    assert_eq!(material.height_scale, None);
    assert_eq!(material.height_offset, None);
    for (channel, pixel) in [
        (material.texture.as_ref().unwrap(), [78, 124, 166, 255]),
        (
            material.normal_texture.as_ref().unwrap(),
            [128, 128, 255, 255],
        ),
        (
            material.metallic_roughness_texture.as_ref().unwrap(),
            [255, 173, 40, 255],
        ),
    ] {
        let asset = scene
            .manifest
            .assets
            .iter()
            .find(|a| a.id == *channel)
            .unwrap();
        let image =
            decode(&scene.asset_bytes[asset.byte_start..asset.byte_start + asset.byte_length])
                .unwrap();
        assert_eq!((image.width, image.height), (8, 8));
        assert_eq!(image.rgba, pixel.repeat(64));
    }
    assert_eq!(scene.manifest.asset_byte_count, scene.asset_bytes.len());
}

#[test]
fn failed_binding_and_delivery_are_transactional_and_recover() {
    let mut scene = scene();
    let before = scene.packet();
    let bake = bake();
    let mut mesh = drawing();
    let original: Vec<_> = mesh.vertices.iter().map(|v| v.uv).collect();
    mesh.indices[0] = u32::MAX;
    assert!(bake.map_uvs(&mut mesh, "paint").is_err());
    assert_eq!(
        original,
        mesh.vertices.iter().map(|v| v.uv).collect::<Vec<_>>()
    );
    let mesh = drawing();
    let bindings = BTreeMap::new();
    assert!(
        scene
            .append_baked_visual("host", &mesh, &bindings, std::slice::from_ref(&bake))
            .is_err()
    );
    assert_eq!(scene.packet(), before);
    let bindings = BTreeMap::from([("paint".into(), bake.material().id.clone())]);
    assert!(
        scene
            .append_baked_visual("missing", &mesh, &bindings, std::slice::from_ref(&bake))
            .is_err()
    );
    assert_eq!(scene.packet(), before);
    scene
        .append_baked_visual("host", &mesh, &bindings, std::slice::from_ref(&bake))
        .unwrap();
    let valid = scene.packet();
    assert!(
        scene
            .append_baked_visual("host", &mesh, &bindings, std::slice::from_ref(&bake))
            .is_err()
    );
    assert_eq!(scene.packet(), valid);
    assert!(BakedMaterial::encode("baked", raster(), &source(), &|| true).is_err());
    let mut malformed = raster();
    malformed.layers[0].normal.as_mut().unwrap().width += 1;
    assert!(BakedMaterial::encode("baked", malformed, &source(), &|| false).is_err());
    let mut invalid = source();
    invalid.opacity = Some(f64::NAN);
    assert!(BakedMaterial::encode("baked", raster(), &invalid, &|| false).is_err());
    for opacity in [0., 0.5] {
        let mut translucent = source();
        translucent.opacity = Some(opacity);
        assert!(BakedMaterial::encode("baked", raster(), &translucent, &|| false).is_err());
    }
    let mut cutout = source();
    cutout.texture = Some("source.png".into());
    cutout.alpha_cutoff = Some(0.5);
    assert!(BakedMaterial::encode("baked", raster(), &cutout, &|| false).is_err());
    let mut translucent = raster();
    translucent.layers[0].color.rgba[3] = 128;
    assert!(BakedMaterial::encode("baked", translucent, &source(), &|| false).is_err());
    let mut malformed = raster();
    malformed.layout.width = 1;
    malformed.layout.height = 1;
    let layer = &mut malformed.layers[0];
    for image in [&mut layer.color, &mut layer.metallic_roughness] {
        image.width = 1;
        image.height = 1;
        image.rgba = vec![255; 4];
    }
    malformed.layers[0].normal = None;
    assert!(BakedMaterial::encode("baked", malformed, &source(), &|| false).is_err());
    let mut late = drawing();
    let last = late.vertices.len() - 1;
    late.vertices[last].uv[0] = f64::NAN;
    let original: Vec<_> = late
        .vertices
        .iter()
        .map(|v| v.uv.map(f64::to_bits))
        .collect();
    assert!(bake.map_uvs(&mut late, "paint").is_err());
    assert_eq!(
        original,
        late.vertices
            .iter()
            .map(|v| v.uv.map(f64::to_bits))
            .collect::<Vec<_>>()
    );
}

#[test]
fn offset_nonsquare_uv_layout_preserves_unselected_slots() {
    let mut raster = raster();
    raster.layout.bounds = [2., -3., 6., -1.];
    let bake = BakedMaterial::encode("offset", raster, &source(), &|| false).unwrap();
    let mut mesh = drawing();
    for vertex in &mut mesh.vertices {
        vertex.uv = [2. + vertex.position[0] * 4., -3. + vertex.position[2] * 2.];
    }
    Pen::new(Matrix::default(), "other")
        .unwrap()
        .panel(
            &mut mesh,
            "other",
            [0., 1., 0., 1.],
            2.,
            &Tint::Color([1.; 3]),
            1.,
        )
        .unwrap();
    let other: Vec<_> = mesh.vertices[4..].iter().map(|v| v.uv).collect();
    bake.map_uvs(&mut mesh, "paint").unwrap();
    for vertex in &mesh.vertices[..4] {
        assert_eq!(
            vertex.uv,
            [
                0.25 + vertex.position[0] * 0.5,
                0.25 + vertex.position[2] * 0.5
            ]
        );
    }
    assert_eq!(
        other,
        mesh.vertices[4..].iter().map(|v| v.uv).collect::<Vec<_>>()
    );
}

#[test]
fn cumulative_authored_and_baked_pixels_reject_before_commit_and_allow_smaller_recovery() {
    let image = Image {
        width: 2048,
        height: 2048,
        rgba: vec![255; 2048 * 2048 * 4],
        mime: "image/png",
    };
    let bytes = superspec_geometry::image::encode(&image).unwrap();
    drop(image);
    let mut source = String::from("<architecture version=\"1\" id=\"budget\">");
    for i in 0..47 {
        source.push_str(&format!("<material id=\"m{i}\" texture=\"{i}.png\"/><box id=\"b{i}\" material=\"m{i}\" position=\"{} 0 0\" width=\"1\" depth=\"1\" height=\"1\"/>",i*2));
    }
    source.push_str("</architecture>");
    let mut scene = superspec_engine::Session::default()
        .compile(&source, 0, &|_| Ok(bytes.clone()))
        .unwrap();
    let large = |id: &str| {
        let layout = Layout::new(RasterSettings {
            bounds: [0., 0., 1., 1.],
            texels_per_meter: 1020.,
            preserve: true,
            repeat: false,
            uv_coordinates: true,
            bump: true,
        })
        .unwrap();
        let image = || Image {
            width: 1024,
            height: 1024,
            rgba: vec![255; 1024 * 1024 * 4],
            mime: "image/png",
        };
        let raster = Raster {
            layout,
            layers: vec![RasterLayer {
                source: "paint".into(),
                color: image(),
                metallic_roughness: image(),
                normal: Some(image()),
            }],
        };
        BakedMaterial::encode(
            id,
            raster,
            &Material {
                id: "paint".into(),
                ..Material::default()
            },
            &|| false,
        )
        .unwrap()
    };
    let first = large("first");
    let mut mesh = drawing();
    let binding = BTreeMap::from([("paint".into(), first.material().id.clone())]);
    let before = scene.packet();
    assert!(
        scene
            .append_baked_visual("b0", &mesh, &BTreeMap::new(), std::slice::from_ref(&first))
            .is_err()
    );
    assert_eq!(scene.packet(), before);
    scene
        .append_baked_visual("b0", &mesh, &binding, &[first])
        .unwrap();
    let accepted = scene.packet();
    mesh.surfaces[0].part = "second".into();
    let second = large("second");
    let binding = BTreeMap::from([("paint".into(), second.material().id.clone())]);
    assert!(
        scene
            .append_baked_visual("b0", &mesh, &binding, &[second])
            .unwrap_err()
            .contains("budget")
    );
    assert_eq!(scene.packet(), accepted);
    let small = bake();
    let binding = BTreeMap::from([("paint".into(), small.material().id.clone())]);
    scene
        .append_baked_visual("b0", &mesh, &binding, &[small])
        .unwrap();
    assert_eq!(scene.manifest.assets.len(), 53);
}

#[test]
fn actual_rust_metal_raster_has_three_complete_channels_and_separate_collision() {
    let scene = baked_scene::scene();
    assert_eq!(scene.manifest.assets.len(), 3);
    assert_eq!(scene.manifest.colliders, ["$collision/host"]);
    let visible: Vec<_> = scene.manifest.meshes.iter().filter(|m| m.visible).collect();
    assert_eq!(visible.len(), 1);
    assert_eq!(visible[0].index_count, 6);
    for asset in &scene.manifest.assets {
        let image =
            decode(&scene.asset_bytes[asset.byte_start..asset.byte_start + asset.byte_length])
                .unwrap();
        assert_eq!((image.width, image.height), (260, 158));
    }
}

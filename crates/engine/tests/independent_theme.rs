use std::collections::BTreeMap;
use superspec_engine::{
    itemspec::{
        mesh::{Mesh, Mode},
        primitives::face,
    },
    materialspec::{
        bake::BakedMaterial,
        surface::{Inputs, Layer, RasterSettings, Sample, rasterize},
    },
};
use superspec_geometry::{
    Matrix,
    image::{Image, decode},
};
use superspec_model::visual::{BakedVisualInput, VisualInput};

struct GalleryTheme {
    tint: [f64; 3],
    roughness: f64,
}

impl GalleryTheme {
    fn panel(&self) -> (Mesh<'static>, BakedMaterial, BTreeMap<String, String>) {
        let concrete = superspec_catalog::material("builtin:concrete").unwrap();
        let materials = BTreeMap::from([(concrete.id.clone(), concrete.clone())]);
        let image = |pixel: [u8; 4]| Image {
            width: 1,
            height: 1,
            rgba: pixel.to_vec(),
            mime: "image/png",
        };
        let images = BTreeMap::from([
            (
                concrete.texture.clone().unwrap(),
                image([128, 128, 128, 255]),
            ),
            (
                concrete.normal_texture.clone().unwrap(),
                image([128, 128, 255, 255]),
            ),
        ]);
        let raster = rasterize(
            RasterSettings {
                bounds: [0., 0., 2., 1.],
                texels_per_meter: 2.,
                preserve: true,
                repeat: false,
                uv_coordinates: false,
                bump: false,
            },
            &|u, v| {
                Ok(Sample {
                    height: 0.,
                    layers: vec![Layer {
                        material: concrete.id.clone(),
                        weight: 1.,
                        uv: [u * concrete.repeat, v * concrete.repeat],
                        tint: Some(self.tint),
                        roughness: Some(self.roughness),
                        coatings: vec![],
                    }],
                    bump: None,
                    roughness: None,
                    height_weight: None,
                    layer_elevation_weight: None,
                })
            },
            &Inputs {
                materials: &materials,
                images: &images,
            },
            1_000_000,
            &|| false,
        )
        .unwrap();
        let bake = BakedMaterial::encode("gallery/panel", raster, &concrete, &|| false).unwrap();
        let mut mesh = Mesh::new(Mode::Render, 4, &|| false).unwrap();
        face(
            &mut mesh,
            Matrix::default(),
            "gallery-panel",
            "gallery-facing",
            &[[0., 0., 0.], [2., 0., 0.], [2., 0., 1.], [0., 0., 1.]],
            [0., 1., 0.],
            1.,
            Some(&[[0., 0.], [2., 0.], [2., 1.], [0., 1.]]),
            None,
            &|_, _, _| Ok([1.; 3]),
        )
        .unwrap();
        let geometry = mesh
            .vertices
            .iter()
            .map(|v| (v.position, v.normal, v.color))
            .collect::<Vec<_>>();
        let indices = mesh.indices.clone();
        bake.map_uvs(&mut mesh, "gallery-facing").unwrap();
        assert_eq!(
            mesh.vertices
                .iter()
                .map(|v| (v.position, v.normal, v.color))
                .collect::<Vec<_>>(),
            geometry
        );
        assert_eq!(mesh.indices, indices);
        let bindings = BTreeMap::from([("gallery-facing".into(), bake.material().id.clone())]);
        (mesh, bake, bindings)
    }
}

#[test]
fn another_theme_composes_the_common_catalog_material_and_local_item_contracts() {
    let catalog = superspec_catalog::material("builtin:concrete").unwrap();
    let (blue_mesh, blue, blue_bindings) = GalleryTheme {
        tint: [0.25, 0.5, 1.],
        roughness: 0.25,
    }
    .panel();
    let (warm_mesh, warm, warm_bindings) = GalleryTheme {
        tint: [1., 0.5, 0.25],
        roughness: 0.8,
    }
    .panel();
    assert_eq!(
        superspec_catalog::material("builtin:concrete").unwrap(),
        catalog
    );
    assert_eq!(blue_mesh.indices, warm_mesh.indices);
    assert_eq!(
        blue_mesh
            .vertices
            .iter()
            .map(|v| (v.position, v.normal, v.uv))
            .collect::<Vec<_>>(),
        warm_mesh
            .vertices
            .iter()
            .map(|v| (v.position, v.normal, v.uv))
            .collect::<Vec<_>>(),
    );
    for (mesh, bake, bindings) in [
        (&blue_mesh, &blue, &blue_bindings),
        (&warm_mesh, &warm, &warm_bindings),
    ] {
        let geometry: &dyn VisualInput = mesh;
        let finish: &dyn BakedVisualInput = bake;
        assert!(!geometry.surveying());
        assert_eq!(
            (geometry.vertices().len(), geometry.indices().len()),
            (4, 6)
        );
        assert_eq!(geometry.triangle_surfaces(), [0, 0]);
        assert!(
            geometry
                .indices()
                .iter()
                .all(|&index| (index as usize) < geometry.vertices().len())
        );
        assert!(
            geometry
                .vertices()
                .iter()
                .flat_map(|v| v.uv)
                .all(|uv| uv > 0. && uv < 1.)
        );
        assert_eq!(bindings[&geometry.surfaces()[0].slot], finish.material().id);
        finish.material().validate().unwrap();
        assert_eq!(finish.images().len(), 3);
        for path in finish.material().companions() {
            let bytes = &finish.images().iter().find(|(id, _)| id == path).unwrap().1;
            let image = decode(bytes).unwrap();
            assert_eq!((image.width, image.height), (8, 6));
            assert_eq!(image.rgba.len(), 8 * 6 * 4);
        }
        assert_eq!(finish.pixels(), 8 * 6 * 3);
    }
    let channel = |bake: &BakedMaterial, path: &str| {
        decode(&bake.images().iter().find(|(id, _)| id == path).unwrap().1).unwrap()
    };
    let blue_color = channel(&blue, blue.material().texture.as_deref().unwrap());
    let warm_color = channel(&warm, warm.material().texture.as_deref().unwrap());
    assert!(blue_color.rgba[2] > blue_color.rgba[0]);
    assert!(warm_color.rgba[0] > warm_color.rgba[2]);
    let blue_properties = channel(
        &blue,
        blue.material()
            .metallic_roughness_texture
            .as_deref()
            .unwrap(),
    );
    let warm_properties = channel(
        &warm,
        warm.material()
            .metallic_roughness_texture
            .as_deref()
            .unwrap(),
    );
    assert_eq!(&blue_properties.rgba[..4], &[255, 64, 0, 255]);
    assert_eq!(&warm_properties.rgba[..4], &[255, 204, 0, 255]);
}

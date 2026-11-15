use serde::Deserialize;
use std::{cell::Cell, collections::BTreeMap};
use superspec_aml::material::Material;
use superspec_engine::materialspec::surface::{
    Coat, Inputs, Layer, Layout, RasterSettings, Sample, color_byte, image_sample, rasterize,
};
use superspec_geometry::image::Image;

#[derive(Deserialize)]
struct InputImage {
    width: usize,
    height: usize,
    data: Vec<u8>,
}
#[derive(Deserialize)]
struct ExpectedLayer {
    source: String,
    color: Vec<u8>,
    packed: Vec<u8>,
    normal: Option<Vec<u8>>,
}
#[derive(Deserialize)]
struct Case {
    mode: String,
    settings: RasterSettings,
    width: usize,
    height: usize,
    layers: Vec<ExpectedLayer>,
}
#[derive(Deserialize)]
struct Fixture {
    materials: Vec<Material>,
    images: BTreeMap<String, InputImage>,
    cases: Vec<Case>,
    transfers: Vec<Transfer>,
}
#[derive(Deserialize)]
struct Transfer {
    bits: String,
    byte: u8,
}

fn layer(material: &str, weight: f64, uv: [f64; 2]) -> Layer {
    Layer {
        material: material.into(),
        weight,
        uv,
        tint: None,
        roughness: None,
        coatings: vec![],
    }
}
fn sample(layers: Vec<Layer>) -> Sample {
    Sample {
        height: 0.,
        layers,
        bump: None,
        roughness: None,
        height_weight: None,
        layer_elevation_weight: None,
    }
}
fn settings() -> RasterSettings {
    RasterSettings {
        bounds: [0., 0., 1., 1.],
        texels_per_meter: 2.,
        preserve: true,
        repeat: true,
        uv_coordinates: false,
        bump: false,
    }
}
fn image(width: usize, height: usize, rgba: Vec<u8>) -> Image {
    Image {
        width,
        height,
        rgba,
        mime: "image/png",
    }
}

/// Shared native/browser acceptance body; every comparison uses frozen reference values.
pub fn check_reference_rasters() {
    let fixture: Fixture = serde_json::from_str(include_str!(
        "../../../tests/fixtures/rust/surface-rasters.json"
    ))
    .unwrap();
    for transfer in &fixture.transfers {
        assert_eq!(
            color_byte(f64::from_bits(
                u64::from_str_radix(&transfer.bits, 16).unwrap()
            ))
            .unwrap(),
            transfer.byte,
            "sRGB reference transition {}",
            transfer.bits
        );
    }
    let materials = fixture
        .materials
        .into_iter()
        .map(|m| (m.id.clone(), m))
        .collect();
    let images = fixture
        .images
        .into_iter()
        .map(|(id, i)| (id, image(i.width, i.height, i.data)))
        .collect();
    let inputs = Inputs {
        materials: &materials,
        images: &images,
    };
    for case in fixture.cases {
        let field = |u: f64, v: f64| {
            let uv = match case.mode.as_str() {
                "rotated" => [v, -u],
                "reflected" => [-u, v],
                "sheared" => [u + 0.3 * v, v],
                _ => [u, v],
            };
            let mut layers = vec![layer("a", 2., uv)];
            layers[0].tint = if v > 0.2 { Some([0.8, 1.1, 0.9]) } else { None };
            layers[0].roughness = if u > 0.2 { Some(0.) } else { None };
            if case.mode == "mixed" || case.mode == "layered-zero" {
                layers.push(layer(
                    "b",
                    if case.mode == "layered-zero" { 0. } else { 1. },
                    [u * 0.7 + 0.1, v * 0.8 - 0.2],
                ));
            }
            if case.mode == "coated" {
                layers[0].coatings = vec![
                    Coat {
                        material: "coat".into(),
                        uv: [u * 0.8, v * 1.1],
                        opacity: 0.6,
                        tint: Some([1., 0.5, 0.8]),
                        roughness: None,
                    },
                    Coat {
                        material: "a".into(),
                        uv: [v, -u],
                        opacity: 0.2,
                        tint: None,
                        roughness: None,
                    },
                ];
            }
            let mut sample = sample(layers);
            sample.roughness = if v < 0.1 { Some(0.3) } else { None };
            sample.bump = Some(0.02 * u * u + 0.03 * v * v + 0.01 * u * v);
            Ok(sample)
        };
        let actual = rasterize(case.settings, &field, &inputs, 1_000_000, &|| false).unwrap();
        assert_eq!(
            (actual.layout.width, actual.layout.height),
            (case.width, case.height),
            "{}",
            case.mode
        );
        assert_eq!(actual.layers.len(), case.layers.len());
        for (actual, expected) in actual.layers.iter().zip(case.layers) {
            assert_eq!(actual.source, expected.source);
            for (channel, a, b) in [
                ("color", &actual.color.rgba, &expected.color),
                ("packed", &actual.metallic_roughness.rgba, &expected.packed),
            ] {
                assert_eq!(a.len(), b.len());
                let first = a.iter().zip(b).position(|(a, b)| a != b);
                assert!(
                    first.is_none(),
                    "{} {channel} first differing byte {first:?}: {:?}",
                    case.mode,
                    first.map(|i| (a[i], b[i]))
                );
            }
            assert_eq!(
                actual.normal.as_ref().map(|i| &i.rgba),
                expected.normal.as_ref(),
                "{} normal",
                case.mode
            );
        }
    }
}

#[test]
fn frozen_rasters_preserve_layers_coats_reflected_normals_and_recycled_bump_rows() {
    check_reference_rasters();
}

#[test]
fn sampling_and_layout_use_linear_color_lower_left_repeat_and_unclamped_gutters() {
    let source = image(2, 1, vec![0, 0, 0, 0, 255, 255, 255, 255]);
    assert_eq!(image_sample(&source, [0.25, 0.5], false).unwrap(), [0.; 4]);
    assert_eq!(image_sample(&source, [-0.25, 0.5], false).unwrap(), [1.; 4]);
    let color = image_sample(&source, [0.5, 0.5], true).unwrap();
    assert_eq!(color, [0.5; 4]);
    assert_eq!(color_byte(color[0]).unwrap(), 188);
    assert_eq!(
        (image_sample(&source, [0.5, 0.5], false).unwrap()[0] * 255.).round(),
        128.
    );
    let vertical = image(1, 2, vec![255, 0, 0, 255, 0, 255, 0, 255]);
    assert_eq!(
        image_sample(&vertical, [0.5, 0.75], false).unwrap(),
        [1., 0., 0., 1.]
    );
    assert_eq!(
        image_sample(&vertical, [0.5, 0.25], false).unwrap(),
        [0., 1., 0., 1.]
    );
    let layout = Layout::new(RasterSettings {
        bounds: [2., 3., 3.1, 3.7],
        texels_per_meter: 2.5,
        repeat: false,
        ..settings()
    })
    .unwrap();
    assert_eq!((layout.width, layout.height), (7, 6));
    assert_eq!(layout.uv([2., 3.]), [2. / 7., 2. / 6.]);
    assert_eq!(layout.uv([3.1, 3.7]), [5. / 7., 4. / 6.]);
    assert!(layout.uv([1., 2.])[0] < 0.);
    assert!(
        Layout::new(RasterSettings {
            preserve: false,
            ..settings()
        })
        .is_err()
    );
}

#[test]
fn geometry_height_separates_blended_maps_selected_plate_offsets_coats_and_bump() {
    let a = Material {
        id: "a".into(),
        height_texture: Some("height".into()),
        height_scale: Some(2.),
        height_offset: Some(0.1),
        ..Material::default()
    };
    let b = Material {
        id: "b".into(),
        height_texture: Some("height".into()),
        height_scale: Some(0.4),
        height_offset: Some(-0.2),
        ..Material::default()
    };
    let materials = BTreeMap::from([("a".into(), a), ("b".into(), b)]);
    let images = BTreeMap::from([(
        "height".into(),
        image(2, 1, vec![0, 0, 0, 255, 255, 0, 0, 255]),
    )]);
    let inputs = Inputs {
        materials: &materials,
        images: &images,
    };
    let mut value = sample(vec![
        layer("a", 2., [0.25, 0.5]),
        layer("b", 1., [0.75, 0.5]),
    ]);
    value.height = 0.5;
    value.height_weight = Some(0.5);
    value.layer_elevation_weight = Some(0.25);
    value.bump = Some(99.);
    value.layers[0].coatings.push(Coat {
        material: "missing-but-height-independent".into(),
        uv: [0.; 2],
        opacity: 1.,
        tint: None,
        roughness: None,
    });
    assert!(
        (inputs.height(&value, None, None).unwrap() - (0.5 + 0.5 * (0.2 + 0.2) / 3.)).abs() < 1e-12
    );
    assert!((inputs.height(&value, Some(1), Some(&[0.2, 0.3])).unwrap() - 0.675).abs() < 1e-12);
    value.height_weight = Some(0.);
    assert_eq!(
        inputs.height(&value, Some(0), Some(&[0.2, 0.3])).unwrap(),
        0.55
    );
    assert!(inputs.height(&value, Some(2), None).is_err());
}

#[test]
fn affine_bump_normals_and_failure_recovery_keep_ownership_and_budgets() {
    let materials = BTreeMap::from([(
        "white".into(),
        Material {
            id: "white".into(),
            color: "#ffffff".into(),
            ..Material::default()
        },
    )]);
    let images = BTreeMap::new();
    let inputs = Inputs {
        materials: &materials,
        images: &images,
    };
    let field = |u: f64, v: f64| {
        let mut s = sample(vec![layer("white", 1., [u, v])]);
        s.bump = Some(0.08 * u - 0.04 * v + 2.);
        Ok(s)
    };
    for density in [2., 7.] {
        let output = rasterize(
            RasterSettings {
                bounds: [-2., 3., -1., 4.],
                bump: true,
                texels_per_meter: density,
                ..settings()
            },
            &field,
            &inputs,
            1_000_000,
            &|| false,
        )
        .unwrap();
        for pixel in output.layers[0]
            .normal
            .as_ref()
            .unwrap()
            .rgba
            .as_chunks::<4>()
            .0
        {
            assert_eq!(*pixel, [117, 133, 254, 255]);
        }
    }
    let base = settings();
    assert!(rasterize(base, &field, &inputs, 4096, &|| false).is_err());
    assert!(rasterize(base, &field, &inputs, 16384, &|| false).is_ok());
    let calls = Cell::new(0);
    let cancel = || {
        calls.set(calls.get() + 1);
        calls.get() > 3
    };
    assert!(
        rasterize(
            RasterSettings { bump: true, ..base },
            &field,
            &inputs,
            1_000_000,
            &cancel
        )
        .err()
        .unwrap()
        .contains("cancel")
    );
    for bad in [0., -1., f64::NAN, f64::INFINITY] {
        let invalid = |u, v| {
            let mut s = field(u, v)?;
            s.layers[0].weight = bad;
            Ok(s)
        };
        assert!(rasterize(base, &invalid, &inputs, 1_000_000, &|| false).is_err());
    }
    let invalid = |u, v| {
        let mut s = field(u, v)?;
        s.layers[0].uv = [f64::INFINITY, 0.];
        Ok(s)
    };
    assert!(rasterize(base, &invalid, &inputs, 1_000_000, &|| false).is_err());
    assert!(
        rasterize(
            RasterSettings {
                bounds: [0., 0., 1e300, 1e300],
                ..base
            },
            &field,
            &inputs,
            usize::MAX,
            &|| false
        )
        .is_err()
    );
    assert!(
        rasterize(
            base,
            &|_, _| Err("sampler failed".into()),
            &inputs,
            1_000_000,
            &|| false
        )
        .is_err()
    );
    assert!(rasterize(base, &field, &inputs, 1_000_000, &|| false).is_ok());
}

#[test]
fn long_material_identities_are_charged_before_bump_rows_are_allocated() {
    let id = "m".repeat(65536);
    let materials = BTreeMap::from([(
        id.clone(),
        Material {
            id: id.clone(),
            color: "#ffffff".into(),
            ..Material::default()
        },
    )]);
    let images = BTreeMap::new();
    let inputs = Inputs {
        materials: &materials,
        images: &images,
    };
    let calls = Cell::new(0);
    let field = |u, v| {
        calls.set(calls.get() + 1);
        let mut s = sample(vec![layer(&id, 1., [u, v])]);
        s.bump = Some(0.);
        Ok(s)
    };
    let output = rasterize(
        RasterSettings {
            bounds: [0., 0., 4096., 1.],
            texels_per_meter: 1.,
            bump: true,
            ..settings()
        },
        &field,
        &inputs,
        8 * 1024 * 1024,
        &|| false,
    );
    assert!(output.is_err());
    assert_eq!(calls.get(), 1, "reject before sampling/retaining full rows");
    calls.set(0);
    let small = RasterSettings {
        bounds: [0., 0., 1., 1.],
        texels_per_meter: 1.,
        bump: true,
        ..settings()
    };
    assert!(
        rasterize(small, &field, &inputs, 600_000, &|| false).is_err(),
        "the output's owned source ID also consumes budget"
    );
    assert_eq!(calls.get(), 1);
    assert!(rasterize(small, &field, &inputs, 700_000, &|| false).is_ok());
}

#[test]
fn ordered_artwork_coatings_obey_count_and_owned_byte_limits() {
    let materials = BTreeMap::from([(
        "m".into(),
        Material {
            id: "m".into(),
            color: "#ffffff".into(),
            ..Material::default()
        },
    )]);
    let images = BTreeMap::new();
    let inputs = Inputs {
        materials: &materials,
        images: &images,
    };
    let calls = Cell::new(0);
    let field = |count| {
        let mut s = sample(vec![layer("m", 1., [0.; 2])]);
        s.layers[0].coatings = vec![
            Coat {
                material: "m".into(),
                uv: [0.; 2],
                opacity: 0.5,
                tint: Some([0.3, 0.5, 0.7]),
                roughness: Some(0.8)
            };
            count
        ];
        s
    };
    assert!(
        rasterize(
            settings(),
            &|_, _| Ok(field(32)),
            &inputs,
            1_000_000,
            &|| false
        )
        .is_ok()
    );
    assert!(rasterize(settings(), &|_, _| Ok(field(32)), &inputs, 1024, &|| false).is_err());
    let too_many = |_, _| {
        calls.set(calls.get() + 1);
        Ok(field(1025))
    };
    assert!(
        rasterize(settings(), &too_many, &inputs, 64 * 1024 * 1024, &|| false)
            .err()
            .unwrap()
            .contains("coating budget")
    );
    assert_eq!(
        calls.get(),
        1,
        "reject excessive artwork before preparing texture sources"
    );
}

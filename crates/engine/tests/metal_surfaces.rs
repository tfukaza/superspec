use serde::Deserialize;
use std::{cell::Cell, collections::BTreeMap};
use superspec_aml::material::Material;
use superspec_engine::{
    materialspec::{
        noise::*,
        surface::{Inputs, RasterSettings, Sample, rasterize},
    },
    worldspec::themes::kowloon::metal::{Fold, Metal, Metric, Settings},
};

#[derive(Deserialize)]
struct Case {
    settings: Settings,
    runoff: bool,
    points: Vec<[f64; 2]>,
    samples: Vec<Sample>,
    width: usize,
    height: usize,
    color: Vec<u8>,
    packed: Vec<u8>,
    normal: Vec<u8>,
}
#[derive(Deserialize)]
struct Noise {
    seed: f64,
    x: f64,
    y: f64,
    z: f64,
    hash: u32,
    value: f64,
    volume: f64,
    fractal: f64,
}
#[derive(Deserialize)]
struct Range {
    bounds: [f64; 4],
    value: [f64; 2],
    fractal: [f64; 2],
}
#[derive(Deserialize)]
struct Fixture {
    materials: Vec<Material>,
    cases: Vec<Case>,
    noise: Vec<Noise>,
    ranges: Vec<Range>,
}
fn fixture() -> Fixture {
    serde_json::from_str(include_str!(
        "../../../tests/fixtures/rust/metal-surfaces.json"
    ))
    .unwrap()
}
fn close(a: f64, b: f64) {
    assert!((a - b).abs() < 2e-12, "{a} != {b}");
}
fn channels(sample: &Sample) -> Vec<f64> {
    let mut values = vec![
        sample.height,
        sample.bump.unwrap(),
        sample.roughness.unwrap(),
    ];
    for layer in &sample.layers {
        values.push(layer.weight);
        values.extend(layer.tint.unwrap());
        for coat in &layer.coatings {
            values.extend([coat.opacity, coat.roughness.unwrap()]);
            values.extend(coat.tint.unwrap());
        }
    }
    values
}
fn same_sample(a: &Sample, b: &Sample) {
    assert_eq!(a.layers.len(), b.layers.len());
    assert_eq!(a.height_weight, b.height_weight);
    assert_eq!(a.layer_elevation_weight, b.layer_elevation_weight);
    for (a, b) in a.layers.iter().zip(&b.layers) {
        assert_eq!(a.material, b.material);
        assert_eq!(a.roughness, b.roughness);
        for (a, b) in a.uv.into_iter().zip(b.uv) {
            close(a, b);
        }
        assert_eq!(a.coatings.len(), b.coatings.len());
        for (a, b) in a.coatings.iter().zip(&b.coatings) {
            assert_eq!(a.material, b.material);
            for (a, b) in a.uv.into_iter().zip(b.uv) {
                close(a, b);
            }
        }
    }
    let av = channels(a);
    let bv = channels(b);
    assert_eq!(av.len(), bv.len());
    for (a, b) in av.into_iter().zip(bv) {
        close(a, b);
    }
}
/// Native and actual browser Wasm execute the same frozen recipe, noise and decoded-pixel expectations.
pub fn check_reference_metal() {
    let fixture = fixture();
    for n in fixture.noise {
        assert_eq!(
            lattice_hash(n.x, n.y, n.seed) * 4294967296.,
            f64::from(n.hash)
        );
        close(value_noise(n.x, n.y, n.seed), n.value);
        close(value_noise3(n.x, n.y, n.z, n.seed), n.volume);
        close(fractal_noise(n.x, n.y, n.seed, 4), n.fractal);
    }
    for r in fixture.ranges {
        for (a, b) in value_noise_range(r.bounds, 37.)
            .unwrap()
            .into_iter()
            .zip(r.value)
        {
            close(a, b);
        }
        for (a, b) in fractal_noise_range(r.bounds, 37., 4)
            .unwrap()
            .into_iter()
            .zip(r.fractal)
        {
            close(a, b);
        }
    }
    let materials: BTreeMap<_, _> = fixture
        .materials
        .into_iter()
        .map(|m| (m.id.clone(), m))
        .collect();
    let images = BTreeMap::new();
    let inputs = Inputs {
        materials: &materials,
        images: &images,
    };
    let runoff = |u: f64, v: f64| Ok((0.4 + u * 0.2 + v * 0.3).clamp(0., 1.));
    for (index, case) in fixture.cases.into_iter().enumerate() {
        let metal = Metal::new(
            &case.settings,
            if case.runoff { Some(&runoff) } else { None },
        )
        .unwrap();
        assert_eq!(case.points.len(), case.samples.len());
        for (point, expected) in case.points.iter().zip(&case.samples) {
            same_structure(&metal.structure(), expected);
            same_sample(&metal.sample(point[0], point[1]).unwrap(), expected);
        }
        let raster = rasterize(
            RasterSettings {
                bounds: case.settings.bounds,
                texels_per_meter: 24.,
                preserve: true,
                repeat: false,
                uv_coordinates: false,
                bump: true,
            },
            &|u, v| metal.sample(u, v),
            &inputs,
            16 * 1024 * 1024,
            &|| false,
        )
        .unwrap();
        assert_eq!(
            (raster.layout.width, raster.layout.height),
            (case.width, case.height)
        );
        assert_eq!(raster.layers.len(), 1);
        let layer = &raster.layers[0];
        assert_eq!(layer.color.rgba, case.color, "metal {index}: color");
        assert_eq!(
            layer.metallic_roughness.rgba, case.packed,
            "metal {index}: packed PBR"
        );
        assert_eq!(
            layer.normal.as_ref().unwrap().rgba,
            case.normal,
            "metal {index}: normal-only relief"
        );
    }
}
#[test]
fn frozen_metal_channels_noise_and_raster_pixels_match() {
    check_reference_metal();
}

fn same_structure(structure: &Sample, sample: &Sample) {
    assert_eq!(structure.height, sample.height);
    assert_eq!(structure.layers.len(), sample.layers.len());
    for (a, b) in structure.layers.iter().zip(&sample.layers) {
        assert_eq!(a.material, b.material);
        assert_eq!(a.weight, b.weight);
        assert_eq!(
            a.coatings.iter().map(|c| &c.material).collect::<Vec<_>>(),
            b.coatings.iter().map(|c| &c.material).collect::<Vec<_>>()
        );
    }
}

#[test]
fn metal_structure_is_stable_and_clean_controls_do_not_erase_intrinsic_finish() {
    let mut s = fixture().cases.remove(0).settings;
    let calls = Cell::new(0);
    let runoff = |_, _| {
        calls.set(calls.get() + 1);
        Ok(0.8)
    };
    for supplied in [false, true] {
        s.contacts = if supplied { Some(vec![]) } else { None };
        let metal = Metal::new(&s, None).unwrap();
        assert_eq!(
            metal.structure().layers[0].coatings.len(),
            if supplied { 3 } else { 2 }
        );
        assert_eq!(
            metal.sample(0.17, 0.29).unwrap().layers[0].coatings.len(),
            if supplied { 3 } else { 2 }
        );
    }
    let metal = Metal::new(&s, Some(&runoff)).unwrap();
    assert_eq!(metal.structure().layers[0].coatings.len(), 3);
    assert_eq!(calls.get(), 0, "structure cannot evaluate exposure");
    same_structure(&metal.structure(), &metal.sample(0.2, 0.3).unwrap());
    assert_eq!(calls.get(), 1);
    s.wear = 0.;
    s.rust = 0.;
    s.grime = 0.;
    for coverage in [0., 0.35, 1.] {
        s.paint_coverage = Some(coverage);
        let metal = Metal::new(&s, Some(&runoff)).unwrap();
        let a = metal.sample(0.17, 0.29).unwrap();
        let b = metal.sample(0.23, 0.31).unwrap();
        assert_eq!(a.height, 0.);
        assert_eq!(a.layers[0].weight, 1.);
        let coats = &a.layers[0].coatings;
        assert_eq!(coats[0].opacity, coverage);
        assert_eq!(coats[1].opacity, 0.);
        assert_eq!(coats[2].opacity, 0.);
        assert_ne!(a.bump, b.bump, "clean metal retains intrinsic brush relief");
        assert_ne!(
            a.layers[0].tint, b.layers[0].tint,
            "clean metal retains source mottling"
        );
        let mut owned = a.clone();
        owned.layers[0].coatings[0].tint = Some([9.; 3]);
        same_sample(&metal.sample(0.17, 0.29).unwrap(), &a);
    }
}

#[test]
fn wrapped_metal_is_periodic_with_integer_source_repeats_and_continuous_seams() {
    let mut s = fixture().cases.remove(3).settings;
    for repeat in [0.2, 3.75, 6.25] {
        s.substrate.repeat = repeat;
        let metal = Metal::new(&s, None).unwrap();
        let period = s.wrap_u.unwrap();
        for u in [-0.13, 0., 0.071, period] {
            for v in [-0.01, 0.27, 0.61] {
                let a = metal.sample(u, v).unwrap();
                let b = metal.sample(u + period, v).unwrap();
                for (a, b) in channels(&a).into_iter().zip(channels(&b)) {
                    close(a, b);
                }
                close(
                    b.layers[0].uv[0] - a.layers[0].uv[0],
                    if repeat == 0.2 {
                        1.
                    } else if repeat == 3.75 {
                        2.
                    } else {
                        3.
                    },
                );
                close(
                    b.layers[0].coatings[0].uv[0] - a.layers[0].coatings[0].uv[0],
                    2.,
                );
            }
        }
        let delta = 1e-7;
        let left = metal.sample(-delta, 0.27).unwrap();
        let at = metal.sample(0., 0.27).unwrap();
        let right = metal.sample(delta, 0.27).unwrap();
        for ((l, a), r) in channels(&left)
            .into_iter()
            .zip(channels(&at))
            .zip(channels(&right))
        {
            assert!((l - r).abs() < 1e-4, "seam value");
            assert!(
                ((a - l) / delta - (r - a) / delta).abs() < 0.03,
                "seam derivative"
            );
        }
    }
}

#[test]
fn exposure_direction_translated_coordinates_and_noise_intervals_are_consistent() {
    let mut s = fixture().cases.remove(0).settings;
    s.contacts = Some(vec![[0.5, 0.4]]);
    s.edge_axes = Some([false, false]);
    let metal = Metal::new(&s, None).unwrap();
    let down = metal.sample(0.5, 0.3).unwrap().layers[0].coatings[2].opacity;
    let up = metal.sample(0.5, 0.5).unwrap().layers[0].coatings[2].opacity;
    assert!(down > up + 0.1);
    s.metric = Some(Metric {
        scale: [1.; 2],
        gravity: [0.; 2],
    });
    let stationary = Metal::new(&s, None).unwrap();
    close(
        stationary.sample(0.5, 0.3).unwrap().layers[0].coatings[2].opacity,
        stationary.sample(0.5, 0.5).unwrap().layers[0].coatings[2].opacity,
    );
    s.folds = vec![
        Fold {
            axis: 0,
            at: 0.2,
            concave: true,
        },
        Fold {
            axis: 1,
            at: 0.4,
            concave: false,
        },
    ];
    let mut shifted = s.clone();
    let offset = [3., -2.];
    for i in 0..4 {
        shifted.bounds[i] += offset[i % 2];
    }
    for p in shifted.contacts.as_mut().unwrap() {
        for i in 0..2 {
            p[i] += offset[i];
        }
    }
    for f in &mut shifted.folds {
        f.at += offset[f.axis];
    }
    let first = Metal::new(&s, None).unwrap();
    let translated = Metal::new(&shifted, None).unwrap();
    for (u, v) in [(0.2, 0.3), (0.17, 0.29), (0.7, 0.4)] {
        for (a, b) in channels(&first.sample(u, v).unwrap())
            .into_iter()
            .zip(channels(
                &translated.sample(u + offset[0], v + offset[1]).unwrap(),
            ))
        {
            close(a, b);
        }
    }
    for bounds in [
        [-0.13, -0.27, 0.42, 0.65],
        [-2., -1., 3., 4.],
        [0.2, 0.3, 0.2, 0.3],
    ] {
        let range = fractal_noise_range(bounds, 37., 4).unwrap();
        for row in 0..31 {
            for col in 0..31 {
                let x = bounds[0] + (bounds[2] - bounds[0]) * col as f64 / 30.;
                let y = bounds[1] + (bounds[3] - bounds[1]) * row as f64 / 30.;
                let value = fractal_noise(x, y, 37., 4);
                assert!(range[0] <= value && value <= range[1]);
            }
        }
    }
    assert_eq!(
        value_noise_range([-1e30, -1e30, 1e30, 1e30], 37.).unwrap(),
        [0., 1.]
    );
}

#[test]
fn invalid_exposure_and_sampler_failures_do_not_poison_later_sampling() {
    let base = fixture().cases.remove(0).settings;
    for value in [f64::NAN, f64::INFINITY, -1., 1.01] {
        let mut s = base.clone();
        s.wear = value;
        assert!(Metal::new(&s, None).is_err());
    }
    let mut s = base.clone();
    s.metric = Some(Metric {
        scale: [2., 1.],
        gravity: [0., -1.],
    });
    assert!(Metal::new(&s, None).is_err());
    s = base.clone();
    s.folds = vec![Fold {
        axis: 2,
        at: 0.,
        concave: false,
    }];
    assert!(Metal::new(&s, None).is_err());
    s = base.clone();
    s.contacts = Some(vec![[0.; 2]; 4097]);
    assert!(Metal::new(&s, None).is_err());
    s = base.clone();
    s.contacts = Some(vec![[f64::NAN, 0.]]);
    assert!(Metal::new(&s, None).is_err());
    for value in [f64::NAN, -0.1, 1.1] {
        let runoff = |_, _| Ok(value);
        let metal = Metal::new(&base, Some(&runoff)).unwrap();
        assert!(metal.sample(0., 0.).is_err());
    }
    let failing = Cell::new(true);
    let runoff = |_, _| {
        if failing.get() {
            Err("exposure failed".into())
        } else {
            Ok(0.2)
        }
    };
    let metal = Metal::new(&base, Some(&runoff)).unwrap();
    assert!(metal.sample(0., 0.).unwrap_err().contains("exposure"));
    failing.set(false);
    assert!(metal.sample(0., 0.).is_ok());
    assert!(metal.sample(f64::NAN, 0.).is_err());
    assert!(metal.sample(1e308, 1e308).is_err());
    let mut excessive_repeat = base.clone();
    excessive_repeat.substrate.repeat = 1e308;
    let metal = Metal::new(&excessive_repeat, None).unwrap();
    assert!(metal.sample(2., 0.).unwrap_err().contains("UV"));
    assert!(metal.sample(0., 0.).is_ok());
    excessive_repeat.wrap_u = Some(2.);
    assert!(
        Metal::new(&excessive_repeat, None)
            .unwrap()
            .sample(0.1, 0.)
            .unwrap_err()
            .contains("UV")
    );
    assert!(value_noise_range([1., 0., 0., 1.], 37.).is_err());
    assert!(fractal_noise_range([0., 0., 1., 1.], 37., 0).is_err());
}

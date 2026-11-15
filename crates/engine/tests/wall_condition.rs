use serde::Deserialize;
use std::collections::BTreeMap;
use superspec_aml::material::Material;
use superspec_engine::{
    materialspec::surface::{Inputs, RasterSettings, Sample, rasterize},
    worldspec::themes::kowloon::wall::{
        Backing, Coating, Condition, Ledge, Opening, Settings, WallSample, finish_seed,
    },
};

#[derive(Deserialize)]
struct Expected {
    sample: WallSample,
    deposit: [f64; 3],
    opening: [f64; 3],
    #[serde(rename = "plasterLoss")]
    plaster_loss: f64,
    plaster: Sample,
}
#[derive(Deserialize)]
struct Range {
    bounds: [f64; 4],
    loss: [f64; 2],
    plaster: [f64; 2],
    fracture: bool,
}
#[derive(Deserialize)]
struct Case {
    settings: Settings,
    points: Vec<[f64; 2]>,
    samples: Vec<Expected>,
    ranges: Vec<Range>,
    raster: RasterReference,
}
#[derive(Deserialize)]
struct RasterReference {
    bounds: [f64; 4],
    width: usize,
    height: usize,
    color: Vec<u8>,
    packed: Vec<u8>,
}
#[derive(Deserialize)]
struct Supplied {
    at: [f64; 2],
    seed: f64,
    target: f64,
    sample: WallSample,
    plaster: Sample,
}
#[derive(Deserialize)]
struct Seed {
    text: String,
    seed: u32,
}
#[derive(Deserialize)]
struct Fixture {
    coat: Coating,
    backing: Backing,
    cases: Vec<Case>,
    seeds: Vec<Seed>,
    supplied: Vec<Supplied>,
    materials: Vec<Material>,
}
fn close(actual: f64, expected: f64) {
    assert!((actual - expected).abs() < 2e-12, "{actual} != {expected}");
}
fn sample_channels(s: &WallSample) -> Vec<f64> {
    let mut values = s.tint.to_vec();
    values.extend([s.loss, s.tone, s.damp, s.runoff]);
    values
}

/// Frozen TypeScript vectors run unchanged on native and actual browser Wasm.
pub fn check_reference_walls() {
    let fixture: Fixture = serde_json::from_str(include_str!(
        "../../../tests/fixtures/rust/wall-condition.json"
    ))
    .unwrap();
    assert_eq!(fixture.cases.len(), 5);
    for seed in fixture.seeds {
        assert_eq!(finish_seed(&seed.text), seed.seed);
    }
    let wall = Condition::new(&fixture.cases[3].settings).unwrap();
    assert_eq!(fixture.supplied.len(), 3);
    for supplied in fixture.supplied {
        let actual = wall
            .plaster_sample_from(
                supplied.at,
                supplied.seed,
                &fixture.coat,
                &fixture.backing,
                &supplied.sample,
            )
            .unwrap();
        let backing = if supplied.target < 0. {
            0.15625
        } else if supplied.target > 0. {
            0.84375
        } else {
            0.5
        };
        close(actual.layers[0].weight, 1. - backing);
        close(actual.layers[1].weight, backing);
        close(
            actual.roughness.unwrap(),
            supplied.plaster.roughness.unwrap(),
        );
        assert_eq!(actual.height, 0.);
        for (a, b) in actual.layers.iter().zip(supplied.plaster.layers) {
            assert_eq!(a.material, b.material);
            assert_eq!(a.uv, b.uv);
            close(a.weight, b.weight);
            for (a, b) in a.tint.unwrap().into_iter().zip(b.tint.unwrap()) {
                close(a, b);
            }
        }
    }
    let materials = BTreeMap::from_iter(fixture.materials.into_iter().map(|m| (m.id.clone(), m)));
    let images = BTreeMap::new();
    let inputs = Inputs {
        materials: &materials,
        images: &images,
    };
    for case in fixture.cases {
        let wall = Condition::new(&case.settings).unwrap();
        let raster = rasterize(
            RasterSettings {
                bounds: case.raster.bounds,
                texels_per_meter: 16.,
                preserve: true,
                repeat: false,
                uv_coordinates: false,
                bump: false,
            },
            &|u, z| wall.plaster_sample(u, z, case.settings.seed, &fixture.coat, &fixture.backing),
            &inputs,
            1024 * 1024,
            &|| false,
        )
        .unwrap();
        assert_eq!(
            (raster.layout.width, raster.layout.height),
            (case.raster.width, case.raster.height)
        );
        assert_eq!(raster.layers.len(), 1);
        assert_eq!(raster.layers[0].color.rgba, case.raster.color);
        assert_eq!(raster.layers[0].metallic_roughness.rgba, case.raster.packed);
        assert_eq!(case.points.len(), case.samples.len());
        for ([u, z], expected) in case.points.into_iter().zip(case.samples) {
            let actual = wall.sample(u, z).unwrap();
            for (a, b) in sample_channels(&actual)
                .into_iter()
                .zip(sample_channels(&expected.sample))
            {
                close(a, b);
            }
            for (a, b) in wall
                .deposit_tint(u, z)
                .unwrap()
                .into_iter()
                .zip(expected.deposit)
            {
                close(a, b);
            }
            for (a, b) in wall
                .opening_tint(u, z)
                .unwrap()
                .into_iter()
                .zip(expected.opening)
            {
                close(a, b);
            }
            close(
                wall.plaster_loss(u, z, case.settings.seed).unwrap(),
                expected.plaster_loss,
            );
            let plaster = wall
                .plaster_sample(u, z, case.settings.seed, &fixture.coat, &fixture.backing)
                .unwrap();
            assert_eq!(plaster.height, 0.);
            assert!(plaster.bump.is_none());
            assert!(plaster.height_weight.is_none());
            assert!(plaster.layer_elevation_weight.is_none());
            close(
                plaster.roughness.unwrap(),
                expected.plaster.roughness.unwrap(),
            );
            assert_eq!(plaster.layers.len(), 2);
            close(plaster.layers.iter().map(|l| l.weight).sum(), 1.);
            for (a, b) in plaster.layers.iter().zip(expected.plaster.layers) {
                assert_eq!(a.material, b.material);
                assert_eq!(a.uv, b.uv);
                assert!(a.coatings.is_empty());
                assert!(a.roughness.is_none());
                close(a.weight, b.weight);
                for (a, b) in a.tint.unwrap().into_iter().zip(b.tint.unwrap()) {
                    close(a, b);
                }
            }
        }
        for range in case.ranges {
            for (a, b) in wall
                .loss_range(range.bounds)
                .unwrap()
                .into_iter()
                .zip(range.loss)
            {
                close(a, b);
            }
            for (a, b) in wall
                .plaster_range(range.bounds)
                .unwrap()
                .into_iter()
                .zip(range.plaster)
            {
                close(a, b);
            }
            assert_eq!(wall.needs_fracture(range.bounds).unwrap(), range.fracture);
        }
    }
}
#[test]
fn frozen_wall_programs_preserve_samples_layers_ranges_and_unicode_seeds() {
    check_reference_walls();
}

fn settings() -> Settings {
    Settings {
        base: Some(0.),
        ledges: vec![Ledge {
            u0: 0.,
            u1: 1.,
            z: 2.,
            flow: 0.8,
        }],
        openings: vec![Opening {
            u0: 0.,
            u1: 1.,
            z0: 0.5,
            z1: 1.5,
        }],
        grime: 0.7,
        wear: 0.8,
        seed: 1234.,
    }
}

#[test]
fn moisture_support_accumulation_and_independent_controls_have_physical_meanings() {
    let s = settings();
    let wall = Condition::new(&s).unwrap();
    for [u, z] in [
        [0.5, 2.001],
        [0.5, 2.],
        [0.5, -0.801],
        [-0.15, 1.99],
        [1.15 + 1e-10, 1.99],
    ] {
        assert_eq!(wall.runoff(u, z).unwrap(), 0.);
    }
    assert!(wall.runoff(0.5, 1.996).unwrap() > 0.);
    assert_eq!(wall.opening_tint(-0.65, 1.).unwrap(), [1.; 3]);
    assert_eq!(wall.opening_tint(0.5, 2.15 + 1e-10).unwrap(), [1.; 3]);
    assert_eq!(wall.sample(0., 0.).unwrap().damp, 1.);
    assert!(wall.sample(0., 3.).unwrap().damp > 0.);
    let mut double = s.clone();
    double.ledges.push(s.ledges[0].clone());
    let double = Condition::new(&double).unwrap();
    let r = wall.runoff(0.2, 1.98).unwrap();
    assert!(r > 0. && r < 1.);
    close(double.runoff(0.2, 1.98).unwrap(), 1. - (1. - r).powi(2));
    let mut clean = s.clone();
    clean.grime = 0.;
    clean.openings.clear();
    let clean = Condition::new(&clean).unwrap();
    let mut intact = s.clone();
    intact.wear = 0.;
    let intact = Condition::new(&intact).unwrap();
    for [u, z] in [[0.1, 0.2], [0.5, 1.], [1., 1.99], [0.1, 4.]] {
        let a = wall.sample(u, z).unwrap();
        let b = clean.sample(u, z).unwrap();
        let c = intact.sample(u, z).unwrap();
        assert_eq!(clean.deposit_tint(u, z).unwrap(), [1.; 3]);
        assert_eq!(clean.opening_tint(u, z).unwrap(), [1.; 3]);
        close(a.loss, b.loss);
        close(a.damp, c.damp);
        close(a.runoff, c.runoff);
        assert_eq!(c.loss, -1.);
        assert_eq!(intact.plaster_loss(u, z, 47.).unwrap(), -1.);
    }
    assert_eq!(intact.loss_range([-3., -1., 4., 5.]).unwrap(), [-1.; 2]);
    assert_eq!(intact.plaster_range([-3., -1., 4., 5.]).unwrap(), [-1.; 2]);
    assert_ne!(clean.sample(0.1, 0.2).unwrap().tone, 1.);
}

#[test]
fn conservative_bounds_enclose_fields_but_still_prove_uniform_finish_regions() {
    let mut s = settings();
    s.wear = 1.;
    s.ledges.clear();
    let wall = Condition::new(&s).unwrap();
    let exposed = [-10., 0., -9.999, 0.001];
    assert!(wall.plaster_range(exposed).unwrap()[0] > 0.);
    assert!(!wall.needs_fracture(exposed).unwrap());
    let protected = [0., 10., 0.001, 10.001];
    assert!(wall.plaster_range(protected).unwrap()[1] < 0.);
    assert!(!wall.needs_fracture(protected).unwrap());
    let wall = Condition::new(&settings()).unwrap();
    for bounds in [
        [0., 0., 0., 0.],
        [-2., -0.001, 2., 0.001],
        [-0.151, 1.991, -0.149, 2.001],
        [0.2, 1.991, 0.2, 2.009],
        [-5., -1., 5., 5.],
    ] {
        let coarse = wall.loss_range(bounds).unwrap();
        let fine = wall.plaster_range(bounds).unwrap();
        if coarse[1] > -0.9 {
            close(fine[0], coarse[0] - 0.02);
            close(fine[1], coarse[1] + 0.02);
        } else {
            assert_eq!(coarse, fine);
        }
        for i in 0..=20 {
            for j in 0..=20 {
                let u = bounds[0] + (bounds[2] - bounds[0]) * i as f64 / 20.;
                let z = bounds[1] + (bounds[3] - bounds[1]) * j as f64 / 20.;
                let sample = wall.sample(u, z).unwrap().loss;
                assert!(sample >= coarse[0] && sample <= coarse[1]);
                for seed in [-11., 0., 9999.] {
                    let sample = wall.plaster_loss(u, z, seed).unwrap();
                    assert!(sample >= fine[0] && sample <= fine[1]);
                }
            }
        }
    }
    assert!(wall.needs_fracture([-5., -1., 5., 5.]).unwrap());
}

#[test]
fn owned_exposure_supports_infinite_ends_and_rejects_invalid_work_without_poisoning() {
    let mut s = settings();
    let original = Condition::new(&s).unwrap();
    let sample = original.sample(0.1, 1.99).unwrap();
    s.ledges[0].flow = 0.;
    s.openings.clear();
    s.grime = 0.;
    s.wear = 0.;
    assert_eq!(
        sample_channels(&sample),
        sample_channels(&original.sample(0.1, 1.99).unwrap())
    );
    for (u0, u1) in [
        (f64::NEG_INFINITY, 1.),
        (0., f64::INFINITY),
        (f64::NEG_INFINITY, f64::INFINITY),
    ] {
        s.wear = 0.8;
        s.ledges = vec![Ledge {
            u0,
            u1,
            z: 2.,
            flow: 1.,
        }];
        let wall = Condition::new(&s).unwrap();
        assert!(wall.runoff(0.5, 1.99).unwrap() > 0.);
        assert!(wall.sample(0.5, 1.99).unwrap().runoff.is_finite());
        for u in [-100., 100.] {
            let active = u >= u0 && u <= u1;
            assert_eq!(wall.runoff(u, 1.99).unwrap() > 0., active);
            let bounds = [u - 0.01, 1.98, u + 0.01, 2.01];
            let coarse = wall.loss_range(bounds).unwrap();
            let fine = wall.plaster_range(bounds).unwrap();
            assert!(coarse.iter().chain(&fine).all(|v| v.is_finite()));
            assert!(coarse[0] <= coarse[1] && fine[0] <= fine[1]);
            for z in [1.98, 1.996, 2., 2.01] {
                let loss = wall.sample(u, z).unwrap().loss;
                assert!(loss >= coarse[0] && loss <= coarse[1]);
                let loss = wall.plaster_loss(u, z, 37.).unwrap();
                assert!(loss >= fine[0] && loss <= fine[1]);
            }
        }
    }
    let valid = settings();
    for field in [f64::NAN, f64::INFINITY, -0.1, 1.1] {
        let mut bad = valid.clone();
        bad.grime = field;
        assert!(Condition::new(&bad).is_err());
    }
    let mut too_many = valid.clone();
    too_many.ledges = vec![valid.ledges[0].clone(); 4097];
    assert!(Condition::new(&too_many).is_err());
    let mut no_wear = valid.clone();
    no_wear.wear = 0.;
    let wall = Condition::new(&no_wear).unwrap();
    for bounds in [
        [1., 0., 0., 1.],
        [0., f64::NAN, 1., 1.],
        [0., 0., f64::INFINITY, 1.],
    ] {
        assert!(wall.loss_range(bounds).is_err());
    }
    assert!(wall.sample(f64::NAN, 0.).is_err());
    assert!(wall.sample(1e8, 0.).is_err());
    assert!(wall.plaster_loss(0., 0., f64::INFINITY).is_err());
    assert_eq!(wall.sample(0., 0.).unwrap().loss, -1.);
}

#[test]
fn supplied_condition_samples_control_composition_without_displacement_or_resampling() {
    let wall = Condition::new(&settings()).unwrap();
    let coat = Coating {
        material: "coat".into(),
        uv: [0.2, 0.3],
        tint: [0.4, 0.5, 0.6],
    };
    let backing = Backing {
        material: "backing".into(),
        uv: [0.7, 0.8],
    };
    let mut sample = WallSample {
        tint: [0.2, 0.3, 0.4],
        loss: -1.,
        tone: 0.8,
        damp: 0.25,
        runoff: 0.75,
    };
    let intact = wall
        .plaster_sample_from([0., 0.], 11., &coat, &backing, &sample)
        .unwrap();
    assert_eq!(intact.layers[0].weight, 1.);
    assert_eq!(intact.layers[1].weight, 0.);
    for (a, b) in intact.layers[0]
        .tint
        .unwrap()
        .into_iter()
        .zip([0.08, 0.15, 0.24])
    {
        close(a, b);
    }
    close(intact.roughness.unwrap(), 0.9 - 0.75 * 0.7 * 0.09);
    assert_eq!(intact.height, 0.);
    sample.loss = 1.;
    let exposed = wall
        .plaster_sample_from([0., 0.], 11., &coat, &backing, &sample)
        .unwrap();
    assert_eq!(exposed.layers[0].weight, 0.);
    assert_eq!(exposed.layers[1].weight, 1.);
    for (a, b) in exposed.layers[1]
        .tint
        .unwrap()
        .into_iter()
        .zip([0.168, 0.252, 0.336])
    {
        close(a, b);
    }
    close(exposed.roughness.unwrap(), 0.97 - 0.75 * 0.7 * 0.09);
    assert_eq!(exposed.height, 0.);
    sample.loss = f64::NAN;
    assert!(
        wall.plaster_sample_from([0., 0.], 11., &coat, &backing, &sample)
            .is_err()
    );
    assert!(wall.plaster_sample(0., 0., 11., &coat, &backing).is_ok());
}

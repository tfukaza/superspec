use serde::Deserialize;
use std::f64::consts::PI;
use superspec_engine::worldspec::themes::kowloon::pipe_runoff::{Field, Ring, Surface};

#[derive(Deserialize)]
struct Geometry {
    u: f64,
    v: f64,
    point: [f64; 3],
    downhill: [f64; 2],
}
#[derive(Deserialize)]
struct Sample {
    u: f64,
    v: f64,
    value: f64,
}
#[derive(Deserialize)]
struct Case {
    name: String,
    rings: Vec<Ring>,
    circumference: f64,
    seed: f64,
    seams: Vec<f64>,
    geometry: Vec<Geometry>,
    samples: Vec<Sample>,
}
#[derive(Deserialize)]
struct Cases {
    cases: Vec<Case>,
}
pub fn check_reference_pipe_runoff() {
    let fixture: Cases = serde_json::from_str(include_str!(
        "../../../tests/fixtures/rust/pipe-runoff.json"
    ))
    .unwrap();
    for case in fixture.cases {
        let surface = Surface::new(case.rings, case.circumference).unwrap();
        for expected in case.geometry {
            let point = surface.point(expected.u, expected.v).unwrap();
            let down = surface.downhill(expected.u, expected.v).unwrap();
            for (a, b) in point
                .into_iter()
                .chain(down)
                .zip(expected.point.into_iter().chain(expected.downhill))
            {
                assert!((a - b).abs() < 1e-10, "{} geometry: {a} != {b}", case.name);
            }
        }
        let field = Field::new(&surface, &case.seams, case.seed, &|| false).unwrap();
        for sample in case.samples {
            let value = field.sample(sample.u, sample.v).unwrap();
            assert!(
                (value - sample.value).abs() < 1e-7,
                "{} runoff: {value} != {} at {},{}",
                case.name,
                sample.value,
                sample.u,
                sample.v
            );
        }
    }
}
#[test]
fn frozen_pipe_points_gravity_and_transport_match_source() {
    check_reference_pipe_runoff();
}

fn cylinder(direction: f64) -> Vec<Ring> {
    [0., 0.2]
        .into_iter()
        .map(|distance| Ring {
            center: [0., 0., distance * direction],
            radial: [1., 0., 0.],
            across: [0., 1., 0.],
            radius: 0.02,
            distance,
        })
        .collect()
}
#[test]
fn physical_gravity_periodicity_and_directional_transport() {
    let circumference = 2. * PI * 0.02;
    let up = Surface::new(cylinder(1.), circumference).unwrap();
    let down = Surface::new(cylinder(-1.), circumference).unwrap();
    let rising = Field::new(&up, &[0.1], 41., &|| false).unwrap();
    let falling = Field::new(&down, &[0.1], 41., &|| false).unwrap();
    for u in [
        0.,
        0.21 * circumference,
        0.58 * circumference,
        -0.17 * circumference,
    ] {
        assert_eq!(up.downhill(u, 0.1).unwrap(), [0., -1.]);
        assert_eq!(down.downhill(u, 0.1).unwrap(), [0., 1.]);
        assert!(rising.sample(u, 0.03).unwrap() > falling.sample(u, 0.03).unwrap());
        assert!(falling.sample(u, 0.17).unwrap() > rising.sample(u, 0.17).unwrap());
        for v in [-0.01, 0., 0.03, 0.1, 0.2, 0.21] {
            let value = rising.sample(u, v).unwrap();
            assert!((0. ..=1.).contains(&value));
            assert!((rising.sample(u + circumference, v).unwrap() - value).abs() < 1e-10);
        }
    }
    let rings = [0., 0.2]
        .into_iter()
        .map(|distance| Ring {
            center: [distance, 0., 0.],
            radial: [0., 0., 1.],
            across: [0., -1., 0.],
            radius: 0.02,
            distance,
        })
        .collect();
    let horizontal = Surface::new(rings, circumference).unwrap();
    assert_eq!(horizontal.point(0., 0.1).unwrap(), [0.1, 0., 0.02]);
    assert_eq!(horizontal.downhill(0., 0.1).unwrap(), [0., 0.]);
    for (fraction, expected) in [(0.25, 1.), (0.75, -1.)] {
        let down = horizontal.downhill(fraction * circumference, 0.1).unwrap();
        assert!((down[0] - expected).abs() < 1e-12 && down[1].abs() < 1e-12);
    }
}

#[test]
fn invalid_and_cancelled_fields_reject_without_poisoning_fresh_work() {
    let rings = cylinder(1.);
    let surface = Surface::new(rings.clone(), 0.12).unwrap();
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(Field::new(&surface, &[value], 1., &|| false).is_err());
        assert!(Field::new(&surface, &[], value, &|| false).is_err());
        assert!(surface.point(value, 0.).is_err());
    }
    for bad in 0..5 {
        let mut input = rings.clone();
        match bad {
            0 => input[0].distance = 0.1,
            1 => input[1].distance = 0.,
            2 => input[1].radius = 0.,
            3 => input[1].radial = [0.; 3],
            _ => input[1].across = input[1].radial,
        };
        assert!(Surface::new(input, 0.12).is_err());
    }
    let mut collapsed = rings.clone();
    collapsed[1].radial = [-1., 0., 0.];
    assert!(
        Field::new(&Surface::new(collapsed, 0.12).unwrap(), &[0.1], 1., &|| {
            false
        })
        .is_err()
    );
    let checks = std::cell::Cell::new(0);
    assert!(
        Field::new(&surface, &[0.1], 1., &|| {
            checks.set(checks.get() + 1);
            checks.get() > 3
        })
        .err()
        .unwrap()
        .contains("cancelled")
    );
    // This short cylinder has eleven initialization rows; the later checkpoint is in transport.
    checks.set(0);
    assert!(
        Field::new(&surface, &[0.1], 1., &|| {
            checks.set(checks.get() + 1);
            checks.get() > 14
        })
        .err()
        .unwrap()
        .contains("cancelled")
    );
    let mut huge = rings;
    huge[1].distance = 10.;
    huge[1].center = [0., 0., 10.];
    assert!(
        Field::new(&Surface::new(huge, 10.).unwrap(), &[0.5; 109], 1., &|| {
            false
        })
        .err()
        .unwrap()
        .contains("work budget")
    );
    let empty = Field::new(&surface, &[], 1., &|| false).unwrap();
    assert_eq!(empty.sample(0., 0.).unwrap(), 0.);
    assert!(
        Field::new(&surface, &[0.1], 1., &|| false)
            .unwrap()
            .sample(0., 0.1)
            .unwrap()
            > 0.
    );
}

use std::f64::consts::PI;
use superspec_engine::worldspec::themes::kowloon::pipe_runoff::{Field, Ring, Surface};

#[test]
fn bent_pipe_points_lie_on_each_ring_and_runoff_stays_bounded() {
    let s = std::f64::consts::FRAC_1_SQRT_2;
    let rings = vec![
        Ring {
            center: [0., 0., 0.],
            radial: [1., 0., 0.],
            across: [0., 1., 0.],
            radius: 0.03,
            distance: 0.,
        },
        Ring {
            center: [0.3 - 0.3 * s, 0., 0.3 * s],
            radial: [s, 0., -s],
            across: [0., 1., 0.],
            radius: 0.04,
            distance: 0.075 * PI,
        },
        Ring {
            center: [0.3, 0., 0.3],
            radial: [0., 0., -1.],
            across: [0., 1., 0.],
            radius: 0.03,
            distance: 0.15 * PI,
        },
    ];
    let circumference = 0.06 * PI;
    let surface = Surface::new(rings.clone(), circumference).unwrap();
    for ring in &rings {
        let at = surface.point(0., ring.distance).unwrap();
        for axis in 0..3 {
            let expected = ring.center[axis] + ring.radius * ring.radial[axis];
            assert!((at[axis] - expected).abs() < 1e-10);
        }
        for fraction in [0.25, 0.5, 0.75] {
            let at = surface
                .point(fraction * circumference, ring.distance)
                .unwrap();
            let distance = (0..3)
                .map(|axis| (at[axis] - ring.center[axis]).powi(2))
                .sum::<f64>()
                .sqrt();
            assert!((distance - ring.radius).abs() < 1e-10);
        }
    }
    let field = Field::new(&surface, &[0.03, 0.4], 41., &|| false).unwrap();
    for u in [0., 0.3, 0.6, 0.9] {
        for v in [0., 0.1, 0.2, 0.3, 0.4, 0.47] {
            let value = field.sample(u * circumference, v).unwrap();
            assert!((0. ..=1.).contains(&value));
            let down = surface.downhill(u * circumference, v).unwrap();
            assert!(down.iter().all(|d| d.is_finite()));
        }
    }
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

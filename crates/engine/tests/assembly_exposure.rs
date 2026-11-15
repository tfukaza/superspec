use serde::Deserialize;
use std::cell::Cell;
use superspec_engine::worldspec::themes::kowloon::{exposure::Assembly, metal::Fold};
use superspec_geometry::V3;

#[derive(Deserialize)]
struct Member {
    a: V3,
    b: V3,
    radius: f64,
}
#[derive(Deserialize)]
struct Face {
    points: Vec<V3>,
    hint: V3,
    uv: Vec<[f64; 2]>,
}
#[derive(Deserialize)]
struct Case {
    inputs: Vec<Member>,
    members: Vec<Member>,
    joints: Vec<Vec<V3>>,
    faces: Vec<Face>,
    folds: Vec<Fold>,
}
#[derive(Deserialize)]
struct Fixture {
    cases: Vec<Case>,
}
fn near(a: f64, b: f64) {
    assert!((a - b).abs() < 2e-12, "{a} != {b}");
}
/// Exercises the finalized assembly contract on native and actual browser Wasm.
pub fn check_reference_exposure() {
    let fixture: Fixture = serde_json::from_str(include_str!(
        "../../../tests/fixtures/rust/assembly-exposure.json"
    ))
    .unwrap();
    for case in fixture.cases {
        let mut assembly = Assembly::default();
        for (i, m) in case.inputs.iter().enumerate() {
            assert_eq!(assembly.member(m.a, m.b, m.radius).unwrap(), i);
        }
        for f in case.faces {
            assembly.face(&f.points, f.hint, &f.uv).unwrap();
        }
        let output = assembly.finish(&|| false).unwrap();
        assert_eq!(output.members.len(), case.members.len());
        assert_eq!(case.joints.len(), case.members.len());
        for (i, (a, b)) in output.members.iter().zip(case.members).enumerate() {
            for (a, b) in
                a.a.into_iter()
                    .chain(a.b)
                    .chain([a.radius])
                    .zip(b.a.into_iter().chain(b.b).chain([b.radius]))
            {
                near(a, b);
            }
            let joints = output.joints(i).unwrap();
            assert_eq!(joints.len(), case.joints[i].len());
            for (a, b) in joints.iter().flatten().zip(case.joints[i].iter().flatten()) {
                near(*a, *b);
            }
        }
        assert_eq!(output.folds.len(), case.folds.len());
        for (a, b) in output.folds.iter().zip(case.folds) {
            assert_eq!(a.axis, b.axis);
            assert_eq!(a.concave, b.concave);
            near(a.at, b.at);
        }
    }
}
#[test]
fn skew_contacts_and_fold_signs_match_frozen_reference() {
    check_reference_exposure();
}

#[test]
fn crossings_endpoint_joints_and_zero_length_members_have_owned_contact_points() {
    let mut assembly = Assembly::default();
    let a = assembly.member([-1., 0., 0.], [1., 0., 0.], 0.05).unwrap();
    let b = assembly.member([0., -1., 0.], [0., 1., 0.], 0.05).unwrap();
    let c = assembly.member([1., 0., 0.], [2., 0., 0.], 0.05).unwrap();
    let point = assembly.member([0.5, 0., 0.], [0.5, 0., 0.], 0.1).unwrap();
    let isolated = assembly.member([0., 0., 0.2], [0., 0., 1.], 0.05).unwrap();
    let exposure = assembly.finish(&|| false).unwrap();
    assert_eq!(
        exposure.joints(a).unwrap(),
        &[[0., 0., 0.], [0.95, 0., 0.], [0.5, 0., 0.]]
    );
    assert_eq!(exposure.joints(b).unwrap(), &[[0., 0., 0.]]);
    assert_eq!(exposure.joints(c).unwrap(), &[[1.05, 0., 0.]]);
    assert_eq!(exposure.members[point].radius, 0.001);
    assert_eq!(exposure.joints(point).unwrap(), &[[0.5, 0., 0.]]);
    assert!(exposure.joints(isolated).unwrap().is_empty());
    assert!(exposure.joints(usize::MAX).is_none());
}

#[test]
fn malformed_and_excessive_assemblies_fail_without_publishing_partial_exposure() {
    let mut assembly = Assembly::default();
    for radius in [-1., f64::NAN, f64::INFINITY, 10001.] {
        assert!(assembly.member([0.; 3], [1.; 3], radius).is_err());
    }
    assert!(assembly.member([f64::NAN; 3], [1.; 3], 0.1).is_err());
    assert_eq!(assembly.member([0.; 3], [1.; 3], 0.1).unwrap(), 0);
    assert!(
        assembly
            .face(&[[0.; 3]; 3], [0., 0., 1.], &[[0.; 2]; 2])
            .is_err()
    );
    assert!(
        assembly
            .face(&[[0.; 3]; 3], [0., 0., 1.], &[[f64::INFINITY; 2]; 3])
            .is_err()
    );
    assert_eq!(assembly.finish(&|| false).unwrap().members.len(), 1);
    let mut crowded = Assembly::default();
    for _ in 0..257 {
        crowded.member([0.; 3], [1.; 3], 0.1).unwrap();
    }
    assert!(
        crowded
            .finish(&|| false)
            .err()
            .unwrap()
            .contains("contact budget")
    );
    let mut many = Assembly::default();
    for i in 0..2048 {
        many.member([i as f64, 0., 0.], [i as f64, 0., 1.], 0.1)
            .unwrap();
    }
    assert!(
        many.member([0.; 3], [1.; 3], 0.1)
            .unwrap_err()
            .contains("2048")
    );
    let calls = Cell::new(0);
    let cancelled = || {
        calls.set(calls.get() + 1);
        calls.get() > 4
    };
    assert!(many.finish(&cancelled).err().unwrap().contains("cancel"));
    assert!(calls.get() > 4);
    let mut recovered = Assembly::default();
    recovered.member([0.; 3], [1.; 3], 0.1).unwrap();
    assert!(recovered.finish(&|| false).is_ok());
}

#[test]
fn constant_v_folds_coplanar_edges_and_contact_tolerance_have_distinct_boundaries() {
    for tilt in [0., 0.5] {
        let mut assembly = Assembly::default();
        assembly
            .face(
                &[[0., 0., 0.], [1., 0., 0.], [1., 1., 0.], [0., 1., 0.]],
                [0., 0., 1.],
                &[[0., 0.], [1., 0.], [1., 1.], [0., 1.]],
            )
            .unwrap();
        assembly
            .face(
                &[[0., 1., 0.], [1., 1., 0.], [1., 2., tilt], [0., 2., tilt]],
                [0., -tilt, 1.],
                &[[0., 1.], [1., 1.], [1., 2.], [0., 2.]],
            )
            .unwrap();
        let exposure = assembly.finish(&|| false).unwrap();
        if tilt == 0. {
            assert!(exposure.folds.is_empty());
        } else {
            assert_eq!(exposure.folds.len(), 1);
            assert_eq!(exposure.folds[0].axis, 1);
            assert_eq!(exposure.folds[0].at, 1.);
            assert!(exposure.folds[0].concave);
        }
    }
    let threshold = 0.05 + 0.05 + 0.001;
    for (distance, contacts) in [(threshold - 1e-8, 1), (threshold, 1), (threshold + 1e-8, 0)] {
        let mut assembly = Assembly::default();
        let a = assembly.member([-1., 0., 0.], [1., 0., 0.], 0.05).unwrap();
        assembly
            .member([0., -1., distance], [0., 1., distance], 0.05)
            .unwrap();
        let exposure = assembly.finish(&|| false).unwrap();
        assert_eq!(exposure.joints(a).unwrap().len(), contacts);
        if contacts == 1 {
            assert_eq!(exposure.joints(a).unwrap()[0], [0., 0., 0.]);
        }
    }
}

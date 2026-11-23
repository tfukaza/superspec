use std::cell::Cell;
use superspec_engine::worldspec::themes::kowloon::exposure::Assembly;

#[test]
fn hinged_faces_fold_concave_when_raised_and_convex_when_lowered() {
    for (rise, concave) in [(0.5, true), (-0.5, false)] {
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
                &[[1., 0., 0.], [2., 0., rise], [2., 1., rise], [1., 1., 0.]],
                [-rise, 0., 1.],
                &[[1., 0.], [2., 0.], [2., 1.], [1., 1.]],
            )
            .unwrap();
        let output = assembly.finish(&|| false).unwrap();
        assert_eq!(output.folds.len(), 1);
        assert_eq!(output.folds[0].axis, 0);
        assert_eq!(output.folds[0].at, 1.);
        assert_eq!(output.folds[0].concave, concave);
    }
}

#[test]
fn skew_member_contacts_lie_on_both_members_and_preserve_member_order() {
    // Two skew members whose closest approach is within their summed radii.
    let mut assembly = Assembly::default();
    let members = [
        ([-1., 0., 0.], [1., 0., 0.], 0.05),
        ([0.3, -1., 0.04], [0.3, 1., 0.06], 0.05),
    ];
    for (i, (a, b, r)) in members.iter().enumerate() {
        assert_eq!(assembly.member(*a, *b, *r).unwrap(), i);
    }
    let output = assembly.finish(&|| false).unwrap();
    assert_eq!(output.members.len(), members.len());
    for (i, (a, b, r)) in members.iter().enumerate() {
        // Free ends may be inset by at most the member radius, along the member.
        let m = &output.members[i];
        assert_eq!(m.radius, *r);
        for (got, end, other) in [(m.a, a, b), (m.b, b, a)] {
            let offset: Vec<f64> = (0..3).map(|k| got[k] - end[k]).collect();
            let length = offset.iter().map(|d| d * d).sum::<f64>().sqrt();
            assert!(length <= r + 1e-12);
            let axis: Vec<f64> = (0..3).map(|k| other[k] - end[k]).collect();
            let along = offset.iter().zip(&axis).map(|(o, x)| o * x).sum::<f64>();
            assert!(along >= 0.);
        }
    }
    let joints = output.joints(0).unwrap();
    assert_eq!(joints.len(), 1);
    let [x, y, z] = joints[0];
    assert!((x - 0.3).abs() < 1e-12 && y.abs() < 1e-12 && z.abs() < 1e-12);
    assert_eq!(output.joints(1).unwrap().len(), 1);
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

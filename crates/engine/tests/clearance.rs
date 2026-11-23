#[cfg(test)]
use superspec_engine::worldspec::themes::kowloon::clearance::CollisionStorage;
use superspec_engine::worldspec::themes::kowloon::{
    clearance::{Collision, GroundSupport, Obstacles, RenderTriangle, Volume},
    pen::Pen,
};
use superspec_geometry::obstructions::{Budget, Triangle};
use superspec_geometry::{Matrix, V3};
/// The closed unit cube with outward winding: CCW seen from above on top, CW on the bottom.
fn cube() -> Vec<Triangle> {
    let p = |x: f64, y: f64, z: f64| [x, y, z];
    vec![
        [p(0., 0., 1.), p(1., 0., 1.), p(1., 1., 1.)],
        [p(0., 0., 1.), p(1., 1., 1.), p(0., 1., 1.)],
        [p(0., 0., 0.), p(1., 1., 0.), p(1., 0., 0.)],
        [p(0., 0., 0.), p(0., 1., 0.), p(1., 1., 0.)],
        [p(0., 0., 0.), p(1., 0., 0.), p(1., 0., 1.)],
        [p(0., 0., 0.), p(1., 0., 1.), p(0., 0., 1.)],
        [p(0., 1., 0.), p(1., 1., 1.), p(1., 1., 0.)],
        [p(0., 1., 0.), p(0., 1., 1.), p(1., 1., 1.)],
        [p(0., 0., 0.), p(0., 1., 1.), p(0., 1., 0.)],
        [p(0., 0., 0.), p(0., 0., 1.), p(0., 1., 1.)],
        [p(1., 0., 0.), p(1., 1., 0.), p(1., 1., 1.)],
        [p(1., 0., 0.), p(1., 1., 1.), p(1., 0., 1.)],
    ]
}
fn obstacles(solid: Option<&str>, volumes: Vec<Volume>, budget: &mut Budget<'_>) -> Obstacles {
    Obstacles::new(
        cube()
            .into_iter()
            .map(|points| Collision {
                points,
                object: "box".into(),
                solid: solid.map(str::to_owned),
            })
            .collect(),
        volumes,
        budget,
    )
    .unwrap()
}
#[test]
fn closed_solids_contain_interior_points_unless_exempt_or_open() {
    let mut budget = Budget::new(4_000_000, &|| false).unwrap();
    let closed = obstacles(Some("box"), vec![], &mut budget);
    assert!(closed.contains([0.5, 0.5, 0.5], None, &mut budget).unwrap());
    assert!(!closed.contains([0.5, 0.5, 1.5], None, &mut budget).unwrap());
    assert!(
        !closed
            .contains([0.5, 0.5, -0.5], None, &mut budget)
            .unwrap()
    );
    assert!(!closed.contains([1.5, 0.5, 0.5], None, &mut budget).unwrap());
    assert!(
        !closed
            .contains([0.5, 0.5, 0.5], Some("box"), &mut budget)
            .unwrap()
    );
    let open = obstacles(None, vec![], &mut budget);
    assert!(!open.contains([0.5, 0.5, 0.5], None, &mut budget).unwrap());
}
#[test]
fn oriented_boxes_hit_collision_faces_and_foreign_volumes_only() {
    let mut budget = Budget::new(4_000_000, &|| false).unwrap();
    let volume = Volume {
        corners: vec![[5., 0., 0.], [6., 0., 0.], [6., 1., 0.], [5., 1., 1.]],
        owner: 1,
    };
    let o = obstacles(Some("box"), vec![volume], &mut budget);
    let identity = Pen::new(Matrix::default(), "query").unwrap();
    let mut blocked = |pen: &Pen, low: V3, high: V3, owner, exempt| {
        o.blocked(pen, low, high, owner, exempt, &mut budget)
            .unwrap()
    };
    // Straddling the top face touches collision; a box wholly inside or outside does not.
    assert!(blocked(
        &identity,
        [0.4, 0.4, 0.9],
        [0.6, 0.6, 1.1],
        0,
        None
    ));
    assert!(!blocked(
        &identity,
        [0.4, 0.4, 0.4],
        [0.6, 0.6, 0.6],
        0,
        None
    ));
    assert!(!blocked(&identity, [2., 2., 2.], [3., 3., 3.], 0, None));
    assert!(!blocked(
        &identity,
        [0.4, 0.4, 0.9],
        [0.6, 0.6, 1.1],
        0,
        Some("box")
    ));
    // A query frame moved onto the face reproduces the world-space hit.
    let moved = identity.moved([0.5, 0.5, 1.]).unwrap();
    assert!(blocked(
        &moved,
        [-0.1, -0.1, -0.1],
        [0.1, 0.1, 0.1],
        0,
        None
    ));
    // Foreign volumes block other owners but never their own.
    assert!(blocked(
        &identity,
        [5.2, 0.2, 0.2],
        [5.4, 0.4, 0.4],
        0,
        None
    ));
    assert!(!blocked(
        &identity,
        [5.2, 0.2, 0.2],
        [5.4, 0.4, 0.4],
        1,
        None
    ));
}
#[test]
fn ground_support_requires_visible_flat_faces_under_every_sample() {
    let mut budget = Budget::new(4_000_000, &|| false).unwrap();
    let floor = |visible| {
        vec![
            RenderTriangle {
                points: [[0., 0., 0.], [4., 0., 0.], [4., 4., 0.]],
                visible,
            },
            RenderTriangle {
                points: [[0., 0., 0.], [4., 4., 0.], [0., 4., 0.]],
                visible,
            },
        ]
    };
    let pen = Pen::new(Matrix::default(), "query").unwrap();
    let ground = GroundSupport::new(&floor(true), &mut budget).unwrap();
    assert!(
        ground
            .covers(&pen, [1., 2., 1., 2.], 0., &mut budget)
            .unwrap()
    );
    assert!(
        !ground
            .covers(&pen, [3., 5., 1., 2.], 0., &mut budget)
            .unwrap()
    );
    assert!(
        !ground
            .covers(&pen, [1., 2., 1., 2.], 0.5, &mut budget)
            .unwrap()
    );
    let hidden = GroundSupport::new(&floor(false), &mut budget).unwrap();
    assert!(
        !hidden
            .covers(&pen, [1., 2., 1., 2.], 0., &mut budget)
            .unwrap()
    );
}
#[test]
fn invalid_cancelled_or_over_budget_queries_fail_and_recover() {
    let mut budget = Budget::new(1000, &|| false).unwrap();
    let o = Obstacles::new(vec![], vec![], &mut budget).unwrap();
    let pen = Pen::new(Matrix::default(), "test").unwrap();
    let mut cancelled = Budget::new(1000, &|| true).unwrap();
    assert!(o.contains([0.; 3], None, &mut cancelled).is_err());
    assert!(
        o.blocked(&pen, [0.; 3], [1.; 3], 0, None, &mut cancelled)
            .is_err()
    );
    assert!(o.contains([f64::NAN, 0., 0.], None, &mut budget).is_err());
    assert!(
        o.blocked(&pen, [1.; 3], [0.; 3], 0, None, &mut budget)
            .is_err()
    );
    let ground = GroundSupport::new(&[], &mut budget).unwrap();
    assert!(
        ground
            .covers(
                &pen,
                [0., 10., 0., 10.],
                0.,
                &mut Budget::new(1, &|| false).unwrap()
            )
            .is_err()
    );
    assert!(
        ground
            .covers(
                &pen,
                [0., 1., 0., 1.],
                0.,
                &mut Budget::new(10000, &|| true).unwrap()
            )
            .is_err()
    );
    assert!(
        !ground
            .covers(&pen, [0., 1., 0., 1.], 0., &mut budget)
            .unwrap()
    );
}

#[test]
fn collision_storage_preflights_identity_growth_without_allocating_dense_meshes() {
    let mut storage = CollisionStorage::default();
    let object = "x".repeat(4096);
    storage.admit(4095, &object, None).unwrap();
    assert!(storage.admit(2, &object, None).is_err());
    storage.admit(1, &object, None).unwrap();
    assert!(storage.admit(1, "x", None).is_err());
    let mut storage = CollisionStorage::default();
    storage.admit(1_048_576, "x", None).unwrap();
    assert!(storage.admit(1, "x", None).is_err());
    let mut storage = CollisionStorage::default();
    assert!(storage.admit(usize::MAX, "x", Some("solid")).is_err());
    storage.admit(1, "valid", Some("solid")).unwrap();
}

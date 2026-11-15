use serde::Deserialize;
#[cfg(test)]
use superspec_engine::worldspec::themes::kowloon::clearance::CollisionStorage;
use superspec_engine::worldspec::themes::kowloon::{
    clearance::{Collision, GroundSupport, Obstacles, RenderTriangle, Volume},
    pen::Pen,
};
use superspec_geometry::obstructions::{Budget, Triangle};
use superspec_geometry::{Matrix, V3};
#[derive(Deserialize)]
struct Physical {
    points: Triangle,
    object: String,
    solid: Option<String>,
}
#[derive(Deserialize)]
struct Foreign {
    corners: Vec<V3>,
    owner: usize,
}
#[derive(Deserialize)]
struct Render {
    points: Triangle,
    visible: bool,
}
#[derive(Deserialize)]
struct Blocked {
    matrix: [V3; 4],
    low: V3,
    high: V3,
    owner: usize,
    exempt: Option<String>,
    result: bool,
}
#[derive(Deserialize)]
struct Contains {
    at: V3,
    exempt: Option<String>,
    result: bool,
}
#[derive(Deserialize)]
struct Covers {
    matrix: [V3; 4],
    rect: [f64; 4],
    z: f64,
    result: bool,
}
#[derive(Deserialize)]
struct Golden {
    collisions: Vec<Physical>,
    volumes: Vec<Foreign>,
    render: Vec<Render>,
    blocked: Vec<Blocked>,
    contains: Vec<Contains>,
    covers: Vec<Covers>,
}
pub fn check_reference_clearance() {
    let g: Golden =
        serde_json::from_str(include_str!("../../../tests/fixtures/rust/clearance.json")).unwrap();
    let mut budget = Budget::new(4_000_000, &|| false).unwrap();
    let obstacles = Obstacles::new(
        g.collisions
            .into_iter()
            .map(|t| Collision {
                points: t.points,
                object: t.object,
                solid: t.solid,
            })
            .collect(),
        g.volumes
            .into_iter()
            .map(|v| Volume {
                corners: v.corners,
                owner: v.owner,
            })
            .collect(),
        &mut budget,
    )
    .unwrap();
    let ground = GroundSupport::new(
        &g.render
            .into_iter()
            .map(|t| RenderTriangle {
                points: t.points,
                visible: t.visible,
            })
            .collect::<Vec<_>>(),
        &mut budget,
    )
    .unwrap();
    for (i, q) in g.blocked.into_iter().enumerate() {
        assert_eq!(
            obstacles
                .blocked(
                    &Pen::new(Matrix(q.matrix), "query").unwrap(),
                    q.low,
                    q.high,
                    q.owner,
                    q.exempt.as_deref(),
                    &mut budget
                )
                .unwrap(),
            q.result,
            "oriented box {i}"
        );
    }
    for (i, q) in g.contains.into_iter().enumerate() {
        assert_eq!(
            obstacles
                .contains(q.at, q.exempt.as_deref(), &mut budget)
                .unwrap(),
            q.result,
            "solid containment {i}"
        );
    }
    for (i, q) in g.covers.into_iter().enumerate() {
        assert_eq!(
            ground
                .covers(
                    &Pen::new(Matrix(q.matrix), "query").unwrap(),
                    q.rect,
                    q.z,
                    &mut budget
                )
                .unwrap(),
            q.result,
            "ground support {i}"
        );
    }
}
#[test]
fn clearance_preserves_closed_solids_exemptions_rotated_volumes_and_render_support() {
    check_reference_clearance();
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

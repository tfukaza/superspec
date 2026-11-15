use serde::Deserialize;
use std::collections::BTreeMap;
use superspec_engine::worldspec::themes::kowloon::seams::{self, Facet, Kind};
use superspec_geometry::V3;
use superspec_geometry::obstructions::Budget;
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Input {
    object: String,
    kind: String,
    points: [V3; 3],
    normal_z: f64,
    visible: bool,
    collidable: bool,
}
#[derive(Debug, Deserialize, PartialEq)]
struct Wall {
    host: String,
    seams: Vec<[V3; 2]>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Reference {
    triangles: Vec<Input>,
    by_object: BTreeMap<String, Vec<[V3; 2]>>,
    walls_by_floor: BTreeMap<String, Vec<Wall>>,
}
pub fn check_reference_contacts() {
    let reference: Reference = serde_json::from_str(include_str!(
        "../../../tests/fixtures/rust/contact-seams.json"
    ))
    .unwrap();
    let facets = || {
        reference.triangles.iter().map(|t| Facet {
            object: &t.object,
            kind: match t.kind.as_str() {
                "wall" => Kind::Wall,
                "floor" => Kind::Floor,
                "stairs" => Kind::Stairs,
                _ => Kind::Other,
            },
            points: t.points,
            normal_z: t.normal_z,
            visible: t.visible,
            collidable: t.collidable,
        })
    };
    let output = seams::contacts(facets(), &mut Budget::new(100_000, &|| false).unwrap()).unwrap();
    assert_eq!(output.by_object, reference.by_object);
    assert_eq!(
        output
            .walls_by_floor
            .into_iter()
            .map(|(f, w)| (
                f,
                w.into_iter()
                    .map(|w| Wall {
                        host: w.host,
                        seams: w.seams
                    })
                    .collect()
            ))
            .collect::<BTreeMap<_, Vec<_>>>(),
        reference.walls_by_floor
    );
    // Doorway [2,3] stays open; ownership order remains z-wall then a-wall.
    assert_eq!(
        reference.walls_by_floor["floor"]
            .iter()
            .map(|w| w.host.as_str())
            .collect::<Vec<_>>(),
        ["z-wall", "a-wall", "crossing"]
    );
    for wall in &reference.walls_by_floor["floor"][..2] {
        assert!(
            wall.seams
                .iter()
                .all(|s| s.iter().all(|p| p[0] <= 2.) || s.iter().all(|p| p[0] >= 3.))
        );
    }
    assert!(
        seams::contacts(facets(), &mut Budget::new(1, &|| false).unwrap())
            .err()
            .unwrap()
            .contains("work allowance")
    );
}
#[test]
fn finite_physical_contacts_match_source_with_doorways_and_filters() {
    check_reference_contacts();
}

#[test]
fn contact_limits_and_cancellation_leave_the_next_request_clean() {
    let triangle = Facet {
        object: "floor",
        kind: Kind::Floor,
        visible: true,
        collidable: true,
        points: [[0., 0., 0.], [1., 0., 0.], [0., 1., 0.]],
        normal_z: 1.,
    };
    let wall = Facet {
        object: "wall",
        kind: Kind::Wall,
        normal_z: 0.,
        points: [[0., 0., 0.], [1., 0., 0.], [1., 0., 1.]],
        ..triangle
    };
    let facets = || std::iter::repeat_n(triangle, 1500).chain(std::iter::repeat_n(wall, 1500));
    assert!(
        seams::contacts(facets(), &mut Budget::new(3_000_000, &|| false).unwrap())
            .err()
            .unwrap()
            .contains("contact budget")
    );
    assert!(
        seams::contacts([triangle, wall], &mut Budget::new(100, &|| true).unwrap())
            .err()
            .unwrap()
            .contains("cancelled")
    );
    let dense = std::iter::repeat_n(triangle, 150).chain(std::iter::repeat_n(wall, 150));
    assert!(
        seams::contacts(dense, &mut Budget::new(100_000, &|| false).unwrap())
            .err()
            .unwrap()
            .contains("output allowance")
    );
    let checkpoints = std::cell::Cell::new(0);
    let cancelled = || {
        checkpoints.set(checkpoints.get() + 1);
        checkpoints.get() > 5
    };
    assert!(
        seams::contacts(
            [triangle, triangle, wall, wall],
            &mut Budget::new(100, &cancelled).unwrap()
        )
        .err()
        .unwrap()
        .contains("cancelled")
    );
    let mut invalid = triangle;
    invalid.points[0][0] = f64::NAN;
    assert!(seams::contacts([invalid], &mut Budget::new(100, &|| false).unwrap()).is_err());
    assert_eq!(
        seams::contacts([triangle, wall], &mut Budget::new(100, &|| false).unwrap())
            .unwrap()
            .by_object["floor"]
            .len(),
        1
    );
}

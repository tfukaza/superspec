use serde::Deserialize;
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
/// A floor and a raised step meet two walls either side of a doorway at u in [2, 3],
/// a wall crossing the room, and facets each filter must reject.
fn room() -> Vec<Input> {
    serde_json::from_value(serde_json::json!([
        {"object":"floor","kind":"floor","points":[[0,0,0],[6,0,0],[6,4,0]],"normalZ":1,"visible":true,"collidable":true},
        {"object":"floor","kind":"floor","points":[[0,0,0],[6,4,0],[0,4,0]],"normalZ":1,"visible":true,"collidable":true},
        {"object":"step","kind":"stairs","points":[[0,0,1],[6,0,1],[0,4,1]],"normalZ":1,"visible":true,"collidable":true},
        {"object":"sloped","kind":"floor","points":[[0,0,0],[6,0,0],[0,4,1]],"normalZ":1,"visible":true,"collidable":true},
        {"object":"hidden-floor","kind":"floor","points":[[0,0,0],[6,0,0],[0,4,0]],"normalZ":1,"visible":false,"collidable":true},
        {"object":"decoration","kind":"floor","points":[[0,0,0],[6,0,0],[0,4,0]],"normalZ":1,"visible":true,"collidable":false},
        {"object":"down","kind":"floor","points":[[0,0,0],[0,4,0],[6,0,0]],"normalZ":-1,"visible":true,"collidable":true},
        {"object":"roof","kind":"other","points":[[0,0,0],[6,0,0],[0,4,0]],"normalZ":1,"visible":true,"collidable":true},
        {"object":"z-wall","kind":"wall","points":[[-1,0,0],[2,0,0],[2,0,3]],"normalZ":0,"visible":true,"collidable":true},
        {"object":"z-wall","kind":"wall","points":[[-1,0,0],[2,0,3],[-1,0,3]],"normalZ":0,"visible":true,"collidable":true},
        {"object":"a-wall","kind":"wall","points":[[3,0,0],[7,0,0],[7,0,3]],"normalZ":0,"visible":true,"collidable":true},
        {"object":"a-wall","kind":"wall","points":[[3,0,0],[7,0,3],[3,0,3]],"normalZ":0,"visible":true,"collidable":true},
        {"object":"crossing","kind":"wall","points":[[-1,2,-1],[7,2,-1],[7,2,2]],"normalZ":0,"visible":true,"collidable":true},
        {"object":"crossing","kind":"wall","points":[[-1,2,-1],[7,2,2],[-1,2,2]],"normalZ":0,"visible":true,"collidable":true},
        {"object":"point","kind":"wall","points":[[-1,0,0],[0,0,0],[0,0,3]],"normalZ":0,"visible":true,"collidable":true},
        {"object":"point","kind":"wall","points":[[-1,0,0],[0,0,3],[-1,0,3]],"normalZ":0,"visible":true,"collidable":true},
        {"object":"remote","kind":"wall","points":[[9,0,0],[11,0,0],[11,0,3]],"normalZ":0,"visible":true,"collidable":true},
        {"object":"remote","kind":"wall","points":[[9,0,0],[11,0,3],[9,0,3]],"normalZ":0,"visible":true,"collidable":true},
        {"object":"hidden-wall","kind":"wall","points":[[0,0,0],[6,0,0],[6,0,3]],"normalZ":0,"visible":false,"collidable":true},
        {"object":"hidden-wall","kind":"wall","points":[[0,0,0],[6,0,3],[0,0,3]],"normalZ":0,"visible":false,"collidable":true},
        {"object":"visual-wall","kind":"wall","points":[[0,0,0],[6,0,0],[6,0,3]],"normalZ":0,"visible":true,"collidable":false},
        {"object":"visual-wall","kind":"wall","points":[[0,0,0],[6,0,3],[0,0,3]],"normalZ":0,"visible":true,"collidable":false},
        {"object":"not-vertical","kind":"wall","points":[[0,0,0],[6,0,0],[0,1,3]],"normalZ":0.01,"visible":true,"collidable":true}
    ]))
    .unwrap()
}
#[test]
fn physical_contacts_keep_doorways_open_and_filter_hidden_visual_and_non_level_facets() {
    let room = room();
    let facets = || {
        room.iter().map(|t| Facet {
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
    // Only collidable, visible, level floors and vertical walls that share an edge produce seams.
    assert_eq!(
        output
            .by_object
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        ["a-wall", "crossing", "floor", "step", "z-wall"]
    );
    for seams in output.by_object.values() {
        for seam in seams {
            for p in seam {
                assert!(
                    p[2] == 0. || p[2] == 1.,
                    "seams stay on the floor or step: {p:?}"
                );
            }
        }
    }
    assert_eq!(
        output
            .walls_by_floor
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        ["floor", "step"]
    );
    // Doorway [2,3] stays open; ownership order remains z-wall then a-wall.
    let floor = &output.walls_by_floor["floor"];
    assert_eq!(
        floor.iter().map(|w| w.host.as_str()).collect::<Vec<_>>(),
        ["z-wall", "a-wall", "crossing"]
    );
    for wall in &floor[..2] {
        assert!(
            wall.seams
                .iter()
                .all(|s| s.iter().all(|p| p[0] <= 2.) || s.iter().all(|p| p[0] >= 3.))
        );
    }
    // The wall stops at the floor edge: z-wall contributes exactly the shared span u in [0, 2].
    assert_eq!(floor[0].seams, [[[0., 0., 0.], [2., 0., 0.]]]);
    assert!(
        seams::contacts(facets(), &mut Budget::new(1, &|| false).unwrap())
            .err()
            .unwrap()
            .contains("work allowance")
    );
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

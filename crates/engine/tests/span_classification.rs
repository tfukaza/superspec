use serde::Deserialize;
use superspec_engine::worldspec::themes::kowloon::spans::{
    anchors,
    classify::{self, Facet, Host, Surface},
};
use superspec_geometry::V3;
use superspec_geometry::obstructions::{Budget, Triangle};
#[derive(Deserialize)]
struct HostInput {
    id: String,
    walls: Vec<String>,
    glazing: Vec<String>,
}
#[derive(Deserialize)]
struct Input {
    object: String,
    host: Option<String>,
    material: String,
    visible: bool,
    dynamic: bool,
    points: Triangle,
    normal: V3,
}
#[derive(Deserialize)]
struct Reference {
    hosts: Vec<HostInput>,
    triangles: Vec<Input>,
    planes: serde_json::Value,
}
#[derive(Debug, Deserialize, PartialEq)]
struct PlaneOutput {
    host: String,
    normal: V3,
    tangent: V3,
    offset: f64,
    bounds: Rectangle,
    windows: Vec<Rectangle>,
}
#[derive(Debug, Deserialize, PartialEq)]
struct Rectangle {
    u0: f64,
    u1: f64,
    z0: f64,
    z1: f64,
}
pub fn check_reference_span_classification() {
    let reference: Reference = serde_json::from_str(include_str!(
        "../../../tests/fixtures/rust/span-classification.json"
    ))
    .unwrap();
    let hosts: Vec<_> = reference
        .hosts
        .into_iter()
        .map(|h| Host {
            id: h.id,
            walls: h.walls,
            glazing: h.glazing,
        })
        .collect();
    let input = || {
        reference.triangles.iter().map(|t| Facet {
            surface: Surface {
                object: &t.object,
                host: t.host.as_deref(),
                material: &t.material,
                visible: t.visible,
                dynamic: t.dynamic,
            },
            points: t.points,
            normal: t.normal,
        })
    };
    let mut budget = Budget::new(100_000, &|| false).unwrap();
    let faces = classify::faces(&hosts, input(), &mut budget).unwrap();
    assert_eq!(
        faces
            .iter()
            .map(|f| (f.host, f.glazing))
            .collect::<Vec<_>>(),
        [
            (0, false),
            (0, false),
            (0, false),
            (0, false),
            (0, false),
            (0, false),
            (0, true),
            (0, true),
            (1, false),
            (1, false)
        ]
    );
    for (face, source) in faces.iter().zip([0, 1, 2, 3, 6, 7, 8, 9, 30, 31]) {
        assert_eq!(face.points, reference.triangles[source].points);
        assert_eq!(face.stored_normal, reference.triangles[source].normal);
    }
    let names: Vec<_> = hosts.iter().map(|h| h.id.clone()).collect();
    assert_eq!(
        serde_json::from_value::<Vec<PlaneOutput>>(
            serde_json::to_value(anchors::planes(&names, &faces, &mut budget).unwrap()).unwrap()
        )
        .unwrap(),
        serde_json::from_value::<Vec<PlaneOutput>>(reference.planes).unwrap()
    );
    assert!(classify::faces(&hosts, input(), &mut Budget::new(1, &|| false).unwrap()).is_err());
    assert!(
        classify::faces(
            &hosts,
            input(),
            &mut Budget::new(100_000, &|| true).unwrap()
        )
        .err()
        .unwrap()
        .contains("cancelled")
    );
    assert_eq!(
        classify::faces(&hosts, input(), &mut budget).unwrap().len(),
        10
    );
}
#[test]
fn finished_faces_use_source_provenance_and_host_specific_material_roles() {
    check_reference_span_classification();
}
#[test]
fn ignored_triangles_still_consume_classification_work() {
    let input = || {
        std::iter::repeat_with(|| Facet {
            surface: Surface {
                object: "unowned",
                host: None,
                material: "wall",
                visible: false,
                dynamic: false,
            },
            points: [[0.; 3]; 3],
            normal: [0., 0., 1.],
        })
        .take(1_048_577)
    };
    assert!(
        classify::faces(&[], input(), &mut Budget::new(100, &|| false).unwrap())
            .err()
            .unwrap()
            .contains("work allowance")
    );
    assert!(
        classify::faces(
            &[],
            input(),
            &mut Budget::new(2_000_000, &|| false).unwrap()
        )
        .err()
        .unwrap()
        .contains("triangle allowance")
    );
    assert!(
        classify::faces(
            &[],
            input().take(1),
            &mut Budget::new(100, &|| false).unwrap()
        )
        .unwrap()
        .is_empty()
    );
}

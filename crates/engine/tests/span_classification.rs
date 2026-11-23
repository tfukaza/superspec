use superspec_engine::worldspec::themes::kowloon::spans::{
    anchors,
    classify::{self, Facet, Host, Surface},
};
use superspec_geometry::obstructions::{Budget, Triangle};
struct Input {
    object: &'static str,
    host: Option<&'static str>,
    material: &'static str,
    visible: bool,
    dynamic: bool,
    points: Triangle,
}
/// Two triangles of an upright quad on the y plane, wound like extracted wall faces.
fn quad(
    object: &'static str,
    host: Option<&'static str>,
    material: &'static str,
    [visible, dynamic]: [bool; 2],
    [x0, x1, y, z0, z1]: [f64; 5],
) -> [Input; 2] {
    let p = [[x0, y, z0], [x1, y, z0], [x1, y, z1], [x0, y, z1]];
    [[p[0], p[1], p[2]], [p[0], p[2], p[3]]].map(|points| Input {
        object,
        host,
        material,
        visible,
        dynamic,
        points,
    })
}
fn triangles() -> Vec<Input> {
    let shown = [true, false];
    [
        // 0-1: an undressed object is owned by its object id.
        quad("tower", None, "unrelated", shown, [0., 2., 0., 0., 4.]),
        // 2-3: host envelope with a declared wall material.
        quad(
            "tower/$dressing/envelope",
            Some("tower"),
            "substrate",
            shown,
            [2., 4., 0., 0., 4.],
        ),
        // 4-5: fixtures are never receiving walls.
        quad(
            "tower/$dressing/fixtures",
            Some("tower"),
            "metal",
            shown,
            [6., 8., 0., 0., 4.],
        ),
        // 6-7: a material declared both wall and glazing stays a wall in openings.
        quad(
            "tower/$dressing/openings",
            Some("tower"),
            "dual",
            shown,
            [4., 6., 0., 0., 4.],
        ),
        // 8-9: host glazing in openings.
        quad(
            "tower/$dressing/openings",
            Some("tower"),
            "glass",
            shown,
            [0.5, 1.5, 0.02, 1., 2.],
        ),
        // 10-13: glazing materials outside the openings part are ignored.
        quad(
            "tower/$dressing/props",
            Some("tower"),
            "glass",
            shown,
            [2.5, 3.5, 0.02, 1., 2.],
        ),
        quad(
            "tower/$dressing/openings-extra",
            Some("tower"),
            "glass",
            shown,
            [2.5, 3.5, 0.02, 2.5, 3.5],
        ),
        // 14-17: dressing paths without, or with an unknown, host provenance.
        quad(
            "faux/$dressing/openings",
            None,
            "glass",
            shown,
            [2.5, 3.5, 0.02, 2.5, 3.5],
        ),
        quad(
            "foreign/$dressing/openings",
            Some("other"),
            "glass",
            shown,
            [2.5, 3.5, 0.02, 2.5, 3.5],
        ),
        // 18-19: the second host's wall material is not this host's wall.
        quad(
            "tower/$dressing/local-slot",
            Some("tower"),
            "wall",
            shown,
            [6., 8., 0., 0., 4.],
        ),
        // 20-21: path-shaped object names do not establish provenance.
        quad(
            "tower/$dressing/no-provenance",
            None,
            "substrate",
            shown,
            [6., 8., 0., 0., 4.],
        ),
        // 22-23: the second host's envelope material is not its declared wall.
        quad(
            "far/$dressing/envelope",
            Some("far"),
            "substrate",
            shown,
            [12., 14., 0., 0., 4.],
        ),
        // 24-25: hidden walls are excluded.
        quad(
            "hidden",
            Some("tower"),
            "authored-wall",
            [false, false],
            [20., 22., 0., 0., 4.],
        ),
        // 26-27: dynamic geometry is excluded.
        quad(
            "tower",
            None,
            "unrelated",
            [true, true],
            [40., 42., 0., 0., 4.],
        ),
        // 28-29: unknown host.
        quad(
            "unknown",
            Some("other"),
            "wall",
            shown,
            [30., 32., 0., 0., 4.],
        ),
        // 30-31: object identity, not the stored host, owns undressed geometry.
        quad(
            "far",
            Some("tower"),
            "unrelated",
            shown,
            [10., 12., 0., 0., 4.],
        ),
    ]
    .into_iter()
    .flatten()
    .collect()
}
fn hosts() -> Vec<Host> {
    vec![
        Host {
            id: "tower".into(),
            walls: vec!["authored-wall".into(), "substrate".into(), "dual".into()],
            glazing: vec!["glass".into(), "lit".into(), "dual".into()],
        },
        Host {
            id: "far".into(),
            walls: vec!["wall".into()],
            glazing: vec!["glass".into()],
        },
    ]
}
#[test]
fn finished_faces_use_source_provenance_and_host_specific_material_roles() {
    let triangles = triangles();
    let hosts = hosts();
    let normal = [0., -1., 0.];
    let input = || {
        triangles.iter().map(|t| Facet {
            surface: Surface {
                object: t.object,
                host: t.host,
                material: t.material,
                visible: t.visible,
                dynamic: t.dynamic,
            },
            points: t.points,
            normal,
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
        assert_eq!(face.points, triangles[source].points);
        assert_eq!(face.stored_normal, normal);
    }
    let names: Vec<_> = hosts.iter().map(|h| h.id.clone()).collect();
    let planes = anchors::planes(&names, &faces, &mut budget).unwrap();
    // Walls face -y, so the plane tangent runs along -x: x in [a, b] maps to u in [-b, -a].
    let rect = |r: anchors::Rect| [r.u0, r.u1, r.z0, r.z1];
    let close = |a: [f64; 4], b: [f64; 4]| a.iter().zip(b).all(|(a, b)| (a - b).abs() < 1e-9);
    assert_eq!(planes.len(), 2);
    let far = planes.iter().find(|p| p.host() == "far").unwrap();
    assert!(close(rect(far.bounds()), [-12., -10., 0., 4.]));
    assert!(far.windows().is_empty());
    let tower = planes.iter().find(|p| p.host() == "tower").unwrap();
    assert!(close(rect(tower.bounds()), [-6., 0., 0., 4.]));
    assert_eq!(tower.windows().len(), 1, "host glazing is the only window");
    assert!(close(rect(tower.windows()[0]), [-1.5, -0.5, 1., 2.]));
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
        faces.len(),
        "failed classification leaves the inputs reusable"
    );
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

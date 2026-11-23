use serde_json::json;
use superspec_aml::rng::SeededRng;
use superspec_engine::worldspec::themes::kowloon::{
    mesh::{Mesh, Mode},
    pen::Pen,
    signs::{
        book::Book,
        copy::Business,
        mounts::{self, FlatKind, Proposal, Site, WallRect},
        panel::Settings,
        survey::{self, Face, Sign, Solid, Verdict},
    },
};
use superspec_geometry::obstructions::{Bounds, Budget, Obstructions, triangle_touches_box};
use superspec_geometry::{Matrix, V3};
/// One 1.2 m x 0.6 m sign facing -Y at 5 m.
fn sign() -> Sign {
    Sign {
        priority: 2.,
        solids: vec![Solid {
            center: [0., 0.02, 5.],
            axes: [[1., 0., 0.], [0., -1., 0.], [0., 0., 1.]],
            half: [0.6, 0.02, 0.3],
        }],
        faces: vec![Face {
            center: [0., 0., 5.],
            right: [1., 0., 0.],
            up: [0., 0., 1.],
            normal: [0., -1., 0.],
            width: 1.2,
            height: 0.6,
            look_down: 0.,
        }],
    }
}
#[test]
fn solid_corners_span_center_plus_minus_half_extents() {
    let s = &sign().solids[0];
    let corners = survey::solid_corners(s);
    assert_eq!(corners.len(), 8);
    for x in [-0.6, 0.6] {
        for y in [-0.02, 0.02] {
            for z in [-0.3, 0.3] {
                let want: V3 = [x, 0.02 - y, 5. + z];
                assert!(
                    corners
                        .iter()
                        .any(|c| c.iter().zip(want).all(|(a, b)| (a - b).abs() < 1e-12)),
                    "{want:?}"
                );
            }
        }
    }
}
#[test]
fn segments_sheets_and_boxes_meet_a_ground_triangle_only_where_they_cross_it() {
    let mut budget = Budget::new(16 * 1024 * 1024, &|| false).unwrap();
    let ground = [[[-1., -1., 0.], [1., -1., 0.], [0., 1., 0.]]];
    let scenery = Obstructions::new(&ground, &mut budget).unwrap();
    for (a, b, blocked) in [
        ([0., 0., -1.], [0., 0., 1.], true),
        ([0., 0., 0.5], [0., 0., 1.], false),
        ([5., 0., -1.], [5., 0., 1.], false),
    ] {
        assert_eq!(scenery.blocked(a, b, &mut budget).unwrap(), blocked);
        assert_eq!(scenery.blocked_path(&[a, b], &mut budget).unwrap(), blocked);
    }
    // A sheet is pierced when a scenery triangle edge passes through it.
    let vertical = |x: f64| [[x, -2., -1.], [x, 0., -1.], [x, 0., 1.], [x, -2., 1.]];
    assert!(scenery.pierced(vertical(0.), &mut budget).unwrap());
    assert!(!scenery.pierced(vertical(5.), &mut budget).unwrap());
    assert!(triangle_touches_box(&ground[0], [0., 0., 0.], [0.1; 3]));
    assert!(!triangle_touches_box(&ground[0], [0., 0., 1.], [0.1; 3]));
    // Reserved sheets block later sight lines but are not body-contact candidates.
    let mut empty = Obstructions::new(&[], &mut budget).unwrap();
    empty
        .reserve([[-7., 0., -1.], [-5., 0., -1.], [-5., 0., 1.], [-7., 0., 1.]])
        .unwrap();
    assert!(
        empty
            .blocked([-6., -1., 0.], [-6., 1., 0.], &mut budget)
            .unwrap()
    );
    assert!(
        !empty
            .blocked([-8., -1., 0.], [-8., 1., 0.], &mut budget)
            .unwrap()
    );
    assert!(
        empty
            .near(
                Bounds {
                    low: [-10.; 3],
                    high: [10.; 3]
                },
                &mut budget
            )
            .unwrap()
            .is_empty()
    );
}
#[test]
fn spatial_work_and_input_limits_reject_then_recover() {
    let mut budget = Budget::new(16 * 1024 * 1024, &|| false).unwrap();
    let signs = vec![sign()];
    let index = Obstructions::new(&[], &mut budget).unwrap();
    let mut invalid = signs.clone();
    invalid[0].faces[0].normal = [1., 1., 1.];
    assert!(survey::survey(&invalid, &index, &mut budget).is_err());
    let mut tiny = Budget::new(1, &|| false).unwrap();
    assert!(survey::survey(&signs, &index, &mut tiny).is_err());
    let mut cancelled = Budget::new(100, &|| true).unwrap();
    assert!(
        survey::survey(&signs, &index, &mut cancelled)
            .unwrap_err()
            .contains("cancelled")
    );
    assert_eq!(
        survey::survey(&signs, &index, &mut budget).unwrap(),
        vec![Verdict::Clear]
    );
    assert!(Obstructions::new(&[[[f64::NAN; 3]; 3]], &mut budget).is_err());
    assert!(
        index
            .blocked([f64::INFINITY; 3], [0.; 3], &mut budget)
            .is_err()
    );
    // A huge triangle occupies one BVH leaf rather than millions of spatial cells.
    let huge = Obstructions::new(
        &[[
            [-100000., -100000., 0.],
            [100000., -100000., 0.],
            [0., 100000., 0.],
        ]],
        &mut budget,
    )
    .unwrap();
    assert!(
        huge.blocked([0., 0., -1.], [0., 0., 1.], &mut budget)
            .unwrap()
    );
}
fn settings(seed: &str) -> Settings {
    serde_json::from_value(json!({
        "weathering": {"seed": seed, "wear": 0.25, "grime": 0.6, "rust": 0.4, "textureDensity": 32,
            "metal": {"material": "metal", "repeat": 1.5},
            "paint": {"material": "paint", "repeat": 1.2},
            "sheet": {"material": "sheet", "repeat": 1.5}},
        "lettering": true,
        "timber": {"textured": false, "timberRepeat": 1.2, "atlasRepeat": 1}
    }))
    .unwrap()
}
fn business(name: &str, english: &str) -> Business {
    serde_json::from_value(json!({
        "name": name, "trade": "冰室", "english": english,
        "tagline": "包辦筵席", "phone": "3-531618", "sector": "food"
    }))
    .unwrap()
}
fn fascia(x: f64, key: &str, business: &Business, settings: &Settings) -> Proposal {
    let site = Site {
        detail: 1,
        wash: None,
        pen: Pen::new(
            Matrix([[1., 0., 0.], [0., 1., 0.], [0., 0., 1.], [x, 0., 0.]]),
            "sign",
        )
        .unwrap(),
        key: key.into(),
        elevation: 0.,
        shown: true,
    };
    let rect = WallRect {
        x0: 0.,
        z0: 2.6,
        width: 2.8,
        height: 0.7,
    };
    mounts::flat_sign(
        &site,
        rect,
        FlatKind::Fascia,
        0.13,
        business,
        settings,
        &mut SeededRng::from_text(key),
    )
    .unwrap()
}
#[test]
fn cross_host_sign_collection_finds_host_shops_and_reports_drawn_triangles() {
    let settings = settings("book/0");
    let kwong = business("廣源", "KWONG YUEN CAFE");
    let yan = business("仁和", "YAN WO CAFE");
    let mut book = Book::default();
    book.propose("a", &settings, fascia(0., "sign/0", &kwong, &settings))
        .unwrap();
    let mut other = Book::default();
    let pending = fascia(4., "sign/1", &yan, &settings);
    other.propose("b", &settings, pending.clone()).unwrap();
    let name = |b: Option<&Business>| b.map(|b| serde_json::to_value(b).unwrap()["name"].clone());
    assert_eq!(
        name(book.shop_near("a", [1., 0., 0.], 3.).unwrap()),
        Some(json!("廣源"))
    );
    assert_eq!(name(book.shop_near("b", [5., 0., 0.], 3.).unwrap()), None);
    assert_eq!(name(book.shop_near("a", [20., 0., 0.], 1.).unwrap()), None);
    assert_eq!(
        name(
            book.shop_near_with("b", [5., 0., 0.], 3., std::slice::from_ref(&pending))
                .unwrap()
        ),
        Some(json!("仁和")),
        "local proposals are visible before they are committed"
    );
    assert!(book.shop_near("a", [f64::NAN, 0., 0.], 3.).is_err());
    book.append(&mut other).unwrap();
    assert!(other.shop_near("b", [5., 0., 0.], 100.).unwrap().is_none());
    assert_eq!(
        name(book.shop_near("b", [5., 0., 0.], 3.).unwrap()),
        Some(json!("仁和"))
    );
    let mut budget = Budget::new(16 * 1024 * 1024, &|| false).unwrap();
    let scenery = Obstructions::new(&[], &mut budget).unwrap();
    let mut mesh = Mesh::new(Mode::Render, 150000, &|| false).unwrap();
    let plans = book
        .settle(&scenery, &mut budget, |_, p, s| {
            let first = mesh.triangle_count();
            p.draw(&mut mesh, &|_| 1., s)?;
            Ok(mesh.triangle_count() - first)
        })
        .unwrap();
    let plans = serde_json::to_value(plans).unwrap();
    let plans = plans.as_array().unwrap();
    assert_eq!(plans.len(), 2);
    for (plan, host) in plans.iter().zip(["a", "b"]) {
        assert_eq!(plan["host"], host);
    }
    let reported: u64 = plans.iter().map(|p| p["triangles"].as_u64().unwrap()).sum();
    assert!(reported > 0);
    assert_eq!(reported as usize, mesh.triangle_count());
    for plan in plans {
        if plan["status"] != "built" {
            assert_eq!(plan["triangles"], 0);
        }
    }
}

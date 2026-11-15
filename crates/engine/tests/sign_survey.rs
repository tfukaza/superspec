use serde::Deserialize;
use superspec_engine::worldspec::themes::kowloon::signs::survey::{self, Sign, Verdict};
use superspec_geometry::V3;
use superspec_geometry::obstructions::{
    Bounds, Budget, Obstructions, Triangle, triangle_touches_box,
};
#[derive(Deserialize)]
struct Case {
    signs: Vec<Sign>,
    triangles: Vec<Triangle>,
    verdicts: Vec<Verdict>,
    corners: Vec<Vec<Vec<V3>>>,
}
#[derive(Deserialize)]
struct Query {
    a: V3,
    b: V3,
    blocked: bool,
}
#[derive(Deserialize)]
struct Sheet {
    quad: [V3; 4],
    pierced: bool,
}
#[derive(Deserialize)]
struct BoxCase {
    triangle: Triangle,
    center: V3,
    half: V3,
    touches: bool,
}
#[derive(Deserialize)]
struct Golden {
    cases: Vec<Case>,
    triangles: Vec<Triangle>,
    queries: Vec<Query>,
    quads: Vec<Sheet>,
    reserved: [V3; 4],
    reservations: Vec<Query>,
    boxes: Vec<BoxCase>,
}
fn golden() -> Golden {
    serde_json::from_str(include_str!(
        "../../../tests/fixtures/rust/sign-survey.json"
    ))
    .unwrap()
}
pub fn check_reference_survey() {
    let golden = golden();
    for (i, c) in golden.cases.iter().enumerate() {
        let mut budget = Budget::new(16 * 1024 * 1024, &|| false).unwrap();
        let scenery = Obstructions::new(&c.triangles, &mut budget).unwrap();
        assert_eq!(
            survey::survey(&c.signs, &scenery, &mut budget).unwrap(),
            c.verdicts,
            "survey {i}"
        );
        for (s, solids) in c.signs.iter().zip(&c.corners) {
            for (s, corners) in s.solids.iter().zip(solids) {
                for (a, b) in survey::solid_corners(s)
                    .iter()
                    .flatten()
                    .zip(corners.iter().flatten())
                {
                    assert!((a - b).abs() < 2e-10)
                }
            }
        }
    }
    let mut budget = Budget::new(16 * 1024 * 1024, &|| false).unwrap();
    let scenery = Obstructions::new(&golden.triangles, &mut budget).unwrap();
    for (i, q) in golden.queries.iter().enumerate() {
        assert_eq!(
            scenery.blocked(q.a, q.b, &mut budget).unwrap(),
            q.blocked,
            "segment {i}"
        );
        assert_eq!(
            scenery.blocked_path(&[q.a, q.b], &mut budget).unwrap(),
            q.blocked
        )
    }
    for q in golden.quads {
        assert_eq!(scenery.pierced(q.quad, &mut budget).unwrap(), q.pierced)
    }
    for c in golden.boxes {
        assert_eq!(
            triangle_touches_box(&c.triangle, c.center, c.half),
            c.touches
        )
    }
    let mut empty = Obstructions::new(&[], &mut budget).unwrap();
    empty.reserve(golden.reserved).unwrap();
    for q in golden.reservations {
        assert_eq!(empty.blocked(q.a, q.b, &mut budget).unwrap(), q.blocked)
    }
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
            .is_empty(),
        "reserved sheets do not become body-contact candidates"
    );
}
#[test]
fn spatial_queries_and_sampled_sign_readability_match_reference() {
    check_reference_survey()
}
#[test]
fn spatial_work_and_input_limits_reject_then_recover() {
    let mut budget = Budget::new(16 * 1024 * 1024, &|| false).unwrap();
    let c = golden().cases.remove(1);
    let index = Obstructions::new(&c.triangles, &mut budget).unwrap();
    let mut invalid = c.signs.clone();
    invalid[0].faces[0].normal = [1., 1., 1.];
    assert!(survey::survey(&invalid, &index, &mut budget).is_err());
    let mut tiny = Budget::new(1, &|| false).unwrap();
    assert!(survey::survey(&c.signs, &index, &mut tiny).is_err());
    let mut cancelled = Budget::new(100, &|| true).unwrap();
    assert!(
        survey::survey(&c.signs, &index, &mut cancelled)
            .unwrap_err()
            .contains("cancelled")
    );
    assert_eq!(
        survey::survey(&c.signs, &index, &mut budget).unwrap(),
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

#[derive(Deserialize)]
struct Entry {
    host: String,
    seed: String,
    business: superspec_engine::worldspec::themes::kowloon::signs::copy::Business,
    rect: superspec_engine::worldspec::themes::kowloon::signs::mounts::WallRect,
    key: String,
    shown: bool,
    kind: superspec_engine::worldspec::themes::kowloon::signs::mounts::FlatKind,
    matrix: [[f64; 3]; 4],
    settings: superspec_engine::worldspec::themes::kowloon::signs::panel::Settings,
}
#[derive(Deserialize)]
struct Near {
    host: String,
    at: V3,
    radius: f64,
    business: Option<superspec_engine::worldspec::themes::kowloon::signs::copy::Business>,
}
#[derive(Deserialize)]
struct BookCase {
    entries: Vec<Entry>,
    triangles: Vec<Triangle>,
    near: Vec<Near>,
    plans: serde_json::Value,
}
#[derive(Deserialize)]
struct Books {
    repeats: std::collections::BTreeMap<String, f64>,
    cases: Vec<BookCase>,
}
fn close(a: &serde_json::Value, b: &serde_json::Value) {
    use serde_json::Value;
    match (a, b) {
        (Value::Number(a), Value::Number(b)) => assert!(
            (a.as_f64().unwrap() - b.as_f64().unwrap()).abs() < 2e-10,
            "{a} != {b}"
        ),
        (Value::Array(a), Value::Array(b)) => {
            assert_eq!(a.len(), b.len());
            for (a, b) in a.iter().zip(b) {
                close(a, b)
            }
        }
        (Value::Object(a), Value::Object(b)) => {
            assert_eq!(a.len(), b.len());
            for (k, b) in b {
                close(&a[k], b)
            }
        }
        _ => assert_eq!(a, b),
    }
}
pub fn check_reference_book() {
    use superspec_aml::rng::SeededRng;
    use superspec_engine::worldspec::themes::kowloon::{
        mesh::{Mesh, Mode},
        pen::Pen,
        signs::{
            book::Book,
            mounts::{self, Site},
        },
    };
    let golden: Books =
        serde_json::from_str(include_str!("../../../tests/fixtures/rust/sign-book.json")).unwrap();
    for c in golden.cases {
        let mut book = Book::default();
        let mut other = Book::default();
        let mut local = Vec::new();
        for (j, e) in c.entries.into_iter().enumerate() {
            let site = Site {
                detail: 1,
                wash: None,
                pen: Pen::new(superspec_geometry::Matrix(e.matrix), "sign").unwrap(),
                key: e.key,
                elevation: 0.,
                shown: e.shown,
            };
            let proposal = mounts::flat_sign(
                &site,
                e.rect,
                e.kind,
                0.13,
                &e.business,
                &e.settings,
                &mut SeededRng::from_text(&e.seed),
            )
            .unwrap();
            if j >= 3 {
                local.push((e.host.clone(), proposal.clone()));
            }
            if j < 3 { &mut book } else { &mut other }
                .propose(&e.host, &e.settings, proposal)
                .unwrap();
        }
        for n in &c.near {
            let pending = local
                .iter()
                .filter(|(host, _)| host == &n.host)
                .map(|(_, p)| p.clone())
                .collect::<Vec<_>>();
            close(
                &serde_json::to_value(
                    book.shop_near_with(&n.host, n.at, n.radius, &pending)
                        .unwrap(),
                )
                .unwrap(),
                &serde_json::to_value(&n.business).unwrap(),
            );
        }
        book.append(&mut other).unwrap();
        assert!(other.shop_near("a", [0.; 3], 100.).unwrap().is_none());
        for n in c.near {
            close(
                &serde_json::to_value(book.shop_near(&n.host, n.at, n.radius).unwrap()).unwrap(),
                &serde_json::to_value(n.business).unwrap(),
            )
        }
        let mut budget = Budget::new(16 * 1024 * 1024, &|| false).unwrap();
        let scenery = Obstructions::new(&c.triangles, &mut budget).unwrap();
        let mut mesh = Mesh::new(Mode::Render, 150000, &|| false).unwrap();
        let plans = book
            .settle(&scenery, &mut budget, |_, p, s| {
                let first = mesh.triangle_count();
                p.draw(&mut mesh, &|slot| golden.repeats[slot], s)?;
                Ok(mesh.triangle_count() - first)
            })
            .unwrap();
        close(&serde_json::to_value(plans).unwrap(), &c.plans);
    }
}
#[test]
fn cross_host_sign_collection_preserves_decisions_lookup_and_reported_triangles() {
    check_reference_book()
}

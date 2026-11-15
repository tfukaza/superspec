use serde::Deserialize;
use serde_json::{Value, json};
use superspec_engine::worldspec::themes::kowloon::pavement::{context, field, layout};
use superspec_geometry::V3;
use superspec_geometry::obstructions::Budget;

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
enum Neighbor {
    Wall {
        path: Vec<V3>,
        thickness: f64,
        contacts: Vec<[V3; 2]>,
    },
    Floor {
        loops: Vec<Vec<V3>>,
    },
}
impl Neighbor {
    fn native(self) -> layout::Neighbor {
        match self {
            Self::Wall {
                path,
                thickness,
                contacts,
            } => layout::Neighbor::Wall {
                path,
                thickness,
                contacts,
            },
            Self::Floor { loops } => layout::Neighbor::Floor { loops },
        }
    }
}
#[derive(Deserialize)]
struct Case {
    loops: Vec<Vec<V3>>,
    patch: f64,
    material: f64,
    seed: u32,
    amount: f64,
    result: Value,
    #[serde(default)]
    origin: V3,
    #[serde(default)]
    receiving: Vec<Vec<V3>>,
    #[serde(default)]
    neighbors: Vec<Neighbor>,
    #[serde(default)]
    connections: Vec<Vec<V3>>,
}
#[derive(Deserialize)]
struct Field {
    points: Vec<V3>,
    boundary: Vec<V3>,
    point: V3,
    area: f64,
    distance: f64,
    intersection: Value,
    split: Value,
}
#[derive(Deserialize)]
struct Exposure {
    point: V3,
    walls: Vec<context::Wall>,
    result: Value,
}
#[derive(Deserialize)]
struct Golden {
    footprints: Vec<Case>,
    authored: Vec<Case>,
    fields: Vec<Field>,
    exposures: Vec<Exposure>,
}
fn compare(a: &Value, b: &Value, at: &str) {
    match (a, b) {
        (Value::Number(a), Value::Number(b)) => assert!(
            (a.as_f64().unwrap() - b.as_f64().unwrap()).abs() < 1e-9,
            "{at}: {a} != {b}"
        ),
        (Value::Array(a), Value::Array(b)) => {
            assert_eq!(a.len(), b.len(), "{at}");
            for (i, (a, b)) in a.iter().zip(b).enumerate() {
                compare(a, b, &format!("{at}/{i}"));
            }
        }
        (Value::Object(a), Value::Object(b)) => {
            assert_eq!(a.len(), b.len(), "{at}");
            for (k, a) in a {
                compare(a, &b[k], &format!("{at}/{k}"));
            }
        }
        _ => assert_eq!(a, b, "{at}"),
    }
}
fn capture(l: &layout::Layout, seed: u32, amount: f64, budget: &mut Budget<'_>) -> Value {
    json!({"world":l.world.0,"columns":l.columns,"rows":l.rows,"zonesX":l.zones_x,"zonesY":l.zones_y,
        "constructionX":l.construction_x,"constructionY":l.construction_y,"anchors":l.anchors,"pours":layout::pours(l,seed,amount,budget).unwrap()})
}
pub fn check_reference_pavement_layout() {
    let g: Golden = serde_json::from_str(include_str!(
        "../../../tests/fixtures/rust/pavement-layout.json"
    ))
    .unwrap();
    let mut budget = Budget::new(32_000_000, &|| false).unwrap();
    for (i, c) in g.footprints.into_iter().enumerate() {
        let l = layout::footprint(&c.loops, c.patch, c.material, &mut budget).unwrap();
        compare(
            &capture(&l, c.seed, c.amount, &mut budget),
            &c.result,
            &format!("footprint/{i}"),
        );
    }
    for (i, c) in g.authored.into_iter().enumerate() {
        let neighbors: Vec<_> = c.neighbors.into_iter().map(Neighbor::native).collect();
        let l = layout::authored(
            layout::Authored {
                loops: &c.loops,
                origin: c.origin,
                receiving: &c.receiving,
                neighbors: &neighbors,
                connections: &c.connections,
            },
            c.patch,
            c.material,
            &mut budget,
        )
        .unwrap();
        compare(
            &capture(&l, c.seed, c.amount, &mut budget),
            &c.result,
            &format!("authored/{i}"),
        );
    }
    for (i, c) in g.fields.into_iter().enumerate() {
        compare(
            &json!(field::area(&c.points)),
            &json!(c.area),
            &format!("field/{i}/area"),
        );
        compare(
            &json!(field::edge_distance(c.point, c.points[0], c.points[1])),
            &json!(c.distance),
            &format!("field/{i}/distance"),
        );
        compare(
            &json!(field::intersect(&c.points, &c.boundary)),
            &c.intersection,
            &format!("field/{i}/intersection"),
        );
        let split = field::split(&c.points, &c.boundary);
        compare(
            &json!({"inside":split.inside,"outside":split.outside}),
            &c.split,
            &format!("field/{i}/split"),
        );
        let partitioned = field::area(&split.inside).abs()
            + split
                .outside
                .iter()
                .map(|p| field::area(p).abs())
                .sum::<f64>();
        assert!(
            (partitioned - c.area.abs()).abs() < 1e-8,
            "partition preserves receiving area"
        );
    }
    for (i, c) in g.exposures.into_iter().enumerate() {
        let e = context::exposure(c.point, &c.walls, &mut budget).unwrap();
        compare(
            &json!({"wallDistance":e.wall_distance,"corridorWidth":e.corridor_width}),
            &c.result,
            &format!("exposure/{i}"),
        );
    }
}
#[test]
fn construction_axes_pours_footprint_cuts_and_finite_contacts_match_reference() {
    check_reference_pavement_layout();
}
#[test]
fn admission_and_work_limits_fail_without_poisoning_recovery() {
    let loops = vec![field::rectangle(0., 0., 8., 6., 0.)];
    let mut budget = Budget::new(1_000_000, &|| false).unwrap();
    assert!(layout::footprint(&[], 1.9, 7., &mut budget).is_err());
    assert!(layout::footprint(&loops, 0., 7., &mut budget).is_err());
    assert!(layout::footprint(&loops, 1.9, f64::NAN, &mut budget).is_err());
    assert!(layout::footprint(&loops, 1.9, 7., &mut Budget::new(1, &|| false).unwrap()).is_err());
    assert!(layout::footprint(&loops, 1.9, 7., &mut Budget::new(1000, &|| true).unwrap()).is_err());
    assert!(context::exposure([0.; 3], &[], &mut Budget::new(1000, &|| true).unwrap()).is_err());
    let oversized = vec![field::rectangle(0., 0., 20_000., 3., 0.)];
    assert!(layout::footprint(&oversized, 0.5, 7., &mut budget).is_err());
    let l = layout::footprint(&loops, 1.9, 7., &mut budget).unwrap();
    assert!(layout::pours(&l, 9, 1.1, &mut budget).is_err());
    assert!(layout::pours(&l, 9, 0.5, &mut Budget::new(1000, &|| true).unwrap()).is_err());
    let pours = layout::pours(&l, 9, 1., &mut budget).unwrap();
    let mut cells = vec![0; (l.columns.len() - 1) * (l.rows.len() - 1)];
    for p in pours {
        for x in p.x..p.end_x {
            for y in p.y..p.end_y {
                cells[x * (l.rows.len() - 1) + y] += 1;
            }
        }
    }
    assert!(
        cells.into_iter().all(|n| n == 1),
        "joined pours cover every original cell exactly once"
    );
}

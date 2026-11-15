use serde::Deserialize;
use serde_json::{Value, json};
use superspec_engine::worldspec::themes::kowloon::pavement::surface::{Field, Finish, Recipe};
use superspec_geometry::V3;
use superspec_geometry::obstructions::Budget;
#[derive(Deserialize)]
struct Sample {
    x: f64,
    y: f64,
    footprint: f64,
    sample: Value,
    raster: Value,
    height: f64,
}
#[derive(Deserialize)]
struct Area {
    bounds: [f64; 4],
    uniform: bool,
    scale: f64,
    cuts: Option<[Vec<f64>; 2]>,
}
#[derive(Deserialize)]
struct Case {
    recipe: Recipe,
    materials: Vec<String>,
    elevations: Option<Vec<f64>>,
    samples: Vec<Sample>,
    areas: Vec<Area>,
}
#[derive(Deserialize)]
struct Golden {
    cases: Vec<Case>,
}
fn fixture() -> Golden {
    serde_json::from_str(include_str!(
        "../../../tests/fixtures/rust/pavement-surface.json"
    ))
    .unwrap()
}
fn color(p: V3, finish: Finish) -> V3 {
    let order = [
        Finish::Concrete,
        Finish::Repair,
        Finish::Tile,
        Finish::Timber,
        Finish::Grout,
        Finish::Substrate,
        Finish::Asphalt,
        Finish::ManholeRim,
        Finish::ManholeCover,
        Finish::ManholeSlot,
        Finish::Crack,
        Finish::Joint,
        Finish::Spall,
        Finish::Chip,
        Finish::TileGrime,
    ];
    [
        0.7 + p[0] * 0.01,
        0.8 + p[1] * 0.02,
        0.6 + order.iter().position(|f| *f == finish).unwrap() as f64 * 0.005,
    ]
}
fn close(a: &Value, b: &Value, path: &str) {
    match (a, b) {
        (Value::Number(a), Value::Number(b)) => assert!(
            (a.as_f64().unwrap() - b.as_f64().unwrap()).abs() < 2e-10,
            "{path}: {a} != {b}"
        ),
        (Value::Array(a), Value::Array(b)) => {
            assert_eq!(a.len(), b.len(), "{path}");
            for (i, (a, b)) in a.iter().zip(b).enumerate() {
                close(a, b, &format!("{path}/{i}"));
            }
        }
        (Value::Object(a), Value::Object(b)) => {
            assert_eq!(a.len(), b.len(), "{path}");
            for (k, a) in a {
                close(a, &b[k], &format!("{path}/{k}"));
            }
        }
        _ => assert_eq!(a, b, "{path}"),
    }
}
pub fn check_reference_pavement_surfaces() {
    for (i, c) in fixture().cases.into_iter().enumerate() {
        let mut budget = Budget::new(16_000_000, &|| false).unwrap();
        let field = Field::new(&c.recipe, &mut budget).unwrap();
        assert_eq!(field.materials, c.materials);
        assert_eq!(field.elevations, c.elevations);
        for (n, q) in c.samples.into_iter().enumerate() {
            let s = field.sample(q.x, q.y, q.footprint).unwrap();
            close(&json!(s), &q.sample, &format!("field/{i}/sample/{n}"));
            close(
                &json!(field.raster_sample(q.x, q.y, q.footprint, &color).unwrap()),
                &q.raster,
                &format!("field/{i}/raster/{n}"),
            );
            assert!(
                (field.rendered_height(q.x, q.y, q.footprint).unwrap() - q.height).abs() < 2e-10,
                "field/{i}/height/{n}"
            );
            assert!((s.layers.iter().map(|l| l.weight).sum::<f64>() - 1.).abs() < 1e-10);
        }
        for (n, q) in c.areas.into_iter().enumerate() {
            assert_eq!(
                field.uniform_coverage(q.bounds, &mut budget).unwrap(),
                q.uniform,
                "field/{i}/uniform/{n}"
            );
            assert_eq!(field.feature_scale(q.bounds), q.scale);
            close(
                &json!(field.construction_cuts(q.bounds, &mut budget).unwrap()),
                &json!(q.cuts),
                &format!("field/{i}/cuts/{n}"),
            );
        }
    }
}
#[test]
fn pavement_fields_match_material_ownership_filtered_tiles_repairs_relief_and_coverage() {
    check_reference_pavement_surfaces();
}
#[test]
fn invalid_cancelled_and_exhausted_fields_leave_valid_queries_usable() {
    let mut recipe = fixture().cases.remove(3).recipe;
    let mut budget = Budget::new(1_000_000, &|| false).unwrap();
    assert!(Field::new(&recipe, &mut Budget::new(1, &|| false).unwrap()).is_err());
    assert!(Field::new(&recipe, &mut Budget::new(1000, &|| true).unwrap()).is_err());
    recipe.worn_edges.pop();
    assert!(Field::new(&recipe, &mut budget).is_err());
    recipe.worn_edges.push(true);
    let f = Field::new(&recipe, &mut budget).unwrap();
    assert!(f.sample(f64::NAN, 0., 0.).is_err());
    assert!(f.sample(0., 0., -1.).is_err());
    assert!(f.uniform_coverage([1., 0., 0., 1.], &mut budget).is_err());
    assert!(
        f.construction_cuts([0., 0., 20_000., 1.], &mut budget)
            .is_err()
    );
    assert!(
        f.uniform_coverage([0., 0., 1., 1.], &mut Budget::new(1000, &|| true).unwrap())
            .is_err()
    );
    assert!(f.raster_sample(0., 0., 0., &|_, _| [f64::NAN; 3]).is_err());
    assert!(f.raster_sample(0.01, 0.01, 0.01, &color).is_ok());
}

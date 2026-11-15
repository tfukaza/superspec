use serde_json::{Value, json};
use superspec_engine::worldspec::themes::kowloon::signs::mounts::Proposal;
pub fn metadata(p: &Proposal) -> Value {
    json!({"key":p.key,"mount":p.mount,"form":p.form,"shown":p.shown,"above":p.above,"business":p.business,"steel":p.steel,"panels":p.panels.iter().map(|p|json!({"design":p.design,"sides":p.sides,"matrix":p.pen.matrix.0})).collect::<Vec<_>>()})
}
pub fn close(a: &Value, b: &Value) {
    close_at(a, b, "reference");
}
pub fn close_at(a: &Value, b: &Value, path: &str) {
    match (a, b) {
        (Value::Number(a), Value::Number(b)) => assert!(
            (a.as_f64().unwrap() - b.as_f64().unwrap()).abs() < 2e-10,
            "{path}: {a} != {b}"
        ),
        (Value::Array(a), Value::Array(b)) => {
            assert_eq!(a.len(), b.len(), "{path}");
            for (i, (a, b)) in a.iter().zip(b).enumerate() {
                close_at(a, b, &format!("{path}/{i}"));
            }
        }
        (Value::Object(a), Value::Object(b)) => {
            for (k, v) in b {
                close_at(&a[k], v, &format!("{path}/{k}"));
            }
        }
        _ => assert_eq!(a, b, "{path}"),
    }
}

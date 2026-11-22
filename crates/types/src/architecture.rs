use crate::diagnostic::{Diagnostic, SourceLocation};
use serde::Serialize;
use std::collections::BTreeMap;

pub type V3 = [f64; 3];

#[derive(Clone, Debug, Serialize)]
#[serde(untagged)]
pub enum Value {
    Text(String),
    Number(f64),
    Boolean(bool),
    Vector(Vec<f64>),
    PlanPoints(Vec<[f64; 2]>),
    Points(Vec<V3>),
    Triangles(Vec<[usize; 3]>),
}

#[derive(Clone, Debug, Serialize)]
pub struct Node {
    pub kind: String,
    pub id: String,
    pub source: SourceLocation,
    pub attributes: BTreeMap<String, Value>,
    pub children: Vec<Node>,
}
impl Node {
    pub fn text(&self, key: &str) -> Option<&str> {
        match self.attributes.get(key) {
            Some(Value::Text(value)) => Some(value),
            _ => None,
        }
    }
    pub fn string(&self, key: &str, default: &str) -> String {
        self.text(key).unwrap_or(default).to_owned()
    }
    pub fn number(&self, key: &str, default: f64) -> f64 {
        match self.attributes.get(key) {
            Some(Value::Number(value)) => *value,
            _ => default,
        }
    }
    pub fn optional_number(&self, key: &str) -> Option<f64> {
        match self.attributes.get(key) {
            Some(Value::Number(value)) => Some(*value),
            _ => None,
        }
    }
    pub fn boolean(&self, key: &str, default: bool) -> bool {
        match self.attributes.get(key) {
            Some(Value::Boolean(value)) => *value,
            _ => default,
        }
    }
    pub fn vector<const N: usize>(&self, key: &str, default: [f64; N]) -> [f64; N] {
        match self.attributes.get(key) {
            Some(Value::Vector(value)) => value
                .clone()
                .try_into()
                .expect("admitted vector dimensions"),
            _ => default,
        }
    }
    pub fn points(&self, key: &str) -> Vec<V3> {
        match self.attributes.get(key) {
            Some(Value::Points(value)) => value.clone(),
            _ => vec![],
        }
    }
    pub fn triangles(&self) -> Vec<[usize; 3]> {
        match self.attributes.get("triangles") {
            Some(Value::Triangles(value)) => value.clone(),
            _ => vec![],
        }
    }
    pub fn fail(&self, code: &'static str, message: impl Into<String>) -> Diagnostic {
        Diagnostic {
            spatial: None,
            severity: "error",
            code,
            message: message.into().into_boxed_str(),
            objects: if self.id.is_empty() {
                vec![]
            } else {
                vec![self.id.clone()]
            },
            source: self.source.clone(),
        }
    }
}

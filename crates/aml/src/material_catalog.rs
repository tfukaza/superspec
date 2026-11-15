//! AML registration for the shared material catalog.
use crate::{
    architecture::{Document, Node, Value},
    diagnostic::Diagnostic,
    material::Material,
};
use std::collections::BTreeMap;

pub use superspec_catalog::{image_name, images, material};

fn attribute_name(key: &str) -> String {
    let mut name = String::new();
    for c in key.chars() {
        if c.is_ascii_uppercase() {
            name.push('-');
            name.push(c.to_ascii_lowercase());
        } else {
            name.push(c);
        }
    }
    name
}
fn declaration(material: &Material, source: &Node) -> Node {
    let value = serde_json::to_value(material).expect("finite catalog");
    let attributes = value
        .as_object()
        .unwrap()
        .iter()
        .filter_map(|(key, value)| {
            let value = match value {
                serde_json::Value::String(v) => Value::Text(v.clone()),
                serde_json::Value::Number(v) => Value::Number(v.as_f64().unwrap()),
                _ => return None,
            };
            Some((attribute_name(key), value))
        })
        .collect();
    Node {
        kind: "material".into(),
        id: material.id.clone(),
        source: source.source.clone(),
        attributes,
        children: vec![],
    }
}

/// Registers references in a compiler-owned document; authored overrides use separate names.
pub fn register(document: &mut Document) -> Result<(), Diagnostic> {
    for node in document
        .declarations
        .values()
        .filter(|n| n.kind == "material" && n.id.starts_with("builtin:"))
    {
        let expected = material(&node.id)
            .ok_or_else(|| node.fail("AML_MATERIAL", "Unknown builtin material."))?;
        if Material::read(node)? != expected {
            return Err(node.fail(
                "AML_MATERIAL",
                "builtin: definitions are reserved; use a named material to override a finish.",
            ));
        }
    }
    fn visit(node: &Node, declarations: &mut BTreeMap<String, Node>) -> Result<(), Diagnostic> {
        for (key, value) in &node.attributes {
            if ![
                "material",
                "reveal-material",
                "asphalt-material",
                "wall-materials",
            ]
            .contains(&key.as_str())
            {
                continue;
            }
            let Value::Text(value) = value else {
                continue;
            };
            for id in value
                .split_whitespace()
                .filter(|id| id.starts_with("builtin:"))
            {
                let material = material(id).ok_or_else(|| {
                    node.fail("AML_MATERIAL", format!("Unknown builtin material {id}."))
                })?;
                declarations
                    .entry(id.into())
                    .or_insert_with(|| declaration(&material, node));
            }
        }
        for child in &node.children {
            visit(child, declarations)?;
        }
        Ok(())
    }
    visit(&document.root, &mut document.declarations)
}

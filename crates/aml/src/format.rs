//! Canonical AML serialization for native producers and explicit migration.
use crate::architecture::{Document, Node, Value};
use std::fmt::Write;

pub use superspec_types::format::escape;
fn value(value: &Value) -> Result<String, String> {
    let number = |n: f64| {
        if n.is_finite() {
            Ok(n.to_string())
        } else {
            Err("AML numbers must be finite.".to_owned())
        }
    };
    match value {
        Value::Text(s) => Ok(s.clone()),
        Value::Number(n) => number(*n),
        Value::Boolean(b) => Ok(b.to_string()),
        Value::Vector(values) => values
            .iter()
            .map(|n| number(*n))
            .collect::<Result<Vec<_>, _>>()
            .map(|v| v.join(" ")),
        Value::PlanPoints(points) => points
            .iter()
            .map(|p| Ok(format!("{} {}", number(p[0])?, number(p[1])?)))
            .collect::<Result<Vec<_>, String>>()
            .map(|v| v.join("; ")),
        Value::Points(points) => points
            .iter()
            .map(|p| {
                p.iter()
                    .map(|n| number(*n))
                    .collect::<Result<Vec<_>, _>>()
                    .map(|v| v.join(" "))
            })
            .collect::<Result<Vec<_>, _>>()
            .map(|v| v.join("; ")),
        Value::Triangles(triangles) => Ok(triangles
            .iter()
            .map(|t| t.iter().map(usize::to_string).collect::<Vec<_>>().join(" "))
            .collect::<Vec<_>>()
            .join("; ")),
    }
}
fn write_node(node: &Node, out: &mut String, depth: usize) -> Result<(), String> {
    if depth > 128 || out.len() > 40_000_000 {
        return Err("AML serialization exceeds its source allowance.".into());
    }
    if node.kind.is_empty()
        || !node
            .kind
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-')
    {
        return Err("Invalid AML element name.".into());
    }
    write!(out, "{}<{}", "  ".repeat(depth), node.kind).unwrap();
    if !node.id.is_empty() && !node.attributes.contains_key("id") {
        write!(out, " id=\"{}\"", escape(&node.id)).unwrap();
    }
    for (name, v) in &node.attributes {
        if name == "dimensions" && ["profile", "hole"].contains(&node.kind.as_str()) {
            continue;
        }
        if name.is_empty() || !name.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-') {
            return Err("Invalid AML attribute name.".into());
        }
        let text = if node.kind == "mask" && name == "points" {
            node.points("points")
                .iter()
                .map(|p| format!("{} {}", p[0], p[1]))
                .collect::<Vec<_>>()
                .join("; ")
        } else {
            value(v)?
        };
        write!(out, " {name}=\"{}\"", escape(&text)).unwrap();
    }
    if ["profile", "hole"].contains(&node.kind.as_str()) && node.attributes.contains_key("points") {
        out.push_str(" dimensions=\"3\"");
    }
    if node.children.is_empty() {
        out.push_str("/>\n");
    } else {
        out.push_str(">\n");
        for child in &node.children {
            write_node(child, out, depth + 1)?;
        }
        writeln!(out, "{}</{}>", "  ".repeat(depth), node.kind).unwrap();
    }
    Ok(())
}
pub fn node(node: &Node) -> Result<String, String> {
    let mut output = String::new();
    write_node(node, &mut output, 0)?;
    Ok(output)
}
/// Serializes core-only typed collections and reparses them to enforce AML admission.
pub fn serialize(document: &Document) -> Result<String, String> {
    serialize_with_registry(document, &crate::extensions::Registry::default())
}

pub fn serialize_with_registry(
    document: &Document,
    registry: &crate::extensions::Registry,
) -> Result<String, String> {
    let mut root = document.root.clone();
    root.children = document
        .declarations
        .values()
        .cloned()
        .chain(document.elements.iter().cloned())
        .chain(document.metadata.iter().cloned())
        .collect();
    let text = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n{}",
        node(&root)?
    );
    crate::architecture::parse_with_registry(&text, registry).map_err(|e| e.to_string())?;
    Ok(text)
}

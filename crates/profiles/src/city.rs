//! Explicit world-space city placements in the shipped AML profile.
use superspec_aml::{
    architecture::{Node, Value, quantize},
    diagnostic::Diagnostic,
    extensions::{Check, Element, Extension, Tag},
    source::AmlSource,
};

const fn tag(name: &'static str, parent: &'static str) -> Tag {
    Tag {
        name,
        parent,
        selector: None,
    }
}
pub static EXTENSION: Extension = Extension {
    id: "city-layout",
    version: 1,
    aml_versions: &[1],
    tags: &[
        tag("city", "architecture"),
        tag("building", "city"),
        tag("road", "city"),
        tag("avenue", "city"),
        tag("sidewalk", "city"),
        tag("tree", "city"),
        tag("lamp", "city"),
    ],
    decode: read,
    checks: &[Check {
        id: "city-placement-limit",
        run: |doc| {
            let mut total = 0;
            let mut owners = std::collections::BTreeSet::new();
            for city in doc.metadata.iter().filter(|n| n.kind == "city") {
                for n in &city.children {
                    let count = n.number("count", 1.) as usize;
                    total += count;
                    if total > 1024 {
                        return Err(
                            n.fail("AML_CITY", "At most 1024 city placements are supported.")
                        );
                    }
                    for index in 1..=count {
                        let owner = placement_id(&n.id, count, index);
                        if !owners.insert(owner) {
                            return Err(n.fail(
                                "AML_CITY",
                                "City rows must expand to distinct placement IDs.",
                            ));
                        }
                    }
                }
            }
            Ok(())
        },
    }],
};

fn read(xml: Element<'_, '_>, source: &AmlSource<'_>) -> Result<Node, Diagnostic> {
    let kind = xml.tag_name().name();
    let mut node = Node {
        kind: kind.into(),
        id: String::new(),
        source: source.location(xml),
        attributes: Default::default(),
        children: vec![],
    };
    for attr in xml.attributes() {
        let (key, text) = (attr.name(), attr.value());
        let invalid = || node.fail("AML_CITY", format!("Invalid {key} on <{kind}>."));
        let number = |s: &str| {
            s.parse::<f64>()
                .ok()
                .filter(|v| v.is_finite() && v.abs() <= 1_000_000.)
                .ok_or_else(invalid)
        };
        let value = match key {
            "id" if kind != "city" => {
                node.id = text.into();
                continue;
            }
            "position" | "step" if kind != "city" => {
                let vector = text
                    .split_whitespace()
                    .map(|s| number(s).map(quantize))
                    .collect::<Result<Vec<_>, _>>()?;
                if vector.len() != 3 {
                    return Err(invalid());
                }
                Value::Vector(vector)
            }
            "heading" if kind != "city" => Value::Number(number(text)?),
            "count" if kind != "city" => Value::Number(
                text.parse::<u16>()
                    .ok()
                    .filter(|v| (1..=1024).contains(v))
                    .ok_or_else(invalid)? as f64,
            ),
            "length" | "width" if matches!(kind, "avenue" | "road" | "sidewalk") => {
                let value = quantize(number(text)?);
                let range = match (kind, key) {
                    ("road", "length") => 0.1..=200.,
                    ("road", "width") => 0.1..=50.,
                    ("avenue", "length") => 2.0..=80.,
                    ("avenue", "width") => 2.3..=40.,
                    ("sidewalk", "length") => 0.3..=100.,
                    _ => 0.4..=10.,
                };
                if !range.contains(&value) {
                    return Err(invalid());
                }
                Value::Number(value)
            }
            "floors" if kind == "building" => Value::Number(
                text.parse::<u8>()
                    .ok()
                    .filter(|v| (2..=12).contains(v))
                    .ok_or_else(invalid)? as f64,
            ),
            _ => return Err(invalid()),
        };
        node.attributes.insert(key.into(), value);
    }
    if kind != "city" {
        if node.id.is_empty() || !node.attributes.contains_key("position") {
            return Err(node.fail(
                "AML_CITY",
                "City placements require an id and XYZ position.",
            ));
        }
        if (node.number("count", 1.) > 1.) != node.attributes.contains_key("step")
            || (node.attributes.contains_key("step") && node.vector("step", [0.; 3]) == [0.; 3])
        {
            return Err(node.fail("AML_CITY", "A repeated placement needs a nonzero XYZ step."));
        }
    }
    for child in xml.children() {
        if child.is_element() {
            node.children.push(read(child, source)?);
        }
        if child.is_text() && child.text().is_some_and(|s| !s.trim().is_empty()) {
            return Err(node.fail("AML_CITY", "Unexpected text in city layout."));
        }
    }
    Ok(node)
}

/// The identity of a single prop or a one-based row member.
pub fn placement_id(id: &str, count: usize, index: usize) -> String {
    if count == 1 {
        format!("city/{id}")
    } else {
        format!("city/{id}/{index}")
    }
}

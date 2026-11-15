//! Source admission for connected visual recipes. Unsupported recipes fail explicitly.
use std::collections::BTreeMap;
use superspec_aml::extensions::{Check, Element, Extension, Tag};
use superspec_aml::{
    architecture::{Document, Node, Value},
    diagnostic::Diagnostic,
    source::AmlSource,
};

pub static EXTENSION: Extension = Extension {
    id: "kowloon-street",
    version: 1,
    aml_versions: &[1],
    tags: &[
        Tag {
            name: "dressing",
            parent: "architecture",
            // The compatibility decoder retains its existing theme defaults and diagnostics.
            selector: None,
        },
        Tag {
            name: "floor",
            parent: "dressing",
            selector: None,
        },
        Tag {
            name: "wall",
            parent: "dressing",
            selector: None,
        },
        Tag {
            name: "building",
            parent: "dressing",
            selector: None,
        },
        Tag {
            name: "spans",
            parent: "dressing",
            selector: None,
        },
    ],
    decode: read,
    checks: &[Check {
        id: "kowloon-single-dressing",
        run: check,
    }],
};

fn check(document: &Document) -> Result<(), Diagnostic> {
    if let Some(duplicate) = document
        .metadata
        .iter()
        .filter(|node| node.kind == "dressing")
        .nth(1)
    {
        return Err(duplicate.fail("AML_DRESSING", "Only one dressing section is allowed."));
    }
    Ok(())
}

fn read(xml: Element<'_, '_>, source: &AmlSource<'_>) -> Result<Node, Diagnostic> {
    let kind = xml.tag_name().name();
    let allowed = match kind {
        "dressing" => "theme seed grime wear rust detail time-of-day finishes wall-materials",
        "floor" => {
            "host material face windows grime wear ground patch-size material-size relief wetness litter repairs tile-share tile-size tile-aspect tile-wear tile-grime tile-color timber-share joined-pours asphalt manholes breakup asphalt-material texture-density"
        }
        "building" => {
            "host material face windows grime wear rust stories shops texture-density base-wall model"
        }
        "wall" => {
            "host material face windows grime wear texture-density tile-share tile-size tile-aspect tile-wear tile-grime tile-color"
        }
        "spans" => "hosts cables clotheslines signs clearance reach",
        _ => {
            return Err(source.failure(
                xml,
                "AML_UNSUPPORTED",
                format!("Rust dressing recipe <{kind}> is not implemented yet."),
            ));
        }
    };
    let mut node = Node {
        kind: kind.into(),
        id: String::new(),
        source: source.location(xml),
        attributes: BTreeMap::new(),
        children: vec![],
    };
    for attribute in xml.attributes() {
        let key = attribute.name();
        let text = attribute.value();
        if !allowed.split_whitespace().any(|value| value == key) {
            return Err(node.fail(
                "AML_UNSUPPORTED",
                format!("Unsupported {key} on dressing <{kind}>."),
            ));
        }
        let value = match key {
            "model" if text != "reference-corner" => {
                return Err(node.fail("AML_DRESSING", "Unknown building model."));
            }
            "grime" | "wear" | "rust" | "shops" | "wetness" | "litter" | "repairs"
            | "tile-share" | "timber-share" | "joined-pours" | "asphalt" | "manholes"
            | "breakup" | "tile-wear" | "tile-grime" | "cables" | "clotheslines" | "signs" => {
                let value = text
                    .parse::<f64>()
                    .ok()
                    .filter(|v| v.is_finite() && (0.0..=1.0).contains(v))
                    .ok_or_else(|| {
                        node.fail("AML_DRESSING", format!("{key} must be between 0 and 1."))
                    })?;
                Value::Number(value)
            }
            "clearance" | "reach" => {
                let maximum = if key == "clearance" { 10. } else { 40. };
                Value::Number(
                    text.parse::<f64>()
                        .ok()
                        .filter(|v| (2. ..=maximum).contains(v))
                        .ok_or_else(|| {
                            node.fail(
                                "AML_DRESSING",
                                format!("{key} must be between 2 and {maximum}."),
                            )
                        })?,
                )
            }
            "patch-size" | "material-size" | "relief" | "tile-size" | "tile-aspect" => {
                Value::Number(
                    text.parse::<f64>()
                        .ok()
                        .filter(|v| v.is_finite())
                        .ok_or_else(|| {
                            node.fail("AML_DRESSING", format!("{key} requires a finite number."))
                        })?,
                )
            }
            "stories" | "texture-density" => {
                let value = text
                    .parse::<f64>()
                    .ok()
                    .filter(|v| v.is_finite() && *v > 0.)
                    .ok_or_else(|| {
                        node.fail(
                            "AML_DRESSING",
                            format!("{key} must be finite and positive."),
                        )
                    })?;
                if (key == "stories" && (value.fract() != 0. || value > 16.))
                    || (key == "texture-density" && !(16. ..=1024.).contains(&value))
                {
                    return Err(node.fail("AML_DRESSING", format!("Invalid building {key}.")));
                }
                Value::Number(value)
            }
            "windows" => Value::Boolean(match text {
                "true" => true,
                "false" => false,
                _ => return Err(node.fail("AML_DRESSING", "windows requires true or false.")),
            }),
            _ => Value::Text(text.into()),
        };
        node.attributes.insert(key.into(), value);
    }
    if kind == "dressing" {
        if node
            .text("seed")
            .is_none_or(|value| value.is_empty() || value.len() > 4096)
        {
            return Err(node.fail(
                "AML_DRESSING",
                "Dressing requires a nonempty seed of at most 4096 bytes.",
            ));
        }
        for (key, default, choices) in [
            ("theme", "kowloon-street@1", &["kowloon-street@1"][..]),
            ("detail", "medium", &["low", "medium", "high"][..]),
            ("time-of-day", "day", &["day", "night"][..]),
        ] {
            if node
                .text(key)
                .is_some_and(|value| !choices.contains(&value))
            {
                return Err(node.fail("AML_DRESSING", format!("Invalid dressing {key}.")));
            }
            node.attributes
                .entry(key.into())
                .or_insert(Value::Text(default.into()));
        }
        node.attributes
            .entry("grime".into())
            .or_insert(Value::Number(0.4));
        node.attributes
            .entry("wear".into())
            .or_insert(Value::Number(0.2));
        for child in xml.children().filter(Element::is_element) {
            if child.tag_name().name() == "dressing" {
                return Err(source.failure(child, "AML_CHILD", "Dressing cannot be nested."));
            }
            if node.children.len() >= 128 {
                return Err(node.fail(
                    "AML_DRESSING",
                    "Dressing is limited to 128 receiving hosts.",
                ));
            }
            node.children.push(read(child, source)?);
        }
    } else if kind == "spans" {
        let hosts: Vec<_> = node
            .text("hosts")
            .unwrap_or("")
            .split_whitespace()
            .collect();
        let unique: std::collections::BTreeSet<_> = hosts.iter().collect();
        if node.text("hosts").is_some()
            && (hosts.len() < 2 || hosts.len() > 512 || unique.len() != hosts.len())
        {
            return Err(node.fail(
                "AML_DRESSING",
                "Spans require 2–512 distinct building hosts.",
            ));
        }
        if xml.children().any(|child| child.is_element()) {
            return Err(node.fail("AML_CHILD", "Spans cannot contain children."));
        }
    } else {
        if node
            .text("host")
            .is_none_or(|value| value.trim().is_empty() || value.len() > 4096)
        {
            return Err(node.fail("AML_DRESSING", "Dressing requires a host."));
        }
        if node
            .text("material")
            .is_some_and(|value| value.trim().is_empty())
        {
            return Err(node.fail("AML_DRESSING", "Floor material cannot be empty."));
        }
        let building = kind == "building";
        let wall = kind == "wall";
        let face = if building {
            "front"
        } else if wall {
            "side"
        } else {
            "top"
        };
        if (wall
            && node
                .text("face")
                .is_some_and(|v| !["side", "front", "back", "left", "right"].contains(&v)))
            || (!wall
                && (node.text("face").is_some_and(|value| value != face)
                    || node.boolean("windows", building) != building))
        {
            return Err(node.fail(
                "AML_DRESSING",
                "Building dressing requires front/windows=true; floor dressing requires top/windows=false.",
            ));
        }
        if xml.children().any(|c| c.is_element()) {
            return Err(node.fail("AML_CHILD", "Floor dressing cannot contain children."));
        }
        node.attributes
            .entry("material".into())
            .or_insert(Value::Text(
                if building {
                    "builtin:plaster"
                } else {
                    "builtin:concrete"
                }
                .into(),
            ));
        node.attributes
            .entry("face".into())
            .or_insert(Value::Text(face.into()));
        node.attributes
            .entry("windows".into())
            .or_insert(Value::Boolean(building));
    }
    Ok(node)
}

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
        tag("shrub", "city"),
        tag("weed", "city"),
        tag("grass-field", "city"),
        tag("lamp", "city"),
        tag("ceiling-bulkhead", "city"),
        tag("wall-extraction-grille", "city"),
        tag("passage-sign-lamp", "city"),
        tag("utility-cabinet", "city"),
        tag("ceiling-twin-strip", "city"),
        tag("ceiling-girder", "city"),
        tag("pillar", "city"),
        tag("wall", "city"),
        tag("procedural-wall", "city"),
        tag("pipe-run", "city"),
        tag("duct-run", "city"),
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
            "footprint" if kind == "building" => {
                let mut points = Vec::new();
                for pair in text.split(';') {
                    let mut xy = pair.split_whitespace();
                    let x = quantize(number(xy.next().ok_or_else(invalid)?)?);
                    let y = quantize(number(xy.next().ok_or_else(invalid)?)?);
                    if xy.next().is_some() || points.len() == 4 {
                        return Err(invalid());
                    }
                    points.push([x, y]);
                }
                if points.len() != 4 {
                    return Err(node.fail(
                        "AML_CITY",
                        "A city building footprint needs four local XY corners.",
                    ));
                }
                for i in 0..4 {
                    let [a, b, c] = [points[i], points[(i + 1) % 4], points[(i + 2) % 4]];
                    let edge = [b[0] - a[0], b[1] - a[1]];
                    let turn = edge[0] * (c[1] - b[1]) - edge[1] * (c[0] - b[0]);
                    let length_squared = edge[0] * edge[0] + edge[1] * edge[1];
                    if turn <= 0. || !(36.0..=196.0).contains(&length_squared) {
                        return Err(node.fail(
                            "AML_CITY",
                            "A city building footprint must be convex and counterclockwise, with 6–14 m edges.",
                        ));
                    }
                }
                Value::PlanPoints(points)
            }
            "points" | "supports" | "hangerStations"
                if (matches!(kind, "pipe-run" | "duct-run") && key != "hangerStations")
                    || (kind, key) == ("ceiling-girder", "hangerStations") =>
            {
                let mut path = Vec::new();
                for xyz in text.split(';') {
                    let coords = xyz
                        .split_whitespace()
                        .map(number)
                        .collect::<Result<Vec<_>, _>>()?;
                    path.push(coords.try_into().map_err(|_| invalid())?);
                }
                let count = match key {
                    "points" => 2..=64,
                    "supports" => 1..=63,
                    _ => 1..=64,
                };
                if !count.contains(&path.len()) {
                    return Err(invalid());
                }
                Value::Points(path)
            }
            "soffitGap" if kind == "ceiling-girder" => {
                let gap = number(text)?;
                if !(0.0..=1.).contains(&gap) {
                    return Err(invalid());
                }
                Value::Number(gap)
            }
            "radius" if kind == "pipe-run" => Value::Number(number(text)?),
            "length" if matches!(kind, "ceiling-twin-strip" | "ceiling-girder") => {
                let length = quantize(number(text)?);
                let range = if kind == "ceiling-twin-strip" {
                    0.4..=2.4
                } else {
                    1.6..=20.
                };
                if !range.contains(&length) {
                    return Err(invalid());
                }
                Value::Number(length)
            }
            "width" | "height" if kind == "duct-run" => Value::Number(number(text)?),
            "variant" if kind == "weed" && matches!(text, "mound" | "seam") => {
                Value::Text(text.into())
            }
            "src"
                if kind == "grass-field"
                    && text.ends_with(".glb")
                    && !text.contains([':', '\\', '\0'])
                    && text.split('/').all(|part| !matches!(part, "" | "." | "..")) =>
            {
                Value::Text(text.into())
            }
            "scale" if matches!(kind, "tree" | "shrub" | "weed") => {
                let scale = quantize(number(text)?);
                if !(0.05..=5.).contains(&scale) {
                    return Err(invalid());
                }
                Value::Number(scale)
            }
            "model" if kind == "building" && text == "reference-corner" => Value::Text(text.into()),
            "front-trade"
                if kind == "building"
                    && matches!(
                        text,
                        "provisions"
                            | "grocer"
                            | "greengrocer"
                            | "hardware"
                            | "herbalist"
                            | "roast"
                    ) =>
            {
                Value::Text(text.into())
            }
            "seed" if kind == "building" => Value::Number(
                text.parse::<u8>()
                    .ok()
                    .filter(|v| *v <= 31)
                    .ok_or_else(invalid)? as f64,
            ),
            "seed" if matches!(kind, "procedural-wall" | "utility-cabinet") => Value::Number(
                text.parse::<u32>()
                    .ok()
                    .filter(|v| *v <= i32::MAX as u32)
                    .ok_or_else(invalid)? as f64,
            ),
            "width" | "depth" | "height"
                if matches!(kind, "pillar" | "wall" | "procedural-wall") =>
            {
                let metres = quantize(number(text)?);
                let limit = if key == "height" { 30. } else { 40. };
                if !(0.05..=limit).contains(&metres) {
                    return Err(invalid());
                }
                Value::Number(metres)
            }
            "width" | "depth" if kind == "building" => {
                let metres = quantize(number(text)?);
                if !(6.0..=14.0).contains(&metres) {
                    return Err(invalid());
                }
                Value::Number(metres)
            }
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
            "banner" | "dressing" | "enclosed"
                if (kind, key) == ("lamp", "banner")
                    || (kind, key) == ("ceiling-twin-strip", "enclosed")
                    || (kind, key) == ("procedural-wall", "dressing") =>
            {
                Value::Boolean(match text {
                    "true" | "1" => true,
                    "false" | "0" => false,
                    _ => return Err(invalid()),
                })
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
        if kind == "ceiling-girder" {
            let length = node.number("length", 4.);
            if node
                .points("hangerStations")
                .iter()
                .any(|&[x, y, z]| x <= 0. || x >= length || y != 0. || z != 0.)
            {
                return Err(node.fail(
                    "AML_CITY",
                    "A girder hanger must lie on the local X axis, between its two ends.",
                ));
            }
        }
        if kind == "grass-field" && node.text("src").is_none() {
            return Err(node.fail(
                "AML_CITY",
                "A grass field requires a relative GLB companion.",
            ));
        }
        if node.text("model") == Some("reference-corner")
            && (node.number("floors", 4.) != 4.
                || ["width", "depth", "footprint", "seed", "front-trade"]
                    .iter()
                    .any(|k| node.attributes.contains_key(*k)))
        {
            return Err(node.fail(
                "AML_CITY",
                "The reference corner uses its authored four-storey lot.",
            ));
        }
        if node.attributes.contains_key("footprint")
            && (node.attributes.contains_key("width") || node.attributes.contains_key("depth"))
        {
            return Err(node.fail(
                "AML_CITY",
                "Choose a building footprint or rectangular width and depth.",
            ));
        }
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

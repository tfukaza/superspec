//! Validated AML source tree. Values are typed and normalized once, before resolution.
use crate::{
    diagnostic::Diagnostic,
    extensions::Registry,
    source::{AmlSource, SourceLimits, parse_source, valid_id},
};
use std::collections::{BTreeMap, BTreeSet};
pub use superspec_types::architecture::{Node, V3, Value};

#[derive(Clone, Debug)]
pub struct Document {
    pub root: Node,
    pub declarations: BTreeMap<String, Node>,
    pub elements: Vec<Node>,
    pub metadata: Vec<Node>,
}

const PLACEMENT: &str = "frame position rotation scale";
const BASE: &str =
    "id label material group visible collidable dynamic frame position rotation scale";
const COLLECTIONS: &[&str] = &[
    "assembly",
    "group",
    "union",
    "subtract",
    "intersect",
    "collision",
    "definition",
];
const GEOMETRY: &[&str] = &[
    "assembly",
    "group",
    "union",
    "subtract",
    "intersect",
    "collision",
    "instance",
    "box",
    "wedge",
    "cylinder",
    "cone",
    "sphere",
    "slab",
    "landing",
    "roof",
    "canopy",
    "wall",
    "opening",
    "panel",
    "glazing",
    "window",
    "infill",
    "frame",
    "extrusion",
    "sweep",
    "beam",
    "column",
    "stairs",
    "ramp",
    "enclosure",
    "retaining-wall",
    "terrain",
    "grade",
    "shared-edge",
    "water",
    "zone",
    "room",
    "volume",
    "clearance",
    "faceted-solid",
    "collision-surface",
    "visual-mesh",
    "face-material",
];
const DECLARATIONS: &[&str] = &[
    "coordinate-frame",
    "datum",
    "path",
    "profile",
    "definition",
    "material",
];
const METADATA: &[&str] = &[
    "player",
    "boat",
    "marker",
    "route",
    "link",
    "connection",
    "light",
    "sky",
    "reference",
    "review-suppression",
    "no-spawn-los",
];

/// Authored dimensions match the existing symmetric half-away-from-zero lattice.
/// Epsilon correction retains decimal ties represented just below half a millimeter.
pub fn quantize(value: f64) -> f64 {
    let scaled = value.abs() * 1000.;
    let units = (scaled + 0.5 + f64::EPSILON * scaled.max(1.)).floor();
    if units == 0. {
        0.
    } else {
        value.signum() * units / 1000.
    }
}

fn permitted(kind: &str) -> Option<String> {
    let attributes = match kind {
        "architecture" => "version id label name description units validation curve-tolerance",
        "material" => {
            "id label color repeat roughness metalness emissive emissive-strength texture normal-texture normal-scale metallic-roughness-texture height-texture height-scale height-offset opacity alpha-cutoff"
        }
        "coordinate-frame" => return Some(format!("id label {PLACEMENT}")),
        "datum" => "id label elevation datum",
        "path" => {
            return Some(format!(
                "id label {PLACEMENT} points paths closed elevation"
            ));
        }
        "profile" => {
            return Some(format!(
                "id label {PLACEMENT} points paths dimensions interior"
            ));
        }
        "hole" => "points paths dimensions",
        "breakline" => "points",
        "line" => "to",
        "quadratic" | "cubic" | "bezier" => "to controls",
        "arc" => "center radii start sweep rotation",
        "definition" => "id label",
        "assembly" | "group" | "union" | "subtract" | "intersect" | "collision" => {
            return Some(format!("{BASE} material-owner coincident-material"));
        }
        "box" | "wedge" | "cylinder" | "cone" | "sphere" => {
            return Some(format!(
                "{BASE} width depth height radius top-radius segments anchor"
            ));
        }
        "wall" => {
            return Some(format!(
                "{BASE} path height thickness alignment base top base-floor contact-floor fit-floor top-floor span-floors join"
            ));
        }
        "slab" | "landing" | "roof" | "canopy" => {
            return Some(format!(
                "{BASE} profile elevation datum offset thickness ceiling area flat playable"
            ));
        }
        "opening" => {
            return Some(format!(
                "{BASE} host shape mode center width height radius radii profile depth offset reveal-material"
            ));
        }
        "window" => "id label group visible collidable dynamic opening open",
        "panel" | "glazing" | "infill" | "frame" => {
            return Some(format!("{BASE} opening depth offset border"));
        }
        "extrusion" => return Some(format!("{BASE} profile distance")),
        "sweep" | "beam" | "column" => return Some(format!("{BASE} profile path")),
        "stairs" | "ramp" => {
            return Some(format!(
                "{BASE} path width width-profile left right count maximum-riser thickness underside underside-profile area"
            ));
        }
        "enclosure" => {
            return Some(format!(
                "{BASE} host side loop edge start-return end-return construction start end offset thickness height top"
            ));
        }
        "rail" => "profile top-offset",
        "posts" => "profile maximum-spacing",
        "retaining-wall" => {
            return Some(format!(
                "{BASE} lower upper lower-floor upper-floor thickness height mode"
            ));
        }
        "instance" => return Some(format!("{BASE} ref")),
        "visual-mesh" => return Some(format!("{BASE} src")),
        "faceted-solid" | "collision-surface" => return Some(format!("{BASE} points triangles")),
        "terrain" => {
            return Some(format!(
                "{BASE} profile heightmap tile-size min-height max-height flip-y points triangles columns rows heights cell-size base orientation playable"
            ));
        }
        "grade" => {
            return Some(format!(
                "{BASE} host path width width-profile cross-slope transition start-transition end-transition order"
            ));
        }
        "shared-edge" => return Some(format!("{BASE} from to path")),
        "water" => return Some(format!("{BASE} profile elevation datum offset")),
        "room" | "zone" | "volume" | "clearance" => {
            return Some(format!(
                "{BASE} profile elevation datum offset height role movement require-support"
            ));
        }
        "face-material" => "id label host face profile projection material",
        "property" => "name value",
        "player" => "radius height step slope crouch vault rappel ladder",
        "boat" => "radius height draft",
        "marker" => "id label group element position kind team movement",
        "route" => "id label from to min-routes min-distance max-distance movement",
        "link" => "id label from to kind bidirectional ability from-area to-area movement",
        "connection" => "id label from to path",
        "light" => "id label kind position target color intensity angle",
        "sky" => "color intensity",
        "reference" => {
            "id label image width height scale scale-note position target up fov near far"
        }
        "landmark" => "id label position image",
        "mask" => "id label points",
        "review-suppression" => "suggestion fingerprint reason",
        "no-spawn-los" => "markers",
        _ => return None,
    };
    Some(attributes.into())
}

fn values(text: &str) -> Result<Vec<f64>, String> {
    let values = text
        .split(|c: char| c.is_whitespace() || c == ',')
        .filter(|v| !v.is_empty())
        .map(str::parse::<f64>)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| "Expected finite numbers.".to_owned())?;
    if values.iter().any(|v| !v.is_finite() || v.abs() > 1e7) {
        return Err("Coordinates/numbers must be finite and within ten million.".into());
    }
    Ok(values)
}

pub(crate) fn core_child(parent: &str, tag: &str) -> bool {
    match parent {
        "architecture" => {
            GEOMETRY.contains(&tag) || DECLARATIONS.contains(&tag) || METADATA.contains(&tag)
        }
        parent if COLLECTIONS.contains(&parent) => GEOMETRY.contains(&tag),
        "path" => ["line", "quadratic", "cubic", "bezier", "arc"].contains(&tag),
        "profile" => ["hole", "breakline"].contains(&tag),
        "enclosure" => ["rail", "posts"].contains(&tag),
        "room" | "zone" | "volume" | "clearance" => tag == "property",
        "reference" => ["landmark", "mask"].contains(&tag),
        _ => false,
    }
}

fn read(
    xml: roxmltree::Node<'_, '_>,
    source: &AmlSource<'_>,
    registry: &Registry,
) -> Result<Node, Diagnostic> {
    let kind = xml.tag_name().name();
    if let Some(result) = registry.decode(xml, source) {
        return result;
    }
    let allowed = permitted(kind).ok_or_else(|| {
        source.failure(
            xml,
            "AML_UNSUPPORTED",
            format!("Unsupported architectural element <{kind}>."),
        )
    })?;
    let mut node = Node {
        kind: kind.into(),
        id: xml.attribute("id").unwrap_or("").into(),
        source: source.location(xml),
        attributes: BTreeMap::new(),
        children: vec![],
    };
    let error = |message: String| source.failure(xml, "AML_ATTRIBUTE", message);
    for attribute in xml.attributes() {
        let key = attribute.name();
        let text = attribute.value();
        if !allowed.split_whitespace().any(|name| name == key) {
            return Err(error(format!("Unknown attribute {key} on <{kind}>.")));
        }
        let vector = match key {
            "position" | "target" | "up" => Some(3),
            "rotation" if kind != "arc" => Some(3),
            "scale" if kind != "reference" => Some(3),
            "center" => Some(if kind == "opening" { 2 } else { 3 }),
            "radii" | "cell-size" => Some(2),
            "elevation" if kind == "path" => Some(2),
            "to" if ["line", "quadratic", "cubic", "bezier", "link"].contains(&kind) => Some(3),
            "from" if kind == "link" => Some(3),
            "image" if kind == "landmark" => Some(2),
            _ => None,
        };
        let value = if let Some(dimensions) = vector {
            let mut parsed = values(text).map_err(error)?;
            if parsed.len() != dimensions {
                return Err(error(format!("{key} requires {dimensions} coordinates.")));
            }
            if key != "rotation"
                && key != "scale"
                && kind != "reference"
                && !(key == "image" && kind == "landmark")
            {
                parsed.iter_mut().for_each(|v| *v = quantize(*v));
            }
            Value::Vector(parsed)
        } else if [
            "visible",
            "open",
            "collidable",
            "dynamic",
            "closed",
            "span-floors",
            "flat",
            "playable",
            "flip-y",
            "require-support",
            "rappel",
            "ladder",
            "bidirectional",
        ]
        .contains(&key)
        {
            if !["true", "false"].contains(&text) {
                return Err(error(format!("{key} must be true or false.")));
            }
            Value::Boolean(text == "true")
        } else if ["points", "interior", "controls"].contains(&key) {
            let dimensions = if kind == "mask"
                || (["profile", "hole"].contains(&kind)
                    && key == "points"
                    && xml.attribute("dimensions").unwrap_or("2") == "2")
            {
                2
            } else {
                3
            };
            let mut points = vec![];
            if !text.trim().is_empty() {
                for point in text.split(';') {
                    let mut coordinates = values(point).map_err(error)?;
                    if coordinates.len() != dimensions {
                        return Err(error(format!(
                            "{key} requires semicolon-separated {dimensions}D points."
                        )));
                    }
                    if dimensions == 2 {
                        coordinates.push(0.);
                    }
                    if kind != "mask" {
                        coordinates.iter_mut().for_each(|v| *v = quantize(*v));
                    }
                    points.push(coordinates.try_into().expect("3 coordinates"));
                }
            }
            Value::Points(points)
        } else if key == "triangles" {
            let mut triangles = vec![];
            if !text.trim().is_empty() {
                for triangle in text.split(';') {
                    let nums = values(triangle).map_err(error)?;
                    if nums.len() != 3 || nums.iter().any(|v| *v < 0. || v.fract() != 0.) {
                        return Err(error(
                            "Triangles require three nonnegative integer indices.".into(),
                        ));
                    }
                    triangles.push([nums[0] as usize, nums[1] as usize, nums[2] as usize]);
                }
            }
            Value::Triangles(triangles)
        } else if key == "heights" {
            Value::Vector(
                values(text)
                    .map_err(error)?
                    .into_iter()
                    .map(quantize)
                    .collect(),
            )
        } else if numeric(kind, key) {
            let nums = values(text).map_err(error)?;
            if nums.len() != 1 {
                return Err(error(format!("{key} requires one finite number.")));
            }
            let value = if linear(kind, key) {
                quantize(nums[0])
            } else {
                nums[0]
            };
            Value::Number(value)
        } else {
            Value::Text(text.into())
        };
        node.attributes.insert(key.into(), value);
    }
    if allowed.split_whitespace().any(|key| key == "id") && (!valid_id(&node.id)) {
        return Err(node.fail("AML_ID", "Missing or invalid source id."));
    }
    validate_choices(&node)?;
    for child in xml.children().filter(roxmltree::Node::is_element) {
        let tag = child.tag_name().name();
        let accepted = core_child(kind, tag) || registry.accepts(kind, tag);
        if !accepted {
            return Err(source.failure(
                child,
                "AML_CHILD",
                format!("<{tag}> is not allowed inside <{kind}> by core AML or the registered extensions."),
            ));
        }
        node.children.push(read(child, source, registry)?);
    }
    validate_required(&node)?;
    Ok(node)
}

fn numeric(kind: &str, key: &str) -> bool {
    match key {
        "base"=>kind=="terrain", "start"|"end"=>["arc","enclosure"].contains(&kind),
        "rotation"|"sweep"=>kind=="arc", "elevation"=>kind!="path",
        _=>"width depth height radius top-radius segments thickness offset border distance count maximum-riser loop edge start-return end-return top-offset maximum-spacing tile-size min-height max-height columns rows cross-slope transition start-transition end-transition order ceiling curve-tolerance repeat roughness metalness emissive-strength normal-scale height-scale height-offset opacity alpha-cutoff step slope crouch vault draft min-routes min-distance max-distance intensity angle fov near far".split_whitespace().any(|name|name==key)
    }
}
fn linear(kind: &str, key: &str) -> bool {
    if kind == "reference" {
        return false;
    }
    "width depth height radius top-radius thickness offset border distance maximum-riser start-return end-return top-offset maximum-spacing min-height max-height transition start-transition end-transition ceiling step crouch vault draft min-distance max-distance elevation".split_whitespace().any(|name|name==key)||(key=="base"&&kind=="terrain")||(["start","end"].contains(&key)&&kind=="enclosure")
}
fn validate_choices(node: &Node) -> Result<(), Diagnostic> {
    let choices: [(&str, &[&str]); 17] = [
        ("units", &["meters"]),
        ("validation", &["premap", "playable"]),
        ("dimensions", &["2", "3"]),
        ("anchor", &["corner", "bottom-center"]),
        ("alignment", &["center", "left", "right"]),
        ("join", &["miter", "round"]),
        (
            "shape",
            &["rectangle", "circle", "ellipse", "arch", "profile"],
        ),
        (
            "underside",
            &["filled", "sloped", "stepped", "custom", "separate"],
        ),
        ("side", &["left", "right", "start", "end"]),
        ("construction", &["wall", "parapet", "curb", "railing"]),
        ("orientation", &["up", "down", "authored", "unknown"]),
        ("movement", &["land", "water", "none"]),
        ("projection", &["xy", "xz", "yz"]),
        (
            "face",
            &[
                "all", "top", "bottom", "front", "back", "left", "right", "side", "reveal",
            ],
        ),
        ("coincident-material", &["error", "retained-face"]),
        (
            "mode",
            if node.kind == "retaining-wall" {
                &["open", "wall"]
            } else {
                &["through", "recess", "notch"]
            },
        ),
        (
            "scale",
            if node.kind == "reference" {
                &["assumed", "measured"]
            } else {
                &[]
            },
        ),
    ];
    for (key, values) in choices {
        if let Some(value) = node.text(key)
            && !values.contains(&value)
        {
            return Err(node.fail("AML_ATTRIBUTE", format!("Invalid {key}={value}.")));
        }
    }
    if node.kind == "architecture" && node.text("version") != Some("1") {
        return Err(node.fail("AML_VERSION", "Expected AML version 1."));
    }
    if node.attributes.contains_key("datum")
        && node.attributes.contains_key("elevation")
        && node.kind != "datum"
    {
        return Err(node.fail("AML_ATTRIBUTE", "Choose datum or absolute elevation."));
    }
    Ok(())
}
fn validate_required(node: &Node) -> Result<(), Diagnostic> {
    let required = match node.kind.as_str() {
        "wall" => "path",
        "slab" | "landing" | "roof" | "canopy" | "extrusion" | "water" | "zone" | "room"
        | "volume" | "clearance" => "profile",
        "opening" | "enclosure" => "host",
        "panel" | "glazing" | "infill" | "frame" | "window" => "opening",
        "sweep" | "beam" | "column" => "profile path",
        "instance" => "ref",
        "retaining-wall" => "lower upper",
        "grade" => "host path",
        "shared-edge" => "from to path",
        "face-material" => "host material",
        "visual-mesh" => "src",
        "line" | "quadratic" | "cubic" | "bezier" => "to",
        "arc" => "center radii sweep",
        "route" | "connection" => "from to",
        "link" => "from to",
        "marker" => "kind position",
        "reference" => "image width height scale-note position target",
        "rail" => "profile",
        "posts" => "profile maximum-spacing",
        "property" => "name value",
        "landmark" => "position image",
        "review-suppression" => "suggestion fingerprint reason",
        "no-spawn-los" => "markers",
        _ => "",
    };
    for key in required.split_whitespace() {
        if !node.attributes.contains_key(key) || node.text(key).is_some_and(|v| v.trim().is_empty())
        {
            return Err(node.fail("AML_ATTRIBUTE", format!("<{}> requires {key}.", node.kind)));
        }
    }
    Ok(())
}

/// Reads core architectural vocabulary without installing source extensions.
pub fn parse(text: &str) -> Result<Document, Diagnostic> {
    parse_with_registry(text, &Registry::default())
}

/// Admits typed source, scoped IDs and registered source checks, without asset or geometry work.
/// Architectural references and physical/navigation validity remain compiler responsibilities.
pub fn parse_with_registry(text: &str, registry: &Registry) -> Result<Document, Diagnostic> {
    let source = parse_source(text, SourceLimits::default())?;
    let root = read(source.root(), &source, registry)?;
    let mut document = Document {
        root: root.clone(),
        declarations: BTreeMap::new(),
        elements: vec![],
        metadata: vec![],
    };
    let mut ids = BTreeSet::new();
    fn unique(node: &Node, ids: &mut BTreeSet<String>) -> Result<(), Diagnostic> {
        if !node.id.is_empty() && !valid_id(&node.id) {
            return Err(node.fail("AML_ID", "Invalid source id."));
        }
        if !node.id.is_empty() && !ids.insert(node.id.clone()) {
            return Err(node.fail("AML_DUPLICATE_ID", "Duplicate source id."));
        }
        let mut local = BTreeSet::new();
        let scope = if node.kind == "definition" {
            &mut local
        } else {
            ids
        };
        for child in &node.children {
            unique(child, scope)?;
        }
        Ok(())
    }
    for node in &root.children {
        unique(node, &mut ids)?;
        if DECLARATIONS.contains(&node.kind.as_str()) {
            document.declarations.insert(node.id.clone(), node.clone());
        } else if GEOMETRY.contains(&node.kind.as_str()) {
            document.elements.push(node.clone());
        } else {
            document.metadata.push(node.clone());
        }
    }
    if document.root.number("curve-tolerance", 0.01) <= 0. {
        return Err(document
            .root
            .fail("AML_ATTRIBUTE", "Curve tolerance must be positive."));
    }
    registry.check(&document)?;
    Ok(document)
}

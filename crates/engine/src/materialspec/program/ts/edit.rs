//! Source-preserving inspection and edits for TypeScript recipes.
//!
//! `inspect` lists parameters, fields, layers, groups and the spans of their values and uses.
//! `apply` turns one structured operation into minimal text patches: only the targeted span
//! changes, so comments and unrelated expressions survive. The patched recipe is compiled again
//! before anything is returned, so an invalid edit yields diagnostics and no patch. The same
//! operations back the Studio graph inspector and the command line.
use super::{
    Init, Node, NodeKind, Parser, Property, Statement, Token, is_call_to, parse_statements,
    tokenize,
};
use crate::materialspec::{
    catalog,
    program::{ProgramDiagnostic, Sources, Type, compile, diagnostic, syntax::Mark},
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// A half-open range of UTF-16 code units, matching editor offsets and diagnostics.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub name: String,
    pub span: Span,
    pub text: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ParameterInfo {
    pub name: String,
    #[serde(rename = "type")]
    pub kind: Type,
    pub declaration: Span,
    pub default: Span,
    pub default_text: String,
    pub range: Option<[f64; 2]>,
    pub range_span: Option<Span>,
    pub unit: Option<String>,
    pub label: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FieldInfo {
    pub name: String,
    pub declaration: Span,
    pub value: Span,
    pub value_text: String,
    /// Spans of every use of this field, in source order.
    pub references: Vec<Span>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HelperInfo {
    pub name: String,
    pub declaration: Span,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GroupInfo {
    pub name: String,
    pub group: String,
    pub declaration: Span,
    pub arguments: Vec<Entry>,
    pub outputs: Vec<String>,
    pub references: Vec<Span>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LayerInfo {
    pub id: String,
    /// The layer's entry in the `layers` array.
    pub item: Span,
    /// The constant that holds the layer, when it is declared separately.
    pub declared_as: Option<String>,
    pub mask: Option<Entry>,
    pub channels: Vec<Entry>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Inspection {
    pub module: String,
    pub sdk: &'static str,
    pub parameters: Vec<ParameterInfo>,
    pub fields: Vec<FieldInfo>,
    pub helpers: Vec<HelperInfo>,
    pub groups: Vec<GroupInfo>,
    pub base: Vec<Entry>,
    pub layers: Vec<LayerInfo>,
    pub channels: Vec<Entry>,
    pub live: Vec<Entry>,
}

/// A text replacement in UTF-16 code units. Patches never overlap.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Patch {
    pub start: usize,
    pub end: usize,
    pub text: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "op", rename_all = "camelCase")]
pub enum EditOp {
    /// Replace the numeric literal that starts at `offset`.
    SetLiteral {
        offset: usize,
        value: f64,
    },
    /// Replace the expression occupying exactly `start..end` with `expression`. This is how a
    /// use of a shared field is reconnected to another field, group output or expression.
    Reconnect {
        start: usize,
        end: usize,
        expression: String,
    },
    SetParameterDefault {
        name: String,
        value: serde_json::Value,
    },
    SetParameterRange {
        name: String,
        range: Option<[f64; 2]>,
    },
    /// Insert a `layer({ ... })` expression, or a name of a declared layer, at `index`.
    AddLayer {
        source: String,
        index: Option<usize>,
    },
    RemoveLayer {
        id: String,
    },
    MoveLayer {
        id: String,
        to: usize,
    },
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Edited {
    pub patches: Vec<Patch>,
    /// The complete module text after the patches.
    pub source: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct EditFailure {
    pub diagnostics: Vec<ProgramDiagnostic>,
}
impl std::fmt::Display for EditFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for (index, error) in self.diagnostics.iter().enumerate() {
            if index > 0 {
                writeln!(f)?;
            }
            write!(f, "{error}")?;
        }
        Ok(())
    }
}
impl std::error::Error for EditFailure {}

struct Analysis<'a> {
    sources: &'a Sources,
    module: &'a str,
    text: &'a str,
    statements: Vec<Statement>,
}

fn utf16(text: &str, byte: usize) -> usize {
    text[..byte.min(text.len())].encode_utf16().count()
}

fn byte_at(text: &str, unit: usize) -> Option<usize> {
    let mut units = 0;
    for (byte, character) in text.char_indices() {
        if units == unit {
            return Some(byte);
        }
        units += character.len_utf16();
        if units > unit {
            return None;
        }
    }
    (units == unit).then_some(text.len())
}

fn failure(sources: &Sources, module: &str, message: impl Into<String>) -> EditFailure {
    EditFailure {
        diagnostics: vec![diagnostic(sources, "MT_EDIT", message, &Mark::root(module))],
    }
}

fn walk<'n>(node: &'n Node, out: &mut Vec<&'n Node>) {
    out.push(node);
    match &node.kind {
        NodeKind::Call(_, args) => args.iter().for_each(|argument| walk(argument, out)),
        NodeKind::Member(base, _) => walk(base, out),
        NodeKind::Object(properties) => properties.iter().for_each(|p| walk(&p.value, out)),
        NodeKind::Array(items) => items.iter().for_each(|item| walk(item, out)),
        _ => {}
    }
}

impl<'a> Analysis<'a> {
    fn new(sources: &'a Sources, module: &'a str) -> Result<Self, EditFailure> {
        let Some(text) = sources.modules.get(module) else {
            return Err(failure(
                sources,
                module,
                format!("Module '{module}' was not supplied."),
            ));
        };
        let statements = parse_statements(sources, module).map_err(|error| EditFailure {
            diagnostics: vec![error],
        })?;
        Ok(Self {
            sources,
            module,
            text,
            statements,
        })
    }

    fn span(&self, start: usize, end: usize) -> Span {
        Span {
            start: utf16(self.text, start),
            end: utf16(self.text, end),
        }
    }
    fn node_span(&self, node: &Node) -> Span {
        self.span(node.mark.start, node.end)
    }
    fn entry(&self, name: &str, node: &Node) -> Entry {
        Entry {
            name: name.into(),
            span: self.node_span(node),
            text: self.text[node.mark.start..node.end].into(),
        }
    }

    fn nodes(&self) -> Vec<&Node> {
        let mut out = Vec::new();
        for statement in &self.statements {
            match statement {
                Statement::Const {
                    init: Init::Expr(node),
                    ..
                }
                | Statement::Default(node) => walk(node, &mut out),
                Statement::Const {
                    init: Init::Function { body, .. },
                    ..
                } => walk(body, &mut out),
                Statement::Import { .. } => {}
            }
        }
        out
    }

    fn references(&self, name: &str) -> Vec<Span> {
        let mut spans: Vec<Span> = self
            .nodes()
            .into_iter()
            .filter(|node| matches!(&node.kind, NodeKind::Name(n) if n == name))
            .map(|node| self.node_span(node))
            .collect();
        spans.sort_by_key(|span| span.start);
        spans
    }

    fn constant(&self, name: &str) -> Option<(&Mark, &Node)> {
        self.statements
            .iter()
            .find_map(|statement| match statement {
                Statement::Const {
                    name: n,
                    mark,
                    init: Init::Expr(node),
                    ..
                } if n == name => Some((mark, node)),
                _ => None,
            })
    }

    /// The options object of `defineMaterial({ ... })`.
    fn material(&self) -> Option<(&Node, &[Property])> {
        self.statements
            .iter()
            .find_map(|statement| match statement {
                Statement::Default(node) => {
                    let args = is_call_to(node, "defineMaterial")?;
                    match args.first() {
                        Some(
                            options @ Node {
                                kind: NodeKind::Object(properties),
                                ..
                            },
                        ) => Some((options, properties.as_slice())),
                        _ => None,
                    }
                }
                _ => None,
            })
    }

    fn property<'n>(properties: &'n [Property], key: &str) -> Option<&'n Property> {
        properties.iter().find(|p| p.key == key)
    }

    fn entries(&self, node: &Node) -> Vec<Entry> {
        match &node.kind {
            NodeKind::Object(properties) => properties
                .iter()
                .map(|p| self.entry(&p.key, &p.value))
                .collect(),
            _ => Vec::new(),
        }
    }

    fn layer_items(&self) -> Vec<(&Node, Option<String>, &Node)> {
        let Some((_, properties)) = self.material() else {
            return Vec::new();
        };
        let Some(Property {
            value:
                Node {
                    kind: NodeKind::Array(items),
                    ..
                },
            ..
        }) = Self::property(properties, "layers")
        else {
            return Vec::new();
        };
        items
            .iter()
            .filter_map(|item| match &item.kind {
                NodeKind::Name(name) => self
                    .constant(name)
                    .filter(|(_, node)| is_call_to(node, "layer").is_some())
                    .map(|(_, node)| (item, Some(name.clone()), node)),
                _ => is_call_to(item, "layer").map(|_| (item, None, item)),
            })
            .collect()
    }
}

fn layer_id(node: &Node) -> Option<(String, &[Property])> {
    let args = is_call_to(node, "layer")?;
    let Node {
        kind: NodeKind::Object(properties),
        ..
    } = args.first()?
    else {
        return None;
    };
    match &properties.iter().find(|p| p.key == "id")?.value.kind {
        NodeKind::Text(id) => Some((id.clone(), properties.as_slice())),
        _ => None,
    }
}

pub fn inspect(sources: &Sources, module: &str) -> Result<Inspection, EditFailure> {
    let analysis = Analysis::new(sources, module)?;
    let mut inspection = Inspection {
        module: module.into(),
        sdk: catalog::SDK_MODULE,
        parameters: Vec::new(),
        fields: Vec::new(),
        helpers: Vec::new(),
        groups: Vec::new(),
        base: Vec::new(),
        layers: Vec::new(),
        channels: Vec::new(),
        live: Vec::new(),
    };
    for statement in &analysis.statements {
        let Statement::Const {
            name, mark, init, ..
        } = statement
        else {
            continue;
        };
        let declaration = analysis.span(mark.start, mark.end);
        match init {
            Init::Function { .. } => inspection.helpers.push(HelperInfo {
                name: name.clone(),
                declaration,
            }),
            Init::Expr(node) => {
                if let Some(parameter) = parameter(&analysis, name, declaration, node) {
                    inspection.parameters.push(parameter);
                } else if let Some(group) = group(&analysis, name, declaration, node) {
                    inspection.groups.push(group);
                } else if is_call_to(node, "layer").is_none() {
                    inspection.fields.push(FieldInfo {
                        name: name.clone(),
                        declaration,
                        value: analysis.node_span(node),
                        value_text: analysis.text[node.mark.start..node.end].into(),
                        references: analysis.references(name),
                    });
                }
            }
        }
    }
    if let Some((_, properties)) = analysis.material() {
        if let Some(base) = Analysis::property(properties, "base") {
            inspection.base = analysis.entries(&base.value);
        }
        if let Some(channels) = Analysis::property(properties, "channels") {
            inspection.channels = analysis.entries(&channels.value);
        }
        if let Some(Property {
            value:
                Node {
                    kind: NodeKind::Array(items),
                    ..
                },
            ..
        }) = Analysis::property(properties, "live")
        {
            inspection.live = items
                .iter()
                .map(|item| analysis.entry("live", item))
                .collect();
        }
        for (item, declared_as, node) in analysis.layer_items() {
            let Some((id, properties)) = layer_id(node) else {
                continue;
            };
            let mask = properties
                .iter()
                .find(|p| p.key == "mask")
                .map(|p| analysis.entry("mask", &p.value));
            let channels = properties
                .iter()
                .filter(|p| p.key != "id" && p.key != "mask")
                .map(|p| analysis.entry(&p.key, &p.value))
                .collect();
            inspection.layers.push(LayerInfo {
                id,
                item: analysis.node_span(item),
                declared_as,
                mask,
                channels,
            });
        }
    }
    Ok(inspection)
}

fn parameter(
    analysis: &Analysis,
    name: &str,
    declaration: Span,
    node: &Node,
) -> Option<ParameterInfo> {
    let NodeKind::Call(callee, args) = &node.kind else {
        return None;
    };
    let NodeKind::Member(base, kind) = &callee.kind else {
        return None;
    };
    if !matches!(&base.kind, NodeKind::Name(n) if n == "param") {
        return None;
    }
    let kind = match kind.as_str() {
        "scalar" => Type::Scalar,
        "vec2" => Type::Vec2,
        "vec3" => Type::Vec3,
        "bool" => Type::Bool,
        _ => return None,
    };
    let Node {
        kind: NodeKind::Object(properties),
        ..
    } = args.first()?
    else {
        return None;
    };
    let default = Analysis::property(properties, "default")?;
    let range = Analysis::property(properties, "range");
    let text = |key: &str| match Analysis::property(properties, key).map(|p| &p.value.kind) {
        Some(NodeKind::Text(value)) => Some(value.clone()),
        _ => None,
    };
    Some(ParameterInfo {
        name: name.into(),
        kind,
        declaration,
        default: analysis.node_span(&default.value),
        default_text: analysis.text[default.value.mark.start..default.value.end].into(),
        range: range.and_then(|p| match &p.value.kind {
            NodeKind::Array(items) => match items.as_slice() {
                [
                    Node {
                        kind: NodeKind::Number(lo),
                        ..
                    },
                    Node {
                        kind: NodeKind::Number(hi),
                        ..
                    },
                ] => Some([*lo, *hi]),
                _ => None,
            },
            _ => None,
        }),
        range_span: range.map(|p| analysis.node_span(&p.value)),
        unit: text("unit"),
        label: text("label"),
    })
}

fn group(analysis: &Analysis, name: &str, declaration: Span, node: &Node) -> Option<GroupInfo> {
    let NodeKind::Call(callee, args) = &node.kind else {
        return None;
    };
    let NodeKind::Name(id) = &callee.kind else {
        return None;
    };
    let group = catalog::find_group(id)?;
    Some(GroupInfo {
        name: name.into(),
        group: id.clone(),
        declaration,
        arguments: args
            .first()
            .map(|options| analysis.entries(options))
            .unwrap_or_default(),
        outputs: group.outputs.iter().map(|o| o.name.to_owned()).collect(),
        references: analysis.references(name),
    })
}

type Replacement = (usize, usize, String);

fn number(sources: &Sources, module: &str, value: f64) -> Result<String, EditFailure> {
    if !value.is_finite() {
        return Err(failure(sources, module, "Values must be finite numbers."));
    }
    let text = format!("{value}");
    Ok(if text == "-0" { "0".into() } else { text })
}

fn vector(
    sources: &Sources,
    module: &str,
    kind: Type,
    value: &serde_json::Value,
) -> Result<String, EditFailure> {
    let size = if kind == Type::Vec2 { 2 } else { 3 };
    let components: Option<Vec<f64>> = value
        .as_array()
        .filter(|items| items.len() == size)
        .and_then(|items| items.iter().map(serde_json::Value::as_f64).collect());
    let Some(components) = components else {
        return Err(failure(
            sources,
            module,
            format!("A {kind:?} default is an array of {size} numbers."),
        ));
    };
    let parts = components
        .into_iter()
        .map(|component| number(sources, module, component))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(format!("vec{size}({})", parts.join(", ")))
}

fn parse_expression(sources: &Sources, module: &str, text: &str) -> Result<(), EditFailure> {
    let scratch = Sources {
        entry: "expression".into(),
        modules: BTreeMap::from([("expression".into(), text.into())]),
    };
    let tokens = tokenize(&scratch, "expression", text).map_err(|error| EditFailure {
        diagnostics: vec![error],
    })?;
    let mut parser = Parser {
        sources: &scratch,
        module: "expression",
        tokens,
        cursor: 0,
        depth: 0,
        last_end: 0,
    };
    parser.expr().map_err(|error| {
        failure(
            sources,
            module,
            format!(
                "The replacement is not a valid expression: {}",
                error.message
            ),
        )
    })?;
    if parser.current().token != super::Token::End {
        return Err(failure(
            sources,
            module,
            "The replacement must be one expression.",
        ));
    }
    Ok(())
}

fn replacements(analysis: &Analysis, op: &EditOp) -> Result<Vec<Replacement>, EditFailure> {
    let (sources, module, text) = (analysis.sources, analysis.module, analysis.text);
    let fail = |message: String| failure(sources, module, message);
    match op {
        EditOp::SetLiteral { offset, value } => {
            let byte = byte_at(text, *offset)
                .ok_or_else(|| fail("The offset is not on a character boundary.".into()))?;
            let node = analysis
                .nodes()
                .into_iter()
                .find(|node| matches!(node.kind, NodeKind::Number(_)) && node.mark.start == byte)
                .ok_or_else(|| fail("No numeric literal starts at that offset.".into()))?;
            Ok(vec![(
                node.mark.start,
                node.end,
                number(sources, module, *value)?,
            )])
        }
        EditOp::Reconnect {
            start,
            end,
            expression,
        } => {
            let (Some(start), Some(end)) = (byte_at(text, *start), byte_at(text, *end)) else {
                return Err(fail("The range is not on character boundaries.".into()));
            };
            let node = analysis
                .nodes()
                .into_iter()
                .find(|node| {
                    node.mark.start == start
                        && node.end == end
                        && !matches!(node.kind, NodeKind::Object(_))
                })
                .ok_or_else(|| fail("The range is not exactly one expression.".into()))?;
            parse_expression(sources, module, expression)?;
            Ok(vec![(node.mark.start, node.end, expression.trim().into())])
        }
        EditOp::SetParameterDefault { name, value } => {
            let (_, node) = analysis
                .constant(name)
                .ok_or_else(|| fail(format!("No parameter named '{name}'.")))?;
            let info = parameter(analysis, name, Span { start: 0, end: 0 }, node)
                .ok_or_else(|| fail(format!("'{name}' is not a parameter.")))?;
            let text = match info.kind {
                Type::Scalar => number(
                    sources,
                    module,
                    value
                        .as_f64()
                        .ok_or_else(|| fail("A scalar default is a number.".into()))?,
                )?,
                Type::Bool => value
                    .as_bool()
                    .ok_or_else(|| fail("A bool default is true or false.".into()))?
                    .to_string(),
                kind => vector(sources, module, kind, value)?,
            };
            let start = byte_at(analysis.text, info.default.start).unwrap();
            let end = byte_at(analysis.text, info.default.end).unwrap();
            Ok(vec![(start, end, text)])
        }
        EditOp::SetParameterRange { name, range } => {
            let (_, node) = analysis
                .constant(name)
                .ok_or_else(|| fail(format!("No parameter named '{name}'.")))?;
            let args = is_call_to_param(node)
                .ok_or_else(|| fail(format!("'{name}' is not a parameter.")))?;
            let Node {
                kind: NodeKind::Object(properties),
                ..
            } = args
                .first()
                .ok_or_else(|| fail("A parameter needs options.".into()))?
            else {
                return Err(fail("A parameter takes an options object.".into()));
            };
            let existing = Analysis::property(properties, "range");
            match (existing, range) {
                (Some(property), Some([lo, hi])) => Ok(vec![(
                    property.value.mark.start,
                    property.value.end,
                    format!(
                        "[{}, {}]",
                        number(sources, module, *lo)?,
                        number(sources, module, *hi)?
                    ),
                )]),
                (None, Some([lo, hi])) => {
                    let default = Analysis::property(properties, "default")
                        .ok_or_else(|| fail("A parameter needs a default.".into()))?;
                    Ok(vec![(
                        default.value.end,
                        default.value.end,
                        format!(
                            ", range: [{}, {}]",
                            number(sources, module, *lo)?,
                            number(sources, module, *hi)?
                        ),
                    )])
                }
                (Some(property), None) => Ok(vec![remove_property(text, properties, property)]),
                (None, None) => Ok(Vec::new()),
            }
        }
        EditOp::AddLayer { source, index } => {
            parse_expression(sources, module, source)?;
            let (options, properties) = analysis
                .material()
                .ok_or_else(|| fail("The module has no defineMaterial call.".into()))?;
            let source = source.trim();
            match Analysis::property(properties, "layers") {
                Some(Property {
                    value:
                        array @ Node {
                            kind: NodeKind::Array(items),
                            ..
                        },
                    ..
                }) => {
                    let index = index.unwrap_or(items.len());
                    if index > items.len() {
                        return Err(fail(format!(
                            "Layer index {index} is past the end ({}).",
                            items.len()
                        )));
                    }
                    let close = array.end - 1;
                    Ok(vec![if items.is_empty() {
                        (close, close, source.into())
                    } else if index == items.len() {
                        (
                            items[index - 1].end,
                            items[index - 1].end,
                            format!(", {source}"),
                        )
                    } else {
                        (
                            items[index].mark.start,
                            items[index].mark.start,
                            format!("{source}, "),
                        )
                    }])
                }
                Some(_) => Err(fail("layers is not an array literal.".into())),
                None => {
                    let close = options.end - 1;
                    let before = text[..close].trim_end();
                    let comma = if before.ends_with(',') { "" } else { "," };
                    Ok(vec![(
                        before.len(),
                        before.len(),
                        format!("{comma} layers: [{source}]"),
                    )])
                }
            }
        }
        EditOp::RemoveLayer { id } => {
            let (_, properties) = analysis
                .material()
                .ok_or_else(|| fail("The module has no defineMaterial call.".into()))?;
            let Some(Property {
                value:
                    array @ Node {
                        kind: NodeKind::Array(items),
                        ..
                    },
                ..
            }) = Analysis::property(properties, "layers")
            else {
                return Err(fail("The recipe has no layers array.".into()));
            };
            let layers = analysis.layer_items();
            let selected = layers
                .iter()
                .find(|(_, _, node)| layer_id(node).is_some_and(|(found, _)| found == *id))
                .ok_or_else(|| fail(format!("No layer with id '{id}'.")))?;
            let position = items
                .iter()
                .position(|item| std::ptr::eq(item, selected.0))
                .unwrap();
            let (start, end) = if items.len() == 1 {
                (items[0].mark.start, array.end - 1)
            } else if position + 1 < items.len() {
                (items[position].mark.start, items[position + 1].mark.start)
            } else {
                (items[position - 1].end, items[position].end)
            };
            let mut patches = vec![(start, end, String::new())];
            // A layer declared separately leaves its constant behind; remove it as well when
            // nothing else uses it.
            if let (_, Some(name), _) = selected {
                if analysis.references(name).len() == 1 {
                    if let Some(range) = declaration_range(analysis, name) {
                        patches.push((range.0, range.1, String::new()));
                    }
                }
            }
            Ok(patches)
        }
        EditOp::MoveLayer { id, to } => {
            let (_, properties) = analysis
                .material()
                .ok_or_else(|| fail("The module has no defineMaterial call.".into()))?;
            let Some(Property {
                value:
                    Node {
                        kind: NodeKind::Array(items),
                        ..
                    },
                ..
            }) = Analysis::property(properties, "layers")
            else {
                return Err(fail("The recipe has no layers array.".into()));
            };
            let layers = analysis.layer_items();
            let selected = layers
                .iter()
                .find(|(_, _, node)| layer_id(node).is_some_and(|(found, _)| found == *id))
                .ok_or_else(|| fail(format!("No layer with id '{id}'.")))?;
            let position = items
                .iter()
                .position(|item| std::ptr::eq(item, selected.0))
                .unwrap();
            if *to >= items.len() {
                return Err(fail(format!(
                    "Layer index {to} is past the end ({}).",
                    items.len()
                )));
            }
            if position == *to {
                return Ok(Vec::new());
            }
            // Items swap places; the separators between them (commas, comments, layout) stay.
            let mut order: Vec<usize> = (0..items.len()).collect();
            let moved = order.remove(position);
            order.insert(*to, moved);
            let mut inner = String::new();
            for (slot, &source) in order.iter().enumerate() {
                inner.push_str(&text[items[source].mark.start..items[source].end]);
                if slot + 1 < items.len() {
                    inner.push_str(&text[items[slot].end..items[slot + 1].mark.start]);
                }
            }
            Ok(vec![(
                items[0].mark.start,
                items[items.len() - 1].end,
                inner,
            )])
        }
    }
}

fn is_call_to_param(node: &Node) -> Option<&Vec<Node>> {
    let NodeKind::Call(callee, args) = &node.kind else {
        return None;
    };
    let NodeKind::Member(base, _) = &callee.kind else {
        return None;
    };
    matches!(&base.kind, NodeKind::Name(n) if n == "param").then_some(args)
}

fn remove_property(text: &str, properties: &[Property], target: &Property) -> Replacement {
    let position = properties
        .iter()
        .position(|p| std::ptr::eq(p, target))
        .unwrap();
    if position + 1 < properties.len() {
        (
            target.mark.start,
            properties[position + 1].mark.start,
            String::new(),
        )
    } else if position > 0 {
        (
            properties[position - 1].value.end,
            target.value.end,
            String::new(),
        )
    } else {
        (
            target.mark.start,
            target.value.end.max(text.len().min(target.value.end)),
            String::new(),
        )
    }
}

/// Remove only the declaration's syntax; neighboring statements and trailing comments stay.
fn declaration_range(analysis: &Analysis, name: &str) -> Option<(usize, usize)> {
    let (mark, node) = analysis.constant(name)?;
    let tokens = tokenize(analysis.sources, analysis.module, analysis.text).ok()?;
    let identifier = tokens
        .iter()
        .position(|token| token.mark.start == mark.start)?;
    let declaration = identifier.checked_sub(1)?;
    if !matches!(&tokens[declaration].token, Token::Name(keyword) if keyword == "const") {
        return None;
    }
    let start = match declaration.checked_sub(1) {
        Some(previous) if matches!(&tokens[previous].token, Token::Name(keyword) if keyword == "export") => {
            tokens[previous].mark.start
        }
        _ => tokens[declaration].mark.start,
    };
    let terminator = tokens.iter().find(|token| token.mark.start >= node.end)?;
    matches!(terminator.token, Token::Symbol(";")).then_some((start, terminator.mark.end))
}

pub fn apply(sources: &Sources, module: &str, op: &EditOp) -> Result<Edited, EditFailure> {
    let analysis = Analysis::new(sources, module)?;
    let mut replacements = replacements(&analysis, op)?;
    replacements.sort_by_key(|replacement| replacement.0);
    if replacements.windows(2).any(|pair| pair[0].1 > pair[1].0) {
        return Err(failure(sources, module, "The edit's patches overlap."));
    }
    let text = analysis.text;
    let patches: Vec<Patch> = replacements
        .iter()
        .map(|(start, end, replacement)| Patch {
            start: utf16(text, *start),
            end: utf16(text, *end),
            text: replacement.clone(),
        })
        .collect();
    let mut edited = text.to_owned();
    for (start, end, replacement) in replacements.iter().rev() {
        edited.replace_range(*start..*end, replacement);
    }
    let mut next = sources.clone();
    next.modules.insert(module.into(), edited.clone());
    compile(&next).map_err(|diagnostics| EditFailure { diagnostics })?;
    Ok(Edited {
        patches,
        source: edited,
    })
}

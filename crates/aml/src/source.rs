use crate::diagnostic::{Diagnostic, SourceLocation};
use roxmltree::{Document, Node, ParsingOptions};
use serde::Serialize;
use std::collections::BTreeMap;

pub const MAX_SOURCE_UNITS: usize = 10_000_000;
/// A valid source within the UTF-16 limit cannot exceed this UTF-8 byte bound.
pub const MAX_SOURCE_BYTES: u64 = (MAX_SOURCE_UNITS * 4) as u64;

#[derive(Clone, Copy, Debug)]
pub struct SourceLimits {
    pub utf16_units: usize,
    pub depth: usize,
    /// Native admission limit; includes elements, text and comments, plus the document root.
    pub nodes: u32,
}

impl Default for SourceLimits {
    fn default() -> Self {
        Self {
            utf16_units: MAX_SOURCE_UNITS,
            depth: 128,
            nodes: 1_000_000,
        }
    }
}

/// Sparse UTF-8/UTF-16 translation avoids rescanning long lines for every source object.
struct SourceIndex {
    lines: Vec<usize>,
    excess: Vec<(usize, usize)>,
    units: usize,
}

impl SourceIndex {
    fn new(text: &str) -> Self {
        let mut result = Self {
            lines: vec![0],
            excess: Vec::new(),
            units: 0,
        };
        let mut excess = 0;
        for (byte, character) in text.char_indices() {
            result.units += character.len_utf16();
            if character == '\n' {
                result.lines.push(byte + 1);
            }
            let difference = character.len_utf8() - character.len_utf16();
            if difference != 0 {
                excess += difference;
                result.excess.push((byte + character.len_utf8(), excess));
            }
        }
        result
    }

    fn units_at(&self, byte: usize) -> usize {
        let count = self.excess.partition_point(|(end, _)| *end <= byte);
        byte - count.checked_sub(1).map_or(0, |index| self.excess[index].1)
    }

    fn location(&self, byte: usize, end: Option<usize>) -> SourceLocation {
        let line = self.lines.partition_point(|start| *start <= byte) - 1;
        let offset = self.units_at(byte);
        SourceLocation {
            line: line + 1,
            column: offset - self.units_at(self.lines[line]) + 1,
            offset,
            end: end.map(|byte| self.units_at(byte)),
        }
    }
}

/// Parsed XML borrows its immutable source. No filesystem or external entity reads occur.
pub struct AmlSource<'input> {
    xml: Document<'input>,
    index: SourceIndex,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceInspection {
    pub id: String,
    pub name: String,
    pub syntax_valid: bool,
    pub semantic_validation: &'static str,
    pub geometry_validation: &'static str,
    pub navigation_validation: &'static str,
    pub element_count: usize,
    pub maximum_depth: usize,
    pub tags: BTreeMap<String, usize>,
    pub source: SourceLocation,
}

fn error(
    code: &'static str,
    message: impl Into<String>,
    source: SourceLocation,
    object: Option<&str>,
) -> Diagnostic {
    Diagnostic {
        spatial: None,
        severity: "error",
        code,
        message: message.into().into_boxed_str(),
        objects: object.into_iter().map(str::to_owned).collect(),
        source,
    }
}

/// Admits XML and the root identity only; children remain unvalidated AML syntax.
pub fn parse_source(text: &str, limits: SourceLimits) -> Result<AmlSource<'_>, Diagnostic> {
    let index = SourceIndex::new(text);
    if limits.utf16_units == 0 || limits.depth == 0 || limits.nodes == 0 {
        return Err(error(
            "AML_LIMIT",
            "Source limits must be positive.",
            index.location(0, None),
            None,
        ));
    }
    if index.units > limits.utf16_units {
        return Err(error(
            "XML_SYNTAX",
            format!(
                "AML source exceeds {} UTF-16 code units.",
                limits.utf16_units
            ),
            index.location(0, None),
            None,
        ));
    }
    let xml = Document::parse_with_options(
        text,
        ParsingOptions {
            allow_dtd: false,
            nodes_limit: limits.nodes,
            ..ParsingOptions::default()
        },
    )
    .map_err(|failure| {
        let position = failure.pos();
        let line = position.row.saturating_sub(1) as usize;
        let start = index.lines.get(line).copied().unwrap_or(text.len());
        let byte = text[start..]
            .char_indices()
            .nth(position.col.saturating_sub(1) as usize)
            .map_or(text.len(), |(offset, _)| start + offset);
        error(
            "XML_SYNTAX",
            failure.to_string(),
            index.location(byte, None),
            None,
        )
    })?;
    let result = AmlSource { xml, index };
    let root = result.xml.root_element();
    if root.tag_name().name() != "architecture" || root.tag_name().namespace().is_some() {
        return Err(result.failure(root, "AML_ROOT", "Expected <architecture version=\"1\">."));
    }
    if root.attribute("version") != Some("1") {
        return Err(result.failure(root, "AML_VERSION", "Expected AML version 1."));
    }
    let id = root.attribute("id").unwrap_or("");
    if !valid_id(id) {
        return Err(result.failure(root, "AML_ATTRIBUTE", "An id must start with an ASCII letter or underscore and use letters, digits, underscore, dot, colon, slash or hyphen."));
    }
    for node in result.xml.descendants() {
        match node {
            node if node.is_element() => {
                let depth = node
                    .ancestors()
                    .filter(|ancestor| ancestor.is_element())
                    .count();
                if depth > limits.depth {
                    return Err(result.failure(
                        node,
                        "XML_SYNTAX",
                        format!("XML nesting exceeds {} elements.", limits.depth),
                    ));
                }
                if node.namespaces().len() != 0
                    || node.tag_name().namespace().is_some()
                    || node
                        .attributes()
                        .any(|attribute| attribute.namespace().is_some())
                {
                    return Err(result.failure(
                        node,
                        "XML_SYNTAX",
                        "AML does not use XML namespaces.",
                    ));
                }
            }
            node if node.is_text() && !node.text().unwrap_or("").trim().is_empty() => {
                return Err(result.failure(
                    node,
                    "XML_SYNTAX",
                    "AML values use attributes, not element text or CDATA.",
                ));
            }
            _ => {}
        }
    }
    Ok(result)
}

pub fn valid_id(value: &str) -> bool {
    let mut bytes = value.bytes();
    let Some(first) = bytes.next() else {
        return false;
    };
    (first.is_ascii_alphabetic() || first == b'_')
        && bytes.all(|byte| byte.is_ascii_alphanumeric() || b"_.:/-".contains(&byte))
}

impl AmlSource<'_> {
    /// Read-only XML for the native semantic reader; source ranges remain available.
    pub fn root(&self) -> Node<'_, '_> {
        self.xml.root_element()
    }

    pub fn location(&self, node: Node<'_, '_>) -> SourceLocation {
        let range = node.range();
        self.index.location(range.start, Some(range.end))
    }

    pub fn failure(
        &self,
        node: Node<'_, '_>,
        code: &'static str,
        message: impl Into<String>,
    ) -> Diagnostic {
        error(code, message, self.location(node), node.attribute("id"))
    }

    pub fn inspect(&self) -> SourceInspection {
        let root = self.root();
        let mut tags = BTreeMap::new();
        let mut maximum_depth = 0;
        let mut element_count = 0;
        for node in root.descendants() {
            match node {
                node if node.is_element() => {
                    let depth = node
                        .ancestors()
                        .filter(|ancestor| ancestor.is_element())
                        .count();
                    maximum_depth = maximum_depth.max(depth);
                    element_count += 1;
                    *tags.entry(node.tag_name().name().to_owned()).or_insert(0) += 1;
                }
                _ => {}
            }
        }
        let id = root
            .attribute("id")
            .expect("validated root identity")
            .to_owned();
        SourceInspection {
            name: root.attribute("name").unwrap_or(&id).to_owned(),
            id,
            syntax_valid: true,
            semantic_validation: "not-run",
            geometry_validation: "not-run",
            navigation_validation: "not-run",
            element_count,
            maximum_depth,
            tags,
            source: self.location(root),
        }
    }
}

use serde::Serialize;
use std::fmt;

/// Source coordinates retain the AML editor's UTF-16 convention, not UTF-8 bytes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct SourceLocation {
    pub line: usize,
    pub column: usize,
    pub offset: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end: Option<usize>,
}

/// Source-linked findings carry optional spatial evidence for direct inspection.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Diagnostic {
    pub severity: &'static str,
    pub code: &'static str,
    pub message: Box<str>,
    pub objects: Vec<String>,
    pub source: SourceLocation,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spatial: Option<Box<SpatialEvidence>>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SpatialEvidence {
    pub bounds: [[f64; 3]; 2],
    pub inspection_target: String,
    pub evidence: String,
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{}:{} {}",
            self.source.line, self.source.column, self.message
        )
    }
}

impl std::error::Error for Diagnostic {}

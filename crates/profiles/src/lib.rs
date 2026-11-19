//! Shipped AML profiles selected by product hosts, separate from generic source admission.
pub mod city;
pub mod kowloon;

use std::sync::OnceLock;
use superspec_aml::{architecture::Document, diagnostic::Diagnostic, extensions::Registry};

/// The native and browser products support the existing Kowloon source vocabulary.
pub fn default_registry() -> &'static Registry {
    static REGISTRY: OnceLock<Registry> = OnceLock::new();
    REGISTRY.get_or_init(|| {
        Registry::new(&[&kowloon::EXTENSION, &city::EXTENSION]).expect("shipped AML profile")
    })
}

/// Admits a source with the same shipped profile used by compilation and asset discovery.
pub fn parse(source: &str) -> Result<Document, Diagnostic> {
    superspec_aml::architecture::parse_with_registry(source, default_registry())
}

/// Round-trips a document through its shipped vocabulary before publishing source text.
pub fn serialize(document: &Document) -> Result<String, String> {
    superspec_aml::format::serialize_with_registry(document, default_registry())
}

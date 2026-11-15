//! Compiled-in source grammar and checks. Hosts choose the available profile.
use crate::{
    architecture::{Document, Node, core_child},
    diagnostic::Diagnostic,
    source::AmlSource,
};
use std::collections::BTreeSet;

pub type Element<'a, 'input> = roxmltree::Node<'a, 'input>;

#[derive(Clone, Copy)]
pub struct Selector {
    pub attribute: &'static str,
    /// Compatibility syntax may omit its extension identity.
    pub default: Option<&'static str>,
}

#[derive(Clone, Copy)]
pub struct Tag {
    pub name: &'static str,
    pub parent: &'static str,
    pub selector: Option<Selector>,
}

#[derive(Clone, Copy)]
pub struct Check {
    pub id: &'static str,
    /// Runs after the complete authored tree and its ID scopes are admitted.
    /// No physical, navigation, or external-asset validation is implied.
    pub run: fn(&Document) -> Result<(), Diagnostic>,
}

#[derive(Clone, Copy)]
pub struct Extension {
    pub id: &'static str,
    pub version: u32,
    pub aml_versions: &'static [u32],
    pub tags: &'static [Tag],
    /// Returns the existing serializable Node representation for the owned subtree.
    pub decode: fn(Element<'_, '_>, &AmlSource<'_>) -> Result<Node, Diagnostic>,
    pub checks: &'static [Check],
}

#[derive(Clone, Default)]
pub struct Registry {
    extensions: Vec<&'static Extension>,
}

impl Registry {
    /// Rejects ambiguous host configuration before any source is read.
    pub fn new(extensions: &[&'static Extension]) -> Result<Self, String> {
        let mut identities = BTreeSet::new();
        let mut tags = BTreeSet::new();
        let mut checks = BTreeSet::new();
        for extension in extensions {
            if !crate::source::valid_id(extension.id)
                || extension.version == 0
                || extension.aml_versions.is_empty()
                || extension.aml_versions.contains(&0)
            {
                return Err("Extension identity and AML versions must be valid.".into());
            }
            if !identities.insert(extension.id) {
                return Err(format!(
                    "Duplicate extension {}@{}.",
                    extension.id, extension.version
                ));
            }
            for tag in extension.tags {
                let valid = |name: &str| {
                    !name.is_empty() && name.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-')
                };
                if !valid(tag.name) || !valid(tag.parent) {
                    return Err("Extension tags and parent scopes must be AML names.".into());
                }
                if tag.name == "architecture" || core_child(tag.parent, tag.name) {
                    return Err(format!(
                        "Extension cannot replace core <{}> in <{}>.",
                        tag.name, tag.parent
                    ));
                }
                if !tags.insert((tag.parent, tag.name)) {
                    return Err(format!(
                        "Conflicting extension ownership of <{}> in <{}>.",
                        tag.name, tag.parent
                    ));
                }
                if let Some(selector) = tag.selector {
                    let identity = format!("{}@{}", extension.id, extension.version);
                    if !valid(selector.attribute)
                        || selector.default.is_some_and(|default| default != identity)
                    {
                        return Err(format!("Invalid identity selector for {identity}."));
                    }
                }
            }
            for check in extension.checks {
                if check.id.is_empty() || !checks.insert(check.id) {
                    return Err(format!("Duplicate or empty extension check {}.", check.id));
                }
            }
        }
        Ok(Self {
            extensions: extensions.to_vec(),
        })
    }

    /// Creates an isolated profile, retaining the existing registration order.
    pub fn with_extensions(&self, extensions: &[&'static Extension]) -> Result<Self, String> {
        let combined: Vec<_> = self.extensions.iter().chain(extensions).copied().collect();
        Self::new(&combined)
    }

    fn owner(&self, parent: &str, name: &str) -> Option<(&Extension, &Tag)> {
        self.extensions.iter().find_map(|extension| {
            extension
                .tags
                .iter()
                .find(|tag| tag.parent == parent && tag.name == name)
                .map(|tag| (*extension, tag))
        })
    }

    pub(crate) fn accepts(&self, parent: &str, name: &str) -> bool {
        self.owner(parent, name).is_some()
    }

    pub(crate) fn decode(
        &self,
        element: Element<'_, '_>,
        source: &AmlSource<'_>,
    ) -> Option<Result<Node, Diagnostic>> {
        let parent = element.parent_element()?.tag_name().name();
        let (extension, _) = self.owner(parent, element.tag_name().name())?;
        Some((|| {
            if !extension.aml_versions.contains(&1) {
                return Err(source.failure(
                    element,
                    "AML_EXTENSION_VERSION",
                    format!(
                        "{}@{} does not support AML version 1.",
                        extension.id, extension.version
                    ),
                ));
            }
            for child in element.descendants().filter(Element::is_element) {
                let parent = child
                    .parent_element()
                    .expect("subtree parent")
                    .tag_name()
                    .name();
                let Some(tag) = extension
                    .tags
                    .iter()
                    .find(|tag| tag.parent == parent && tag.name == child.tag_name().name())
                else {
                    return Err(source.failure(
                        child,
                        "AML_CHILD",
                        format!(
                            "<{}> is not owned by {}@{} in <{parent}>.",
                            child.tag_name().name(),
                            extension.id,
                            extension.version
                        ),
                    ));
                };
                if let Some(selector) = tag.selector {
                    let identity = format!("{}@{}", extension.id, extension.version);
                    if child.attribute(selector.attribute).or(selector.default)
                        != Some(identity.as_str())
                    {
                        return Err(source.failure(
                            child,
                            "AML_EXTENSION_VERSION",
                            format!(
                                "<{}> requires {}=\"{identity}\".",
                                tag.name, selector.attribute
                            ),
                        ));
                    }
                }
            }
            (extension.decode)(element, source)
        })())
    }

    pub(crate) fn check(&self, document: &Document) -> Result<(), Diagnostic> {
        for extension in &self.extensions {
            for check in extension.checks {
                (check.run)(document)?;
            }
        }
        Ok(())
    }
}

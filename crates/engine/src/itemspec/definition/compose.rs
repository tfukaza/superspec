//! Composition: sub-items (another item definition placed as a part), native generators
//! supplied by the host, and the arguments, templates and copy indices they read.
use super::{
    AssetReader, Attachment, Derived, ItemDefinition, ItemDiagnostic, ItemMesh, PreparedItem,
    Values, fail, parse,
    schema::{Arguments, Entry, ItemReference, NativeReference, Scalar},
};
use serde_json::Value;
use std::{fmt, sync::Arc};

/// What a native generator returns: meshes in its own coordinates, each with a material
/// slot, and any attachments it offers.
#[derive(Clone, Debug, Default)]
pub struct NativeOutput {
    pub meshes: Vec<ItemMesh>,
    pub attachments: Vec<Attachment>,
}

/// Generators supplied by the host, such as a theme's fixtures. They must be deterministic:
/// the same name, identity and parameters always give the same meshes.
pub trait NativeGenerators: Send + Sync {
    /// The implementation identity of `name`, which changes whenever its output would;
    /// `None` when the generator is unknown.
    fn identity(&self, name: &str) -> Option<String>;
    /// The names this host offers, for diagnostics and editors.
    fn names(&self) -> Vec<String>;
    fn generate(
        &self,
        name: &str,
        parameters: &Values,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<NativeOutput, String>;
}

/// Everything outside a definition that preparing it may read: asset bytes (GLB files and
/// sub-item definitions, by the same paths) and the host's native generators.
pub struct Environment<'a> {
    pub assets: &'a AssetReader<'a>,
    pub natives: Option<Arc<dyn NativeGenerators>>,
    /// Entries being prepared, outermost first, to refuse cycles and deep nesting.
    pub(super) stack: Vec<String>,
}
impl<'a> Environment<'a> {
    pub fn new(assets: &'a AssetReader<'a>) -> Self {
        Self {
            assets,
            natives: None,
            stack: vec![],
        }
    }
    pub fn with_natives(mut self, natives: Arc<dyn NativeGenerators>) -> Self {
        self.natives = Some(natives);
        self
    }
}

/// Sub-items nest at most this deep.
pub const MAXIMUM_DEPTH: usize = 8;

/// A shared value carried inside serializable part shapes but kept out of their hashes.
pub(super) struct Opaque<T: ?Sized>(pub Arc<T>);
impl<T: ?Sized> Clone for Opaque<T> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}
impl<T: ?Sized> fmt::Debug for Opaque<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("..")
    }
}
impl<T: ?Sized> serde::Serialize for Opaque<T> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_unit()
    }
}

/// Replaces `{name}` with the integer value of a derived value or copy index.
pub(super) fn template(
    definition: &ItemDefinition,
    text: &str,
    derived: &Derived,
    pointer: &str,
) -> Result<String, ItemDiagnostic> {
    let mut result = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(open) = rest.find('{') {
        result.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        let close = after.find('}').ok_or_else(|| {
            fail(
                &definition.source,
                pointer,
                "ITEM_TEMPLATE",
                "An opening brace has no closing brace.",
            )
        })?;
        let name = &after[..close];
        let value = derived.get(name).ok_or_else(|| {
            fail(
                &definition.source,
                pointer,
                "ITEM_TEMPLATE",
                format!("Unknown template name {name}."),
            )
        })?;
        result.push_str(&format!("{}", value.round() as i64));
        rest = &after[close + 1..];
    }
    result.push_str(rest);
    Ok(result)
}

/// Resolves the arguments of a sub-item or native generator against the parent's controls.
pub(super) fn arguments(
    definition: &ItemDefinition,
    arguments: &Arguments,
    values: &Values,
    derived: &Derived,
    pointer: &str,
) -> Result<Values, ItemDiagnostic> {
    if arguments.len() > 64 {
        return Err(fail(
            &definition.source,
            pointer,
            "ITEM_LIMIT",
            "A part passes at most 64 parameters.",
        ));
    }
    let mut result = Values::new();
    for (name, value) in arguments {
        let pointer = format!("{pointer}/{}", super::pointer_key(name));
        // `{"choose": [values...], "by": expression}` picks one value by the floored,
        // wrapped expression: how a copy chooses a text or choice control.
        if let Some(object) = value.as_object().filter(|object| {
            object.len() == 2 && object.contains_key("choose") && object.contains_key("by")
        }) {
            let options = object["choose"]
                .as_array()
                .filter(|o| !o.is_empty() && o.len() <= 64);
            let by = object["by"].as_str();
            let (Some(options), Some(by)) = (options, by) else {
                return Err(fail(
                    &definition.source,
                    &pointer,
                    "ITEM_PARAMETER",
                    "A choice lists from 1 to 64 values and picks them by an expression.",
                ));
            };
            let pick = definition.expression(by, values, derived, &pointer)?;
            let index = (pick.floor() as i64).rem_euclid(options.len() as i64) as usize;
            result.insert(name.clone(), options[index].clone());
            continue;
        }
        let single = value.as_object().filter(|object| object.len() == 1);
        let resolved = match single.and_then(|object| object.iter().next()) {
            Some((key, Value::String(reference))) if key == "parameter" => definition
                .parameter(reference, values, &pointer, false)?
                .clone(),
            Some((key, Value::String(reference))) if key == "derived" => {
                serde_json::json!(definition.scalar(
                    &Scalar::Derived(super::DerivedReference {
                        derived: reference.clone()
                    }),
                    values,
                    derived,
                    &pointer
                )?)
            }
            Some((key, Value::String(text))) if key == "expr" => {
                let number = definition.expression(text, values, derived, &pointer)?;
                serde_json::json!(number)
            }
            _ => value.clone(),
        };
        result.insert(name.clone(), resolved);
    }
    Ok(result)
}

/// The entry a sub-item part names for these copy indices.
pub(super) fn entry(
    definition: &ItemDefinition,
    reference: &ItemReference,
    values: &Values,
    derived: &Derived,
    pointer: &str,
) -> Result<String, ItemDiagnostic> {
    let text = match &reference.entry {
        Entry::One(entry) => entry,
        Entry::Choice(entries) => {
            if entries.is_empty() || entries.len() > 64 {
                return Err(fail(
                    &definition.source,
                    &format!("{pointer}/entry"),
                    "ITEM_LIMIT",
                    "A choice lists from 1 to 64 entries.",
                ));
            }
            let pick = match &reference.pick {
                Some(pick) => {
                    definition.scalar(pick, values, derived, &format!("{pointer}/pick"))?
                }
                None => 0.,
            };
            let index = (pick.floor() as i64).rem_euclid(entries.len() as i64) as usize;
            &entries[index]
        }
    };
    let entry = template(definition, text, derived, &format!("{pointer}/entry"))?;
    if entry.is_empty() || entry.len() > 4096 {
        return Err(fail(
            &definition.source,
            &format!("{pointer}/entry"),
            "ITEM_ASSET",
            "Invalid sub-item entry.",
        ));
    }
    Ok(entry)
}

/// Reads, parses and prepares a sub-item with its arguments, refusing cycles and nesting
/// deeper than `MAXIMUM_DEPTH`.
pub(super) fn sub_item(
    definition: &ItemDefinition,
    entry: &str,
    parameters: &Values,
    environment: &mut Environment<'_>,
    cancelled: &dyn Fn() -> bool,
    pointer: &str,
) -> Result<PreparedItem, ItemDiagnostic> {
    let failure = |message: String| fail(&definition.source, pointer, "ITEM_SUB_ITEM", message);
    if environment.stack.iter().any(|open| open == entry) {
        return Err(failure(format!("{entry} contains itself.")));
    }
    if environment.stack.len() >= MAXIMUM_DEPTH {
        return Err(failure(format!(
            "Sub-items nest at most {MAXIMUM_DEPTH} deep."
        )));
    }
    let bytes =
        (environment.assets)(entry).map_err(|error| failure(format!("{entry}: {error}")))?;
    let text =
        std::str::from_utf8(&bytes).map_err(|_| failure(format!("{entry} is not UTF-8 text.")))?;
    let child = parse(entry, text)?;
    environment.stack.push(entry.to_owned());
    let prepared = super::prepare_in(&child, parameters, environment, cancelled);
    environment.stack.pop();
    prepared
}

/// A native part: its generator's identity and resolved parameters, generated at build time.
#[derive(Clone, Debug, serde::Serialize)]
pub(super) struct Native {
    pub name: String,
    pub identity: String,
    pub parameters: Values,
    #[serde(skip_serializing)]
    pub generators: Opaque<dyn NativeGenerators>,
}

pub(super) fn native(
    definition: &ItemDefinition,
    reference: &NativeReference,
    values: &Values,
    derived: &Derived,
    environment: &Environment<'_>,
    pointer: &str,
) -> Result<Native, ItemDiagnostic> {
    let generators = environment.natives.clone().ok_or_else(|| {
        fail(
            &definition.source,
            pointer,
            "ITEM_NATIVE",
            format!(
                "{} needs native generators, which this host does not supply.",
                reference.name
            ),
        )
    })?;
    let identity = generators.identity(&reference.name).ok_or_else(|| {
        let mut names = generators.names();
        names.sort();
        fail(
            &definition.source,
            &format!("{pointer}/name"),
            "ITEM_NATIVE",
            format!(
                "Unknown native generator {}; this host offers {}.",
                reference.name,
                names.join(", ")
            ),
        )
    })?;
    let parameters = arguments(
        definition,
        &reference.parameters,
        values,
        derived,
        &format!("{pointer}/parameters"),
    )?;
    Ok(Native {
        name: reference.name.clone(),
        identity,
        parameters,
        generators: Opaque(generators),
    })
}

//! Composition: sub-items (another item definition placed as a part), native generators
//! supplied by the host, and the arguments, templates and copy indices they read.
use super::{
    AssetReader, Attachment, Derived, ItemDefinition, ItemDiagnostic, ItemMesh, Mount,
    PreparedItem, Prototype, Values, fail, parse,
    schema::{Arguments, Entry, ItemReference, NativeReference, PrototypeReference, Scalar},
};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    fmt,
    sync::{Arc, Mutex},
};

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
    /// Retain the original smoothing islands for slots that derive tangent
    /// frames from indexed triangles at delivery time.
    fn preserve_signed_zero(&self, _material_slot: &str) -> bool {
        true
    }

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
    /// The features `name` reports for `parameters`, in the coordinates of its meshes: edges
    /// such as a wall's `top`, `sill` and `base` that a part's zone rules hang zones below, as
    /// a panel's. Their normals are surface normals. As deterministic as the meshes, and cheap:
    /// they are asked for whenever the part's features are, without drawing anything. Their
    /// `part` is replaced by the part's ID. None by default.
    fn features(&self, _name: &str, _parameters: &Values) -> Vec<super::Feature> {
        Vec::new()
    }
    /// What `name` draws for `parameters`, described as a shared shape and this instance's
    /// own inputs; `None` (the default) where the output is the instance's alone. As cheap
    /// and as deterministic as [`Self::features`]: it draws nothing. Instances that return
    /// equal `content` must differ only by the colours named in `inputs`.
    fn prototype(&self, _name: &str, _parameters: &Values) -> Option<NativePrototype> {
        None
    }
    /// The shape of the prototype `content` describes, in the coordinates and slots
    /// [`Self::generate`] uses, with every [`ItemMesh::roles`] filled and neutral vertex
    /// colours: a vertex of role `k` carries its shade of the `k - 1`th input in name order,
    /// so that colour times that input is what `generate` draws for an instance. Asked for
    /// once per prototype of one prepared item, however many parts place it. The default
    /// serves a generator without inputs whose content is its parameters.
    fn prototype_meshes(
        &self,
        name: &str,
        content: &Values,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<NativeOutput, String> {
        self.generate(name, content, cancelled)
    }
}

/// How a native generator describes one instance as a placed prototype.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NativePrototype {
    /// Everything that shapes the output, canonically: no seed, no part and no placement.
    pub content: Values,
    /// The instance's own colours by role name, in linear RGB.
    pub inputs: BTreeMap<String, [f64; 3]>,
    pub mount: Mount,
}

/// The prototypes drawn so far for one prepared item and its sub-items, by key.
pub(super) type PrototypeCache = Mutex<BTreeMap<String, Arc<Prototype>>>;

/// Everything outside a definition that preparing it may read: asset bytes (GLB files and
/// sub-item definitions, by the same paths) and the host's native generators.
pub struct Environment<'a> {
    pub assets: &'a AssetReader<'a>,
    pub natives: Option<Arc<dyn NativeGenerators>>,
    /// Entries being prepared, outermost first, to refuse cycles and deep nesting.
    pub(super) stack: Vec<String>,
    /// Shared by every native part prepared here, so a prototype is drawn once.
    pub(super) prototypes: Arc<PrototypeCache>,
}
impl<'a> Environment<'a> {
    pub fn new(assets: &'a AssetReader<'a>) -> Self {
        Self {
            assets,
            natives: None,
            stack: vec![],
            prototypes: Arc::default(),
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
    if arguments.len() > 128 {
        return Err(fail(
            &definition.source,
            pointer,
            "ITEM_LIMIT",
            "A part passes at most 128 parameters.",
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
        // `{"element": grid, "at": expression}` passes one element of a grid control (a row of
        // a plan); none is passed where the grid has no such element, so the sub-item keeps
        // its default.
        if let Some(object) = value.as_object().filter(|object| {
            object.len() == 2 && object.contains_key("element") && object.contains_key("at")
        }) {
            let (Some(grid), Some(at)) = (object["element"].as_str(), object["at"].as_str()) else {
                return Err(fail(
                    &definition.source,
                    &pointer,
                    "ITEM_PARAMETER",
                    "An element names a grid control and an index expression.",
                ));
            };
            let index = definition.expression(at, values, derived, &pointer)?;
            let element = definition
                .parameter(grid, values, &pointer, false)?
                .as_array()
                .and_then(|items| items.get(index.floor().max(0.) as usize))
                .filter(|_| index >= 0.);
            if let Some(element) = element {
                result.insert(name.clone(), element.clone());
            }
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
    /// Set where the part is a prototype drawn as it is (`kind: "prototype"`): `parameters`
    /// is then the prototype's content, not an instance's parameters.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shared: Option<Shared>,
    #[serde(skip_serializing)]
    pub generators: Opaque<dyn NativeGenerators>,
    #[serde(skip_serializing)]
    pub prototypes: Opaque<PrototypeCache>,
}

/// How a `prototype` part draws its prototype.
#[derive(Clone, Copy, Debug, serde::Serialize)]
pub(super) struct Shared {
    /// Reflected in its local X, as [`Prototype::reflected`].
    pub mirrored: bool,
}

impl Native {
    /// The meshes of the prototype this part is (`shared`), as the engine holds them in
    /// [`super::ItemGeometry::prototypes`]: indexed, neutral colours, roles filled.
    pub(super) fn shared_meshes(
        &self,
        shared: Shared,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Vec<ItemMesh>, String> {
        let generators = &self.generators.0;
        let output = generators
            .prototype_meshes(&self.name, &self.parameters, cancelled)
            .map_err(|e| format!("{} prototype: {e}", self.name))?;
        let mut meshes = output.meshes;
        for mesh in &mut meshes {
            if !mesh.roles.is_empty() && mesh.roles.len() != mesh.positions.len() {
                return Err(format!(
                    "{} prototype: a mesh names roles for other vertices than its own.",
                    self.name
                ));
            }
            mesh.index_vertices(generators.preserve_signed_zero(&mesh.material_slot));
            if shared.mirrored {
                mesh.reflect();
            }
        }
        Ok(meshes)
    }

    /// The prototype this part places and its own inputs, where its generator describes one:
    /// drawn on first use and shared after.
    pub(super) fn prototype(
        &self,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Option<(String, Arc<Prototype>, BTreeMap<String, [f64; 3]>)>, String> {
        let generators = &self.generators.0;
        let Some(described) = generators.prototype(&self.name, &self.parameters) else {
            return Ok(None);
        };
        let key = Prototype::key(
            "native",
            &self.name,
            &self.identity,
            &described.content,
            false,
        )?;
        let known = self
            .prototypes
            .0
            .lock()
            .map_err(|_| "Prototype cache poisoned.")?
            .get(&key)
            .cloned();
        let prototype = match known {
            Some(prototype) => prototype,
            None => {
                let roles: Vec<String> = described.inputs.keys().cloned().collect();
                let output = generators
                    .prototype_meshes(&self.name, &described.content, cancelled)
                    .map_err(|e| format!("{} prototype: {e}", self.name))?;
                let mut meshes = output.meshes;
                for mesh in &mut meshes {
                    if !mesh.roles.is_empty() && mesh.roles.len() != mesh.positions.len()
                        || mesh
                            .roles
                            .iter()
                            .any(|&role| usize::from(role) > roles.len())
                    {
                        return Err(format!(
                            "{} prototype: a mesh names a role it has no input for.",
                            self.name
                        ));
                    }
                    mesh.index_vertices(generators.preserve_signed_zero(&mesh.material_slot));
                }
                let prototype = Arc::new(Prototype {
                    kind: "native".into(),
                    name: self.name.clone(),
                    identity: self.identity.clone(),
                    features: generators.features(&self.name, &described.content),
                    content: described.content,
                    mirrored: false,
                    meshes,
                    roles,
                    mount: described.mount,
                });
                self.prototypes
                    .0
                    .lock()
                    .map_err(|_| "Prototype cache poisoned.")?
                    .entry(key.clone())
                    .or_insert(prototype)
                    .clone()
            }
        };
        if !prototype.roles.iter().eq(described.inputs.keys()) {
            return Err(format!(
                "{}: instances of one prototype name different inputs.",
                self.name
            ));
        }
        Ok(Some((key, prototype, described.inputs)))
    }
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
        shared: None,
        generators: Opaque(generators),
        prototypes: Opaque(environment.prototypes.clone()),
    })
}

/// A `prototype` part: the generator's identity and the content it is drawn from, as stated.
pub(super) fn prototype(
    definition: &ItemDefinition,
    reference: &PrototypeReference,
    environment: &Environment<'_>,
    pointer: &str,
) -> Result<Native, ItemDiagnostic> {
    if reference.content.len() > 128 {
        return Err(fail(
            &definition.source,
            &format!("{pointer}/content"),
            "ITEM_LIMIT",
            "A prototype's content has at most 128 values.",
        ));
    }
    let mut native = native(
        definition,
        &NativeReference {
            name: reference.name.clone(),
            parameters: Arguments::new(),
        },
        &Values::new(),
        &Derived::new(),
        environment,
        pointer,
    )?;
    native.parameters = reference.content.clone();
    native.shared = Some(Shared {
        mirrored: reference.mirrored,
    });
    Ok(native)
}

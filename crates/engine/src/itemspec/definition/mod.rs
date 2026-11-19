//! Persistent ItemSpec definitions, local generation and bounded scene-preserving GLB ingestion.
mod arc;
mod compose;
mod decal;
pub mod expression;
mod footprint;
mod glb;
mod location;
mod memory;
mod output;
mod panel;
mod parts;
mod procedural;
mod route;
mod schema;
mod surfaces;
use crate::materialspec::program::MaterialRequest;
pub use compose::{Environment, NativeGenerators, NativeOutput};
pub use location::locate_diagnostic;
pub use output::*;
pub use schema::*;
use serde::Serialize;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};
use superspec_geometry::{Matrix, math::unit};

pub type AssetReader<'a> = dyn Fn(&str) -> Result<Arc<[u8]>, String> + 'a;

#[derive(Clone, Debug, Serialize)]
pub struct ItemDiagnostic {
    pub code: String,
    pub message: String,
    pub source: String,
    pub pointer: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub column: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub offset: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end: Option<usize>,
}
impl std::fmt::Display for ItemDiagnostic {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}{}: {}", self.source, self.pointer, self.message)
    }
}
impl std::error::Error for ItemDiagnostic {}
fn fail(source: &str, pointer: &str, code: &str, message: impl Into<String>) -> ItemDiagnostic {
    ItemDiagnostic {
        code: code.into(),
        message: message.into(),
        source: source.into(),
        pointer: pointer.into(),
        line: None,
        column: None,
        offset: None,
        end: None,
    }
}
fn pointer_key(value: &str) -> String {
    value.replace('~', "~0").replace('/', "~1")
}
fn check_cancelled(source: &str, cancelled: &dyn Fn() -> bool) -> Result<(), ItemDiagnostic> {
    if cancelled() {
        Err(fail(
            source,
            "/",
            "ITEM_CANCELLED",
            "Item generation cancelled.",
        ))
    } else {
        Ok(())
    }
}

pub fn parse(source: &str, text: &str) -> Result<ItemDefinition, ItemDiagnostic> {
    if text.len() > 1024 * 1024 {
        return Err(fail(
            source,
            "/",
            "ITEM_LIMIT",
            "Item definition exceeds 1 MiB.",
        ));
    }
    let mut definition: ItemDefinition = serde_json::from_str(text).map_err(|error| {
        let mut diagnostic = ItemDiagnostic {
            code: "ITEM_JSON".into(),
            message: error.to_string(),
            source: source.into(),
            pointer: "/".into(),
            line: Some(error.line()),
            column: Some(error.column()),
            offset: None,
            end: None,
        };
        locate_diagnostic(text, &mut diagnostic);
        diagnostic
    })?;
    definition.source = source.into();
    definition
        .resolve_parameters(&Values::new())
        .and_then(|values| definition.resolve_derived(&values))
        .map_err(|mut error| {
            locate_diagnostic(text, &mut error);
            error
        })?;
    Ok(definition)
}

enum Shape {
    Pipe {
        length: f64,
        radius: f64,
        thickness: f64,
        segments: usize,
        capped: bool,
    },
    Duct {
        length: f64,
        width: f64,
        height: f64,
        thickness: f64,
        capped: bool,
    },
    Glb(glb::Package),
    Parts(Vec<parts::PreparedPart>),
}

/// Parameter/material resolution is separate from mesh construction so hosts can reuse local geometry.
pub struct PreparedItem {
    pub geometry_key: String,
    /// Version 2: each part's ID and geometry key, in definition order.
    pub part_keys: Vec<(String, String)>,
    pub parameters: Values,
    /// Evaluated derived values.
    pub derived: Derived,
    pub materials: BTreeMap<String, MaterialRequest>,
    /// Version 2: decals placed for these control values, in the item's coordinates.
    pub decals: Vec<ItemDecal>,
    /// Version 2: drag handles for these control values.
    pub handles: Vec<ItemHandle>,
    /// Weathering layers per slot, the item's own first, then its sub-items'.
    pub layers: BTreeMap<String, Vec<LayerRequest>>,
    /// Texels per meter each slot asks for: the item's own, else the greatest any sub-item
    /// states for a slot landing there.
    pub densities: BTreeMap<String, f64>,
    /// Authored features, in definition coordinates (before the item transform).
    authored: Vec<Feature>,
    definition: ItemDefinition,
    shape: Shape,
    attachments: Vec<Attachment>,
    assets: BTreeMap<String, String>,
}

/// Resolves the declared drag handles for the current control values.
fn handles(
    definition: &ItemDefinition,
    parameters: &Values,
    derived: &Derived,
) -> Result<Vec<ItemHandle>, ItemDiagnostic> {
    if definition.handles.len() > 32 {
        return Err(fail(
            &definition.source,
            "/handles",
            "ITEM_LIMIT",
            "An item has at most 32 handles.",
        ));
    }
    let mut ids = BTreeSet::new();
    let mut resolved = Vec::new();
    for (index, handle) in definition.handles.iter().enumerate() {
        let pointer = format!("/handles/{index}");
        let failure = |message: &str| fail(&definition.source, &pointer, "ITEM_HANDLE", message);
        if !schema::bounded_name(&handle.id) || !ids.insert(handle.id.as_str()) {
            return Err(failure("Handle IDs are unique bounded identifiers."));
        }
        let declaration = definition
            .parameters
            .get(&handle.parameter)
            .filter(|parameter| {
                parameter.role == Role::Geometry
                    && matches!(
                        parameter.kind,
                        ParameterType::Number | ParameterType::Integer
                    )
            })
            .ok_or_else(|| failure("A handle drags a numeric geometry control."))?;
        if !handle.scale.is_finite() || handle.scale == 0. {
            return Err(failure("A handle scale is finite and not zero."));
        }
        let mut position = [0.; 3];
        for (axis, value) in position.iter_mut().enumerate() {
            *value = definition.scalar(
                &handle.at[axis],
                parameters,
                derived,
                &format!("{pointer}/at/{axis}"),
            )?;
        }
        let matrix = Matrix(definition.transform);
        let axis = unit(handle.axis)
            .and_then(|axis| unit(matrix.direction(axis)))
            .map_err(|error| fail(&definition.source, &pointer, "ITEM_HANDLE", error))?;
        resolved.push(ItemHandle {
            id: handle.id.clone(),
            parameter: handle.parameter.clone(),
            position: matrix.point(position),
            axis,
            scale: handle.scale,
            value: parameters[&handle.parameter].as_f64().unwrap_or(0.),
            min: declaration.min,
            max: declaration.max,
        });
    }
    Ok(resolved)
}

pub fn prepare(
    definition: &ItemDefinition,
    overrides: &Values,
    assets: &AssetReader<'_>,
    cancelled: &dyn Fn() -> bool,
) -> Result<PreparedItem, ItemDiagnostic> {
    prepare_in(
        definition,
        overrides,
        &mut Environment::new(assets),
        cancelled,
    )
}

/// Prepares a definition whose parts may be sub-items (read through the environment's
/// assets) or native generators (from its host). Sub-item slots without an assignment in
/// the parent take the sub-item's own material requests.
pub fn prepare_in(
    definition: &ItemDefinition,
    overrides: &Values,
    environment: &mut Environment<'_>,
    cancelled: &dyn Fn() -> bool,
) -> Result<PreparedItem, ItemDiagnostic> {
    let assets = environment.assets;
    check_cancelled(&definition.source, cancelled)?;
    let parameters = definition.resolve_parameters(overrides)?;
    let mut materials = definition.materials(&parameters)?;
    let mut layers = definition.resolved_layers(&parameters)?;
    let mut densities = definition.slot_densities()?;
    // What sub-items ask for, kept apart so the item's own statement comes first.
    let mut inherited = BTreeMap::<String, f64>::new();
    let derived = definition.resolve_derived(&parameters)?;
    let context = parts::Context {
        definition,
        parameters: &parameters,
        derived: &derived,
    };
    let mut dependencies = BTreeMap::new();
    let mut authored = Vec::new();
    let mut attachments = Vec::new();
    let mut part_keys = Vec::new();
    let mut decals = Vec::new();
    if !definition.generator.is_parts()
        && (!definition.decals.is_empty() || !definition.handles.is_empty())
    {
        return Err(fail(
            &definition.source,
            if definition.decals.is_empty() {
                "/handles"
            } else {
                "/decals"
            },
            "ITEM_VERSION",
            "Decals and handles need itemspec version 2.",
        ));
    }
    let handles = handles(definition, &parameters, &derived)?;
    let (shape, identity) = match &definition.generator {
        Generator::Pipe {
            length,
            radius,
            wall_thickness,
            segments,
            capped,
        } => {
            let (length, radius, thickness, segments, capped) = context.pipe(
                "/generator",
                [length, radius, wall_thickness, segments],
                capped,
            )?;
            attachments.extend(parts::endpoints("", length));
            (
                Shape::Pipe {
                    length,
                    radius,
                    thickness,
                    segments,
                    capped,
                },
                json!(["pipe", length, radius, thickness, segments as f64, capped]),
            )
        }
        Generator::Duct {
            length,
            width,
            height,
            wall_thickness,
            capped,
        } => {
            let (length, width, height, thickness, capped) = context.duct(
                "/generator",
                [length, width, height, wall_thickness],
                capped,
            )?;
            attachments.extend(parts::endpoints("", length));
            (
                Shape::Duct {
                    length,
                    width,
                    height,
                    thickness,
                    capped,
                },
                json!(["duct", length, width, height, thickness, capped]),
            )
        }
        Generator::Glb { asset, scene } => {
            if asset.is_empty() || asset.len() > 4096 {
                return Err(fail(
                    &definition.source,
                    "/generator/asset",
                    "ITEM_ASSET",
                    "Invalid GLB asset reference.",
                ));
            }
            let bytes = assets(asset).map_err(|error| {
                fail(
                    &definition.source,
                    "/generator/asset",
                    "ITEM_ASSET",
                    format!("{asset}: {error}"),
                )
            })?;
            let package = glb::prepare(asset, bytes, *scene, assets, cancelled)?;
            dependencies = package.hashes.clone();
            (Shape::Glb(package), json!(["glb", scene, dependencies]))
        }
        Generator::Parts => {
            let prepared = parts::prepare(&context, &mut attachments, environment, cancelled)?;
            for part in &prepared {
                if let Some(child) = part.sub_item() {
                    for (slot, request) in &child.materials {
                        if let Some(target) = part.slot(slot) {
                            materials.entry(target).or_insert_with(|| request.clone());
                        }
                    }
                    for (asset, hash) in &child.assets {
                        dependencies.insert(asset.clone(), hash.clone());
                    }
                    for (slot, stack) in &child.layers {
                        if let Some(target) = part.slot(slot) {
                            let slot_layers = layers.entry(target).or_default();
                            for layer in stack {
                                if !slot_layers.contains(layer) {
                                    slot_layers.push(layer.clone());
                                }
                            }
                        }
                    }
                    // Several sub-items may land in one slot: the finest request holds, in
                    // whatever order the parts are listed.
                    for (slot, density) in &child.densities {
                        if let Some(target) = part.slot(slot) {
                            let held = inherited.entry(target).or_insert(*density);
                            *held = held.max(*density);
                        }
                    }
                }
            }
            for (slot, density) in inherited {
                densities.entry(slot).or_insert(density);
            }
            part_keys = prepared
                .iter()
                .zip(&definition.parts)
                .map(|(part, source)| (source.id.clone(), part.key().to_owned()))
                .collect();
            authored = parts::authored(&context, &prepared)?;
            let identity = json!(["parts", part_keys, authored]);
            decals = parts::decals(&context, &prepared, Matrix(definition.transform))?;
            (Shape::Parts(prepared), identity)
        }
        Generator::Box { .. }
        | Generator::Panel(_)
        | Generator::Route(_)
        | Generator::Prism { .. }
        | Generator::Arc(_)
        | Generator::Item(_)
        | Generator::Native(_) => {
            return Err(fail(
                &definition.source,
                "/generator",
                "ITEM_PART",
                "Box, panel, route, item and native generators belong to the parts of a version 2 item.",
            ));
        }
    };
    let mut names = attachments
        .iter()
        .map(|a| a.name.clone())
        .collect::<BTreeSet<_>>();
    for (index, attachment) in definition.attachments.iter().enumerate() {
        let pointer = format!("/attachments/{index}");
        if !schema::bounded_name(&attachment.name) || !names.insert(attachment.name.clone()) {
            return Err(fail(
                &definition.source,
                &pointer,
                "ITEM_ATTACHMENT",
                "Attachment names must be bounded and unique, including generated endpoints.",
            ));
        }
        let mut position = [0.; 3];
        for (axis, value) in position.iter_mut().enumerate() {
            *value = definition.scalar(
                &attachment.position[axis],
                &parameters,
                &derived,
                &format!("{pointer}/position/{axis}"),
            )?;
            if value.abs() > 1e7 {
                return Err(fail(
                    &definition.source,
                    &pointer,
                    "ITEM_ATTACHMENT",
                    "Attachment coordinate exceeds its limit.",
                ));
            }
        }
        unit(attachment.direction)
            .map_err(|error| fail(&definition.source, &pointer, "ITEM_ATTACHMENT", error))?;
        attachments.push(Attachment {
            name: attachment.name.clone(),
            position,
            direction: attachment.direction,
        });
    }
    for attachment in &mut attachments {
        attachment.position = Matrix(definition.transform).point(attachment.position);
        attachment.direction = unit(Matrix(definition.transform).direction(attachment.direction))
            .map_err(|error| {
            fail(&definition.source, "/attachments", "ITEM_ATTACHMENT", error)
        })?;
        if attachment
            .position
            .iter()
            .any(|v| !v.is_finite() || v.abs() > 1e7)
        {
            return Err(fail(
                &definition.source,
                "/attachments",
                "ITEM_ATTACHMENT",
                "Transformed attachment exceeds coordinate bounds.",
            ));
        }
    }
    let mut hash = Sha256::new();
    hash.update(b"itemspec-local-geometry-v1\0");
    hash.update(
        serde_json::to_vec(&(identity, definition.transform))
            .map_err(|e| fail(&definition.source, "/", "ITEM_JSON", e.to_string()))?,
    );
    check_cancelled(&definition.source, cancelled)?;
    Ok(PreparedItem {
        geometry_key: format!("{:x}", hash.finalize()),
        part_keys,
        parameters,
        derived,
        materials,
        decals,
        handles,
        layers,
        densities,
        authored,
        definition: definition.clone(),
        shape,
        attachments,
        assets: dependencies,
    })
}

impl PreparedItem {
    /// The pattern anchor of the item's first part, in the item's coordinates (after its
    /// transform), found from the prepared shapes without building any mesh.
    pub(super) fn first_anchor(&self) -> Option<PatternAnchor> {
        let Shape::Parts(parts) = &self.shape else {
            return None;
        };
        let part = parts.first()?;
        let (_, local) = part.features();
        let matrix = Matrix(self.definition.transform).then(part.placement());
        Some(PatternAnchor {
            part: local.part,
            origin: matrix.point(local.origin),
            u: matrix.direction(local.u),
            v: matrix.direction(local.v),
            normal: matrix.direction(local.normal),
        })
    }
    pub fn build(&self, cancelled: &dyn Fn() -> bool) -> Result<GeneratedItem, ItemDiagnostic> {
        Ok(self.construct(None, cancelled)?.0)
    }
    /// Builds against an earlier result of the same definition. Parts whose geometry key
    /// is unchanged keep their meshes; the second result names the parts that were built.
    /// An unchanged item shares the earlier geometry outright, as `reuse` does.
    pub fn rebuild(
        &self,
        previous: &GeneratedItem,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<(GeneratedItem, Vec<String>), ItemDiagnostic> {
        if self.geometry_key == previous.geometry_key {
            return Ok((self.reuse(previous)?, vec![]));
        }
        self.construct(Some(&previous.geometry), cancelled)
    }
    fn construct(
        &self,
        previous: Option<&ItemGeometry>,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<(GeneratedItem, Vec<String>), ItemDiagnostic> {
        check_cancelled(&self.definition.source, cancelled)?;
        let mut built = vec!["item".to_owned()];
        let mut geometry = match &self.shape {
            Shape::Parts(parts) => {
                let (geometry, parts) = parts::build(
                    parts,
                    Matrix(self.definition.transform),
                    previous,
                    cancelled,
                )
                .map_err(|(pointer, message)| {
                    let code = if cancelled() {
                        "ITEM_CANCELLED"
                    } else {
                        "ITEM_GEOMETRY"
                    };
                    fail(&self.definition.source, &pointer, code, message)
                })?;
                built = parts;
                let mut geometry = geometry;
                let matrix = Matrix(self.definition.transform);
                for feature in &self.authored {
                    let mut feature = feature.clone();
                    for point in &mut feature.points {
                        *point = matrix.point(*point);
                    }
                    feature.normal = unit(matrix.direction(feature.normal)).map_err(|e| {
                        fail(&self.definition.source, "/features", "ITEM_FEATURE", e)
                    })?;
                    feature.place_zone(matrix).map_err(|e| {
                        fail(&self.definition.source, "/features", "ITEM_FEATURE", e)
                    })?;
                    geometry.features.push(feature);
                }
                geometry
            }
            Shape::Pipe {
                length,
                radius,
                thickness,
                segments,
                capped,
            } => procedural::pipe(*length, *radius, *thickness, *segments, *capped, cancelled)
                .map_err(|e| fail(&self.definition.source, "/generator", "ITEM_GEOMETRY", e))?,
            Shape::Duct {
                length,
                width,
                height,
                thickness,
                capped,
            } => procedural::duct(*length, *width, *height, *thickness, *capped, cancelled)
                .map_err(|e| fail(&self.definition.source, "/generator", "ITEM_GEOMETRY", e))?,
            Shape::Glb(package) => package.build(cancelled)?,
        };
        for node in &mut geometry.nodes {
            if node.parent.is_none() {
                node.transform = Matrix(self.definition.transform)
                    .then(Matrix(node.transform))
                    .0;
            }
        }
        geometry
            .measure()
            .map_err(|e| fail(&self.definition.source, "/generator", "ITEM_GEOMETRY", e))?;
        check_cancelled(&self.definition.source, cancelled)?;
        Ok((self.finish(Arc::new(geometry))?, built))
    }
    /// Reuses only a matching immutable geometry artifact; all current bindings and provenance are renewed.
    pub fn reuse(&self, existing: &GeneratedItem) -> Result<GeneratedItem, ItemDiagnostic> {
        if self.geometry_key != existing.geometry_key {
            return Err(fail(
                &self.definition.source,
                "/generator",
                "ITEM_IDENTITY",
                "Geometry artifact does not match this definition's effective geometry inputs.",
            ));
        }
        self.finish(existing.geometry.clone())
    }
    fn finish(&self, geometry: Arc<ItemGeometry>) -> Result<GeneratedItem, ItemDiagnostic> {
        for slot in self.materials.keys() {
            if !geometry
                .meshes
                .iter()
                .any(|mesh| &mesh.material_slot == slot)
            {
                return Err(fail(
                    &self.definition.source,
                    &format!("/slots/{}", pointer_key(slot)),
                    "ITEM_MATERIAL",
                    "Material assignment refers to an unknown generated slot.",
                ));
            }
        }
        Ok(GeneratedItem {
            id: self.definition.id.clone(),
            geometry_key: self.geometry_key.clone(),
            geometry,
            materials: self.materials.clone(),
            attachments: self.attachments.clone(),
            collision: self.definition.collision.clone(),
            transform: self.definition.transform,
            provenance: Provenance {
                definition_id: self.definition.id.clone(),
                source: self.definition.source.clone(),
                parameters: self.parameters.clone(),
                assets: self.assets.clone(),
            },
            decals: self.decals.clone(),
            handles: self.handles.clone(),
            layers: self.layers.clone(),
            densities: self.densities.clone(),
        })
    }
}

pub fn generate(
    definition: &ItemDefinition,
    parameters: &Values,
    assets: &AssetReader<'_>,
    cancelled: &dyn Fn() -> bool,
) -> Result<GeneratedItem, ItemDiagnostic> {
    prepare(definition, parameters, assets, cancelled)?.build(cancelled)
}

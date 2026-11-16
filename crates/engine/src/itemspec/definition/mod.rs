//! Persistent ItemSpec definitions, local generation and bounded scene-preserving GLB ingestion.
pub mod expression;
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
    definition: ItemDefinition,
    shape: Shape,
    attachments: Vec<Attachment>,
    assets: BTreeMap<String, String>,
}

pub fn prepare(
    definition: &ItemDefinition,
    overrides: &Values,
    assets: &AssetReader<'_>,
    cancelled: &dyn Fn() -> bool,
) -> Result<PreparedItem, ItemDiagnostic> {
    check_cancelled(&definition.source, cancelled)?;
    let parameters = definition.resolve_parameters(overrides)?;
    let materials = definition.materials(&parameters)?;
    let derived = definition.resolve_derived(&parameters)?;
    let context = parts::Context {
        definition,
        parameters: &parameters,
        derived: &derived,
    };
    let mut dependencies = BTreeMap::new();
    let mut attachments = Vec::new();
    let mut part_keys = Vec::new();
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
            let prepared = parts::prepare(&context, &mut attachments)?;
            part_keys = prepared
                .iter()
                .zip(&definition.parts)
                .map(|(part, source)| (source.id.clone(), part.key().to_owned()))
                .collect();
            let identity = json!(["parts", part_keys]);
            (Shape::Parts(prepared), identity)
        }
        Generator::Box { .. } | Generator::Panel(_) | Generator::Route(_) => {
            return Err(fail(
                &definition.source,
                "/generator",
                "ITEM_PART",
                "Box, panel and route generators belong to the parts of a version 2 item.",
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
        definition: definition.clone(),
        shape,
        attachments,
        assets: dependencies,
    })
}

impl PreparedItem {
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

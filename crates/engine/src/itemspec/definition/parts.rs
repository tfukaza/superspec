//! Version 2 items: several generators ("parts"), each with its own meshes per face
//! group, features, pattern anchor, attachments and geometry key.
use super::{
    Attachment, Derived, Feature, ImportedMaterial, ItemDecal, ItemDefinition, ItemDiagnostic,
    ItemGeometry, ItemMesh, ItemNode, ItemPart, PatternAnchor, Values, decal, fail, panel,
    procedural, route,
    schema::{Generator, Scalar, Switch, bounded_name},
    surfaces::{Frame, Outline, Surfaces, prism},
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use superspec_geometry::{
    Matrix, V3,
    math::{dot, unit},
};

/// A definition with its resolved controls: what every generator field is read through.
pub(super) struct Context<'a> {
    pub definition: &'a ItemDefinition,
    pub parameters: &'a Values,
    pub derived: &'a Derived,
}
impl Context<'_> {
    pub(super) fn fail(
        &self,
        pointer: &str,
        code: &str,
        message: impl Into<String>,
    ) -> ItemDiagnostic {
        fail(&self.definition.source, pointer, code, message)
    }
    pub(super) fn scalar(&self, value: &Scalar, pointer: &str) -> Result<f64, ItemDiagnostic> {
        self.definition
            .scalar(value, self.parameters, self.derived, pointer)
    }
    pub(super) fn switch(&self, value: &Switch, pointer: &str) -> Result<bool, ItemDiagnostic> {
        self.definition.switch(value, self.parameters, pointer)
    }
    pub(super) fn dimension(&self, value: &Scalar, pointer: &str) -> Result<f64, ItemDiagnostic> {
        let value = self.scalar(value, pointer)?;
        if value <= 0. || value > 10_000. {
            return Err(self.fail(
                pointer,
                "ITEM_GEOMETRY",
                "Dimensions must be positive and at most 10000 meters.",
            ));
        }
        Ok(value)
    }
    /// Length, radius, wall thickness, segments and capping of a straight pipe.
    pub(super) fn pipe(
        &self,
        pointer: &str,
        [length, radius, wall_thickness, segments]: [&Scalar; 4],
        capped: &Switch,
    ) -> Result<(f64, f64, f64, usize, bool), ItemDiagnostic> {
        let length = self.dimension(length, &format!("{pointer}/length"))?;
        let radius = self.dimension(radius, &format!("{pointer}/radius"))?;
        let thickness = self.scalar(wall_thickness, &format!("{pointer}/wallThickness"))?;
        let segments = self.scalar(segments, &format!("{pointer}/segments"))?;
        if thickness < 0. || thickness >= radius {
            return Err(self.fail(
                &format!("{pointer}/wallThickness"),
                "ITEM_GEOMETRY",
                "Pipe wall thickness must be nonnegative and smaller than its radius.",
            ));
        }
        if segments.fract() != 0. || !(3. ..=256.).contains(&segments) {
            return Err(self.fail(
                &format!("{pointer}/segments"),
                "ITEM_GEOMETRY",
                "Pipe segments must be an integer from 3 to 256.",
            ));
        }
        let capped = self.switch(capped, &format!("{pointer}/capped"))?;
        Ok((length, radius, thickness, segments as usize, capped))
    }
    /// Length, width, height, wall thickness and capping of a straight duct.
    pub(super) fn duct(
        &self,
        pointer: &str,
        [length, width, height, wall_thickness]: [&Scalar; 4],
        capped: &Switch,
    ) -> Result<(f64, f64, f64, f64, bool), ItemDiagnostic> {
        let length = self.dimension(length, &format!("{pointer}/length"))?;
        let width = self.dimension(width, &format!("{pointer}/width"))?;
        let height = self.dimension(height, &format!("{pointer}/height"))?;
        let thickness = self.scalar(wall_thickness, &format!("{pointer}/wallThickness"))?;
        if thickness < 0. || thickness * 2. >= width.min(height) {
            return Err(self.fail(
                &format!("{pointer}/wallThickness"),
                "ITEM_GEOMETRY",
                "Duct wall thickness must leave a positive inner section.",
            ));
        }
        let capped = self.switch(capped, &format!("{pointer}/capped"))?;
        Ok((length, width, height, thickness, capped))
    }
}

#[derive(Clone, Debug, Serialize)]
enum PartShape {
    Box {
        size: V3,
    },
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
    Panel(panel::Panel),
    Route(route::Route),
}

pub(super) struct PreparedPart {
    id: String,
    key: String,
    shape: PartShape,
    /// Part coordinates to item definition coordinates.
    placement: Matrix,
    slots: BTreeMap<String, String>,
    pointer: String,
}
impl PreparedPart {
    pub(super) fn key(&self) -> &str {
        &self.key
    }
}

pub(super) fn endpoints(prefix: &str, length: f64) -> [Attachment; 2] {
    [
        Attachment {
            name: format!("{prefix}start"),
            position: [0.; 3],
            direction: [0., 0., -1.],
        },
        Attachment {
            name: format!("{prefix}end"),
            position: [0., 0., length],
            direction: [0., 0., 1.],
        },
    ]
}

/// Resolves every part in order and appends its attachments in item definition
/// coordinates. A route may start from an attachment of an earlier part.
pub(super) fn prepare(
    context: &Context<'_>,
    attachments: &mut Vec<Attachment>,
) -> Result<Vec<PreparedPart>, ItemDiagnostic> {
    let mut ids = BTreeSet::new();
    let mut parts = Vec::new();
    for (index, part) in context.definition.parts.iter().enumerate() {
        let pointer = format!("/parts/{index}");
        if !bounded_name(&part.id) || part.id.contains('/') || !ids.insert(part.id.as_str()) {
            return Err(context.fail(
                &format!("{pointer}/id"),
                "ITEM_PART",
                "Part IDs are unique bounded identifiers without a slash.",
            ));
        }
        let mut placement = Matrix::default();
        if let Some(definition) = &part.placement {
            let mut values = [[0.; 3]; 2];
            for (row, (name, source)) in [
                ("position", &definition.position),
                ("rotation", &definition.rotation),
            ]
            .into_iter()
            .enumerate()
            {
                for axis in 0..3 {
                    values[row][axis] = context
                        .scalar(&source[axis], &format!("{pointer}/placement/{name}/{axis}"))?;
                }
            }
            if values[0].iter().any(|v| v.abs() > 1e7) {
                return Err(context.fail(
                    &format!("{pointer}/placement"),
                    "ITEM_PART",
                    "Part position exceeds coordinate bounds.",
                ));
            }
            placement = Matrix::placement(values[0], values[1], [1.; 3])
                .map_err(|e| context.fail(&format!("{pointer}/placement"), "ITEM_PART", e))?;
        }
        let generator = format!("{pointer}/generator");
        let (shape, groups, local): (PartShape, &[&str], Vec<Attachment>) = match &part.generator {
            Generator::Box { size } => {
                let mut resolved = [0.; 3];
                for axis in 0..3 {
                    resolved[axis] =
                        context.dimension(&size[axis], &format!("{generator}/size/{axis}"))?;
                }
                (
                    PartShape::Box { size: resolved },
                    &["side", "top", "bottom"],
                    vec![],
                )
            }
            Generator::Pipe {
                length,
                radius,
                wall_thickness,
                segments,
                capped,
            } => {
                let (length, radius, thickness, segments, capped) = context.pipe(
                    &generator,
                    [length, radius, wall_thickness, segments],
                    capped,
                )?;
                (
                    PartShape::Pipe {
                        length,
                        radius,
                        thickness,
                        segments,
                        capped,
                    },
                    &["surface"],
                    endpoints(&format!("{}/", part.id), length).into(),
                )
            }
            Generator::Duct {
                length,
                width,
                height,
                wall_thickness,
                capped,
            } => {
                let (length, width, height, thickness, capped) =
                    context.duct(&generator, [length, width, height, wall_thickness], capped)?;
                (
                    PartShape::Duct {
                        length,
                        width,
                        height,
                        thickness,
                        capped,
                    },
                    &["surface"],
                    endpoints(&format!("{}/", part.id), length).into(),
                )
            }
            Generator::Panel(definition) => (
                PartShape::Panel(panel::resolve(context, definition, &generator)?),
                panel::GROUPS,
                vec![],
            ),
            Generator::Route(definition) => {
                let origin = match &definition.from {
                    None => [0.; 3],
                    Some(reference) => {
                        if part.placement.is_some() {
                            return Err(context.fail(
                                &format!("{pointer}/placement"),
                                "ITEM_ROUTE",
                                "A route that starts from an attachment has no placement of its own.",
                            ));
                        }
                        attachments
                            .iter()
                            .find(|attachment| attachment.name == reference.attachment)
                            .ok_or_else(|| {
                                context.fail(
                                    &format!("{generator}/from"),
                                    "ITEM_ROUTE",
                                    format!(
                                        "{} is not an attachment of an earlier part.",
                                        reference.attachment
                                    ),
                                )
                            })?
                            .position
                    }
                };
                let route = route::resolve(context, definition, &generator, origin)?;
                let local = route.attachments(&part.id);
                (PartShape::Route(route), route::GROUPS, local)
            }
            Generator::Glb { .. } | Generator::Parts => {
                return Err(context.fail(
                    &generator,
                    "ITEM_PART",
                    "A part uses a box, panel, route, pipe or duct generator.",
                ));
            }
        };
        for (group, slot) in &part.slots {
            if !groups.contains(&group.as_str()) || !bounded_name(slot) {
                return Err(context.fail(
                    &format!("{pointer}/slots/{}", super::pointer_key(group)),
                    "ITEM_PART",
                    format!(
                        "Unknown face group {group} or invalid slot name; this generator has {}.",
                        groups.join(", ")
                    ),
                ));
            }
        }
        for mut attachment in local {
            attachment.position = placement.point(attachment.position);
            attachment.direction = placement.direction(attachment.direction);
            attachments.push(attachment);
        }
        let mut hash = Sha256::new();
        hash.update(b"itemspec-part-geometry-v1\0");
        hash.update(
            serde_json::to_vec(&(&part.id, &shape, placement.0, &part.slots))
                .map_err(|e| context.fail(&pointer, "ITEM_JSON", e.to_string()))?,
        );
        parts.push(PreparedPart {
            id: part.id.clone(),
            key: format!("{:x}", hash.finalize()),
            shape,
            placement,
            slots: part.slots.clone(),
            pointer: generator,
        });
    }
    Ok(parts)
}

/// Places every decal of the definition on its part, in the item's coordinates (`item` is
/// the definition transform). A decal that does not fit fails with `ITEM_DECAL_FIT` at its
/// own pointer.
pub(super) fn decals(
    context: &Context<'_>,
    parts: &[PreparedPart],
    item: Matrix,
) -> Result<Vec<ItemDecal>, ItemDiagnostic> {
    let definitions = &context.definition.decals;
    if definitions.len() > decal::MAXIMUM {
        return Err(context.fail("/decals", "ITEM_LIMIT", "An item has at most 64 decals."));
    }
    let mut ids = BTreeSet::new();
    let mut placed = Vec::with_capacity(definitions.len());
    for (index, definition) in definitions.iter().enumerate() {
        let pointer = format!("/decals/{index}");
        if !bounded_name(&definition.id)
            || definition.id.contains('/')
            || !ids.insert(definition.id.as_str())
        {
            return Err(context.fail(
                &format!("{pointer}/id"),
                "ITEM_DECAL",
                "Decal IDs are unique bounded identifiers without a slash.",
            ));
        }
        let Some(part) = parts.iter().find(|part| part.id == definition.on.part) else {
            return Err(context.fail(
                &format!("{pointer}/on/part"),
                "ITEM_DECAL",
                format!("{} is not a part of this item.", definition.on.part),
            ));
        };
        let surface = match &part.shape {
            PartShape::Panel(panel) => decal::Surface::Panel(panel),
            PartShape::Box { size } => decal::Surface::Box(*size),
            PartShape::Route(route) => decal::Surface::Route(route),
            PartShape::Pipe { .. } | PartShape::Duct { .. } => {
                return Err(context.fail(
                    &format!("{pointer}/on/part"),
                    "ITEM_DECAL",
                    "A decal sits on a panel, box or route part.",
                ));
            }
        };
        placed.push(decal::resolve(
            context,
            definition,
            &surface,
            item.then(part.placement),
            &pointer,
        )?);
    }
    Ok(placed)
}

impl PreparedPart {
    fn meshes(&self, cancelled: &dyn Fn() -> bool) -> Result<Vec<ItemMesh>, String> {
        let straight = |geometry: ItemGeometry| {
            let slot = if self.slots.is_empty() {
                Some("surface".to_owned())
            } else {
                self.slots.get("surface").cloned()
            };
            geometry
                .meshes
                .into_iter()
                .zip(slot)
                .map(|(mut mesh, slot)| {
                    mesh.id = format!("{}/surface", self.id);
                    mesh.material_slot = slot;
                    mesh
                })
                .collect()
        };
        let mut surfaces = Surfaces::new(cancelled);
        match &self.shape {
            PartShape::Pipe {
                length,
                radius,
                thickness,
                segments,
                capped,
            } => {
                return Ok(straight(procedural::pipe(
                    *length, *radius, *thickness, *segments, *capped, cancelled,
                )?));
            }
            PartShape::Duct {
                length,
                width,
                height,
                thickness,
                capped,
            } => {
                return Ok(straight(procedural::duct(
                    *length, *width, *height, *thickness, *capped, cancelled,
                )?));
            }
            PartShape::Box { size } => prism(
                &mut surfaces,
                ["side", "bottom", "top"],
                &Frame {
                    origin: [0.; 3],
                    x: [1., 0., 0.],
                    y: [0., 1., 0.],
                },
                &Outline::rectangle([0., 0.], [size[0], size[1]]),
                None,
                [0., size[2]],
            )?,
            PartShape::Panel(panel) => panel.build(&mut surfaces)?,
            PartShape::Route(route) => route.build(&mut surfaces)?,
        }
        Ok(surfaces.into_meshes(&self.id, &self.slots))
    }

    fn features(&self) -> (Vec<Feature>, PatternAnchor) {
        match &self.shape {
            PartShape::Panel(panel) => panel.features(&self.id),
            PartShape::Route(route) => route.features(&self.id),
            // Straight generators and boxes report an anchor at their origin and no features.
            _ => (
                vec![],
                PatternAnchor {
                    part: self.id.clone(),
                    origin: [0.; 3],
                    u: [1., 0., 0.],
                    v: [0., 0., 1.],
                    normal: [0., -1., 0.],
                },
            ),
        }
    }
}

/// Builds the item's geometry. A part whose key matches one in `previous` copies that
/// part's meshes instead of regenerating them; the second result lists the parts built.
/// Nodes carry part placements only; features and anchors are in `item` coordinates.
pub(super) fn build(
    parts: &[PreparedPart],
    item: Matrix,
    previous: Option<&ItemGeometry>,
    cancelled: &dyn Fn() -> bool,
) -> Result<(ItemGeometry, Vec<String>), (String, String)> {
    let mut geometry = ItemGeometry {
        meshes: vec![],
        nodes: vec![],
        bounds: [[0.; 3]; 2],
        imported_materials: BTreeMap::new(),
        images: vec![],
        parts: vec![],
        features: vec![],
        anchors: vec![],
    };
    let mut built = Vec::new();
    for part in parts {
        let failure = |message: String| (part.pointer.clone(), message);
        if cancelled() {
            return Err(failure("Item generation cancelled.".into()));
        }
        let kept = previous.and_then(|previous| {
            let old = previous
                .parts
                .iter()
                .find(|old| old.id == part.id && old.geometry_key == part.key)?;
            old.meshes
                .iter()
                .map(|index| previous.meshes.get(*index).cloned())
                .collect::<Option<Vec<_>>>()
        });
        let meshes = match kept {
            Some(meshes) => meshes,
            None => {
                built.push(part.id.clone());
                part.meshes(cancelled).map_err(failure)?
            }
        };
        let first = geometry.meshes.len();
        let indices: Vec<usize> = (first..first + meshes.len()).collect();
        for mesh in meshes {
            geometry
                .imported_materials
                .entry(mesh.material_slot.clone())
                .or_insert_with(|| {
                    let mut fallback = ImportedMaterial::plain(&mesh.material_slot);
                    fallback.metallic_factor = 0.;
                    fallback.roughness_factor = 0.8;
                    fallback
                });
            geometry.meshes.push(mesh);
        }
        geometry.parts.push(ItemPart {
            id: part.id.clone(),
            geometry_key: part.key.clone(),
            node: geometry.nodes.len(),
            meshes: indices.clone(),
        });
        geometry.nodes.push(ItemNode {
            id: format!("part/{}", part.id),
            name: Some(part.id.clone()),
            parent: None,
            transform: part.placement.0,
            meshes: indices,
            extras: None,
        });
        let matrix = item.then(part.placement);
        let inverse = matrix.inverse().map_err(failure)?;
        // Surface normals follow the inverse transpose; route directions follow the matrix.
        let covector = |normal: V3| {
            unit([
                dot(inverse.0[0], normal),
                dot(inverse.0[1], normal),
                dot(inverse.0[2], normal),
            ])
        };
        let (features, anchor) = part.features();
        let surface = matches!(part.shape, PartShape::Panel(_));
        for mut feature in features {
            for point in &mut feature.points {
                *point = matrix.point(*point);
            }
            feature.normal = if surface {
                covector(feature.normal)
            } else {
                unit(matrix.direction(feature.normal))
            }
            .map_err(failure)?;
            geometry.features.push(feature);
        }
        geometry.anchors.push(PatternAnchor {
            part: anchor.part,
            origin: matrix.point(anchor.origin),
            u: matrix.direction(anchor.u),
            v: matrix.direction(anchor.v),
            normal: covector(anchor.normal).map_err(failure)?,
        });
    }
    Ok((geometry, built))
}

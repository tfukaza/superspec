//! Version 2 items: several generators ("parts"), each with its own meshes per face
//! group, features, pattern anchor, attachments and geometry key.
use super::{
    Attachment, Derived, Feature, ImportedMaterial, ItemDecal, ItemDefinition, ItemDiagnostic,
    ItemGeometry, ItemImage, ItemMesh, ItemNode, ItemPart, PatternAnchor, PreparedItem, Values,
    arc,
    compose::{self, Environment, Opaque},
    decal, fail, footprint, panel, procedural, route,
    schema::{Axis, Generator, PartDefinition, Scalar, Switch, bounded_name},
    surfaces::{self, Frame, Outline, Surfaces, beveled_prism, prism},
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
    /// A bevel's radius (0 for none, at most `limit`) and its 1 or 2 chords.
    pub(super) fn bevel(
        &self,
        pointer: &str,
        radius: &Scalar,
        segments: &Scalar,
        limit: f64,
    ) -> Result<(f64, usize), ItemDiagnostic> {
        let radius = self.scalar(radius, &format!("{pointer}/bevel"))?;
        let segments = self.scalar(segments, &format!("{pointer}/bevelSegments"))?;
        if radius < 0. || radius > limit {
            return Err(self.fail(
                &format!("{pointer}/bevel"),
                "ITEM_GEOMETRY",
                format!(
                    "A bevel is a radius from 0 to less than half of every dimension it rounds (here at most {limit:.4} m)."
                ),
            ));
        }
        if segments != 1. && segments != 2. {
            return Err(self.fail(
                &format!("{pointer}/bevelSegments"),
                "ITEM_GEOMETRY",
                "A bevel has 1 or 2 segments.",
            ));
        }
        // Without a radius the segments say nothing, so they stay out of the geometry key.
        Ok((radius, if radius > 0. { segments as usize } else { 1 }))
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
        #[serde(skip_serializing_if = "Bevel::none")]
        bevel: Bevel,
        /// The one axis whose edges the bevel rounds; every edge when absent.
        #[serde(skip_serializing_if = "Option::is_none")]
        along: Option<Axis>,
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
    Arc(arc::Arc),
    /// A sub-item: its entry, geometry key and prepared definition.
    Item {
        entry: String,
        key: String,
        #[serde(skip_serializing)]
        prepared: Opaque<PreparedItem>,
    },
    Native(compose::Native),
    /// A footprint outline swept up by `height` from the part origin.
    Prism {
        outline: Vec<footprint::P2>,
        height: f64,
        #[serde(skip_serializing_if = "Bevel::none")]
        bevel: Bevel,
    },
}

/// A resolved bevel: its radius and chords. Absent from a geometry key when the radius is 0,
/// so parts without one keep the keys they had.
#[derive(Clone, Copy, Debug, Serialize)]
pub(super) struct Bevel {
    pub radius: f64,
    pub segments: usize,
}
impl Bevel {
    pub(super) fn none(&self) -> bool {
        self.radius == 0.
    }
}

/// Most parts an item may expand to, counting every copy of a repeated part.
pub const MAXIMUM_PARTS: usize = 512;

pub(super) struct PreparedPart {
    id: String,
    key: String,
    shape: PartShape,
    /// Part coordinates to item definition coordinates.
    placement: Matrix,
    slots: BTreeMap<String, String>,
    pointer: String,
    /// Zones below the features the generator reports.
    zones: Vec<ZoneRule>,
}

/// A part's zone rule with its values resolved (see `schema::ZoneRuleDefinition`).
#[derive(Clone, Debug, Serialize)]
struct ZoneRule {
    name: String,
    below: String,
    face: Option<String>,
    length: f64,
    depth: f64,
    u0: f64,
    scale: f64,
    id: f64,
    until: bool,
    clip: bool,
}

/// A zone's checked length, depth, scale and identity from its definition's values.
fn zone_values(
    context: &Context<'_>,
    pointer: &str,
    [length, depth, scale, seed]: [Option<&Scalar>; 4],
) -> Result<(f64, f64, f64, f64), ItemDiagnostic> {
    let read = |value: Option<&Scalar>, field: &str, fallback: f64| match value {
        Some(value) => context.scalar(value, &format!("{pointer}/{field}")),
        None => Ok(fallback),
    };
    let length = read(length, "length", 0.)?;
    let depth = read(depth, "depth", crate::materialspec::program::ZONE_DEPTH)?;
    let scale = read(scale, "scale", 1.)?;
    let seed = read(seed, "seed", 0.)?;
    if !(length > 0. && length <= 1000.) || !(depth > 0. && depth <= 10.) || !scale.is_finite() {
        return Err(context.fail(
            pointer,
            "ITEM_ZONE",
            "A zone has a length from 0 to 1000 m, a depth from 0 to 10 m and a finite scale.",
        ));
    }
    Ok((
        length,
        depth,
        scale,
        crate::materialspec::program::zone_id(&[seed.to_bits()]),
    ))
}

impl PreparedPart {
    /// The generator's features with the zones this part's rules hang below them: on the
    /// feature itself where the zone takes its name, else on a copy under the zone's name.
    fn zoned(&self, features: Vec<Feature>) -> Vec<Feature> {
        if self.zones.is_empty() {
            return features;
        }
        let mut result = features.clone();
        for rule in &self.zones {
            for (index, feature) in features.iter().enumerate() {
                if feature.name != rule.below
                    || rule.face.as_ref().is_some_and(|face| *face != feature.face)
                    || feature.points.is_empty()
                {
                    continue;
                }
                let first = feature.points[0];
                let (mut stops, mut holes) = (Vec::new(), Vec::new());
                // A level edge of a panel knows the openings below it, in its own u and v.
                let level = feature
                    .points
                    .iter()
                    .all(|p| (p[2] - first[2]).abs() < 1e-9);
                if let (PartShape::Panel(panel), true, true) =
                    (&self.shape, level, rule.until || rule.clip)
                {
                    for [x, z, width, height] in panel.openings_on(feature.face == "front") {
                        let head = z + height;
                        if head > first[2] - 1e-9 {
                            continue;
                        }
                        let (from, to) = (rule.u0 + x, rule.u0 + x + width);
                        if rule.until {
                            stops.push([from, to, first[2] - head]);
                        } else {
                            holes.push([from, to, first[2] - head, first[2] - z]);
                        }
                    }
                }
                let zone = crate::materialspec::program::Zone {
                    down: [0., 0., -1.],
                    length: rule.length,
                    depth: rule.depth,
                    u0: rule.u0 + first[0],
                    id: rule.id,
                    scale: rule.scale,
                    stops,
                    holes,
                };
                if rule.name == feature.name {
                    result[index].zone = Some(zone);
                } else {
                    let mut copy = feature.clone();
                    copy.name = rule.name.clone();
                    copy.zone = Some(zone);
                    result.push(copy);
                }
            }
        }
        result
    }
    pub(super) fn key(&self) -> &str {
        &self.key
    }
    /// The prepared sub-item of an item part.
    pub(super) fn sub_item(&self) -> Option<&PreparedItem> {
        match &self.shape {
            PartShape::Item { prepared, .. } => Some(&prepared.0),
            _ => None,
        }
    }
    /// The item slot that a generated slot of this part lands in.
    pub(super) fn slot(&self, group: &str) -> Option<String> {
        if self.slots.is_empty() {
            Some(group.to_owned())
        } else {
            self.slots.get(group).cloned()
        }
    }
}

/// One emitted copy of a part definition: its ID and the derived values it reads, which
/// include its copy indices.
struct Copy {
    id: String,
    derived: Derived,
}

/// Expands repeats and conditions. Copy indices join the derived values of each copy.
fn copies(
    context: &Context<'_>,
    part: &PartDefinition,
    pointer: &str,
) -> Result<Vec<Copy>, ItemDiagnostic> {
    if part.repeat.len() > 3 {
        return Err(context.fail(
            &format!("{pointer}/repeat"),
            "ITEM_REPEAT",
            "A part repeats over at most three axes.",
        ));
    }
    let mut copies = vec![Copy {
        id: part.id.clone(),
        derived: context.derived.clone(),
    }];
    for (axis, repeat) in part.repeat.iter().enumerate() {
        let at = format!("{pointer}/repeat/{axis}");
        let identifier = !repeat.name.is_empty()
            && repeat.name.len() <= 64
            && repeat
                .name
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'_')
            && !repeat.name.as_bytes()[0].is_ascii_digit();
        if !identifier
            || context.definition.parameters.contains_key(&repeat.name)
            || context.derived.contains_key(&repeat.name)
        {
            return Err(context.fail(
                &format!("{at}/name"),
                "ITEM_REPEAT",
                "A copy index is an identifier that differs from every control and derived value.",
            ));
        }
        let mut next = Vec::new();
        for copy in copies {
            // Along a footprint, one copy per edge, each reading that edge's geometry.
            let edges = match &repeat.footprint {
                Some(reference) => Some(footprint::resolve(
                    context.definition,
                    reference,
                    context.parameters,
                    &copy.derived,
                    &format!("{at}/footprint"),
                )?),
                None => None,
            };
            let count = match &edges {
                Some(edges) => edges.len() as f64,
                None => context.definition.scalar(
                    &repeat.count,
                    context.parameters,
                    &copy.derived,
                    &format!("{at}/count"),
                )?,
            };
            if count.fract() != 0. || !(0. ..=256.).contains(&count) {
                return Err(context.fail(
                    &format!("{at}/count"),
                    "ITEM_REPEAT",
                    "A repeat count is an integer from 0 to 256.",
                ));
            }
            for index in 0..count as usize {
                let mut derived = copy.derived.clone();
                derived.insert(repeat.name.clone(), index as f64);
                derived.insert(format!("{}_count", repeat.name), count);
                if let Some(edges) = &edges {
                    derived.extend(footprint::locals(&repeat.name, edges, index));
                }
                next.push(Copy {
                    id: format!("{}-{index}", copy.id),
                    derived,
                });
            }
            if next.len() > MAXIMUM_PARTS {
                return Err(context.fail(
                    &format!("{pointer}/repeat"),
                    "ITEM_LIMIT",
                    format!("An item expands to at most {MAXIMUM_PARTS} parts."),
                ));
            }
        }
        copies = next;
    }
    let mut kept = Vec::with_capacity(copies.len());
    for copy in copies {
        let shown = match &part.when {
            None => true,
            Some(condition) => {
                context.definition.scalar(
                    condition,
                    context.parameters,
                    &copy.derived,
                    &format!("{pointer}/when"),
                )? > 0.
            }
        };
        if shown {
            kept.push(copy);
        }
    }
    Ok(kept)
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
    outer: &Context<'_>,
    attachments: &mut Vec<Attachment>,
    environment: &mut Environment<'_>,
    cancelled: &dyn Fn() -> bool,
) -> Result<Vec<PreparedPart>, ItemDiagnostic> {
    let mut ids = BTreeSet::new();
    let mut parts = Vec::new();
    for (index, part) in outer.definition.parts.iter().enumerate() {
        let pointer = format!("/parts/{index}");
        if !bounded_name(&part.id) || part.id.contains('/') {
            return Err(outer.fail(
                &format!("{pointer}/id"),
                "ITEM_PART",
                "Part IDs are unique bounded identifiers without a slash.",
            ));
        }
        for copy in copies(outer, part, &pointer)? {
            super::check_cancelled(&outer.definition.source, cancelled)?;
            if !ids.insert(copy.id.clone()) || parts.len() >= MAXIMUM_PARTS {
                return Err(outer.fail(
                    &format!("{pointer}/id"),
                    "ITEM_PART",
                    format!(
                        "Part ID {} repeats, or the item expands beyond {MAXIMUM_PARTS} parts.",
                        copy.id
                    ),
                ));
            }
            let context = &Context {
                definition: outer.definition,
                parameters: outer.parameters,
                derived: &copy.derived,
            };
            let id = copy.id.as_str();
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
                placement = match &definition.along {
                    None => Matrix::placement(values[0], values[1], [1.; 3]).map_err(|e| {
                        context.fail(&format!("{pointer}/placement"), "ITEM_PART", e)
                    })?,
                    Some(along) => {
                        let at = format!("{pointer}/placement/along");
                        let x = context.scalar(&along[0], &format!("{at}/0"))?;
                        let y = context.scalar(&along[1], &format!("{at}/1"))?;
                        let size = (x * x + y * y).sqrt();
                        if !(size > 1e-9) || values[1].iter().any(|v| *v != 0.) {
                            return Err(context.fail(
                                &at,
                                "ITEM_PART",
                                "A part placed along a direction needs a nonzero direction and no rotation.",
                            ));
                        }
                        let (x, y) = (x / size, y / size);
                        Matrix([[x, y, 0.], [-y, x, 0.], [0., 0., 1.], values[0]])
                    }
                };
            }
            let generator = format!("{pointer}/generator");
            let (shape, groups, local): (PartShape, &[&str], Vec<Attachment>) = match &part
                .generator
            {
                Generator::Box {
                    size,
                    bevel,
                    bevel_segments,
                    bevel_axis,
                } => {
                    let mut resolved = [0.; 3];
                    for axis in 0..3 {
                        resolved[axis] =
                            context.dimension(&size[axis], &format!("{generator}/size/{axis}"))?;
                    }
                    // Edges along one axis are rounded out of the other two dimensions.
                    let smallest = (0..3)
                        .filter(|axis| (*bevel_axis).is_none_or(|along| along as usize != *axis))
                        .map(|axis| resolved[axis])
                        .fold(f64::INFINITY, f64::min);
                    let (radius, segments) = context.bevel(
                        &generator,
                        bevel,
                        bevel_segments,
                        (smallest / 2. - 5e-4).max(0.),
                    )?;
                    (
                        PartShape::Box {
                            size: resolved,
                            bevel: Bevel { radius, segments },
                            along: (*bevel_axis).filter(|_| radius > 0.),
                        },
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
                        endpoints(&format!("{id}/"), length).into(),
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
                        &generator,
                        [length, width, height, wall_thickness],
                        capped,
                    )?;
                    (
                        PartShape::Duct {
                            length,
                            width,
                            height,
                            thickness,
                            capped,
                        },
                        &["surface"],
                        endpoints(&format!("{id}/"), length).into(),
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
                            let wanted = compose::template(
                                context.definition,
                                &reference.attachment,
                                context.derived,
                                &format!("{generator}/from"),
                            )?;
                            if part.placement.is_some() {
                                return Err(context.fail(
                                &format!("{pointer}/placement"),
                                "ITEM_ROUTE",
                                "A route that starts from an attachment has no placement of its own.",
                            ));
                            }
                            attachments
                                .iter()
                                .find(|attachment| attachment.name == wanted)
                                .ok_or_else(|| {
                                    context.fail(
                                        &format!("{generator}/from"),
                                        "ITEM_ROUTE",
                                        format!(
                                            "{wanted} is not an attachment of an earlier part."
                                        ),
                                    )
                                })?
                                .position
                        }
                    };
                    let route = route::resolve(context, definition, &generator, origin)?;
                    let local = route.attachments(id);
                    (PartShape::Route(route), route::GROUPS, local)
                }
                Generator::Prism {
                    footprint: reference,
                    height,
                    inset,
                    bevel,
                    bevel_segments,
                } => {
                    let edges = footprint::resolve(
                        context.definition,
                        reference,
                        context.parameters,
                        context.derived,
                        &format!("{generator}/footprint"),
                    )?;
                    let inset = context.scalar(inset, &format!("{generator}/inset"))?;
                    let height = context.dimension(height, &format!("{generator}/height"))?;
                    let outline = footprint::outline(&edges, inset);
                    let (radius, segments) = context.bevel(
                        &generator,
                        bevel,
                        bevel_segments,
                        (height / 2. - 5e-4).max(0.),
                    )?;
                    if radius > 0. && !surfaces::insets(&outline, radius) {
                        return Err(context.fail(
                            &format!("{generator}/bevel"),
                            "ITEM_GEOMETRY",
                            "The bevel is wider than an edge of the outline can lose.",
                        ));
                    }
                    (
                        PartShape::Prism {
                            outline,
                            height,
                            bevel: Bevel { radius, segments },
                        },
                        &["side", "top", "bottom"],
                        vec![],
                    )
                }
                Generator::Arc(definition) => (
                    PartShape::Arc(arc::resolve(context, definition, &generator)?),
                    arc::GROUPS,
                    vec![],
                ),
                Generator::Item(reference) => {
                    let entry = compose::entry(
                        context.definition,
                        reference,
                        context.parameters,
                        context.derived,
                        &generator,
                    )?;
                    let arguments = compose::arguments(
                        context.definition,
                        &reference.parameters,
                        context.parameters,
                        context.derived,
                        &format!("{generator}/parameters"),
                    )?;
                    let prepared = compose::sub_item(
                        context.definition,
                        &entry,
                        &arguments,
                        environment,
                        cancelled,
                        &generator,
                    )?;
                    let local = prepared
                        .attachments
                        .iter()
                        .map(|attachment| Attachment {
                            name: format!("{id}/{}", attachment.name),
                            ..attachment.clone()
                        })
                        .collect();
                    (
                        PartShape::Item {
                            entry,
                            key: prepared.geometry_key.clone(),
                            prepared: Opaque(std::sync::Arc::new(prepared)),
                        },
                        &[],
                        local,
                    )
                }
                Generator::Native(reference) => (
                    PartShape::Native(compose::native(
                        context.definition,
                        reference,
                        context.parameters,
                        context.derived,
                        environment,
                        &generator,
                    )?),
                    &[],
                    vec![],
                ),
                Generator::Glb { .. } | Generator::Parts => {
                    return Err(context.fail(
                        &generator,
                        "ITEM_PART",
                        "A part uses a box, panel, route, arc, pipe, duct, item or native generator.",
                    ));
                }
            };
            // Sub-items and native generators name their own slots, known when they are built.
            let open = matches!(shape, PartShape::Item { .. } | PartShape::Native(_));
            for (group, slot) in &part.slots {
                if (!open && !groups.contains(&group.as_str())) || !bounded_name(slot) {
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
            let mut zones = Vec::with_capacity(part.zones.len());
            if part.zones.len() > 16 {
                return Err(context.fail(
                    &format!("{pointer}/zones"),
                    "ITEM_ZONE",
                    "A part has at most 16 zone rules.",
                ));
            }
            for (index, rule) in part.zones.iter().enumerate() {
                let at = format!("{pointer}/zones/{index}");
                let name = rule.name.clone().unwrap_or_else(|| rule.below.clone());
                if !bounded_name(&name) || !bounded_name(&rule.below) {
                    return Err(context.fail(
                        &at,
                        "ITEM_ZONE",
                        "A zone rule names a generated feature and a bounded zone name.",
                    ));
                }
                let (length, depth, scale, id) = zone_values(
                    context,
                    &at,
                    [
                        Some(&rule.length),
                        rule.depth.as_ref(),
                        rule.scale.as_ref(),
                        Some(&rule.seed),
                    ],
                )?;
                zones.push(ZoneRule {
                    name,
                    below: rule.below.clone(),
                    face: rule.face.clone(),
                    length,
                    depth,
                    u0: context.scalar(&rule.u0, &format!("{at}/u0"))?,
                    scale,
                    id,
                    until: rule.until == super::ZoneStop::Openings,
                    clip: rule.clip == super::ZoneStop::Openings,
                });
            }
            let mut hash = Sha256::new();
            hash.update(b"itemspec-part-geometry-v1\0");
            hash.update(
                serde_json::to_vec(&(id, &shape, placement.0, &part.slots))
                    .map_err(|e| context.fail(&pointer, "ITEM_JSON", e.to_string()))?,
            );
            // Zones are part of the part's features; a part without any keeps its key.
            if !zones.is_empty() {
                hash.update(
                    serde_json::to_vec(&zones)
                        .map_err(|e| context.fail(&pointer, "ITEM_JSON", e.to_string()))?,
                );
            }
            parts.push(PreparedPart {
                id: copy.id.clone(),
                key: format!("{:x}", hash.finalize()),
                shape,
                placement,
                slots: part.slots.clone(),
                pointer: generator,
                zones,
            });
        }
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
            PartShape::Box { size, .. } => decal::Surface::Box(*size),
            PartShape::Route(route) => decal::Surface::Route(route),
            PartShape::Pipe { .. }
            | PartShape::Duct { .. }
            | PartShape::Prism { .. }
            | PartShape::Arc(_)
            | PartShape::Item { .. }
            | PartShape::Native(_) => {
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

/// Resolves the authored features, in item definition coordinates. A feature's `part` may
/// name a copy by template (`wall-{j}`); one whose part was not emitted (dropped by its
/// `when`) is dropped too, so a feature can follow a conditional part.
pub(super) fn authored(
    context: &Context<'_>,
    parts: &[PreparedPart],
) -> Result<Vec<Feature>, ItemDiagnostic> {
    let definitions = &context.definition.features;
    if definitions.len() > 64 {
        return Err(context.fail(
            "/features",
            "ITEM_LIMIT",
            "An item has at most 64 authored features.",
        ));
    }
    let mut result = Vec::new();
    for (index, definition) in definitions.iter().enumerate() {
        let pointer = format!("/features/{index}");
        if !bounded_name(&definition.name)
            || !bounded_name(&definition.face)
            || definition.points.is_empty()
            || definition.points.len() > 128
        {
            return Err(context.fail(
                &pointer,
                "ITEM_FEATURE",
                "A feature has a bounded name and face and 1 to 128 points.",
            ));
        }
        let normal = unit(definition.normal)
            .map_err(|e| context.fail(&format!("{pointer}/normal"), "ITEM_FEATURE", e))?;
        let copies = match &definition.repeat {
            None => vec![context.derived.clone()],
            Some(axis) => {
                let count = context.scalar(&axis.count, &format!("{pointer}/repeat/count"))?;
                if count.fract() != 0. || !(0. ..=256.).contains(&count) {
                    return Err(context.fail(
                        &format!("{pointer}/repeat"),
                        "ITEM_REPEAT",
                        "A feature repeats from 0 to 256 times.",
                    ));
                }
                (0..count as usize)
                    .map(|copy| {
                        let mut derived = context.derived.clone();
                        derived.insert(axis.name.clone(), copy as f64);
                        derived.insert(format!("{}_count", axis.name), count);
                        derived
                    })
                    .collect()
            }
        };
        for derived in copies {
            let copy = Context {
                definition: context.definition,
                parameters: context.parameters,
                derived: &derived,
            };
            if let Some(condition) = &definition.when {
                if copy.scalar(condition, &format!("{pointer}/when"))? <= 0. {
                    continue;
                }
            }
            let part = compose::template(
                context.definition,
                &definition.part,
                &derived,
                &format!("{pointer}/part"),
            )?;
            if !parts.iter().any(|known| known.id == part) {
                continue;
            }
            let mut points = Vec::with_capacity(definition.points.len());
            for (at, point) in definition.points.iter().enumerate() {
                let mut resolved = [0.; 3];
                for axis in 0..3 {
                    resolved[axis] =
                        copy.scalar(&point[axis], &format!("{pointer}/points/{at}/{axis}"))?;
                }
                points.push(resolved);
            }
            // A wider reach is bounded: receivers further than `within` are not given it.
            let within = match (&definition.reach, &definition.within) {
                (super::Reach::Part, None) => 0.,
                (super::Reach::Part, Some(_)) => {
                    return Err(context.fail(
                        &format!("{pointer}/within"),
                        "ITEM_FEATURE",
                        "Only a feature with reach item or scene has a within distance.",
                    ));
                }
                (_, None) => super::REACH_WITHIN,
                (_, Some(within)) => {
                    let within = copy.scalar(within, &format!("{pointer}/within"))?;
                    if !(within > 0. && within <= super::MAX_REACH_WITHIN) {
                        return Err(context.fail(
                            &format!("{pointer}/within"),
                            "ITEM_FEATURE",
                            "A feature reaches receivers within more than 0 and at most 50 meters.",
                        ));
                    }
                    within
                }
            };
            let zone = match &definition.zone {
                None => None,
                Some(zone) => {
                    let at = format!("{pointer}/zone");
                    let (length, depth, scale, id) = zone_values(
                        &copy,
                        &at,
                        [
                            Some(&zone.length),
                            zone.depth.as_ref(),
                            zone.scale.as_ref(),
                            Some(&zone.seed),
                        ],
                    )?;
                    let down = unit(zone.down)
                        .map_err(|e| context.fail(&format!("{at}/down"), "ITEM_ZONE", e))?;
                    Some(crate::materialspec::program::Zone {
                        down,
                        length,
                        depth,
                        u0: copy.scalar(&zone.u0, &format!("{at}/u0"))?,
                        id,
                        scale,
                        stops: Vec::new(),
                        holes: Vec::new(),
                    })
                }
            };
            result.push(Feature {
                name: definition.name.clone(),
                part,
                source: Some(AUTHORED.into()),
                face: definition.face.clone(),
                points,
                closed: definition.closed,
                normal,
                width: copy.scalar(&definition.width, &format!("{pointer}/width"))?,
                reach: definition.reach,
                within,
                zone,
            });
            if result.len() > 4096 {
                return Err(context.fail(
                    "/features",
                    "ITEM_LIMIT",
                    "An item has at most 4096 authored features, counting copies.",
                ));
            }
        }
    }
    Ok(result)
}

/// What building one part gives: its meshes, and any materials and images a sub-item brings.
#[derive(Default)]
struct Built {
    meshes: Vec<ItemMesh>,
    materials: BTreeMap<String, ImportedMaterial>,
    images: Vec<ItemImage>,
    /// A sub-item's features, in part coordinates, given to this part.
    features: Vec<Feature>,
}

/// The source an authored feature carries at its own level; a sub-item's authored features
/// become `authored-sub` when adopted, so a kept part's copy never duplicates the parent's.
pub(super) const AUTHORED: &str = "authored";

impl PreparedPart {
    /// Renames generated meshes into this part and maps their slots, dropping unmapped ones.
    fn adopt(&self, meshes: Vec<ItemMesh>) -> Vec<ItemMesh> {
        meshes
            .into_iter()
            .filter_map(|mut mesh| {
                mesh.material_slot = self.slot(&mesh.material_slot)?;
                mesh.id = format!("{}/{}", self.id, mesh.id);
                Some(mesh)
            })
            .collect()
    }
    fn build(&self, cancelled: &dyn Fn() -> bool) -> Result<Built, String> {
        match &self.shape {
            PartShape::Item { prepared, .. } => {
                let item = prepared.0.build(cancelled).map_err(|e| e.to_string())?;
                let meshes = item.flattened().map_err(|e| e.to_string())?;
                let mut materials = BTreeMap::new();
                for (slot, material) in &item.geometry.imported_materials {
                    if let Some(target) = self.slot(slot) {
                        materials.entry(target).or_insert_with(|| material.clone());
                    }
                }
                let features = item
                    .geometry
                    .features
                    .iter()
                    .map(|feature| {
                        let mut feature = feature.clone();
                        feature.part = self.id.clone();
                        if feature.source.as_deref() == Some(AUTHORED) {
                            feature.source = Some("authored-sub".into());
                        }
                        feature
                    })
                    .collect();
                Ok(Built {
                    meshes: self.adopt(meshes),
                    materials,
                    images: item.geometry.images.clone(),
                    features,
                })
            }
            PartShape::Native(native) => {
                let output = native
                    .generators
                    .0
                    .generate(&native.name, &native.parameters, cancelled)
                    .map_err(|e| format!("{}: {e}", native.name))?;
                // A generator's meshes name each vertex once, however it emitted them.
                let mut meshes = output.meshes;
                for mesh in &mut meshes {
                    mesh.share_vertices();
                }
                Ok(Built {
                    meshes: self.adopt(meshes),
                    ..Built::default()
                })
            }
            _ => Ok(Built {
                meshes: self.meshes(cancelled)?,
                ..Built::default()
            }),
        }
    }
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
            PartShape::Box { size, bevel, .. } if bevel.none() => prism(
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
            PartShape::Box {
                size,
                bevel,
                along: Some(axis @ (Axis::X | Axis::Y)),
            } => surfaces::bar(
                &mut surfaces,
                *size,
                *axis == Axis::X,
                bevel.radius,
                bevel.segments,
            )?,
            PartShape::Box { size, bevel, along } => {
                let frame = Frame {
                    origin: [0.; 3],
                    x: [1., 0., 0.],
                    y: [0., 1., 0.],
                };
                let outline = Outline::rounded_rectangle(
                    [0., 0.],
                    [size[0], size[1]],
                    bevel.radius,
                    bevel.segments,
                );
                let groups = ["side", "bottom", "top"];
                if along.is_some() {
                    // Upright edges only: the rounded outline swept up, with flat caps.
                    prism(&mut surfaces, groups, &frame, &outline, None, [0., size[2]])?
                } else {
                    beveled_prism(
                        &mut surfaces,
                        groups,
                        &frame,
                        &outline,
                        [0., size[2]],
                        bevel.radius,
                        bevel.segments,
                    )?
                }
            }
            PartShape::Panel(panel) => panel.build(&mut surfaces)?,
            PartShape::Route(route) => route.build(&mut surfaces)?,
            PartShape::Arc(arc) => arc.build(&mut surfaces)?,
            PartShape::Prism {
                outline,
                height,
                bevel,
            } => beveled_prism(
                &mut surfaces,
                ["side", "bottom", "top"],
                &Frame {
                    origin: [0.; 3],
                    x: [1., 0., 0.],
                    y: [0., 1., 0.],
                },
                &Outline::flat(outline.clone()),
                [0., *height],
                bevel.radius,
                bevel.segments,
            )?,
            PartShape::Item { .. } | PartShape::Native(_) => {
                return Err("Sub-item and native parts are built by their owner.".into());
            }
        }
        Ok(surfaces.into_meshes(&self.id, &self.slots))
    }

    /// Where the part sits in its item: part coordinates to item definition coordinates.
    pub(super) fn placement(&self) -> Matrix {
        self.placement
    }
    /// The part's own features and pattern anchor, in part coordinates. A sub-item part's
    /// anchor is its sub-item's first part's, so a pattern measured from it continues across the
    /// parts that place their sub-items at matching offsets (a building's storeys and sides).
    pub(super) fn features(&self) -> (Vec<Feature>, PatternAnchor) {
        match &self.shape {
            PartShape::Panel(panel) => panel.features(&self.id),
            PartShape::Route(route) => route.features(&self.id),
            PartShape::Arc(arc) => arc.features(&self.id),
            PartShape::Item { prepared, .. } if prepared.0.first_anchor().is_some() => {
                let mut anchor = prepared.0.first_anchor().unwrap();
                anchor.part = self.id.clone();
                (vec![], anchor)
            }
            // Straight generators and boxes report an anchor at their origin and no features;
            // a native generator may report features of its own.
            _ => (
                match &self.shape {
                    PartShape::Native(native) => native
                        .generators
                        .0
                        .features(&native.name, &native.parameters)
                        .into_iter()
                        .map(|mut feature| {
                            feature.part = self.id.clone();
                            feature
                        })
                        .collect(),
                    _ => vec![],
                },
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
        let mut adopted = Vec::new();
        let meshes = match kept {
            Some(meshes) => {
                // A kept sub-item's features are the ones it had, already in item coordinates.
                if let Some(previous) = previous.filter(|_| part.sub_item().is_some()) {
                    geometry.features.extend(
                        previous
                            .features
                            .iter()
                            .filter(|f| f.part == part.id && f.source.as_deref() != Some(AUTHORED))
                            .cloned(),
                    );
                }
                meshes
            }
            None => {
                built.push(part.id.clone());
                let result = part.build(cancelled).map_err(failure)?;
                adopted = result.features;
                for (slot, material) in result.materials {
                    geometry.imported_materials.entry(slot).or_insert(material);
                }
                for image in result.images {
                    if !geometry.images.iter().any(|known| known.id == image.id) {
                        geometry.images.push(image);
                    }
                }
                result.meshes
            }
        };
        let first = geometry.meshes.len();
        let indices: Vec<usize> = (first..first + meshes.len()).collect();
        for mesh in meshes {
            geometry
                .imported_materials
                .entry(mesh.material_slot.clone())
                .or_insert_with(|| {
                    // A kept sub-item mesh keeps the material it was imported with.
                    if let Some(material) = previous
                        .and_then(|previous| previous.imported_materials.get(&mesh.material_slot))
                    {
                        return material.clone();
                    }
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
        let features = part.zoned(features);
        let surface = matches!(
            part.shape,
            PartShape::Panel(_) | PartShape::Arc(_) | PartShape::Native(_)
        );
        // A sub-item's features are surface features where they came from a panel; their
        // normals follow the inverse transpose as a panel's do (a rigid placement keeps both).
        for (mut feature, surface) in features
            .into_iter()
            .map(|f| (f, surface))
            .chain(adopted.into_iter().map(|f| (f, true)))
        {
            for point in &mut feature.points {
                *point = matrix.point(*point);
            }
            feature.normal = if surface {
                covector(feature.normal)
            } else {
                unit(matrix.direction(feature.normal))
            }
            .map_err(failure)?;
            feature.place_zone(matrix).map_err(failure)?;
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

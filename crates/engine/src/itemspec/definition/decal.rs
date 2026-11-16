//! Decals: an image placed on a part's face by the shared placement rule, delivered as a
//! render-only sheet that stands 2 mm off the surface (plus 0.6 mm per layer). The sheet is
//! flat on panel and box faces and on rectangular sections, and wrapped on round ones.
//!
//! A decal must fit: inside its face with a 2 cm margin and clear of openings, or on one
//! straight run of a route and clear of its fittings. Its size is fixed; its position is
//! re-evaluated from its rule whenever the part is resized.
use super::{
    ItemDecal, ItemDiagnostic,
    panel::{self, Panel},
    parts::Context,
    route::{self, Route, Section},
    schema::{DecalDefinition, Rule},
};
use superspec_geometry::{
    Matrix, V3,
    math::{add, cos, scale, sin, unit},
};

/// At most this many decals on one item.
pub(super) const MAXIMUM: usize = 64;
/// Distance of the first layer from the surface, and of each further layer from the last.
pub(super) const OFFSET: f64 = 0.002;
pub(super) const LAYER: f64 = 0.0006;
/// A decal stays this far inside its face and away from openings and fittings.
pub(super) const MARGIN: f64 = 0.02;

/// What a decal can sit on.
pub(super) enum Surface<'a> {
    Panel(&'a Panel),
    /// A box spanning from its origin to this size; its `front` is the y = 0 side.
    Box(V3),
    Route(&'a Route),
}

/// A sheet in part coordinates.
struct Sheet {
    face: String,
    positions: Vec<V3>,
    normals: Vec<V3>,
    uvs: Vec<[f64; 2]>,
}

fn fit(context: &Context<'_>, pointer: &str, message: String) -> ItemDiagnostic {
    context.fail(pointer, "ITEM_DECAL_FIT", message)
}

/// Resolves one decal on its part. `matrix` takes part coordinates to the item's.
pub(super) fn resolve(
    context: &Context<'_>,
    definition: &DecalDefinition,
    surface: &Surface<'_>,
    matrix: Matrix,
    pointer: &str,
) -> Result<ItemDecal, ItemDiagnostic> {
    let mut size = [0.; 2];
    for (axis, value) in size.iter_mut().enumerate() {
        *value = context.dimension(&definition.size[axis], &format!("{pointer}/size/{axis}"))?;
    }
    if definition.layer > 16 {
        return Err(context.fail(
            &format!("{pointer}/layer"),
            "ITEM_DECAL",
            "A decal layer is at most 16.",
        ));
    }
    if definition.image.is_empty() || definition.image.len() > 4096 {
        return Err(context.fail(
            &format!("{pointer}/image"),
            "ITEM_DECAL",
            "A decal names its image asset.",
        ));
    }
    let offset = OFFSET + LAYER * f64::from(definition.layer);
    let sheet = match surface {
        Surface::Panel(panel) => {
            let face = definition.on.face.as_deref().unwrap_or("front");
            if !matches!(face, "front" | "back") {
                return Err(context.fail(
                    &format!("{pointer}/on/face"),
                    "ITEM_DECAL",
                    "A decal sits on a panel's front or back face.",
                ));
            }
            let [width, height, thickness] = panel.size();
            let cuts = panel.cuts(face == "back");
            flat(
                context,
                definition,
                [width, height],
                &cuts,
                size,
                if face == "front" {
                    -offset
                } else {
                    thickness + offset
                },
                face,
                pointer,
            )?
        }
        Surface::Box(extent) => {
            if definition.on.face.as_deref().unwrap_or("front") != "front" {
                return Err(context.fail(
                    &format!("{pointer}/on/face"),
                    "ITEM_DECAL",
                    "A decal sits on a box's front face (its y = 0 side).",
                ));
            }
            flat(
                context,
                definition,
                [extent[0], extent[2]],
                &[],
                size,
                -offset,
                "front",
                pointer,
            )?
        }
        Surface::Route(route) => wrapped(context, definition, route, size, offset, pointer)?,
    };
    let inverse = matrix
        .inverse()
        .map_err(|error| context.fail(pointer, "ITEM_DECAL", error))?;
    let mut normals = Vec::with_capacity(sheet.normals.len());
    for normal in &sheet.normals {
        // Surface normals follow the inverse transpose.
        normals.push(
            unit([
                superspec_geometry::math::dot(inverse.0[0], *normal),
                superspec_geometry::math::dot(inverse.0[1], *normal),
                superspec_geometry::math::dot(inverse.0[2], *normal),
            ])
            .map_err(|error| context.fail(pointer, "ITEM_DECAL", error))?,
        );
    }
    let positions: Vec<V3> = sheet.positions.iter().map(|p| matrix.point(*p)).collect();
    // Columns of two vertices (bottom, top) from left to right.
    let columns = positions.len() / 2;
    let mut indices = Vec::with_capacity((columns - 1) * 6);
    // Seen from outside the columns must run left to right. Where they run the other way (a
    // section frame of the other hand, or a mirroring placement) the image and the winding
    // are both turned, so the sheet faces out and reads the right way round.
    let facing = superspec_geometry::math::dot(
        superspec_geometry::math::cross(
            superspec_geometry::math::sub(positions[2], positions[0]),
            superspec_geometry::math::sub(positions[1], positions[0]),
        ),
        normals[0],
    );
    let mirrored = facing < 0.;
    let mut sheet = sheet;
    if mirrored {
        for uv in &mut sheet.uvs {
            uv[0] = 1. - uv[0];
        }
    }
    for column in 0..columns - 1 {
        let [a, b, c, d] =
            [column * 2, column * 2 + 2, column * 2 + 3, column * 2 + 1].map(|i| i as u32);
        indices.extend(if mirrored {
            [a, c, b, a, d, c]
        } else {
            [a, b, c, a, c, d]
        });
    }
    let corners = [
        positions[0],
        positions[positions.len() - 2],
        positions[positions.len() - 1],
        positions[1],
    ];
    Ok(ItemDecal {
        id: definition.id.clone(),
        part: definition.on.part.clone(),
        face: sheet.face,
        image: definition.image.clone(),
        layer: definition.layer,
        size,
        offset,
        positions,
        normals,
        uvs: sheet.uvs,
        indices,
        corners,
    })
}

/// A rectangle on a face in front-face coordinates (x across, z up); `y` is the sheet's plane.
#[allow(clippy::too_many_arguments)]
fn flat(
    context: &Context<'_>,
    definition: &DecalDefinition,
    [width, height]: [f64; 2],
    cuts: &[(&str, [f64; 4])],
    size: [f64; 2],
    y: f64,
    face: &str,
    pointer: &str,
) -> Result<Sheet, ItemDiagnostic> {
    let (Some(u), Some(v)) = (&definition.u, &definition.v) else {
        return Err(context.fail(
            pointer,
            "ITEM_DECAL",
            "A decal on a face is placed by u and v rules.",
        ));
    };
    if definition.along.is_some() || definition.around.is_some() {
        return Err(context.fail(
            pointer,
            "ITEM_DECAL",
            "along and around place a decal on a route; a face uses u and v.",
        ));
    }
    let x = place(context, u, width, size[0], &format!("{pointer}/u"))?;
    let z = place(context, v, height, size[1], &format!("{pointer}/v"))?;
    if x < MARGIN - 1e-9
        || z < MARGIN - 1e-9
        || x + size[0] > width - MARGIN + 1e-9
        || z + size[1] > height - MARGIN + 1e-9
    {
        return Err(fit(
            context,
            pointer,
            format!(
                "Decal {} spans u {:.4} to {:.4} and v {:.4} to {:.4}; it must stay {MARGIN} m inside the {width} by {height} m face.",
                definition.id,
                x,
                x + size[0],
                z,
                z + size[1]
            ),
        ));
    }
    // Seen from outside, the back face runs the other way across; cuts are in front coordinates.
    let back = face == "back";
    let near = if back { width - x - size[0] } else { x };
    if let Some((opening, _)) = cuts.iter().find(|(_, [ox, oz, ow, oh])| {
        near < ox + ow + MARGIN - 1e-9
            && *ox < near + size[0] + MARGIN - 1e-9
            && z < oz + oh + MARGIN - 1e-9
            && *oz < z + size[1] + MARGIN - 1e-9
    }) {
        return Err(fit(
            context,
            pointer,
            format!(
                "Decal {} lands in opening {opening} or within {MARGIN} m of it.",
                definition.id
            ),
        ));
    }
    let across = |along: f64| if back { width - along } else { along };
    let normal = [0., if back { 1. } else { -1. }, 0.];
    let mut sheet = Sheet {
        face: face.to_owned(),
        positions: vec![],
        normals: vec![normal; 4],
        uvs: vec![],
    };
    for (along, s) in [(x, 0.), (x + size[0], 1.)] {
        for (up, t) in [(z, 0.), (z + size[1], 1.)] {
            sheet.positions.push([across(along), y, up]);
            sheet.uvs.push([s, t]);
        }
    }
    Ok(sheet)
}

/// The shared placement rule for one axis of a face; `every` and `point` belong to fittings.
fn place(
    context: &Context<'_>,
    rule: &Rule,
    available: f64,
    size: f64,
    pointer: &str,
) -> Result<f64, ItemDiagnostic> {
    panel::place(context, rule, available, size, pointer).map_err(|mut error| {
        if error.code == "ITEM_OPENING" {
            error.code = "ITEM_DECAL".into();
            error.message = error.message.replace("Openings use", "Decals use");
        }
        error
    })
}

/// A sheet on a straight run of a route: wrapped on a round section, flat on a side of a
/// rectangular one.
fn wrapped(
    context: &Context<'_>,
    definition: &DecalDefinition,
    route: &Route,
    size: [f64; 2],
    offset: f64,
    pointer: &str,
) -> Result<Sheet, ItemDiagnostic> {
    let Some(along) = &definition.along else {
        return Err(context.fail(
            pointer,
            "ITEM_DECAL",
            "A decal on a route is placed by an along rule.",
        ));
    };
    if definition.u.is_some() || definition.v.is_some() {
        return Err(context.fail(
            pointer,
            "ITEM_DECAL",
            "u and v place a decal on a face; a route uses along and around.",
        ));
    }
    let path = route.path();
    let stations = route::stations(context, along, path, &format!("{pointer}/along")).map_err(
        |mut error| {
            if error.code == "ITEM_FITTING" {
                error.code = "ITEM_DECAL".into();
            }
            error
        },
    )?;
    let [station] = stations[..] else {
        return Err(context.fail(
            &format!("{pointer}/along"),
            "ITEM_DECAL",
            "A decal is placed once; every is for fittings.",
        ));
    };
    let around = match &definition.around {
        Some(around) => context.scalar(around, &format!("{pointer}/around"))?,
        None => 0.,
    };
    let (from, to) = (station - size[1] / 2., station + size[1] / 2.);
    if from < MARGIN - 1e-9 || to > path.length - MARGIN + 1e-9 || !path.straight(from, to) {
        return Err(fit(
            context,
            pointer,
            format!(
                "Decal {} spans {from:.4} to {to:.4} m along the route; it must lie on one straight run, {MARGIN} m inside the route's {:.4} m.",
                definition.id, path.length
            ),
        ));
    }
    if let Some((fitting, _, _)) = route
        .fitting_spans()
        .into_iter()
        .find(|(_, low, high)| from < high + MARGIN - 1e-9 && *low < to + MARGIN - 1e-9)
    {
        return Err(fit(
            context,
            pointer,
            format!(
                "Decal {} lands on fitting {fitting} or within {MARGIN} m of it.",
                definition.id
            ),
        ));
    }
    let (low, high) = (path.at(from), path.at(to));
    let angle = around.to_radians();
    let mut sheet = Sheet {
        face: "body".into(),
        positions: vec![],
        normals: vec![],
        uvs: vec![],
    };
    // A direction in the section plane as a vector of the part.
    let radial = |theta: f64| add(scale(low.a, cos(theta)), scale(low.b, sin(theta)));
    match route.section() {
        Section::Round { radius, segments } => {
            let reach = radius + offset;
            let span = size[0] / reach;
            if span > std::f64::consts::PI {
                return Err(fit(
                    context,
                    pointer,
                    format!(
                        "Decal {} is {} m wide; it may cover at most half of the pipe's circumference.",
                        definition.id, size[0]
                    ),
                ));
            }
            let steps =
                ((span / (std::f64::consts::TAU / *segments as f64)).ceil() as usize).max(2);
            for step in 0..=steps {
                let t = step as f64 / steps as f64;
                let normal = radial(angle + span * (t - 0.5));
                for (ring, v) in [(&low, 0.), (&high, 1.)] {
                    sheet
                        .positions
                        .push(add(ring.position, scale(normal, reach)));
                    sheet.normals.push(normal);
                    sheet.uvs.push([t, v]);
                }
            }
        }
        Section::Rect { width, height } => {
            // The side the direction points at most.
            let (c, s) = (cos(angle), sin(angle));
            let (normal, tangent, side) = if c.abs() * height >= s.abs() * width {
                ([c.signum(), 0.], [0., c.signum()], *height)
            } else {
                ([0., s.signum()], [-s.signum(), 0.], *width)
            };
            if size[0] > side - 2. * MARGIN + 1e-9 {
                return Err(fit(
                    context,
                    pointer,
                    format!(
                        "Decal {} is {} m wide; the duct's side is {side} m and it must stay {MARGIN} m inside it.",
                        definition.id, size[0]
                    ),
                ));
            }
            let reach = route.section().support(normal) + offset;
            let to_part = |v: [f64; 2]| add(scale(low.a, v[0]), scale(low.b, v[1]));
            let (normal, tangent) = (to_part(normal), to_part(tangent));
            for (across, t) in [(-size[0] / 2., 0.), (size[0] / 2., 1.)] {
                for (ring, v) in [(&low, 0.), (&high, 1.)] {
                    sheet.positions.push(add(
                        ring.position,
                        add(scale(normal, reach), scale(tangent, across)),
                    ));
                    sheet.normals.push(normal);
                    sheet.uvs.push([t, v]);
                }
            }
        }
    }
    Ok(sheet)
}

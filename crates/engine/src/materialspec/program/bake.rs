use super::{syntax::Mark, *};
use superspec_geometry::math::{cross, dot, length, scale};

pub(super) fn bake(
    program: &Program,
    request: &BakeRequest,
    inputs: &Inputs,
) -> Result<Baked, ProgramDiagnostic> {
    bake_cancellable(program, request, inputs, &|| false)
}
pub(super) fn bake_cancellable(
    program: &Program,
    request: &BakeRequest,
    inputs: &Inputs,
    cancelled: &dyn Fn() -> bool,
) -> Result<Baked, ProgramDiagnostic> {
    let root = Mark::root(&program.sources.entry);
    let error = |code, message: &str| program.diagnostic(code, message, &root);
    let count = request
        .width
        .checked_mul(request.height)
        .filter(|n| *n > 0 && *n <= 262_144)
        .ok_or_else(|| error("MS_LIMIT", "Bake dimensions must contain 1–262144 pixels."))?;
    if request.width > 4096 || request.height > 4096 {
        return Err(error("MS_LIMIT", "A bake side cannot exceed 4096 pixels."));
    }
    if let Some(name) = &request.inspection {
        if !program.metadata.fields.iter().any(|f| f.name == *name)
            && !matches!(
                name.as_str(),
                "base_color"
                    | "roughness"
                    | "metalness"
                    | "height"
                    | "emissive"
                    | "opacity"
                    | "normal"
                    | "receiver_normal"
            )
        {
            return Err(error(
                "MS_INSPECTION",
                "Unknown intermediate field or output channel.",
            ));
        }
    }
    let d = &request.domain;
    if d.origin
        .iter()
        .chain(&d.u)
        .chain(&d.v)
        .chain(&d.world_from_object)
        .any(|v| !v.is_finite() || v.abs() > 1e12)
        || [3, 7, 11].into_iter().any(|i| d.world_from_object[i] != 0.)
        || d.world_from_object[15] != 1.
    {
        return Err(error(
            "MS_INPUT",
            "Bake domain requires finite physical axes and an affine object-to-world matrix.",
        ));
    }
    let lu = length(d.u);
    let lv = length(d.v);
    if lu < 1e-9 || lv < 1e-9 {
        return Err(error(
            "MS_INPUT",
            "Bake axes must have nonzero lengths in meters.",
        ));
    }
    let e1 = scale(d.u, 1. / lu);
    let e2 = scale(d.v, 1. / lv);
    let cosine = dot(e1, e2);
    let sine = length(cross(e1, e2));
    if sine < 1e-6 {
        return Err(error("MS_INPUT", "Bake axes must not be parallel."));
    }
    let world_u = transform(d.u, &d.world_from_object, 0.);
    let world_v = transform(d.v, &d.world_from_object, 0.);
    let normal = cross(world_u, world_v);
    let normal_length = length(normal);
    if normal_length < 1e-12 {
        return Err(error(
            "MS_INPUT",
            "The world transform collapses the bake surface.",
        ));
    }
    let normal = scale(normal, 1. / normal_length);
    // World-space axes of the encoded tangent space: u, then v made perpendicular to it.
    let tangent = scale(world_u, 1. / length(world_u));
    let along = dot(world_v, tangent);
    let bitangent = {
        let v = std::array::from_fn(|i| world_v[i] - tangent[i] * along);
        scale(v, 1. / length(v))
    };
    let footprint = (lu / request.width as f64).max(lv / request.height as f64);
    // The bake rectangle's four world-space corners bound every sample.
    let (bounds_min, bounds_max) = {
        let corners = [[0., 0.], [1., 0.], [0., 1.], [1., 1.]].map(|[u, v]| {
            let object: [f64; 3] = std::array::from_fn(|i| d.origin[i] + d.u[i] * u + d.v[i] * v);
            transform(object, &d.world_from_object, 1.)
        });
        let fold = |pick: fn(f64, f64) -> f64, start: f64| {
            std::array::from_fn::<f64, 3, _>(|i| corners.iter().map(|c| c[i]).fold(start, pick))
        };
        (
            fold(f64::min, f64::INFINITY),
            fold(f64::max, f64::NEG_INFINITY),
        )
    };
    let make_sample = |u: f64, v: f64| {
        let object = std::array::from_fn(|i| d.origin[i] + d.u[i] * u + d.v[i] * v);
        Sample {
            uv: [u, v],
            surface: [u * lu, v * lv],
            object,
            world: transform(object, &d.world_from_object, 1.),
            normal,
            footprint,
            uv_footprint: Some((1. / request.width as f64).max(1. / request.height as f64)),
            bounds_min,
            bounds_max,
            ..Sample::default()
        }
    };
    let mut evaluator = program.evaluator(inputs)?;
    let mut result = Baked {
        width: request.width,
        height: request.height,
        base_color: Vec::with_capacity(count * 4),
        normal: Vec::with_capacity(count * 4),
        properties: Vec::with_capacity(count * 4),
        emissive: Vec::with_capacity(count * 4),
        emissive_strength: 1.,
        height_meters: Vec::with_capacity(count),
        inspection: request
            .inspection
            .as_ref()
            .map(|_| Vec::with_capacity(count * 4)),
    };
    let mut inspected = Vec::new();
    let mut emission = Vec::with_capacity(count);
    // Fields are computed only when inspected; channel values are the same either way.
    let inspect = request.inspection.as_deref();
    for y in 0..request.height {
        if cancelled() {
            return Err(error("MS_CANCELLED", "Material bake cancelled."));
        }
        // Each row of texels has its own operation allowance; every sample keeps its step budget.
        evaluator.replenish(64_000_000);
        for x in 0..request.width {
            let u = (x as f64 + 0.5) / request.width as f64;
            let v = 1. - (y as f64 + 0.5) / request.height as f64;
            let sample = make_sample(u, v);
            let output = evaluator.evaluate_channels(&sample, inspect)?;
            for color in output.base_color {
                result.base_color.push(
                    super::super::surface::color_byte(color)
                        .map_err(|e| error("MS_NONFINITE", &e))?,
                );
            }
            result.base_color.push(byte(output.opacity));
            result.emissive_strength = output
                .emissive
                .into_iter()
                .fold(result.emissive_strength, f64::max);
            emission.push(output.emissive);
            result.properties.extend_from_slice(&[
                255,
                byte(output.roughness),
                byte(output.metalness),
                255,
            ]);
            result.height_meters.push(output.height);
            let du = 0.25 / request.width as f64;
            let dv = 0.25 / request.height as f64;
            let mut height = |u, v| evaluator.evaluate_height(&make_sample(u, v));
            let dx = (height(u + du, v)? - height(u - du, v)?) / (2. * du * lu);
            let along_v = (height(u, v + dv)? - height(u, v - dv)?) / (2. * dv * lv);
            let dy = (along_v - dx * cosine) / sine;
            let relief = [-dx, -dy, 1.];
            let magnitude = length(relief);
            if !magnitude.is_finite() {
                return Err(error(
                    "MS_NONFINITE",
                    "Height derivatives exceed the finite normal range.",
                ));
            }
            let mut normal = relief.map(|v| v / magnitude);
            if let Some(adjusted) = output.receiver_normal {
                normal = super::orient_relief(normal, tangent, bitangent, sample.normal, adjusted)
                    .map_err(|message| error("MS_OUTPUT", message))?;
            }
            if let Some(offset) = output.normal_offset {
                normal = super::bend_shaded(normal, tangent, bitangent, sample.normal, offset)
                    .map_err(|message| error("MS_OUTPUT", message))?;
            }
            result.normal.extend(normal.map(|v| byte(v * 0.5 + 0.5)));
            result.normal.push(255);
            if let Some(name) = &request.inspection {
                let value = match name.as_str() {
                    "base_color" => FieldValue::Vec3(output.base_color),
                    "roughness" => FieldValue::Scalar(output.roughness),
                    "metalness" => FieldValue::Scalar(output.metalness),
                    "height" => FieldValue::Scalar(output.height),
                    "emissive" => FieldValue::Vec3(output.emissive),
                    "opacity" => FieldValue::Scalar(output.opacity),
                    "normal" => FieldValue::Vec3(normal.map(|v| v * 0.5 + 0.5)),
                    "receiver_normal" => FieldValue::Vec3(
                        output
                            .receiver_normal
                            .unwrap_or(sample.normal)
                            .map(|v| v * 0.5 + 0.5),
                    ),
                    _ => output.fields[name].clone(),
                };
                inspected.push(value);
            }
        }
    }
    for rgb in emission {
        for color in rgb {
            result.emissive.push(
                super::super::surface::color_byte(color / result.emissive_strength)
                    .map_err(|e| error("MS_NONFINITE", &e))?,
            );
        }
        result.emissive.push(255);
    }
    if let Some(pixels) = &mut result.inspection {
        let scalar_range = inspected
            .iter()
            .filter_map(|v| match v {
                FieldValue::Scalar(v) => Some(*v),
                _ => None,
            })
            .fold([f64::INFINITY, f64::NEG_INFINITY], |[lo, hi], v| {
                [lo.min(v), hi.max(v)]
            });
        for value in inspected {
            let color = match value {
                FieldValue::Scalar(mut v) => {
                    if (scalar_range[0] < 0. || scalar_range[1] > 1.)
                        && scalar_range[1] > scalar_range[0]
                    {
                        v = (v - scalar_range[0]) / (scalar_range[1] - scalar_range[0]);
                    }
                    [v; 3]
                }
                FieldValue::Vec2(v) => [v[0], v[1], 0.],
                FieldValue::Vec3(v) => v,
                FieldValue::Bool(v) => [if v { 1. } else { 0. }; 3],
            };
            pixels.extend(color.map(byte));
            pixels.push(255);
        }
    }
    if cancelled() {
        return Err(error("MS_CANCELLED", "Material bake cancelled."));
    }
    Ok(result)
}
fn byte(v: f64) -> u8 {
    (v.clamp(0., 1.) * 255.).round() as u8
}
fn transform(point: [f64; 3], m: &[f64; 16], w: f64) -> [f64; 3] {
    std::array::from_fn(|i| {
        m[i] * point[0] + m[i + 4] * point[1] + m[i + 8] * point[2] + m[i + 12] * w
    })
}

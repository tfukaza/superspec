use super::{
    Artifact, Diagnostic, Output, Request, Session,
    receiver::{self, Atlas, Chart},
};
use crate::{
    materialspec::program::{self, FieldValue, Inputs, Program},
    runtime::dependencies::{Dependency, Node, Plan},
};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, sync::Arc};
use superspec_geometry::image;
use superspec_model::instances::{IDENTITY, ImageResource, Instance, LocalMesh, Model};
use superspec_types::{
    diagnostic::SourceLocation,
    material::{AlphaMode, LiveArgument, LiveOperation as LiveDescriptor, Material},
};

pub(crate) struct BakedReceiver {
    pub mesh: Arc<LocalMesh>,
    pub material: Material,
    pub images: BTreeMap<String, ImageResource>,
}
fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn node(id: String, implementation: &str, inputs: Vec<u8>, dependencies: Vec<Dependency>) -> Node {
    Node {
        id,
        implementation: implementation.into(),
        inputs,
        dependencies,
        source: None,
    }
}
fn dependency(role: &str, node: impl Into<String>) -> Dependency {
    Dependency {
        role: role.into(),
        node: node.into(),
    }
}
fn json(value: &impl serde::Serialize) -> Vec<u8> {
    serde_json::to_vec(value).expect("finite admitted authoring request")
}

impl Session {
    pub(super) fn material(
        &mut self,
        request: &Request,
        _revision: u64,
        assets: &dyn Fn(&str) -> Result<Arc<[u8]>, String>,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Output, Vec<Diagnostic>> {
        let compiled = self.program(request)?;
        let fail = |message: String| vec![Diagnostic::message(&request.entry, message)];
        let receivers = if request.receivers.is_empty() {
            vec![receiver::swatch(request.domain)]
        } else {
            request.receivers.clone()
        };
        if receivers.len() > 256
            || receivers.iter().map(|r| r.indices.len()).sum::<usize>() > 98_304
        {
            return Err(fail(
                "Selected receivers exceed the authoring triangle allowance.".into(),
            ));
        }
        let mut nodes = Vec::new();
        let mut sources = request.modules.clone();
        sources.insert(request.entry.clone(), request.source.clone());
        let mut source_deps = Vec::new();
        for name in
            std::iter::once(&compiled.metadata().entry).chain(&compiled.metadata().dependencies)
        {
            let id = format!("source/{name}");
            let text = sources
                .get(name)
                .ok_or_else(|| fail(format!("Missing source module {name}.")))?;
            nodes.push(node(
                id.clone(),
                "material-source-v1",
                text.as_bytes().into(),
                vec![],
            ));
            source_deps.push(dependency(name, id));
        }
        nodes.push(node(
            "program".into(),
            "material-language-v1",
            request.entry.as_bytes().into(),
            source_deps,
        ));
        let mut image_bytes = BTreeMap::new();
        for name in compiled
            .metadata()
            .required_inputs
            .iter()
            .filter_map(|input| input.strip_prefix("image:"))
        {
            let bytes = assets(name).map_err(|e| fail(format!("{name}: {e}")))?;
            image_bytes.insert(name.to_owned(), bytes);
        }
        let mut input_deps = vec![dependency("program", "program")];
        for (name, bytes) in &image_bytes {
            let id = format!("image/{name}");
            nodes.push(node(id.clone(), "source-image-v1", bytes.to_vec(), vec![]));
            input_deps.push(dependency(&format!("image/{name}"), id));
        }
        nodes.push(node(
            "inputs".into(),
            "material-inputs-v1",
            json(&(&request.parameters, request.seed, &request.edges)),
            input_deps,
        ));
        let mut targets = vec![];
        for (i, receiver) in receivers.iter().enumerate() {
            let layout = format!("layout/{i}");
            let bake = format!("bake/{i}");
            let mut identity = receiver.clone();
            identity.id.clear();
            identity.object_id.clear();
            nodes.push(node(
                layout.clone(),
                "receiver-atlas-v2",
                json(&(
                    identity,
                    request.resolution,
                    request.mapping,
                    request.domain,
                    request.gravity,
                )),
                vec![],
            ));
            nodes.push(node(
                bake.clone(),
                "material-receiver-v2",
                // Production bakes compress their PNG maps harder: same pixels, different bytes.
                json(&(
                    &request.inspection,
                    request.profile == super::Profile::Production,
                )),
                vec![
                    dependency("mapping", layout),
                    dependency("program", "program"),
                    dependency("inputs", "inputs"),
                ],
            ));
            targets.push(bake);
        }
        let plan = Plan::new(nodes, targets).map_err(|e| fail(e.to_string()))?;
        let mut evaluation_error = None;
        let complete = self
            .runtime
            .run(
                self.build_epoch(),
                &plan,
                cancelled,
                |node, dependencies, stopped| match node.id.as_str() {
                    "program" => Ok((
                        Artifact::Program(compiled.clone()),
                        compiled.retained_bytes(),
                    )),
                    id if id.starts_with("image/") => {
                        // Decoding depends only on the encoded bytes, so parameter edits reuse it.
                        let image = image::decode(&image_bytes[&id["image/".len()..]])?;
                        let bytes = image.rgba.len();
                        Ok((
                            Artifact::Image(Arc::new(program::Image {
                                width: image.width,
                                height: image.height,
                                rgba: image.rgba,
                            })),
                            bytes,
                        ))
                    }
                    "inputs" => {
                        let mut images = BTreeMap::new();
                        let mut count = 0usize;
                        for name in image_bytes.keys() {
                            let Artifact::Image(image) =
                                dependencies[&format!("image/{name}")].as_ref()
                            else {
                                unreachable!()
                            };
                            count += image.rgba.len();
                            if count > 64 * 1024 * 1024 {
                                return Err(
                                    "Decoded material images exceed the authoring allowance."
                                        .into(),
                                );
                            }
                            images.insert(name.clone(), image.as_ref().clone());
                        }
                        Ok((
                            Artifact::Inputs(Arc::new(Inputs {
                                parameters: request.parameters.clone(),
                                seed: request.seed,
                                images,
                                edges: request.edges.clone(),
                                features: Vec::new(),
                                mips: Default::default(),
                            })),
                            // Filtered sampling may add mip levels of at most 3/4 the decoded bytes.
                            count * 2 + json(&request.parameters).len() + 1024,
                        ))
                    }
                    id if id.starts_with("layout/") => {
                        let index = id[7..].parse::<usize>().unwrap();
                        let atlas = Atlas::new(
                            receivers[index].clone(),
                            request.resolution,
                            request.mapping,
                            request.domain,
                            request.gravity,
                            request.profile.max_resolution(),
                        )?;
                        let bytes = atlas.mesh.positions.len() * 88
                            + atlas.mesh.indices.len() * 4
                            + receivers[index].positions.len() * 8 * 3;
                        Ok((Artifact::Receiver(Arc::new(atlas)), bytes))
                    }
                    id if id.starts_with("bake/") => {
                        let Artifact::Program(program) = dependencies["program"].as_ref() else {
                            unreachable!()
                        };
                        let Artifact::Inputs(inputs) = dependencies["inputs"].as_ref() else {
                            unreachable!()
                        };
                        let Artifact::Receiver(atlas) = dependencies["mapping"].as_ref() else {
                            unreachable!()
                        };
                        match bake(
                            program,
                            inputs,
                            atlas,
                            request.inspection.as_deref(),
                            request.profile.threads(),
                            request.profile == super::Profile::Production,
                            stopped,
                        ) {
                            Ok(value) => {
                                let bytes = value
                                    .images
                                    .values()
                                    .map(|image| image.bytes.len())
                                    .sum::<usize>()
                                    + value.mesh.positions.len() * 88;
                                Ok((Artifact::Material(Arc::new(value)), bytes))
                            }
                            Err(error) => {
                                let message = error.message.clone();
                                evaluation_error = Some(error);
                                Err(message)
                            }
                        }
                    }
                    _ => Ok((Artifact::Source, node.inputs.len())),
                },
            )
            .map_err(|e| {
                evaluation_error
                    .clone()
                    .map(|e| vec![e])
                    .unwrap_or_else(|| fail(e))
            })?;
        let mut model = Model {
            id: "studio/material".into(),
            name: "Material preview".into(),
            ..Default::default()
        };
        for (i, receiver) in receivers.iter().enumerate() {
            let Artifact::Material(baked) = complete[&format!("bake/{i}")].as_ref() else {
                unreachable!()
            };
            let mesh = format!(
                "studio/mesh/{}",
                hash(&json(&(
                    &baked.mesh.positions,
                    &baked.mesh.uvs,
                    &baked.mesh.indices
                )))
            );
            model.meshes.insert(mesh.clone(), baked.mesh.clone());
            model.images.extend(baked.images.clone());
            model
                .materials
                .insert(baked.material.id.clone(), baked.material.clone());
            model.instances.push(Instance {
                id: receiver.id.clone(),
                object_id: receiver.object_id.clone(),
                mesh,
                material: Some(baked.material.id.clone()),
                transform: IDENTITY,
                source: SourceLocation {
                    line: 1,
                    column: 1,
                    offset: 0,
                    end: None,
                },
                visible: true,
                collidable: false,
            });
        }
        let mut metadata = serde_json::to_value(compiled.metadata()).unwrap();
        metadata["mapping"] = serde_json::to_value(request.mapping).unwrap();
        Ok(Output { model, metadata })
    }
}

fn byte(value: f64) -> u8 {
    (value.clamp(0., 1.) * 255.).round() as u8
}
/// Quantizes a unit-range value with an offset of up to `amount` half steps either way that
/// varies per texel and channel as integer-hashed white noise. Gentle relief otherwise lands in
/// wide 8-bit plateaus whose edges a glossy highlight shows as contour stripes; white noise has no
/// lattice structure to beat against screen pixels. Callers pass a smaller amount where the value
/// already changes by a step or more per texel, so busy relief stays as compressible as plain
/// rounding. It depends only on the atlas texel, so threading and the target (native or Wasm)
/// never change bytes.
fn dithered(value: f64, texel: [usize; 2], channel: usize, amount: f64) -> u8 {
    let mut h = (texel[0] as u32)
        .wrapping_mul(0x9E37_79B1)
        .wrapping_add((texel[1] as u32).wrapping_mul(0x85EB_CA77))
        .wrapping_add((channel as u32).wrapping_mul(0xC2B2_AE3D));
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^= h >> 12;
    h = h.wrapping_mul(0x297A_2D39);
    h ^= h >> 15;
    let noise = f64::from(h) / 4_294_967_296.;
    (value.clamp(0., 1.) * 255. + (noise - 0.5) * amount)
        .round()
        .clamp(0., 255.) as u8
}
fn srgb(value: f64) -> u8 {
    crate::materialspec::surface::color_byte(value).expect("finite checked material output")
}
fn inspection(
    value: &program::Outputs,
    field: &str,
    normal: [f64; 3],
    receiver: [f64; 3],
) -> Result<[f64; 3], String> {
    Ok(match field {
        "base_color" | "baseColor" => value.base_color,
        "roughness" => [value.roughness; 3],
        "metalness" => [value.metalness; 3],
        "height" => [0.5 + value.height * 50.; 3],
        "opacity" => [value.opacity; 3],
        "normal" => normal.map(|v| v * 0.5 + 0.5),
        "receiver_normal" | "receiverNormal" => value
            .receiver_normal
            .unwrap_or(receiver)
            .map(|v| v * 0.5 + 0.5),
        "emissive" => value.emissive,
        name => match value
            .fields
            .get(name)
            .ok_or_else(|| format!("Unknown inspection field {name}."))?
        {
            FieldValue::Scalar(v) => [*v; 3],
            FieldValue::Bool(v) => [if *v { 1. } else { 0. }; 3],
            FieldValue::Vec2(v) => [v[0], v[1], 0.],
            FieldValue::Vec3(v) => *v,
        },
    })
}

/// Live arguments that never vary become constants; the others are quantized four to a linear
/// RGBA texture on the atlas UVs, decoded as `byte / 255 * scale + offset`.
fn live_operations(
    program: &Program,
    fields: &[&str],
    values: &[Vec<f64>],
    pixels: usize,
    encode: &mut dyn FnMut(Vec<u8>) -> Result<String, Diagnostic>,
) -> Result<Vec<LiveDescriptor>, Diagnostic> {
    let mut arguments: BTreeMap<&str, LiveArgument> = BTreeMap::new();
    let mut varying = Vec::new();
    for (field, samples) in fields.iter().zip(values) {
        let (low, high) = samples
            .iter()
            .filter(|v| !v.is_nan())
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(low, high), v| {
                (low.min(*v), high.max(*v))
            });
        if !low.is_finite() {
            arguments.insert(field, LiveArgument::Constant { value: 0. });
        } else if high - low <= 1e-9 * (1. + high.abs()) {
            arguments.insert(field, LiveArgument::Constant { value: low });
        } else {
            varying.push((*field, samples, low, high));
        }
    }
    for group in varying.chunks(4) {
        let mut rgba = vec![0u8; pixels * 4];
        for (channel, (_, samples, low, high)) in group.iter().enumerate() {
            for (texel, value) in samples.iter().enumerate() {
                if !value.is_nan() {
                    rgba[texel * 4 + channel] =
                        (((value - low) / (high - low)).clamp(0., 1.) * 255.).round() as u8;
                }
            }
        }
        let texture = encode(rgba)?;
        for (channel, (field, _, low, high)) in group.iter().enumerate() {
            arguments.insert(
                field,
                LiveArgument::Texture {
                    texture: texture.clone(),
                    channel: channel as u8,
                    scale: high - low,
                    offset: *low,
                },
            );
        }
    }
    Ok(program
        .metadata()
        .live
        .iter()
        .map(|operation| LiveDescriptor {
            id: operation.id.clone(),
            stage: operation.stage.clone(),
            requires: operation.requires.clone(),
            arguments: operation
                .inputs
                .iter()
                .map(|(argument, field)| (argument.clone(), arguments[field.as_str()].clone()))
                .collect(),
        })
        .collect())
}

/// One texel row of one atlas chart.
struct Row {
    color: Vec<u8>,
    normal: Vec<u8>,
    roughness: Vec<u8>,
    metalness: Vec<u8>,
    emitted: Vec<[f64; 3]>,
    live: Vec<Vec<f64>>,
    blend: bool,
    emits: bool,
    strength: f64,
}

/// Evaluates one atlas row: channels, the relief normal and any live arguments.
fn bake_row(
    evaluator: &mut program::Evaluator<'_>,
    atlas: &Atlas,
    chart: &Chart,
    y: usize,
    inspect: Option<&str>,
    live_fields: &[&str],
    entry: &str,
) -> Result<Row, Diagnostic> {
    let failure = |message: String| Diagnostic::message(entry, message);
    let cell = chart.width;
    let mut row = Row {
        color: vec![0; cell * 4],
        normal: vec![0; cell * 4],
        roughness: vec![0; cell],
        metalness: vec![0; cell],
        emitted: vec![[0.; 3]; cell],
        live: vec![vec![f64::NAN; cell]; live_fields.len()],
        blend: false,
        emits: false,
        strength: 1.,
    };
    // Each row of texels has its own operation allowance; every sample keeps its step budget.
    evaluator.replenish(64_000_000);
    let mut previous: Option<[f64; 3]> = None;
    for x in 0..cell {
        let (triangle, u, v) = atlas.texel(chart, x, y);
        let sample = atlas.sample(triangle, u, v).map_err(&failure)?;
        // Fields are computed only when inspected; channel values are the same either way.
        let value = if live_fields.is_empty() {
            evaluator.evaluate_channels(&sample, inspect)
        } else {
            evaluator.evaluate_channels_with(&sample, live_fields)
        }
        .map_err(Diagnostic::from)?;
        let [step_u, step_v] = atlas.steps(triangle);
        let hu = evaluator
            .evaluate_height(&atlas.sample(triangle, u + step_u, v).map_err(&failure)?)
            .map_err(Diagnostic::from)?;
        let hv = evaluator
            .evaluate_height(&atlas.sample(triangle, u, v + step_v).map_err(&failure)?)
            .map_err(Diagnostic::from)?;
        let mut direction = atlas
            .normal(
                triangle,
                (hu - value.height) / step_u,
                (hv - value.height) / step_v,
            )
            .map_err(&failure)?;
        if let Some(adjusted) = value.receiver_normal {
            let [tangent, bitangent] = atlas.frame(triangle).map_err(&failure)?;
            direction =
                program::orient_relief(direction, tangent, bitangent, sample.normal, adjusted)
                    .map_err(|message| failure(message.into()))?;
        }
        if let Some(offset) = value.normal_offset {
            let [tangent, bitangent] = atlas.frame(triangle).map_err(&failure)?;
            direction = program::bend_shaded(direction, tangent, bitangent, sample.normal, offset)
                .map_err(|message| failure(message.into()))?;
        }
        let rgb = if let Some(field) = inspect {
            inspection(&value, field, direction, sample.normal).map_err(&failure)?
        } else {
            value.base_color
        };
        row.color[x * 4..x * 4 + 3].copy_from_slice(&rgb.map(srgb));
        row.color[x * 4 + 3] = byte(value.opacity);
        let texel = [chart.x + x, chart.y + y];
        for (channel, v) in direction.into_iter().enumerate() {
            // Full dither on plateaus (under a step per texel), none where relief already varies.
            let step = previous.map_or(0., |p: [f64; 3]| (v - p[channel]).abs() * 127.5);
            let amount = (1. - step).clamp(0., 1.);
            row.normal[x * 4 + channel] = dithered(v * 0.5 + 0.5, texel, channel, amount);
        }
        previous = Some(direction);
        row.normal[x * 4 + 3] = 255;
        row.roughness[x] = byte(value.roughness);
        row.metalness[x] = byte(value.metalness);
        row.emitted[x] = value.emissive;
        row.strength = value.emissive.into_iter().fold(row.strength, f64::max);
        row.blend |= value.opacity < 1.;
        row.emits |= value.emissive.iter().any(|v| *v > 0.);
        for (values, field) in row.live.iter_mut().zip(live_fields) {
            values[x] = match &value.fields[*field] {
                FieldValue::Scalar(v) => *v,
                FieldValue::Bool(v) => f64::from(u8::from(*v)),
                FieldValue::Vec2(v) => v[0],
                FieldValue::Vec3(v) => v[0],
            };
        }
    }
    Ok(row)
}

fn bake(
    program: &Program,
    inputs: &Inputs,
    atlas: &Atlas,
    inspect: Option<&str>,
    threads: usize,
    compact: bool,
    cancelled: &dyn Fn() -> bool,
) -> Result<BakedReceiver, Diagnostic> {
    let failure = |message: String| Diagnostic::message(&program.metadata().entry, message);
    let pixels = atlas.width * atlas.height;
    if pixels > 4 * 1024 * 1024 {
        return Err(failure(
            "Material atlas exceeds its pixel allowance.".into(),
        ));
    }
    let mut color = vec![0u8; pixels * 4];
    // Texels outside every chart are transparent; unused normals stay flat, so mip levels that
    // average across a chart's gutter never pull in an invalid direction.
    let mut normal = [128u8, 128, 255, 0].repeat(pixels);
    let mut properties = vec![0u8; pixels * 4];
    let mut emitted = vec![[0.; 3]; pixels];
    let mut blend = false;
    let mut emits = false;
    // Feature queries see this receiver's own creases and open edges, from the same world-space
    // positions the atlas samples; they are part of the receiver layout's cached identity.
    let features = if program
        .metadata()
        .required_inputs
        .iter()
        .any(|input| input == "features")
    {
        let receiver = &atlas.receiver;
        let mut features = program::receiver_features(&receiver.positions, &receiver.indices);
        features.extend(program::receiver_seams(
            &receiver.positions,
            &receiver.uvs,
            &receiver.indices,
        ));
        features.extend(receiver.contacts.iter().map(|contact| program::Feature {
            start: contact.start,
            end: contact.end,
            kind: program::FeatureKind::Contact,
            angle: 0.,
            normal: None,
        }));
        features.truncate(program::MAX_FEATURES);
        features
    } else {
        Vec::new()
    };
    let mut emission_strength = 1f64;
    // Live renderer arguments are fields; previews of a single field carry no live operations.
    let mut live_fields: Vec<&str> = Vec::new();
    if inspect.is_none() {
        for operation in &program.metadata().live {
            for field in operation.inputs.values() {
                if !live_fields.contains(&field.as_str()) {
                    live_fields.push(field);
                }
            }
        }
    }
    let mut live_values = vec![vec![f64::NAN; pixels]; live_fields.len()];
    let rows: Vec<(usize, usize)> = atlas
        .charts
        .iter()
        .enumerate()
        .flat_map(|(chart, placed)| (0..placed.height).map(move |y| (chart, y)))
        .collect();
    let evaluator = || {
        program
            .evaluator_with_features(inputs, &features)
            .map_err(Diagnostic::from)
    };
    let entry = program.metadata().entry.as_str();
    let threads = threads.max(1);
    let mut serial = if threads == 1 {
        Some(evaluator()?)
    } else {
        None
    };
    // Rows are evaluated independently and written back in order, so threads never change bytes.
    for batch in rows.chunks(threads * 2) {
        if cancelled() {
            return Err(failure("Material authoring cancelled.".into()));
        }
        let results: Vec<Result<Row, Diagnostic>> = match &mut serial {
            Some(evaluator) => batch
                .iter()
                .map(|&(chart, y)| {
                    let chart = &atlas.charts[chart];
                    bake_row(evaluator, atlas, chart, y, inspect, &live_fields, entry)
                })
                .collect(),
            None => std::thread::scope(|scope| {
                let handles: Vec<_> = batch
                    .chunks(batch.len().div_ceil(threads))
                    .map(|share| {
                        let live_fields = &live_fields;
                        let evaluator = &evaluator;
                        scope.spawn(move || -> Vec<Result<Row, Diagnostic>> {
                            let mut evaluator = match evaluator() {
                                Ok(evaluator) => evaluator,
                                Err(error) => {
                                    return share.iter().map(|_| Err(error.clone())).collect();
                                }
                            };
                            share
                                .iter()
                                .map(|&(chart, y)| {
                                    bake_row(
                                        &mut evaluator,
                                        atlas,
                                        &atlas.charts[chart],
                                        y,
                                        inspect,
                                        live_fields,
                                        entry,
                                    )
                                })
                                .collect()
                        })
                    })
                    .collect();
                handles
                    .into_iter()
                    .flat_map(|handle| handle.join().expect("bake rows do not panic"))
                    .collect()
            }),
        };
        for (&(chart, y), result) in batch.iter().zip(results) {
            let row = result?;
            let chart = &atlas.charts[chart];
            let start = (chart.y + y) * atlas.width + chart.x;
            for x in 0..chart.width {
                let (texel, offset) = (start + x, (start + x) * 4);
                color[offset..offset + 4].copy_from_slice(&row.color[x * 4..x * 4 + 4]);
                normal[offset..offset + 4].copy_from_slice(&row.normal[x * 4..x * 4 + 4]);
                properties[offset..offset + 4].copy_from_slice(&[
                    255,
                    row.roughness[x],
                    row.metalness[x],
                    255,
                ]);
                emitted[texel] = row.emitted[x];
                for (values, samples) in live_values.iter_mut().zip(&row.live) {
                    values[texel] = samples[x];
                }
            }
            emission_strength = emission_strength.max(row.strength);
            blend |= row.blend;
            emits |= row.emits;
        }
    }
    let mut images = BTreeMap::new();
    let mut encode = |rgba: Vec<u8>| -> Result<String, Diagnostic> {
        let encode = if compact {
            image::encode_small_cancellable
        } else {
            image::encode_cancellable
        };
        let bytes = encode(
            &image::Image {
                width: atlas.width,
                height: atlas.height,
                rgba,
                mime: "image/png",
            },
            cancelled,
        )
        .map_err(&failure)?;
        let id = format!("studio/image/{}", hash(&bytes));
        images.insert(
            id.clone(),
            ImageResource {
                mime: "image/png",
                bytes: bytes.into(),
                pixels,
            },
        );
        Ok(id)
    };
    let texture = encode(color)?;
    let mut material = Material {
        color: "#ffffff".into(),
        repeat: 1.,
        roughness: 1.,
        metalness: 1.,
        texture: Some(texture.clone()),
        alpha_mode: blend.then_some(AlphaMode::Blend),
        ..Default::default()
    };
    if inspect.is_some() {
        material.color = "#000000".into();
        material.emissive = Some("#ffffff".into());
        material.emissive_texture = Some(texture);
        material.metalness = 0.;
    } else {
        material.normal_texture = Some(encode(normal)?);
        material.normal_scale = Some(1.);
        material.metallic_roughness_texture = Some(encode(properties)?);
        if emits {
            let emissive = emitted
                .into_iter()
                .flat_map(|rgb| {
                    [
                        srgb(rgb[0] / emission_strength),
                        srgb(rgb[1] / emission_strength),
                        srgb(rgb[2] / emission_strength),
                        255,
                    ]
                })
                .collect();
            material.emissive = Some("#ffffff".into());
            material.emissive_strength = Some(emission_strength);
            material.emissive_texture = Some(encode(emissive)?);
        }
        material.live = live_operations(program, &live_fields, &live_values, pixels, &mut encode)?;
    }
    material.id.clear();
    material.id = format!("studio/material/{}", hash(&json(&material)));
    Ok(BakedReceiver {
        mesh: atlas.mesh.clone(),
        material,
        images,
    })
}

//! glTF 2.0: https://registry.khronos.org/glTF/specs/2.0/glTF-2.0.html
use superspec_types::limits;
mod buffer;
use crate::{Options, Purpose, tangents::material_tangents};
use buffer::Buffer;
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use superspec_geometry::math::unit;
use superspec_geometry::{image, ktx2};
use superspec_model::{
    Mesh, Scene,
    batches::{Batch, Collision},
};
use superspec_types::material::Material;

#[derive(Serialize)]
struct Document<'a> {
    #[serde(flatten)]
    gltf: &'a Value,
    extras: Extras<'a>,
}
#[derive(Serialize)]
struct Extras<'a> {
    superspec: Metadata<'a>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Metadata<'a> {
    schema_version: u32,
    id: &'a str,
    purpose: &'static str,
    source_axes: &'static str,
    export_axes: &'static str,
    units: &'static str,
    architecture: &'a Option<superspec_model::architecture::Context>,
    geometry_validation: &'a str,
    navigation_validation: &'a str,
    diagnostics: &'a [superspec_types::diagnostic::Diagnostic],
    /// The scene's whole lighting record, for consumers that honor more than punctual lights.
    #[serde(skip_serializing_if = "Option::is_none")]
    lighting: Option<&'a superspec_types::lighting::Lighting>,
}

fn linear(color: &str) -> Result<[f64; 3], String> {
    if color.len() != 7
        || !color.starts_with('#')
        || !color[1..].bytes().all(|c| c.is_ascii_hexdigit())
    {
        return Err("Export material requires six-digit sRGB color.".into());
    }
    Ok(superspec_types::color::srgb(color))
}
fn check_material(material: &Material) -> Result<(), String> {
    linear(&material.color)?;
    if let Some(color) = &material.emissive {
        linear(color)?;
    }
    if ![
        material.roughness,
        material.metalness,
        material.opacity.unwrap_or(1.),
        if material.alpha_mode.is_some() {
            0.
        } else {
            material.alpha_cutoff.unwrap_or(0.)
        },
    ]
    .iter()
    .all(|v| (0.0..=1.0).contains(v))
        || material
            .alpha_cutoff
            .is_some_and(|v| !v.is_finite() || v < 0.)
        || [
            material.normal_scale.unwrap_or(1.),
            material.emissive_strength.unwrap_or(1.),
        ]
        .iter()
        .any(|v| !v.is_finite() || *v < 0.)
    {
        return Err("Invalid export material factors.".into());
    }
    Ok(())
}

/// Checks public scene ranges before indexing; selected geometry may never silently disappear.
fn selected<'a>(scene: &'a Scene, options: &Options<'_>) -> Result<Vec<&'a Mesh>, String> {
    options.check(0)?;
    superspec_model::lod::check(scene)?;
    let (alternate_meshes, _) = superspec_model::lod::alternate_ids(&scene.manifest.geometry_lods);
    let vertices = scene.manifest.vertex_count;
    if !scene.manifest.success
        || vertices > limits::MAX_WORLD_VERTICES
        || scene.manifest.index_count > limits::MAX_WORLD_INDICES
        || scene.positions.len() != vertices * 3
        || scene.normals.len() != vertices * 3
        || scene.colors.len() != vertices * 3
        || scene.uvs.len() != vertices * 2
        || scene.indices.len() != scene.manifest.index_count
    {
        return Err("Export requires a complete successful scene.".into());
    }
    let colliders: BTreeSet<_> = scene
        .manifest
        .colliders
        .iter()
        .map(String::as_str)
        .collect();
    let mut ids = BTreeSet::new();
    let mut selected = vec![];
    let mut estimate = scene.manifest.id.len() * 2;
    let owners: BTreeSet<_> = scene
        .manifest
        .architecture
        .iter()
        .flat_map(|context| &context.objects)
        .map(|o| o.id.as_str())
        .collect();
    for mesh in &scene.manifest.meshes {
        options.check(0)?;
        if !ids.insert(mesh.id.as_str()) {
            return Err("Duplicate scene mesh identity.".into());
        }
        if !(if options.purpose == Purpose::Collision {
            colliders.contains(mesh.id.as_str()) && !alternate_meshes.contains(mesh.id.as_str())
        } else {
            mesh.visible
        }) {
            continue;
        }
        if mesh.vertex_count == 0
            || mesh.index_count == 0
            || !mesh.index_count.is_multiple_of(3)
            || mesh
                .vertex_start
                .checked_add(mesh.vertex_count)
                .is_none_or(|n| n > vertices)
            || mesh
                .index_start
                .checked_add(mesh.index_count)
                .is_none_or(|n| n > scene.indices.len())
        {
            return Err(format!("Invalid export mesh range: {}", mesh.id));
        }
        if !owners.contains(mesh.object_id.as_str()) {
            return Err("Export mesh references an unknown source owner.".into());
        }
        estimate = estimate
            .checked_add(
                mesh.vertex_count * 60
                    + mesh.index_count * 4
                    + mesh.id.len() * 3
                    + mesh.object_id.len(),
            )
            .ok_or("Export size overflow.")?;
        options.check(estimate)?;
        let a = mesh.vertex_start;
        let b = a + mesh.vertex_count;
        if scene.positions[a * 3..b * 3]
            .iter()
            .chain(&scene.normals[a * 3..b * 3])
            .chain(&scene.uvs[a * 2..b * 2])
            .any(|v| !v.is_finite())
            || scene.indices[mesh.index_start..mesh.index_start + mesh.index_count]
                .iter()
                .any(|i| *i as usize >= mesh.vertex_count)
        {
            return Err(format!("Invalid export attributes or indices: {}", mesh.id));
        }
        if options.purpose == Purpose::Visual
            && scene.colors[a * 3..b * 3]
                .iter()
                .any(|v| !v.is_finite() || *v < 0.)
        {
            return Err(format!(
                "glTF vertex colors require finite nonnegative values; {} contains invalid color.",
                mesh.id
            ));
        }
        selected.push(mesh);
    }
    if colliders.iter().any(|id| !ids.contains(id)) {
        return Err("Collision references an unknown mesh.".into());
    }
    Ok(selected)
}

/// Self-contained binary glTF. Collision exports contain canonical physical geometry only.
/// How each texture used by the exported materials is sampled: color (sRGB base color and
/// emissive) or data (normals, metallic-roughness, occlusion and live-operation channels). A
/// texture used both ways is treated as data, the stricter encoding.
fn texture_roles<'a>(
    used: &BTreeSet<&str>,
    materials: &BTreeMap<&str, &'a Material>,
) -> BTreeMap<&'a str, ktx2::Kind> {
    use ktx2::Kind::{Color, Data};
    let mut uses: Vec<(&'a str, ktx2::Kind)> = vec![];
    for material in used.iter().filter_map(|id| materials.get(id)) {
        for (id, kind) in [
            (&material.texture, Color),
            (&material.emissive_texture, Color),
            (&material.normal_texture, Data),
            (&material.metallic_roughness_texture, Data),
            (&material.occlusion_texture, Data),
        ] {
            if let Some(id) = id {
                uses.push((id.as_str(), kind));
            }
        }
        for operation in &material.live {
            for argument in operation.arguments.values() {
                if let superspec_types::material::LiveArgument::Texture { texture, .. } = argument {
                    uses.push((texture.as_str(), Data));
                }
            }
        }
    }
    let mut roles = BTreeMap::new();
    for (id, kind) in uses {
        let role = roles.entry(id).or_insert(kind);
        if kind == Data {
            *role = Data;
        }
    }
    roles
}

struct Payload<'a> {
    bytes: std::borrow::Cow<'a, [u8]>,
    mime: &'static str,
}

/// Image bytes for every used texture in the requested format. PNG and JPEG pass through for PNG
/// output and KTX2 sources are transcoded to PNG; for KTX2 output, PNG and JPEG sources are
/// encoded by role and KTX2 sources pass through. Encoding is per texture and deterministic, so
/// encoding them in parallel (native targets only) never changes the bytes.
fn image_source<'a>(
    scene: &'a Scene,
    assets: &BTreeMap<&str, &superspec_model::Asset>,
    id: &str,
) -> Result<(&'a [u8], &'static str), String> {
    let asset = assets
        .get(id)
        .ok_or_else(|| format!("Missing export image: {id}"))?;
    let end = asset
        .byte_start
        .checked_add(asset.byte_length)
        .ok_or("Invalid export image range.")?;
    let bytes = scene
        .asset_bytes
        .get(asset.byte_start..end)
        .ok_or("Invalid export image range.")?;
    if bytes.is_empty() || !["image/png", "image/jpeg", ktx2::MIME].contains(&asset.mime) {
        return Err("Unsupported export image type.".into());
    }
    Ok((bytes, asset.mime))
}

/// Most threads one export encodes textures on; each holds one texture's encoder state.
const MAX_ENCODE_THREADS: usize = 16;

fn texture_payloads<'a>(
    scene: &'a Scene,
    assets: &BTreeMap<&str, &superspec_model::Asset>,
    roles: &BTreeMap<&str, ktx2::Kind>,
    options: &Options<'a>,
) -> Result<BTreeMap<String, Payload<'a>>, String> {
    // Only the format and pre-encoded payloads cross into worker threads; the cancellation
    // callback stays on this one.
    let format = options.textures;
    let encoded = options.encoded;
    let convert = |id: &str, kind: ktx2::Kind| -> Result<Payload<'a>, String> {
        let (bytes, mime) = image_source(scene, assets, id)?;
        let borrowed = || Payload {
            bytes: bytes.into(),
            mime,
        };
        match (format, mime == ktx2::MIME) {
            (crate::Textures::Png, false) | (crate::Textures::Ktx2, true) => Ok(borrowed()),
            (crate::Textures::Png, true) => Ok(Payload {
                bytes: image::encode(&image::decode(bytes)?)?.into(),
                mime: "image/png",
            }),
            (crate::Textures::Ktx2, false) => Ok(Payload {
                bytes: match encoded.and_then(|encoded| encoded.get(id)) {
                    Some(payload) if payload.starts_with(&ktx2::MAGIC) => payload.as_slice().into(),
                    Some(_) => return Err(format!("{id}: supplied texture is not KTX2.")),
                    None => encode_texture(bytes, kind)
                        .map_err(|e| format!("{id}: {e}"))?
                        .into(),
                },
                mime: ktx2::MIME,
            }),
        }
    };
    let jobs: Vec<(&str, ktx2::Kind)> = roles.iter().map(|(id, kind)| (*id, *kind)).collect();
    let parallel = !cfg!(target_arch = "wasm32")
        && options.textures == crate::Textures::Ktx2
        && options.encoded.is_none();
    let results: Vec<Result<Payload<'a>, String>> = if parallel && jobs.len() > 1 {
        // Threads take one texture at a time from a shared queue, costliest first (UASTC data
        // maps, then by source size), so one thread is never left with all the slow textures.
        // Each result lands in its job's slot: the output does not depend on thread count or
        // on which thread encoded what.
        let mut queue: Vec<usize> = (0..jobs.len()).collect();
        let cost = |job: usize| {
            let (id, kind) = jobs[job];
            let size = image_source(scene, assets, id).map_or(0, |(bytes, _)| bytes.len());
            (kind == ktx2::Kind::Data, size)
        };
        queue.sort_by_key(|job| std::cmp::Reverse(cost(*job)));
        let threads = std::thread::available_parallelism()
            .map_or(1, |n| n.get())
            .min(MAX_ENCODE_THREADS)
            .min(jobs.len());
        let next = std::sync::atomic::AtomicUsize::new(0);
        let stop = std::sync::atomic::AtomicBool::new(false);
        let work = || {
            let mut done = vec![];
            while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                let at = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                let Some(&job) = queue.get(at) else { break };
                let (id, kind) = jobs[job];
                done.push((job, convert(id, kind)));
            }
            done
        };
        let mut slots: Vec<Option<Result<Payload<'a>, String>>> =
            jobs.iter().map(|_| None).collect();
        std::thread::scope(|scope| {
            let handles: Vec<_> = (1..threads).map(|_| scope.spawn(work)).collect();
            // This thread encodes too and is the only one that asks about cancellation.
            let mut done = vec![];
            while !(options.cancelled)() {
                let at = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                let Some(&job) = queue.get(at) else { break };
                let (id, kind) = jobs[job];
                done.push((job, convert(id, kind)));
            }
            stop.store(true, std::sync::atomic::Ordering::Relaxed);
            for handle in handles {
                done.extend(handle.join().expect("texture encoding does not panic"));
            }
            for (job, result) in done {
                slots[job] = Some(result);
            }
        });
        options.check(0)?;
        slots
            .into_iter()
            .map(|slot| slot.unwrap_or_else(|| Err("Export cancelled.".into())))
            .collect()
    } else {
        jobs.iter()
            .map(|(id, kind)| {
                options.check(0)?;
                convert(id, *kind)
            })
            .collect()
    };
    options.check(0)?;
    jobs.iter()
        .zip(results)
        .map(|((id, _), payload)| payload.map(|payload| ((*id).to_owned(), payload)))
        .collect()
}

/// What an export resolves before writing: the selected meshes, images and materials by
/// identity, the materials those meshes use and, for visual exports, each used texture's role.
struct Plan<'a> {
    selected: Vec<&'a Mesh>,
    /// Instanced batches the export writes: visible ones, or for collision those that collide.
    batches: Vec<&'a Batch>,
    assets: BTreeMap<&'a str, &'a superspec_model::Asset>,
    materials: BTreeMap<&'a str, &'a Material>,
    used: BTreeSet<&'a str>,
    roles: BTreeMap<&'a str, ktx2::Kind>,
}

fn plan<'a>(scene: &'a Scene, options: &Options<'_>) -> Result<Plan<'a>, String> {
    let selected = selected(scene, options)?;
    superspec_model::batches::check(scene)?;
    let (_, alternate_batches) = superspec_model::lod::alternate_ids(&scene.manifest.geometry_lods);
    let batches: Vec<&Batch> = scene
        .manifest
        .batches
        .iter()
        .filter(|batch| {
            if options.purpose == Purpose::Collision {
                batch.collision == Collision::Instances
                    && !alternate_batches.contains(batch.id.as_str())
            } else {
                batch.visible
            }
        })
        .collect();
    if selected.is_empty() && batches.is_empty() {
        return Err("Cannot export an empty model.".into());
    }
    let mut assets = BTreeMap::new();
    for asset in &scene.manifest.assets {
        if assets.insert(asset.id.as_str(), asset).is_some() {
            return Err("Duplicate export image identity.".into());
        }
    }
    let mut materials: BTreeMap<_, _> = scene
        .manifest
        .materials
        .iter()
        .map(|m| (m.id.as_str(), m))
        .collect();
    if materials.len() != scene.manifest.materials.len() {
        return Err("Duplicate export material identity.".into());
    }
    let mut used: BTreeSet<_> = selected
        .iter()
        .filter_map(|m| m.material.as_deref())
        .chain(batches.iter().filter_map(|b| b.material.as_deref()))
        .collect();
    let roles = if options.purpose == Purpose::Visual {
        for material in scene
            .manifest
            .materials
            .iter()
            .filter(|m| !m.lods.is_empty())
        {
            if !used.contains(material.id.as_str()) {
                continue;
            }
            material.validate()?;
            for level in &material.lods {
                let alternate = level.material.as_ref();
                if materials
                    .insert(alternate.id.as_str(), alternate)
                    .is_some_and(|prior| prior != alternate)
                {
                    return Err(format!(
                        "Conflicting LOD material identity {} on {}.",
                        alternate.id, material.id
                    ));
                }
                used.insert(alternate.id.as_str());
            }
        }
        texture_roles(&used, &materials)
    } else {
        BTreeMap::new()
    };
    Ok(Plan {
        selected,
        batches,
        assets,
        materials,
        used,
        roles,
    })
}

/// One texture a KTX2 export has to encode: its image identity, sampling role and source bytes.
pub struct TextureJob<'a> {
    pub id: &'a str,
    pub kind: ktx2::Kind,
    pub source: &'a [u8],
}

/// The textures `glb` would encode for these options, in its order. A host without threads
/// (the browser) encodes them concurrently with `encode_texture` elsewhere and passes the results
/// back through `Options::encoded`; the GLB is then byte-identical to encoding them in `glb`.
pub fn texture_jobs<'a>(
    scene: &'a Scene,
    options: &Options<'_>,
) -> Result<Vec<TextureJob<'a>>, String> {
    let plan = plan(scene, options)?;
    if options.textures != crate::Textures::Ktx2 {
        return Ok(vec![]);
    }
    let mut jobs = vec![];
    for (id, kind) in plan.roles {
        let (source, mime) = image_source(scene, &plan.assets, id)?;
        if mime != ktx2::MIME {
            jobs.push(TextureJob { id, kind, source });
        }
    }
    Ok(jobs)
}

/// Encodes one PNG or JPEG texture as KTX2 for its role; the only KTX2 encoding path for exports.
pub fn encode_texture(source: &[u8], kind: ktx2::Kind) -> Result<Vec<u8>, String> {
    ktx2::encode(&image::decode(source)?, kind)
}

/// https://github.com/KhronosGroup/glTF/tree/main/extensions/2.0/Vendor/EXT_mesh_gpu_instancing
const INSTANCING: &str = "EXT_mesh_gpu_instancing";

/// Lower variants exist in the document but are reachable only through the optional extension.
/// Core glTF therefore draws the authored near fallback exactly once, including plain-node batches.
fn geometry_lods(scene: &Scene, nodes: &mut Vec<Value>) -> Result<Vec<usize>, String> {
    if scene.manifest.geometry_lods.is_empty() {
        return Ok((0..nodes.len()).collect());
    }
    let mut by_mesh = BTreeMap::<&str, Vec<usize>>::new();
    let mut by_batch = BTreeMap::<&str, Vec<usize>>::new();
    for (index, node) in nodes.iter().enumerate() {
        if let Some(id) = node["extras"]["meshId"].as_str() {
            by_mesh.entry(id).or_default().push(index);
        }
        if let Some(id) = node["extras"]["batchId"].as_str() {
            by_batch.entry(id).or_default().push(index);
        }
    }
    let mut grouped = BTreeSet::new();
    let mut groups = Vec::new();
    for group in &scene.manifest.geometry_lods {
        let mut levels = Vec::new();
        for (level_index, level) in group.levels.iter().enumerate() {
            let mut children = Vec::new();
            for (ids, mapping) in [(&level.meshes, &by_mesh), (&level.batches, &by_batch)] {
                for id in ids {
                    let emitted = mapping.get(id.as_str()).ok_or_else(|| {
                        format!(
                            "Geometry LOD {} references unemitted geometry {id}.",
                            group.id
                        )
                    })?;
                    children.extend_from_slice(emitted);
                    grouped.extend(emitted.iter().copied());
                }
            }
            levels
                .push(json!({"name":format!("{}:lod:{level_index}",group.id),"children":children}));
        }
        groups.push((group, levels));
    }
    let mut roots: Vec<usize> = (0..nodes.len()).filter(|i| !grouped.contains(i)).collect();
    // Release node-borrowing lookup tables before appending group nodes.
    drop(by_mesh);
    drop(by_batch);
    for (group, mut levels) in groups {
        let near = nodes.len();
        levels[0]["extensions"] =
            json!({"MSFT_lod":{"ids":(near+1..near+levels.len()).collect::<Vec<_>>()}});
        levels[0]["extras"] = json!({"worldSpecLod":{"version":1,"distances":group.levels.iter().map(|l|l.distance).collect::<Vec<_>>()}});
        if group.levels.iter().all(|l| l.screen_coverage.is_some()) {
            levels[0]["extras"]["MSFT_screencoverage"] = json!(
                group
                    .levels
                    .iter()
                    .map(|l| l.screen_coverage.unwrap())
                    .collect::<Vec<_>>()
            );
        }
        roots.push(near);
        nodes.extend(levels);
    }
    Ok(roots)
}

/// Writes the selected instanced batches: each batch's mesh once, then its instances. A visual
/// export with `Instancing::Extension` gives a batch one node with `EXT_mesh_gpu_instancing`
/// (TRANSLATION, ROTATION and SCALE accessors; the `instance` values as `_INSTANCE` and the tints
/// as `_TINT`). With `Instancing::Nodes`, and always for collision, every instance is a node of
/// its own that references the same mesh, placed by the node's translation, rotation and scale:
/// plain glTF every loader reads, with the per-instance attributes in the node's extras. Either
/// way the placements are under the root node and so stay in the domain's Z-up frame, and the
/// authored numbers are written as they are. Returns whether the extension was used.
fn write_batches(
    scene: &Scene,
    batches: &[&Batch],
    material_indices: &BTreeMap<&str, usize>,
    buffer: &mut Buffer<'_, '_>,
    meshes: &mut Vec<Value>,
    nodes: &mut Vec<Value>,
) -> Result<bool, String> {
    let options = buffer.options;
    let visual = options.purpose == Purpose::Visual;
    let extension = visual && options.instancing == crate::Instancing::Extension;
    let (_, alternate_batches) = superspec_model::lod::alternate_ids(&scene.manifest.geometry_lods);
    for batch in batches {
        options.check(0)?;
        let view = scene.batches.view(batch);
        if view
            .positions
            .iter()
            .chain(view.normals)
            .chain(view.uvs)
            .chain(view.placements)
            .any(|v| !v.is_finite())
        {
            return Err(format!("Invalid export batch attributes: {}", batch.id));
        }
        let position = buffer.floats(view.positions, 3, true)?;
        let normals = view
            .normals
            .as_chunks::<3>()
            .0
            .iter()
            .map(|n| unit(n.map(f64::from)).map(|n| n.map(|v| v as f32)))
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .flatten()
            .collect::<Vec<_>>();
        let normal = buffer.floats(&normals, 3, false)?;
        let mut attributes = json!({"POSITION":position,"NORMAL":normal});
        if visual {
            let uv = view
                .uvs
                .iter()
                .enumerate()
                .map(|(i, v)| if i % 2 == 0 { *v } else { 1. - v })
                .collect::<Vec<_>>();
            attributes["TEXCOORD_0"] = json!(buffer.floats(&uv, 2, false)?);
            if view.colors.iter().any(|v| !v.is_finite() || *v < 0.) {
                return Err(format!(
                    "glTF vertex colors require finite nonnegative values; {} contains invalid color.",
                    batch.id
                ));
            }
            if view.colors.iter().any(|v| *v > 1.) {
                attributes["_SUPERSPEC_HDR_COLOR"] = json!(buffer.floats(view.colors, 3, false)?);
                attributes["COLOR_0"] = json!(buffer.floats(
                    &view.colors.iter().map(|v| v.min(1.)).collect::<Vec<_>>(),
                    3,
                    false
                )?);
            } else {
                attributes["COLOR_0"] = json!(buffer.floats(view.colors, 3, false)?);
            }
        }
        use superspec_model::batches::Domain;
        // Named attributes are custom attributes: `shadeNormal` is written as `_SHADE_NORMAL`.
        let custom = |name: &str| {
            let mut out = String::from("_");
            for character in name.chars() {
                if character.is_ascii_uppercase() {
                    out.push('_');
                }
                out.push(character.to_ascii_uppercase());
            }
            out
        };
        if visual {
            for attribute in batch
                .attributes
                .iter()
                .filter(|a| a.domain == Domain::Vertex)
            {
                let values = scene.batches.attribute(batch, attribute);
                attributes[custom(&attribute.name)] =
                    json!(buffer.floats(values, attribute.components, false)?);
            }
        }
        let indices = buffer.indices(view.indices)?;
        let mut primitive = json!({"attributes":attributes,"indices":indices,"mode":4});
        if let Some(index) = batch
            .material
            .as_deref()
            .and_then(|id| material_indices.get(id))
        {
            primitive["material"] = json!(index);
        }
        let mesh = meshes.len();
        meshes.push(json!({"name":batch.id,"primitives":[primitive]}));
        let count = batch.instance_count;
        let collidable = batch.collision == Collision::Instances
            && !alternate_batches.contains(batch.id.as_str());
        if extension {
            let column = |buffer: &mut Buffer<'_, '_>, start: usize, components: usize| {
                let values: Vec<f32> = view
                    .placements
                    .chunks_exact(superspec_model::batches::PLACEMENT_FLOATS)
                    .flat_map(|placement| placement[start..start + components].iter().copied())
                    .collect();
                buffer.attribute(&values, components, false, None)
            };
            let mut instance = json!({
                "TRANSLATION": column(buffer, 0, 3)?,
                "ROTATION": column(buffer, 3, 4)?,
                "SCALE": column(buffer, 7, 3)?,
            });
            if let Some(values) = view.values {
                instance["_INSTANCE"] = json!(buffer.attribute(values, 4, false, None)?);
            }
            if let Some(tints) = view.tints {
                instance["_TINT"] = json!(buffer.attribute(tints, 3, false, None)?);
            }
            for attribute in batch
                .attributes
                .iter()
                .filter(|a| a.domain == Domain::Instance)
            {
                let values = scene.batches.attribute(batch, attribute);
                instance[custom(&attribute.name)] =
                    json!(buffer.attribute(values, attribute.components, false, None)?);
            }
            let mut extras = json!({"batchId":batch.id,"objectId":batch.object_id,"source":batch.source,
                "instances":count,"collidable":collidable});
            // A foliage look travels beside the batch for a renderer that has its shader families.
            if let Some(foliage) = batch.foliage.as_ref().filter(|_| visual) {
                extras["foliage"] = json!(foliage);
            }
            nodes.push(json!({"name":batch.id,"mesh":mesh,
                "extensions":{(INSTANCING):{"attributes":instance}},"extras":extras}));
        } else {
            for (index, placement) in view
                .placements
                .chunks_exact(superspec_model::batches::PLACEMENT_FLOATS)
                .enumerate()
            {
                if index % 1024 == 0 {
                    options.check(0)?;
                }
                let mut extras = json!({"batchId":batch.id,"instance":index,
                    "objectId":batch.object_id,"collidable":collidable});
                if let Some(foliage) = batch.foliage.as_ref().filter(|_| visual) {
                    extras["foliage"] = json!(foliage);
                }
                if index == 0 {
                    extras["source"] = json!(batch.source);
                }
                if let Some(values) = view.values.filter(|_| visual) {
                    extras["value"] = json!(values[index * 4..index * 4 + 4]);
                }
                if let Some(tints) = view.tints.filter(|_| visual) {
                    extras["tint"] = json!(tints[index * 3..index * 3 + 3]);
                }
                if visual {
                    for attribute in batch
                        .attributes
                        .iter()
                        .filter(|a| a.domain == Domain::Instance)
                    {
                        let values = scene.batches.attribute(batch, attribute);
                        let n = attribute.components;
                        extras[&attribute.name] = if n == 1 {
                            json!(values[index])
                        } else {
                            json!(values[index * n..index * n + n])
                        };
                    }
                }
                nodes.push(json!({"name":format!("{}/{index}", batch.id),"mesh":mesh,
                    "translation":placement[..3],"rotation":placement[3..7],"scale":placement[7..],
                    "extras":extras}));
            }
        }
    }
    Ok(extension && !batches.is_empty())
}

/// Navigation/geometry audit statuses remain explicit metadata; export success is not validation.
pub fn glb(scene: &Scene, options: &Options<'_>) -> Result<Vec<u8>, String> {
    let Plan {
        selected,
        batches,
        assets,
        materials: source_materials,
        used,
        roles,
    } = plan(scene, options)?;
    let visual = options.purpose == Purpose::Visual;
    let mut buffer = Buffer::new(options);
    let (alternate_meshes, _) = superspec_model::lod::alternate_ids(&scene.manifest.geometry_lods);
    let mut materials = vec![];
    let mut material_indices = BTreeMap::new();
    let mut images = vec![];
    let mut textures = vec![];
    let mut texture_indices = BTreeMap::new();
    let mut emissive_strength = false;
    let payloads = if visual {
        texture_payloads(scene, &assets, &roles, options)?
    } else {
        BTreeMap::new()
    };
    let ktx2 = payloads.values().any(|payload| payload.mime == ktx2::MIME);
    if visual {
        let mut texture = |id: &str| -> Result<usize, String> {
            if let Some(index) = texture_indices.get(id) {
                return Ok(*index);
            }
            let payload = payloads
                .get(id)
                .ok_or_else(|| format!("Missing export image: {id}"))?;
            let view = buffer.view(&payload.bytes, None)?;
            let index = images.len();
            images.push(json!({"name":id,"bufferView":view,"mimeType":payload.mime}));
            textures.push(if payload.mime == ktx2::MIME {
                // Required extension without a fallback source: the point is the smaller file.
                json!({"sampler":0,"extensions":{"KHR_texture_basisu":{"source":index}}})
            } else {
                json!({"source":index,"sampler":0})
            });
            texture_indices.insert(id.to_string(), index);
            Ok(index)
        };
        for id in used {
            options.check(0)?;
            let material = source_materials
                .get(id)
                .ok_or_else(|| format!("Unknown export material: {id}"))?;
            check_material(material)?;
            let color = material
                .base_color_factor
                .unwrap_or(linear(&material.color)?);
            let mut pbr = json!({"baseColorFactor":[color[0],color[1],color[2],material.opacity.unwrap_or(1.)],"roughnessFactor":material.roughness,"metallicFactor":material.metalness});
            if let Some(id) = &material.texture {
                pbr["baseColorTexture"] = json!({"index":texture(id)?,"texCoord":0});
            }
            if let Some(id) = &material.metallic_roughness_texture {
                pbr["metallicRoughnessTexture"] = json!({"index":texture(id)?,"texCoord":0});
            }
            let mut entry = json!({"name":id,"pbrMetallicRoughness":pbr,"doubleSided":material.opacity.unwrap_or(1.)>=1.});
            if let Some(double_sided) = material.double_sided {
                entry["doubleSided"] = json!(double_sided);
            }
            if let Some(id) = &material.normal_texture {
                entry["normalTexture"] = json!({"index":texture(id)?,"texCoord":0,"scale":material.normal_scale.unwrap_or(1.)});
            }
            if let Some(color) = &material.emissive {
                let strength = material.emissive_strength.unwrap_or(1.);
                entry["emissiveFactor"] = json!(
                    material
                        .emissive_factor
                        .unwrap_or(linear(color)?)
                        .map(|v| v * strength.min(1.))
                );
                if strength > 1. {
                    entry["extensions"] =
                        json!({"KHR_materials_emissive_strength":{"emissiveStrength":strength}});
                    emissive_strength = true;
                }
            }
            if let Some(id) = &material.emissive_texture {
                entry["emissiveTexture"] = json!({"index":texture(id)?,"texCoord":0});
            }
            if let Some(id) = &material.occlusion_texture {
                entry["occlusionTexture"] = json!({"index":texture(id)?,"texCoord":0,"strength":material.occlusion_strength.unwrap_or(1.)});
            }
            if let Some(cutoff) = material.alpha_cutoff {
                entry["alphaMode"] = json!("MASK");
                entry["alphaCutoff"] = json!(cutoff);
            }
            if material.opacity.unwrap_or(1.) < 1. {
                entry["alphaMode"] = json!("BLEND");
            }
            if let Some(mode) = material.alpha_mode {
                entry["alphaMode"] = json!(mode);
            }
            let mut spec = serde_json::Map::new();
            if !material.live.is_empty() {
                // Live renderer operations travel with the portable PBR material; texture
                // arguments name glTF textures on UV0.
                let mut live = vec![];
                for operation in &material.live {
                    let mut arguments = serde_json::Map::new();
                    for (name, argument) in &operation.arguments {
                        if !argument.valid() {
                            return Err(format!(
                                "Invalid live argument {name} of {}.",
                                operation.id
                            ));
                        }
                        let value = match argument {
                            superspec_types::material::LiveArgument::Constant { value } => {
                                json!({ "value": value })
                            }
                            superspec_types::material::LiveArgument::Texture {
                                texture: image,
                                channel,
                                scale,
                                offset,
                            } => json!({
                                "texture": texture(image)?, "channel": channel, "scale": scale, "offset": offset,
                            }),
                        };
                        arguments.insert(name.clone(), value);
                    }
                    live.push(json!({
                        "id": operation.id, "stage": operation.stage,
                        "requires": operation.requires, "arguments": arguments,
                    }));
                }
                spec.insert("live".into(), json!(live));
            }
            // A painted material keeps its PBR maps as the fallback any glTF consumer shows; the
            // shading model and the per-pixel program are for a SuperSpec runtime.
            if let Some(shading) = material.shading {
                spec.insert("shading".into(), json!(shading));
            }
            if let Some(program) = &material.program {
                spec.insert("program".into(), json!(program));
            }
            if !spec.is_empty() {
                spec.insert("version".into(), json!(1));
                entry["extras"] = json!({ "materialSpec": spec });
            }
            material_indices.insert(id, materials.len());
            materials.push(entry);
        }
        for (&id, &index) in &material_indices {
            let levels = &source_materials[id].lods;
            if levels.is_empty() {
                continue;
            }
            let ids: Vec<_> = levels
                .iter()
                .map(|level| material_indices[level.material.id.as_str()])
                .collect();
            materials[index]["extensions"]["MSFT_lod"] = json!({"ids":ids});
            let coverage: Vec<_> = levels
                .iter()
                .map(|level| level.screen_coverage)
                .chain([0.])
                .collect();
            materials[index]["extras"]["MSFT_screencoverage"] = json!(coverage);
        }
    }
    let mut meshes = vec![];
    let mut nodes = vec![];
    for mesh in selected {
        options.check(0)?;
        let a = mesh.vertex_start;
        let b = a + mesh.vertex_count;
        let position = buffer.floats(&scene.positions[a * 3..b * 3], 3, true)?;
        let normals = scene.normals[a * 3..b * 3]
            .as_chunks::<3>()
            .0
            .iter()
            .map(|n| unit(n.map(f64::from)).map(|n| n.map(|v| v as f32)))
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .flatten()
            .collect::<Vec<_>>();
        let normal = buffer.floats(&normals, 3, false)?;
        let mut attributes = json!({"POSITION":position,"NORMAL":normal});
        if visual {
            let uv = scene.uvs[a * 2..b * 2]
                .iter()
                .enumerate()
                .map(|(i, v)| if i % 2 == 0 { *v } else { 1. - v })
                .collect::<Vec<_>>();
            if uv.iter().any(|v| !v.is_finite()) {
                return Err("Export UV conversion overflow.".into());
            }
            attributes["TEXCOORD_0"] = json!(buffer.floats(&uv, 2, false)?);
            if let Some(lightmap) = &scene.lightmap_uvs {
                let uv1 = lightmap
                    .get(a * 2..b * 2)
                    .ok_or("Invalid lightmap UV range.")?;
                attributes["TEXCOORD_1"] = json!(buffer.floats(uv1, 2, false)?);
            }
            let colors = &scene.colors[a * 3..b * 3];
            if colors.iter().any(|v| *v > 1.) {
                attributes["_SUPERSPEC_HDR_COLOR"] = json!(buffer.floats(colors, 3, false)?);
                attributes["COLOR_0"] = json!(buffer.floats(
                    &colors.iter().map(|v| v.min(1.)).collect::<Vec<_>>(),
                    3,
                    false
                )?);
            } else {
                attributes["COLOR_0"] = json!(buffer.floats(colors, 3, false)?);
            }
            if scene.manifest.mesh_tangents.contains_key(&mesh.id)
                || mesh
                    .material
                    .as_deref()
                    .and_then(|id| source_materials.get(id))
                    .is_some_and(|m| {
                        m.normal_texture.is_some()
                            || m.lods
                                .iter()
                                .any(|lod| lod.material.normal_texture.is_some())
                    })
            {
                attributes["TANGENT"] =
                    json!(buffer.floats(&material_tangents(scene, mesh, options)?, 4, false)?);
            }
        }
        let indices = buffer
            .indices(&scene.indices[mesh.index_start..mesh.index_start + mesh.index_count])?;
        let mut primitive = json!({"attributes":attributes,"indices":indices,"mode":4});
        if let Some(index) = mesh
            .material
            .as_deref()
            .and_then(|id| material_indices.get(id))
        {
            primitive["material"] = json!(index);
        }
        let index = meshes.len();
        meshes.push(json!({"name":mesh.id,"primitives":[primitive]}));
        nodes.push(json!({"name":mesh.id,"mesh":index,"extras":{"meshId":mesh.id,"objectId":mesh.object_id,"source":mesh.source,"collidable":scene.manifest.colliders.contains(&mesh.id) && !alternate_meshes.contains(mesh.id.as_str())}}));
    }
    let instanced = write_batches(
        scene,
        &batches,
        &material_indices,
        &mut buffer,
        &mut meshes,
        &mut nodes,
    )?;
    if visual && !scene.manifest.render_visibility.is_empty() {
        for node in &mut nodes {
            if let Some(minimum) = node["extras"]["objectId"]
                .as_str()
                .and_then(|id| scene.manifest.render_visibility.get(id))
            {
                node["extras"]["worldSpecMinGraphics"] = json!(minimum);
            }
        }
    }
    // A visual export carries the scene's lighting record: its lights as nodes under the root,
    // so their directions stay in the domain's Z-up frame, and the whole record in the extras.
    let lighting = if visual {
        options.lighting.or(scene.manifest.lighting.as_ref())
    } else {
        None
    };
    let (lights, light_nodes) = lighting.map(crate::lighting::punctual).unwrap_or_default();
    nodes.extend(light_nodes);
    let children = if visual {
        geometry_lods(scene, &mut nodes)?
    } else {
        (0..nodes.len()).collect()
    };
    let root = nodes.len();
    nodes.push(json!({"name":scene.manifest.id,"rotation":[-std::f64::consts::FRAC_1_SQRT_2,0.,0.,std::f64::consts::FRAC_1_SQRT_2],"children":children }));
    let mut document = json!({"asset":{"version":"2.0","generator":"SuperSpec Rust"},"scene":0,"scenes":[{"nodes":[root]}],"nodes":nodes,"meshes":meshes,
        "buffers":[{"byteLength":buffer.bytes.len()}],"bufferViews":buffer.views,"accessors":buffer.accessors});
    if !lights.is_empty() {
        document["extensions"] = json!({ (crate::lighting::EXTENSION): { "lights": lights } });
    }
    if !materials.is_empty() {
        document["materials"] = json!(materials);
    }
    if !images.is_empty() {
        document["images"] = json!(images);
        document["textures"] = json!(textures);
        document["samplers"] =
            json!([{"magFilter":9729,"minFilter":9987,"wrapS":10497,"wrapT":10497}]);
    }
    let mut extensions = vec![];
    if ["nodes", "materials"].iter().any(|key| {
        document[*key].as_array().is_some_and(|items| {
            items
                .iter()
                .any(|v| v["extensions"]["MSFT_lod"].is_object())
        })
    }) {
        extensions.push("MSFT_lod");
    }
    if emissive_strength {
        extensions.push("KHR_materials_emissive_strength");
    }
    let mut required = vec![];
    if ktx2 {
        extensions.push("KHR_texture_basisu");
        required.push("KHR_texture_basisu");
    }
    if instanced {
        // Required: a loader without it would draw each batch once, at the origin.
        extensions.push(INSTANCING);
        required.push(INSTANCING);
    }
    if !required.is_empty() {
        document["extensionsRequired"] = json!(required);
    }
    if !lights.is_empty() {
        extensions.push(crate::lighting::EXTENSION);
    }
    if !extensions.is_empty() {
        document["extensionsUsed"] = json!(extensions);
    }
    let metadata = Metadata {
        schema_version: 1,
        id: &scene.manifest.id,
        purpose: if visual { "visual" } else { "collision" },
        source_axes: "right-handed-z-up",
        export_axes: "right-handed-y-up",
        units: "meters",
        architecture: &scene.manifest.architecture,
        geometry_validation: scene.manifest.geometry_validation,
        navigation_validation: scene.manifest.navigation_validation,
        diagnostics: &scene.manifest.diagnostics,
        lighting,
    };
    let wrapped = Document {
        gltf: &document,
        extras: Extras {
            superspec: metadata,
        },
    };
    let overhead = 28 + buffer.bytes.len() + 6; // Up to three padding bytes for each chunk.
    let mut json = crate::json::encode(&wrapped, options, overhead)?;
    while !json.len().is_multiple_of(4) {
        json.push(b' ');
    }
    while !buffer.bytes.len().is_multiple_of(4) {
        buffer.bytes.push(0);
    }
    let length = 28usize
        .checked_add(json.len())
        .and_then(|n| n.checked_add(buffer.bytes.len()))
        .ok_or("Export size overflow.")?;
    options.check(length)?;
    let mut output = Vec::with_capacity(length);
    for value in [0x46546c67, 2, length as u32, json.len() as u32, 0x4e4f534a] {
        output.extend(value.to_le_bytes());
    }
    output.extend(json);
    output.extend((buffer.bytes.len() as u32).to_le_bytes());
    output.extend(0x004e4942u32.to_le_bytes());
    output.extend(buffer.bytes);
    Ok(output)
}

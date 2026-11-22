use super::input::Input;
use serde_json::{Value, json};
use std::borrow::Cow;
use superspec_engine::authoring::{
    Diagnostic, Project, ProjectOutput, Session, occlusion_store::OcclusionStore,
};

/// Prepare only the occlusion planes consumed by this project's materials, using the same
/// engine algorithms and content-keyed cache as the browser reference implementation.
pub fn prepare_planes<'a>(
    session: &mut Session,
    store: &mut OcclusionStore,
    input: &'a Input,
    meshes: Option<&[String]>,
    revision: u32,
    cancelled: &dyn Fn() -> bool,
) -> Result<(Cow<'a, Project>, Option<Value>), Vec<Diagnostic>> {
    use superspec_engine::authoring::occlusion::{OcclusionJob, PLANES, bind_project_occlusion};
    let project = &input.project;
    if project.schema_version != 2 || project.kind != "material" || project.materials.is_empty() {
        return Ok((Cow::Borrowed(project), None));
    }
    let started = std::time::Instant::now();
    let lighting = input.lighting.clone().unwrap_or_default();
    let job = OcclusionJob {
        lighting: &lighting,
        settings: &Default::default(),
        names: &PLANES,
        held: &|key| PLANES.iter().any(|name| store.holds(name, key)),
    };
    let baked = session.project_occlusion_read(
        project,
        meshes,
        &job,
        revision.into(),
        &|n| input.asset(n),
        cancelled,
    )?;
    if baked.is_empty() {
        return Ok((Cow::Borrowed(project), None));
    }
    let mut project = project.clone();
    let added = bind_project_occlusion(&mut project, &baked);
    let mut used = Vec::new();
    let mut planes = Vec::new();
    for receiver in &baked {
        for plane in &receiver.planes {
            used.push(plane.asset());
            planes.push(json!({"mesh":receiver.id,"name":plane.name,"key":plane.key,"width":plane.width,"height":plane.height,
                "rays":plane.rays,"occluders":plane.occluders,"baked":plane.bytes.is_some()}));
        }
    }
    store.keep(&used, added);
    let report = json!({"ms":{"total":started.elapsed().as_secs_f64()*1000.0},"planes":planes});
    Ok((Cow::Owned(project), Some(report)))
}

pub fn build(
    session: &mut Session,
    store: &mut OcclusionStore,
    input: &Input,
    revision: u32,
    cancelled: &dyn Fn() -> bool,
) -> Result<(ProjectOutput, Option<Value>), Vec<Diagnostic>> {
    let (project, report) = prepare_planes(session, store, input, None, revision, cancelled)?;
    let output = session.build_project(
        &project,
        revision.into(),
        &|name| {
            store
                .get(name)
                .cloned()
                .map(Ok)
                .unwrap_or_else(|| input.asset(name))
        },
        cancelled,
    )?;
    Ok((output, report))
}

pub fn editor_packet(metadata: &Value, packet: Vec<u8>) -> Result<Vec<u8>, String> {
    let mut json = serde_json::to_vec(metadata).map_err(|e| e.to_string())?;
    if metadata.get("shadingBlocks").is_some() {
        // Permit aligned typed-array views over both f64 features and f32 charts.
        json.resize((4 + json.len()).next_multiple_of(8) - 4, b' ');
    }
    let length = u32::try_from(json.len()).map_err(|_| "Editor metadata is too large.")?;
    let mut bytes = Vec::with_capacity(4 + json.len() + packet.len());
    bytes.extend_from_slice(&length.to_le_bytes());
    bytes.extend(json);
    bytes.extend(packet);
    Ok(bytes)
}

/// Both the HTTP host and the batch CLI publish the same native build.
pub fn project(
    output: &ProjectOutput,
    input: &Input,
    revision: u32,
    action: &str,
    occlusion: Option<&Value>,
    textures: superspec_export::Textures,
    cancelled: &dyn Fn() -> bool,
) -> Result<Vec<u8>, String> {
    if action == "glb" || action == "glb-png" {
        return superspec_export::world_glb(
            &output.model,
            revision,
            &superspec_export::Options {
                cancelled,
                textures,
                lighting: input.lighting.as_ref(),
                batches: &input.batches,
                ..Default::default()
            },
        );
    }
    let mut scene = output.scene(revision)?;
    superspec_export::add_project_batches(&mut scene, &input.batches)?;
    if input.lighting.is_some() {
        scene.manifest.lighting.clone_from(&input.lighting);
    }
    match action {
        "scene" => Ok(scene.packet()),
        "editor-project" => editor_packet(&json!({"metadata":output.metadata,"occlusion":occlusion}), scene.packet()),
        "check" => serde_json::to_vec(&json!({"schemaVersion":1,"success":true,"diagnostics":scene.manifest.diagnostics,
            "metadata":output.metadata,"scene":{"id":scene.manifest.id,"meshes":scene.manifest.meshes.len(),
            "vertices":scene.manifest.vertex_count,"colliders":scene.manifest.colliders.len()}})).map_err(|error| error.to_string()),
        _ => Err(format!("Unknown publication operation {action}.")),
    }
}

//! Portable file formats. Domain geometry stays Z-up; each exporter owns axis conversion.
mod brushes;
mod gltf;
mod json;
mod lighting;
mod lightmap;
pub mod quake;
mod quake_textures;
mod tangents;
mod text;
pub use gltf::{TextureJob, encode_texture, glb, texture_jobs};
pub use lightmap::{prepare_lightmap, prepare_lightmap_sized};
pub use superspec_geometry::ktx2::Kind as TextureKind;
/// The instanced batches a project document carries, for `Options::batches`.
pub use superspec_model::batches::{
    ProjectBatch, add_project_batches, of_project as project_batches,
};
/// The lighting record a project document carries, for `Options::lighting` or a staged scene's
/// `Manifest::lighting`.
pub use superspec_model::lighting::{Lighting, of_project as project_lighting};
pub use text::{Obj, obj, svg_plan};

/// Prepares independent output from shared local geometry without modifying authored resources.
pub fn instance_glb(
    model: &superspec_model::instances::Model,
    options: &Options<'_>,
) -> Result<Vec<u8>, String> {
    options.check(0)?;
    let mut scene = model.scene(0)?;
    lit(&mut scene, options)?;
    glb(&scene, options)
}

/// Expands persistent world resources only for this output; no canonical inputs are mutated.
pub fn world_glb(
    model: &superspec_model::composition::WorldModel,
    revision: u32,
    options: &Options<'_>,
) -> Result<Vec<u8>, String> {
    options.check(0)?;
    let mut scene = model.scene_cancellable(revision, options.cancelled)?;
    lit(&mut scene, options)?;
    glb(&scene, options)
}

/// The delivered scene states the lighting record it is exported with (`Manifest::lighting`) and
/// holds the batches handed over with the export (`Options::batches`).
fn lit(scene: &mut superspec_model::Scene, options: &Options<'_>) -> Result<(), String> {
    if let Some(lighting) = options.lighting {
        scene.manifest.lighting = Some(lighting.clone());
    }
    superspec_model::batches::add_project_batches(scene, options.batches)
}

/// Splits the portable GLB into a JSON document and its referenced binary buffer.
pub fn gltf_files(glb: &[u8]) -> Result<(Vec<u8>, Vec<u8>), String> {
    if glb.len() < 28 || &glb[..4] != b"glTF" {
        return Err("Invalid generated GLB.".into());
    }
    let json_length = u32::from_le_bytes(glb[12..16].try_into().unwrap()) as usize;
    let binary_header = 20 + json_length;
    if binary_header + 8 > glb.len() {
        return Err("Invalid generated GLB chunks.".into());
    }
    let mut document: serde_json::Value =
        serde_json::from_slice(&glb[20..binary_header]).map_err(|e| e.to_string())?;
    document["buffers"][0]["uri"] = serde_json::json!("scene.bin");
    Ok((
        serde_json::to_vec_pretty(&document).map_err(|e| e.to_string())?,
        glb[binary_header + 8..].to_vec(),
    ))
}

/// Runtime JSON references a sibling scene.assets blob using the manifest's encoded byte ranges.
/// Instanced batches (`manifest.batches`) come with their buffers under `batches`, unexpanded.
pub fn runtime_json(scene: &superspec_model::Scene) -> Result<Vec<u8>, String> {
    let mut document = serde_json::json!({"schemaVersion":1,"manifest":scene.manifest,
        "positions":scene.positions,"normals":scene.normals,"uvs":scene.uvs,"lightmapUvs":scene.lightmap_uvs,
        "colors":scene.colors,"indices":scene.indices,"assetsUri":"scene.assets"});
    if !scene.batches.is_empty() {
        let batches = &scene.batches;
        document["batches"] = serde_json::json!({"positions":batches.positions,"normals":batches.normals,
            "uvs":batches.uvs,"colors":batches.colors,"indices":batches.indices,
            "placements":batches.placements,"values":batches.values,"tints":batches.tints,
            "attributes":batches.attributes});
    }
    serde_json::to_vec(&document).map_err(|e| e.to_string())
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Purpose {
    Visual,
    Collision,
}

/// How exported glTF stores texture images.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Textures {
    /// Lossless PNG (JPEG sources pass through): readable everywhere, including Blender.
    #[default]
    Png,
    /// KTX2 with Basis Universal payloads and mipmaps (`KHR_texture_basisu`, required): several
    /// times smaller on disk and in GPU memory; loaders need a KTX2 transcoder. On the web the
    /// codec must be loaded first (`superspec_geometry::ktx2::prepare`).
    Ktx2,
}

/// How a visual glTF stores instanced batches (`superspec_model::batches`). Collision exports
/// always write one node per colliding instance.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Instancing {
    /// `EXT_mesh_gpu_instancing`, required: one node per batch, its placements and per-instance
    /// attributes as accessors. Loaders without the extension must refuse the file.
    #[default]
    Extension,
    /// The fallback without the extension: one node per instance, every one referencing the
    /// batch's single mesh. Read everywhere; a consumer draws each instance on its own unless it
    /// batches equal meshes itself.
    Nodes,
}

pub struct Options<'a> {
    pub purpose: Purpose,
    /// How instanced batches are stored in a visual glTF.
    pub instancing: Instancing,
    /// Texture image format. The library default is PNG; user-facing exports choose KTX2.
    pub textures: Textures,
    /// Bounds the entire owned result, including embedded images and JSON metadata.
    pub maximum_bytes: usize,
    pub cancelled: &'a dyn Fn() -> bool,
    /// KTX2 payloads already encoded with `encode_texture`, by image identity (see
    /// `texture_jobs`); textures missing here are encoded during export.
    pub encoded: Option<&'a std::collections::BTreeMap<String, Vec<u8>>>,
    /// The lighting record the scene is exported with; it replaces the scene's own
    /// (`Manifest::lighting`). A visual GLB carries it as `KHR_lights_punctual` lights and whole
    /// under `extras.superspec.lighting`; without one the output has neither.
    pub lighting: Option<&'a Lighting>,
    /// Instanced batches `instance_glb` and `world_glb` add to the scene they deliver: a project
    /// document's (`project_batches`), which the authoring engine's project does not hold.
    pub batches: &'a [ProjectBatch],
}
impl Default for Options<'static> {
    fn default() -> Self {
        Self {
            purpose: Purpose::Visual,
            instancing: Instancing::Extension,
            textures: Textures::Png,
            maximum_bytes: superspec_types::limits::MAX_EXPORT_BYTES,
            cancelled: &|| false,
            encoded: None,
            lighting: None,
            batches: &[],
        }
    }
}
impl Options<'_> {
    pub(crate) fn check(&self, bytes: usize) -> Result<(), String> {
        if (self.cancelled)() {
            return Err("Export cancelled.".into());
        }
        if bytes > self.maximum_bytes || bytes > u32::MAX as usize {
            return Err(format!(
                "Export byte budget exceeded: {bytes} bytes requested, {} allowed.",
                self.maximum_bytes
            ));
        }
        Ok(())
    }
}

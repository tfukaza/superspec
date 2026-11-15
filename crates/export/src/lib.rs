//! Portable file formats. Domain geometry stays Z-up; each exporter owns axis conversion.
mod brushes;
mod gltf;
mod json;
mod lightmap;
pub mod quake;
mod quake_textures;
mod tangents;
mod text;
pub use gltf::glb;
pub use lightmap::{prepare_lightmap, prepare_lightmap_sized};
pub use text::{Obj, obj, svg_plan};

/// Prepares independent output from shared local geometry without modifying authored resources.
pub fn instance_glb(
    model: &superspec_model::instances::Model,
    options: &Options<'_>,
) -> Result<Vec<u8>, String> {
    options.check(0)?;
    let scene = model.scene(0)?;
    glb(&scene, options)
}

/// Expands persistent world resources only for this output; no canonical inputs are mutated.
pub fn world_glb(
    model: &superspec_model::composition::WorldModel,
    revision: u32,
    options: &Options<'_>,
) -> Result<Vec<u8>, String> {
    options.check(0)?;
    let scene = model.scene_cancellable(revision, options.cancelled)?;
    glb(&scene, options)
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
pub fn runtime_json(scene: &superspec_model::Scene) -> Result<Vec<u8>, String> {
    serde_json::to_vec(&serde_json::json!({"schemaVersion":1,"manifest":scene.manifest,
        "positions":scene.positions,"normals":scene.normals,"uvs":scene.uvs,"lightmapUvs":scene.lightmap_uvs,
        "colors":scene.colors,"indices":scene.indices,"assetsUri":"scene.assets"})).map_err(|e|e.to_string())
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

pub struct Options<'a> {
    pub purpose: Purpose,
    /// Texture image format. The library default is PNG; user-facing exports choose KTX2.
    pub textures: Textures,
    /// Bounds the entire owned result, including embedded images and JSON metadata.
    pub maximum_bytes: usize,
    pub cancelled: &'a dyn Fn() -> bool,
}
impl Default for Options<'static> {
    fn default() -> Self {
        Self {
            purpose: Purpose::Visual,
            textures: Textures::Png,
            maximum_bytes: superspec_types::limits::MAX_EXPORT_BYTES,
            cancelled: &|| false,
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

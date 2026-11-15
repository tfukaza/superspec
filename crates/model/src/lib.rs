//! Portable scene and typed delivery metadata shared by producers, preview and exporters.
pub mod architecture;
pub mod composition;
pub mod instances;
pub mod review;
pub mod theme;
pub mod updates;
pub mod validation;
pub mod visual;
use serde::Serialize;
use superspec_types::{
    architecture::{Node, V3},
    diagnostic::{Diagnostic, SourceLocation},
    material::Material,
};

/// Delivered source images plus generated maps, measured as RGBA8 base-level pixels.
/// Sampling and bake working sets have separate, smaller limits.
pub const MAX_SCENE_IMAGE_PIXELS: usize = superspec_types::limits::MAX_SCENE_IMAGE_PIXELS;
pub const MAX_SCENE_IMAGE_BYTES: usize = superspec_types::limits::MAX_SCENE_IMAGE_BYTES;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Mesh {
    pub id: String,
    /// Stable owner in architecture.objects, distinct from collision/material mesh identities.
    pub object_id: String,
    pub source: SourceLocation,
    pub material: Option<String>,
    pub visible: bool,
    pub vertex_start: usize,
    pub vertex_count: usize,
    pub index_start: usize,
    pub index_count: usize,
    pub bounds: [V3; 2],
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Asset {
    pub id: String,
    pub mime: &'static str,
    pub byte_start: usize,
    pub byte_length: usize,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LightmapAtlas {
    pub width: u32,
    pub height: u32,
    pub charts: u32,
    pub texels_per_meter: f32,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub architecture: Option<architecture::Context>,
    pub schema_version: u32,
    pub compiler: &'static str,
    pub revision: u32,
    pub success: bool,
    pub id: String,
    pub name: String,
    pub units: &'static str,
    pub axes: &'static str,
    pub geometry_validation: &'static str,
    pub navigation_validation: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub validation: Option<validation::Report>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lightmap: Option<LightmapAtlas>,
    pub diagnostics: Vec<Diagnostic>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub reviews: Vec<review::Finding>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub sign_plans: Vec<theme::signs::Plan>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub ground_plans: Vec<theme::ground::Plan>,
    pub materials: Vec<Material>,
    pub meshes: Vec<Mesh>,
    /// Optional authored tangent bases for imported meshes; image-space conversion does not alter them.
    #[serde(skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub mesh_tangents: std::collections::BTreeMap<String, Vec<[f32; 4]>>,
    /// Collision is explicitly selected; invisible meshes may still be physical.
    pub colliders: Vec<String>,
    pub openings: Vec<Node>,
    pub vertex_count: usize,
    pub index_count: usize,
    pub assets: Vec<Asset>,
    pub asset_byte_count: usize,
}

#[derive(Clone, Debug)]
pub struct ExportFace {
    pub points: [V3; 3],
    pub material: String,
    pub visible: bool,
    pub collidable: bool,
    pub dynamic: bool,
    pub object: String,
}

#[derive(Clone, Debug)]
pub struct ExportSolid {
    pub geometry: superspec_geometry::Geometry,
    pub faces: Vec<ExportFace>,
}

#[derive(Clone, Debug)]
pub struct Scene {
    pub manifest: Manifest,
    pub positions: Vec<f32>,
    pub normals: Vec<f32>,
    pub uvs: Vec<f32>,
    pub lightmap_uvs: Option<Vec<f32>>,
    pub lightmap_charts: Option<Vec<Option<u32>>>,
    /// Linear RGB multipliers; architectural vertices are white, procedural tints may exceed one.
    pub colors: Vec<f32>,
    pub asset_bytes: Vec<u8>,
    /// Indices are local to each mesh's vertex range.
    pub indices: Vec<u32>,
    /// Retained only by explicit brush delivery compilation.
    pub export_solids: Vec<ExportSolid>,
    visual_identity_bytes: usize,
    visual_ancestry_bytes: usize,
    image_pixels: usize,
    image_sizes: std::collections::BTreeMap<String, usize>,
}

impl Scene {
    pub fn image_pixels(&self) -> usize {
        self.image_pixels
    }

    /// Accounts admitted source images; procedural image publication uses the same allowance.
    pub fn reserve_image_pixels(&mut self, additional: usize) -> Result<(), String> {
        let total = self
            .image_pixels
            .checked_add(additional)
            .ok_or("Image pixel budget overflow.")?;
        if total > MAX_SCENE_IMAGE_PIXELS {
            return Err("Scene image pixel budget exceeded.".into());
        }
        self.image_pixels = total;
        Ok(())
    }

    pub fn empty(revision: u32, producer: &'static str) -> Self {
        Self {
            manifest: Manifest {
                architecture: None,
                schema_version: 3,
                compiler: producer,
                revision,
                success: false,
                id: String::new(),
                name: String::new(),
                units: "meters",
                axes: "right-handed-z-up",
                geometry_validation: "not-run",
                navigation_validation: "not-run",
                validation: None,
                lightmap: None,
                diagnostics: vec![],
                reviews: vec![],
                sign_plans: vec![],
                ground_plans: vec![],
                materials: vec![],
                meshes: vec![],
                mesh_tangents: Default::default(),
                colliders: vec![],
                openings: vec![],
                vertex_count: 0,
                index_count: 0,
                assets: vec![],
                asset_byte_count: 0,
            },
            positions: vec![],
            normals: vec![],
            uvs: vec![],
            lightmap_uvs: None,
            lightmap_charts: None,
            colors: vec![],
            asset_bytes: vec![],
            indices: vec![],
            export_solids: vec![],
            visual_identity_bytes: 0,
            visual_ancestry_bytes: 0,
            image_pixels: 0,
            image_sizes: Default::default(),
        }
    }

    /// LSP3: magic, LE u32 padded JSON byte count, UTF-8 JSON padded with spaces;
    /// XYZ f32 positions/normals, XY f32 UVs, RGB f32 linear colors, u32 indices, encoded assets.
    /// Counts in the manifest determine each contiguous payload length; no numeric JSON arrays.
    pub fn packet(&self) -> Vec<u8> {
        let mut json = serde_json::to_vec(&self.manifest).expect("finite manifest");
        while !json.len().is_multiple_of(4) {
            json.push(b' ');
        }
        let mut bytes = Vec::with_capacity(
            8 + json.len()
                + self.positions.len() * 8
                + self.uvs.len() * 4
                + self.colors.len() * 4
                + self.indices.len() * 4
                + self.asset_bytes.len(),
        );
        bytes.extend_from_slice(b"LSP3");
        bytes.extend_from_slice(&(json.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&json);
        for value in self
            .positions
            .iter()
            .chain(&self.normals)
            .chain(&self.uvs)
            .chain(&self.colors)
        {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        for value in &self.indices {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes.extend(&self.asset_bytes);
        bytes
    }
}

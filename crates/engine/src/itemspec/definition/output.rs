use super::limits::{MAX_SOURCE_INDICES, MAX_SOURCE_VERTICES, PlacedGeometry};
use super::{Collision, ItemDiagnostic, Transform, Values, fail};
use crate::materialspec::program::MaterialRequest;
use serde::{Serialize, Serializer};
use serde_json::Value;
use std::{collections::BTreeMap, sync::Arc};
use superspec_geometry::{
    Matrix,
    math::{add, cross, dot, scale, sub, unit},
};
use superspec_types::material::{AlphaMode, Material};

pub type Bounds = [[f64; 3]; 2];

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemMesh {
    pub id: String,
    pub positions: Vec<[f64; 3]>,
    pub normals: Vec<[f64; 3]>,
    pub uvs: Vec<[f64; 2]>,
    pub additional_uvs: BTreeMap<u32, Vec<[f64; 2]>>,
    pub colors: Vec<[f64; 4]>,
    pub tangents: Vec<[f64; 4]>,
    pub indices: Vec<u32>,
    pub material_slot: String,
}

impl ItemMesh {
    /// Index equal native mesh corners in source vertex order. Include every
    /// populated vertex stream so UV, normal, color and tangent seams survive.
    /// Distinct unreferenced vertices still contribute to the mesh bounds.
    pub fn share_vertices(&mut self) {
        self.index_vertices(true);
    }

    pub(super) fn index_vertices(&mut self, preserve_signed_zero: bool) {
        let count = self.positions.len();
        let optional = |len| len == 0 || len == count;
        if count < 2
            || self.normals.len() != count
            || !optional(self.uvs.len())
            || !optional(self.colors.len())
            || !optional(self.tangents.len())
            || self.additional_uvs.values().any(|uvs| uvs.len() != count)
            || self.indices.iter().any(|&index| index as usize >= count)
        {
            // Leave malformed streams intact for the caller's validation.
            return;
        }
        // The common native mesh has position, normal, UV and color. Use a
        // fixed-size key so indexing it needs no per-vertex heap allocation.
        // Transforming a face can produce either sign of zero for the same
        // geometric value. Keep the first corner's original attributes.
        let bits = |v: f64| {
            if !preserve_signed_zero && v == 0. {
                0
            } else {
                v.to_bits()
            }
        };
        let (kept, renamed) = if self.tangents.is_empty() && self.additional_uvs.is_empty() {
            vertex_remap(count, |at| {
                (
                    self.positions[at].map(bits),
                    self.normals[at].map(bits),
                    self.uvs.get(at).copied().unwrap_or_default().map(bits),
                    self.colors.get(at).copied().unwrap_or_default().map(bits),
                )
            })
        } else {
            vertex_remap(count, |at| {
                self.positions[at]
                    .iter()
                    .chain(&self.normals[at])
                    .chain(self.uvs.get(at).into_iter().flatten())
                    .chain(self.colors.get(at).into_iter().flatten())
                    .chain(self.tangents.get(at).into_iter().flatten())
                    .chain(self.additional_uvs.values().flat_map(|uvs| &uvs[at]))
                    .map(|&v| bits(v))
                    .collect::<Vec<_>>()
            })
        };
        if kept.len() == count {
            return;
        }
        fn keep<T: Copy>(values: &mut Vec<T>, kept: &[usize], count: usize) {
            if values.len() == count {
                for (to, &from) in kept.iter().enumerate() {
                    values[to] = values[from];
                }
                values.truncate(kept.len());
            }
        }
        keep(&mut self.positions, &kept, count);
        keep(&mut self.normals, &kept, count);
        keep(&mut self.uvs, &kept, count);
        keep(&mut self.colors, &kept, count);
        keep(&mut self.tangents, &kept, count);
        for uvs in self.additional_uvs.values_mut() {
            keep(uvs, &kept, count);
        }
        for index in &mut self.indices {
            *index = renamed[*index as usize];
        }
    }
}
fn vertex_remap<K: Eq + std::hash::Hash>(
    count: usize,
    key: impl Fn(usize) -> K,
) -> (Vec<usize>, Vec<u32>) {
    let mut first = std::collections::HashMap::with_capacity(count);
    let mut kept = Vec::with_capacity(count);
    let renamed = (0..count)
        .map(|at| {
            *first.entry(key(at)).or_insert_with(|| {
                kept.push(at);
                (kept.len() - 1) as u32
            })
        })
        .collect();
    (kept, renamed)
}

/// Preserves the tangent basis of a normal-mapped glTF primitive before delivery
/// adapters change UV conventions. Authored tangents take precedence. Otherwise
/// derive the basis from the original UVs, including mirrored chart handedness.
pub fn delivery_tangents(mesh: &ItemMesh) -> Result<Vec<[f64; 4]>, String> {
    let count = mesh.positions.len();
    if count > MAX_SOURCE_VERTICES
        || mesh.normals.len() != count
        || mesh.uvs.len() != count
        || !mesh.indices.len().is_multiple_of(3)
        || mesh.indices.len() > MAX_SOURCE_INDICES
        || mesh.indices.iter().any(|i| *i as usize >= count)
    {
        return Err("Invalid tangent source geometry.".into());
    }
    if !mesh.tangents.is_empty() {
        if mesh.tangents.len() != count {
            return Err("Tangent count does not match its vertices.".into());
        }
        return Ok(mesh.tangents.clone());
    }
    let mut tangents = vec![[0.; 3]; count];
    let mut bitangents = vec![[0.; 3]; count];
    for triangle in mesh.indices.chunks_exact(3) {
        let [a, b, c] = [
            triangle[0] as usize,
            triangle[1] as usize,
            triangle[2] as usize,
        ];
        let e1 = sub(mesh.positions[b], mesh.positions[a]);
        let e2 = sub(mesh.positions[c], mesh.positions[a]);
        let [u1, v1] = [
            mesh.uvs[b][0] - mesh.uvs[a][0],
            mesh.uvs[b][1] - mesh.uvs[a][1],
        ];
        let [u2, v2] = [
            mesh.uvs[c][0] - mesh.uvs[a][0],
            mesh.uvs[c][1] - mesh.uvs[a][1],
        ];
        let determinant = u1 * v2 - u2 * v1;
        if !determinant.is_finite() {
            return Err("Nonfinite tangent texture coordinates.".into());
        }
        if determinant.abs() < 1e-12 {
            continue;
        }
        let t = scale(sub(scale(e1, v2), scale(e2, v1)), 1. / determinant);
        let b_axis = scale(sub(scale(e2, u1), scale(e1, u2)), 1. / determinant);
        for index in [a, b, c] {
            tangents[index] = add(tangents[index], t);
            bitangents[index] = add(bitangents[index], b_axis);
        }
    }
    mesh.normals
        .iter()
        .zip(tangents)
        .zip(bitangents)
        .map(|((&normal, tangent), bitangent)| {
            if tangent.iter().chain(&bitangent).any(|v| !v.is_finite()) {
                return Err("Tangent accumulation overflow.".into());
            }
            let normal = unit(normal)?;
            let tangent =
                unit(sub(tangent, scale(normal, dot(normal, tangent)))).or_else(|_| {
                    let axis = (0..3)
                        .min_by(|a, b| normal[*a].abs().total_cmp(&normal[*b].abs()))
                        .unwrap();
                    let mut basis = [0.; 3];
                    basis[axis] = 1.;
                    unit(sub(basis, scale(normal, dot(normal, basis))))
                })?;
            let sign = if dot(cross(normal, tangent), bitangent) < 0. {
                -1.
            } else {
                1.
            };
            Ok([tangent[0], tangent[1], tangent[2], sign])
        })
        .collect()
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemNode {
    pub id: String,
    pub name: Option<String>,
    pub parent: Option<usize>,
    pub transform: Transform,
    pub meshes: Vec<usize>,
    pub extras: Option<Value>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportedTexture {
    pub image: String,
    pub tex_coord: u32,
    pub sampler: Value,
    pub transform: Option<Value>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportedMaterial {
    pub id: String,
    pub name: Option<String>,
    pub base_color_factor: [f64; 4],
    pub metallic_factor: f64,
    pub roughness_factor: f64,
    pub emissive_factor: [f64; 3],
    pub alpha_mode: String,
    pub alpha_cutoff: f64,
    pub double_sided: bool,
    pub normal_scale: f64,
    pub occlusion_strength: f64,
    pub textures: BTreeMap<String, ImportedTexture>,
    pub extensions: BTreeMap<String, Value>,
}
impl ImportedMaterial {
    pub fn plain(id: &str) -> Self {
        Self {
            id: id.into(),
            name: None,
            base_color_factor: [1.; 4],
            metallic_factor: 1.,
            roughness_factor: 1.,
            emissive_factor: [0.; 3],
            alpha_mode: "OPAQUE".into(),
            alpha_cutoff: 0.5,
            double_sided: false,
            normal_scale: 1.,
            occlusion_strength: 1.,
            textures: BTreeMap::new(),
            extensions: BTreeMap::new(),
        }
    }
    /// Checks the narrower UV/sampler contract of the compatibility scene renderer.
    /// The imported record itself retains this state for other consumers.
    pub fn ensure_scene_compatible(&self) -> Result<(), String> {
        for extension in self.extensions.keys() {
            if extension != "KHR_materials_emissive_strength" {
                return Err(format!(
                    "Material {} uses unsupported scene extension {extension}.",
                    self.id
                ));
            }
        }
        if self.normal_scale < 0. {
            return Err("The scene renderer requires a nonnegative normal scale.".into());
        }
        for (channel, texture) in &self.textures {
            if texture.tex_coord != 0 {
                return Err(format!(
                    "Material {} {channel} requires UV{}, while the scene renderer supports UV0.",
                    self.id, texture.tex_coord
                ));
            }
            for (field, value) in texture.sampler.as_object().into_iter().flatten() {
                let supported = match field.as_str() {
                    "wrapS" | "wrapT" => value.as_u64() == Some(10497),
                    "magFilter" => value.as_u64() == Some(9729),
                    "minFilter" => value.as_u64() == Some(9987),
                    "name" | "extras" => true,
                    _ => false,
                };
                if !supported {
                    return Err(format!(
                        "Material {} {channel} has an unsupported scene sampler {field}.",
                        self.id
                    ));
                }
            }
            if let Some(transform) = &texture.transform {
                let offset = transform
                    .get("offset")
                    .cloned()
                    .unwrap_or(serde_json::json!([0., 0.]));
                let scale = transform
                    .get("scale")
                    .cloned()
                    .unwrap_or(serde_json::json!([1., 1.]));
                let pair = |value: &Value, expected: f64| {
                    value.as_array().is_some_and(|a| {
                        a.len() == 2 && a.iter().all(|v| v.as_f64() == Some(expected))
                    })
                };
                if !pair(&offset, 0.)
                    || !pair(&scale, 1.)
                    || transform
                        .get("rotation")
                        .and_then(Value::as_f64)
                        .unwrap_or(0.)
                        != 0.
                {
                    return Err(format!(
                        "Material {} {channel} requires a texture transform unsupported by the scene renderer.",
                        self.id
                    ));
                }
            }
        }
        Ok(())
    }
    /// Call `ensure_scene_compatible` before handing this metadata to the compatibility renderer.
    pub fn material(&self) -> Material {
        let color = |rgb: [f64; 3]| {
            format!(
                "#{:02x}{:02x}{:02x}",
                byte(rgb[0]),
                byte(rgb[1]),
                byte(rgb[2])
            )
        };
        let texture = |name| self.textures.get(name).map(|t| t.image.clone());
        let base = [
            self.base_color_factor[0],
            self.base_color_factor[1],
            self.base_color_factor[2],
        ];
        let emissive_texture = texture("emissive");
        Material {
            id: self.id.clone(),
            color: color(base),
            base_color_factor: Some(base),
            roughness: self.roughness_factor,
            metalness: self.metallic_factor,
            emissive: Some(color(self.emissive_factor)),
            emissive_factor: Some(self.emissive_factor),
            emissive_strength: self
                .extensions
                .get("KHR_materials_emissive_strength")
                .and_then(|v| v["emissiveStrength"].as_f64()),
            emissive_texture,
            texture: texture("baseColor"),
            normal_texture: texture("normal"),
            normal_scale: self
                .textures
                .contains_key("normal")
                .then_some(self.normal_scale),
            metallic_roughness_texture: texture("metallicRoughness"),
            occlusion_texture: texture("occlusion"),
            occlusion_strength: self
                .textures
                .contains_key("occlusion")
                .then_some(self.occlusion_strength),
            opacity: (self.alpha_mode != "OPAQUE").then_some(self.base_color_factor[3]),
            alpha_mode: Some(match self.alpha_mode.as_str() {
                "MASK" => AlphaMode::Mask,
                "BLEND" => AlphaMode::Blend,
                _ => AlphaMode::Opaque,
            }),
            alpha_cutoff: (self.alpha_mode == "MASK").then_some(self.alpha_cutoff),
            double_sided: Some(self.double_sided),
            ..Material::default()
        }
    }
}
fn byte(value: f64) -> u8 {
    crate::materialspec::surface::color_byte(value).unwrap_or(0)
}
fn bytes<S: Serializer>(value: &Arc<[u8]>, serializer: S) -> Result<S::Ok, S::Error> {
    value.as_ref().serialize(serializer)
}
fn geometry<S: Serializer>(value: &Arc<ItemGeometry>, serializer: S) -> Result<S::Ok, S::Error> {
    value.as_ref().serialize(serializer)
}

#[derive(Clone, Debug, Serialize)]
pub struct ItemImage {
    pub id: String,
    pub mime: String,
    #[serde(serialize_with = "bytes")]
    pub bytes: Arc<[u8]>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemGeometry {
    pub meshes: Vec<ItemMesh>,
    pub nodes: Vec<ItemNode>,
    pub bounds: Bounds,
    pub imported_materials: BTreeMap<String, ImportedMaterial>,
    pub images: Vec<ItemImage>,
    /// Version 2: one record per part, in definition order.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub parts: Vec<ItemPart>,
    /// Version 2: named features in the same space as `bounds` and attachments.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub features: Vec<Feature>,
    /// Version 2: one pattern anchor frame per part that has one.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub anchors: Vec<PatternAnchor>,
}

/// A part's share of an item's geometry. Its meshes are in part coordinates; the node
/// carries the part placement and the item transform.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemPart {
    pub id: String,
    /// Hash of every resolved value this part reads, including its placement and slots.
    pub geometry_key: String,
    pub node: usize,
    pub meshes: Vec<usize>,
}

/// Where an effect may go, supplied by the geometry's owner. `name` is one of `joint`,
/// `end`, `bracket-contact`, `opening-edge`, `sill`, `base` and `top`. The feature is a
/// polyline (closed when `closed`); a later stage maps it to MaterialSpec receiver features.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Feature {
    pub name: String,
    pub part: String,
    /// The opening or fitting that produced it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    /// The face group the feature lies on.
    pub face: String,
    pub points: Vec<[f64; 3]>,
    pub closed: bool,
    /// The surface normal for panel features; the route direction for route features.
    pub normal: [f64; 3],
    /// Extent across the polyline: a fitting's length along its route, otherwise zero.
    pub width: f64,
    /// Whose receivers are given the feature: its own part's (what every generated feature
    /// has), or also those of other parts and items that come within `within` meters of it.
    #[serde(skip_serializing_if = "super::Reach::is_part")]
    pub reach: super::Reach,
    /// How near a receiver outside the feature's part must come to be given it, in the
    /// feature's own space; zero for a feature that reaches its part alone.
    #[serde(skip_serializing_if = "is_zero")]
    pub within: f64,
    /// The weathering zone hanging below the polyline, in the feature's space (its `down`
    /// turns with the points; `id` is a seed until the feature is placed, see
    /// `authoring::item_features`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub zone: Option<crate::materialspec::program::Zone>,
}
impl Feature {
    /// The feature's points and zone direction through `matrix`.
    pub(super) fn place_zone(&mut self, matrix: superspec_geometry::Matrix) -> Result<(), String> {
        if let Some(zone) = &mut self.zone {
            zone.down = superspec_geometry::math::unit(matrix.direction(zone.down))?;
        }
        Ok(())
    }
}
fn is_zero(value: &f64) -> bool {
    *value == 0.
}

/// The frame a part's surface pattern is fixed to, so a resize does not shift it.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PatternAnchor {
    pub part: String,
    pub origin: [f64; 3],
    pub u: [f64; 3],
    pub v: [f64; 3],
    pub normal: [f64; 3],
}

/// A placed decal: a render-only sheet standing off its surface, in the space of `bounds`.
/// The image is named, never embedded: one image serves every placement, and a placement
/// is no part of an image or geometry identity.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemDecal {
    pub id: String,
    pub part: String,
    pub face: String,
    pub image: String,
    pub layer: u32,
    /// Width by height in meters.
    pub size: [f64; 2],
    /// Distance from the surface in meters: 2 mm plus 0.6 mm per layer.
    pub offset: f64,
    pub positions: Vec<[f64; 3]>,
    pub normals: Vec<[f64; 3]>,
    /// The image's lower-left corner is (0, 0).
    pub uvs: Vec<[f64; 2]>,
    pub indices: Vec<u32>,
    /// The sheet's four corners (lower left, lower right, upper right, upper left), for overlays.
    pub corners: [[f64; 3]; 4],
}

/// A declared link from a dimension to a control, resolved for the current values:
/// moving `position` by `d` meters along `axis` changes `parameter` by `d * scale`.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemHandle {
    pub id: String,
    pub parameter: String,
    pub position: [f64; 3],
    pub axis: [f64; 3],
    pub scale: f64,
    pub value: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max: Option<f64>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Attachment {
    pub name: String,
    pub position: [f64; 3],
    pub direction: [f64; 3],
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Provenance {
    pub definition_id: String,
    pub source: String,
    pub parameters: Values,
    pub assets: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GeneratedItem {
    pub id: String,
    pub geometry_key: String,
    #[serde(serialize_with = "geometry")]
    pub geometry: Arc<ItemGeometry>,
    pub materials: BTreeMap<String, MaterialRequest>,
    pub attachments: Vec<Attachment>,
    /// Explicit collision shapes use definition coordinates and this item's transform.
    pub collision: Collision,
    pub transform: Transform,
    pub provenance: Provenance,
    /// Version 2: placed decals. They are renewed with the bindings, never part of `geometry`.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub decals: Vec<ItemDecal>,
    /// Version 2: drag handles for the current control values.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub handles: Vec<ItemHandle>,
    /// Weathering layers over each slot's material, lowest first, including those of the
    /// item's sub-items under the slots they land in.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub layers: BTreeMap<String, Vec<super::LayerRequest>>,
    /// Texels per meter each slot asks for where its host states none, including what the
    /// item's sub-items ask for the slots they land in.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub densities: BTreeMap<String, f64>,
}
impl GeneratedItem {
    pub fn with_material_overrides(
        &self,
        overrides: &BTreeMap<String, MaterialRequest>,
    ) -> Result<Self, ItemDiagnostic> {
        let mut result = self.clone();
        for (slot, request) in overrides {
            if !self
                .geometry
                .meshes
                .iter()
                .any(|mesh| &mesh.material_slot == slot)
            {
                return Err(fail(
                    &self.provenance.source,
                    &format!("/slots/{}", super::pointer_key(slot)),
                    "ITEM_MATERIAL",
                    "Unknown material slot override.",
                ));
            }
            if request.entry.is_empty()
                || request.entry.len() > 1024
                || request.parameters.len() > 64
            {
                return Err(fail(
                    &self.provenance.source,
                    &format!("/slots/{}", super::pointer_key(slot)),
                    "ITEM_MATERIAL",
                    "Invalid material override.",
                ));
            }
            result.materials.insert(slot.clone(), request.clone());
        }
        Ok(result)
    }
    pub fn flattened(&self) -> Result<Vec<ItemMesh>, ItemDiagnostic> {
        self.geometry.flattened().map_err(|message| {
            fail(
                &self.provenance.source,
                "/generator",
                "ITEM_TRANSFORM",
                message,
            )
        })
    }
}

impl ItemGeometry {
    pub fn world_transforms(&self) -> Result<Vec<Matrix>, String> {
        let mut result = Vec::<Matrix>::with_capacity(self.nodes.len());
        for (index, node) in self.nodes.iter().enumerate() {
            let local = Matrix(node.transform).checked()?;
            let matrix = if let Some(parent) = node.parent {
                if parent >= index {
                    return Err("Item nodes must be ordered after their parent.".into());
                }
                result[parent].then(local).checked()?
            } else {
                local
            };
            matrix.inverse()?;
            result.push(matrix);
        }
        Ok(result)
    }
    pub fn flattened(&self) -> Result<Vec<ItemMesh>, String> {
        let transforms = self.world_transforms()?;
        let mut result = Vec::new();
        let mut placed = PlacedGeometry::default();
        for (node, matrix) in self.nodes.iter().zip(transforms) {
            let inverse = matrix.inverse()?;
            for &index in &node.meshes {
                let source = self.meshes.get(index).ok_or("Unknown item mesh.")?;
                placed.add(&node.id, source.positions.len(), source.indices.len() / 3)?;
                let mut mesh = source.clone();
                mesh.id = format!("{}/{}", node.id, mesh.id);
                for position in &mut mesh.positions {
                    *position = matrix.point(*position);
                    if position.iter().any(|v| !v.is_finite() || v.abs() > 1e7) {
                        return Err("Transformed item position exceeds coordinate bounds.".into());
                    }
                }
                for normal in &mut mesh.normals {
                    *normal = unit([
                        dot(inverse.0[0], *normal),
                        dot(inverse.0[1], *normal),
                        dot(inverse.0[2], *normal),
                    ])?;
                }
                for tangent in &mut mesh.tangents {
                    let direction = unit(matrix.direction([tangent[0], tangent[1], tangent[2]]))?;
                    *tangent = [
                        direction[0],
                        direction[1],
                        direction[2],
                        tangent[3] * matrix.determinant().signum(),
                    ];
                }
                if matrix.determinant() < 0. {
                    for triangle in mesh.indices.chunks_exact_mut(3) {
                        triangle.swap(1, 2);
                    }
                }
                result.push(mesh);
            }
        }
        Ok(result)
    }
    pub(super) fn measure(&mut self) -> Result<(), String> {
        let transforms = self.world_transforms()?;
        let mut bounds = [[f64::INFINITY; 3], [f64::NEG_INFINITY; 3]];
        let mut placed = PlacedGeometry::default();
        for (node, matrix) in self.nodes.iter().zip(transforms) {
            for &index in &node.meshes {
                let mesh = self.meshes.get(index).ok_or("Unknown item mesh.")?;
                placed.add(&node.id, mesh.positions.len(), mesh.indices.len() / 3)?;
                for p in &mesh.positions {
                    let p = matrix.point(*p);
                    if p.iter().any(|v| !v.is_finite() || v.abs() > 1e7) {
                        return Err("Transformed item position exceeds coordinate bounds.".into());
                    }
                    for axis in 0..3 {
                        bounds[0][axis] = bounds[0][axis].min(p[axis]);
                        bounds[1][axis] = bounds[1][axis].max(p[axis]);
                    }
                }
            }
        }
        if placed.vertices == 0 || placed.triangles == 0 {
            return Err("Item contains no visible triangle geometry.".into());
        }
        self.bounds = bounds;
        Ok(())
    }
}

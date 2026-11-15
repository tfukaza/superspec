//! Bounded geometry-only GLB imports in canonical Z-up metres.
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use superspec_geometry::{Geometry, math::*};

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct Gltf {
    scene: Option<usize>,
    #[serde(default)]
    scenes: Vec<Scene>,
    #[serde(default)]
    nodes: Vec<Node>,
    #[serde(default)]
    meshes: Vec<Mesh>,
    #[serde(default)]
    accessors: Vec<Accessor>,
    #[serde(default)]
    buffer_views: Vec<View>,
    #[serde(default)]
    buffers: Vec<Buffer>,
}
#[derive(Deserialize)]
struct Scene {
    #[serde(default)]
    nodes: Vec<usize>,
}
#[derive(Deserialize)]
struct Node {
    mesh: Option<usize>,
    #[serde(default)]
    children: Vec<usize>,
    matrix: Option<Vec<f64>>,
    translation: Option<[f64; 3]>,
    rotation: Option<[f64; 4]>,
    scale: Option<[f64; 3]>,
}
#[derive(Deserialize)]
struct Mesh {
    primitives: Vec<Primitive>,
}
#[derive(Deserialize)]
struct Primitive {
    attributes: BTreeMap<String, usize>,
    indices: Option<usize>,
    mode: Option<u32>,
    #[serde(default)]
    extensions: BTreeMap<String, serde_json::Value>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Accessor {
    buffer_view: Option<usize>,
    #[serde(default)]
    byte_offset: usize,
    component_type: u32,
    count: usize,
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    normalized: bool,
    sparse: Option<serde_json::Value>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct View {
    buffer: usize,
    #[serde(default)]
    byte_offset: usize,
    byte_length: usize,
    byte_stride: Option<usize>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Buffer {
    byte_length: usize,
    uri: Option<String>,
}
fn u32_at(bytes: &[u8], offset: usize) -> Result<u32, String> {
    let value = bytes.get(offset..offset + 4).ok_or("Truncated GLB word.")?;
    Ok(u32::from_le_bytes(value.try_into().unwrap()))
}
impl Gltf {
    fn accessor(&self, bin: &[u8], index: usize, kind: &str) -> Result<Vec<Vec<f64>>, String> {
        let a = self.accessors.get(index).ok_or("Missing GLB accessor.")?;
        if a.kind != kind || a.count > 750000 || a.normalized || a.sparse.is_some() {
            return Err(
                "Unsupported GLB accessor type, count, normalization or sparse data.".into(),
            );
        }
        let components = if kind == "VEC3" { 3 } else { 1 };
        let size = match a.component_type {
            5121 => 1,
            5123 => 2,
            5125 | 5126 => 4,
            _ => return Err("Unsupported GLB component type.".into()),
        };
        if (kind == "VEC3" && a.component_type != 5126)
            || (kind == "SCALAR" && a.component_type == 5126)
        {
            return Err(
                "GLB positions require FLOAT and indices require unsigned integers.".into(),
            );
        }
        let view = self
            .buffer_views
            .get(a.buffer_view.ok_or("Accessor has no buffer view.")?)
            .ok_or("Missing GLB buffer view.")?;
        let buffer = self.buffers.first().ok_or("Missing GLB buffer.")?;
        if view.buffer != 0 || buffer.uri.is_some() || buffer.byte_length > bin.len() {
            return Err("GLB geometry must use its embedded buffer.".into());
        }
        let packed = components * size;
        let stride = view.byte_stride.unwrap_or(packed);
        if stride < packed || stride % size != 0 {
            return Err("Invalid GLB accessor stride.".into());
        }
        let start = view
            .byte_offset
            .checked_add(a.byte_offset)
            .ok_or("GLB offset overflow.")?;
        let end = start
            .checked_add(
                a.count
                    .saturating_sub(1)
                    .checked_mul(stride)
                    .ok_or("GLB count overflow.")?,
            )
            .and_then(|n| n.checked_add(if a.count == 0 { 0 } else { packed }))
            .ok_or("GLB range overflow.")?;
        let view_end = view
            .byte_offset
            .checked_add(view.byte_length)
            .ok_or("GLB view overflow.")?;
        if end > bin.len() || end > view_end || end > buffer.byte_length || start < view.byte_offset
        {
            return Err("GLB accessor exceeds its buffer view.".into());
        }
        let mut result = Vec::with_capacity(a.count);
        for i in 0..a.count {
            let mut row = Vec::with_capacity(components);
            for c in 0..components {
                let offset = start + i * stride + c * size;
                let n = match a.component_type {
                    5121 => bin[offset] as f64,
                    5123 => u16::from_le_bytes(bin[offset..offset + 2].try_into().unwrap()) as f64,
                    5125 => u32_at(bin, offset)? as f64,
                    5126 => f32::from_le_bytes(bin[offset..offset + 4].try_into().unwrap()) as f64,
                    _ => unreachable!(),
                };
                if !n.is_finite() {
                    return Err("Nonfinite GLB vertex.".into());
                }
                row.push(n);
            }
            result.push(row);
        }
        Ok(result)
    }
}
fn transform(n: &Node) -> Result<Matrix, String> {
    if let Some(m) = &n.matrix {
        if m.len() != 16
            || m.iter().any(|n| !n.is_finite())
            || m[3] != 0.
            || m[7] != 0.
            || m[11] != 0.
            || m[15] != 1.
        {
            return Err("Invalid affine GLB node matrix.".into());
        }
        return Matrix([
            [m[0], m[1], m[2]],
            [m[4], m[5], m[6]],
            [m[8], m[9], m[10]],
            [m[12], m[13], m[14]],
        ])
        .checked();
    }
    let [mut x, mut y, mut z, mut w] = n.rotation.unwrap_or([0., 0., 0., 1.]);
    let length = (x * x + y * y + z * z + w * w).sqrt();
    if length == 0. || !length.is_finite() {
        return Err("Invalid GLB rotation.".into());
    }
    x /= length;
    y /= length;
    z /= length;
    w /= length;
    let s = n.scale.unwrap_or([1.; 3]);
    Matrix([
        scale(
            [
                1. - 2. * (y * y + z * z),
                2. * (x * y + w * z),
                2. * (x * z - w * y),
            ],
            s[0],
        ),
        scale(
            [
                2. * (x * y - w * z),
                1. - 2. * (x * x + z * z),
                2. * (y * z + w * x),
            ],
            s[1],
        ),
        scale(
            [
                2. * (x * z + w * y),
                2. * (y * z - w * x),
                1. - 2. * (x * x + y * y),
            ],
            s[2],
        ),
        n.translation.unwrap_or([0.; 3]),
    ])
    .checked()
}
struct Import<'a> {
    gltf: &'a Gltf,
    bin: &'a [u8],
    geometry: Geometry,
    visits: usize,
    cancelled: &'a dyn Fn() -> bool,
}
impl Import<'_> {
    fn walk(
        &mut self,
        index: usize,
        parent: Matrix,
        stack: &mut BTreeSet<usize>,
    ) -> Result<(), String> {
        if (self.cancelled)() {
            return Err("GLB import cancelled.".into());
        }
        self.visits += 1;
        if self.visits > 65536 || stack.len() >= 128 || !stack.insert(index) {
            return Err("GLB hierarchy exceeds traversal bounds or contains a cycle.".into());
        }
        let n = self
            .gltf
            .nodes
            .get(index)
            .ok_or("Missing GLB scene node.")?;
        let world = parent.then(transform(n)?).checked()?;
        if let Some(mesh) = n.mesh {
            for p in &self
                .gltf
                .meshes
                .get(mesh)
                .ok_or("Missing GLB mesh.")?
                .primitives
            {
                if p.mode.unwrap_or(4) != 4
                    || p.extensions.contains_key("KHR_draco_mesh_compression")
                {
                    return Err("GLB requires uncompressed triangle primitives.".into());
                }
                let positions = self.gltf.accessor(
                    self.bin,
                    *p.attributes
                        .get("POSITION")
                        .ok_or("GLB primitive lacks POSITION.")?,
                    "VEC3",
                )?;
                let indices = if let Some(index) = p.indices {
                    self.gltf
                        .accessor(self.bin, index, "SCALAR")?
                        .iter()
                        .map(|r| r[0] as usize)
                        .collect::<Vec<_>>()
                } else {
                    (0..positions.len()).collect()
                };
                if indices.len() % 3 != 0
                    || self.geometry.triangles.len() + indices.len() / 3 > 250000
                {
                    return Err("GLB triangle count exceeds its allowance or is incomplete.".into());
                }
                let mut remap = BTreeMap::new();
                for t in indices.chunks_exact(3) {
                    let mut face = [0; 3];
                    for (corner, &old) in t.iter().enumerate() {
                        if let Some(&known) = remap.get(&old) {
                            face[corner] = known;
                            continue;
                        }
                        let p = positions
                            .get(old)
                            .ok_or("GLB index references a missing vertex.")?;
                        let p = world.point([p[0], p[1], p[2]]);
                        if p.iter().any(|n| !n.is_finite() || n.abs() > 1e7) {
                            return Err("GLB transformed coordinates exceed bounds.".into());
                        }
                        let next = self.geometry.points.len();
                        self.geometry.points.push([p[0], -p[2], p[1]]);
                        remap.insert(old, next);
                        face[corner] = next;
                    }
                    if world.determinant() < 0. {
                        face.swap(1, 2);
                    }
                    self.geometry.triangles.push(face);
                }
            }
        }
        for &child in &n.children {
            self.walk(child, world, stack)?;
        }
        stack.remove(&index);
        Ok(())
    }
}
pub fn glb(bytes: &[u8], cancelled: &dyn Fn() -> bool) -> Result<Geometry, String> {
    if bytes.len() < 20
        || bytes.len() > superspec_types::limits::MAX_FILE_BYTES
        || u32_at(bytes, 0)? != 0x46546c67
        || u32_at(bytes, 4)? != 2
        || u32_at(bytes, 8)? as usize != bytes.len()
    {
        return Err("Expected a bounded glTF 2.0 GLB container.".into());
    }
    let mut json = None;
    let mut bin = None;
    let mut at = 12;
    while at < bytes.len() {
        let length = u32_at(bytes, at)? as usize;
        let kind = u32_at(bytes, at + 4)?;
        at += 8;
        let end = at.checked_add(length).ok_or("GLB chunk overflow.")?;
        let data = bytes.get(at..end).ok_or("Truncated GLB chunk.")?;
        match kind {
            0x4e4f534a => {
                if json.replace(data).is_some() {
                    return Err("Repeated GLB JSON chunk.".into());
                }
            }
            0x004e4942 => {
                if bin.replace(data).is_some() {
                    return Err("Repeated GLB BIN chunk.".into());
                }
            }
            _ => {}
        }
        at = end;
    }
    let data = std::str::from_utf8(json.ok_or("Missing GLB JSON chunk.")?)
        .map_err(|e| e.to_string())?
        .trim_end_matches(['\0', ' ']);
    let gltf: Gltf = serde_json::from_str(data).map_err(|e| e.to_string())?;
    let roots = if let Some(scene) = gltf.scene {
        gltf.scenes
            .get(scene)
            .ok_or("Missing GLB default scene.")?
            .nodes
            .clone()
    } else if let Some(scene) = gltf.scenes.first() {
        scene.nodes.clone()
    } else {
        let children = gltf
            .nodes
            .iter()
            .flat_map(|n| n.children.iter().copied())
            .collect::<BTreeSet<_>>();
        (0..gltf.nodes.len())
            .filter(|i| !children.contains(i))
            .collect()
    };
    let mut import = Import {
        gltf: &gltf,
        bin: bin.unwrap_or(&[]),
        geometry: Geometry::default(),
        visits: 0,
        cancelled,
    };
    for root in roots {
        import.walk(root, Matrix::default(), &mut BTreeSet::new())?;
    }
    if import.geometry.triangles.is_empty() {
        return Err("GLB default scene contains no triangle geometry.".into());
    }
    Ok(import.geometry)
}

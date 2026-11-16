use super::{Package, accessor::Data, materials::name};
use crate::itemspec::definition::{
    ItemDiagnostic, ItemGeometry, ItemMesh, ItemNode, check_cancelled,
};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use superspec_geometry::{
    Matrix,
    math::{cross, length, scale, sub, unit},
};

fn up([x, y, z]: [f64; 3]) -> [f64; 3] {
    [x, -z, y]
}
fn basis() -> Matrix {
    Matrix([[1., 0., 0.], [0., 0., 1.], [0., -1., 0.], [0.; 3]])
}

// glTF requires flat shading and ignores authored tangents when NORMAL is absent.
fn flat_normals(mesh: &mut ItemMesh) {
    let indices = std::mem::take(&mut mesh.indices);
    mesh.positions = indices
        .iter()
        .map(|i| mesh.positions[*i as usize])
        .collect();
    mesh.uvs = indices.iter().map(|i| mesh.uvs[*i as usize]).collect();
    for uvs in mesh.additional_uvs.values_mut() {
        *uvs = indices.iter().map(|i| uvs[*i as usize]).collect();
    }
    if !mesh.colors.is_empty() {
        mesh.colors = indices.iter().map(|i| mesh.colors[*i as usize]).collect();
    }
    mesh.normals = mesh
        .positions
        .chunks_exact(3)
        .flat_map(|triangle| {
            let normal = unit(cross(
                sub(triangle[1], triangle[0]),
                sub(triangle[2], triangle[0]),
            ))
            .unwrap_or([0., 0., 1.]);
            [normal; 3]
        })
        .collect();
    mesh.indices = (0..indices.len() as u32).collect();
    mesh.tangents.clear();
}
impl Package {
    pub(in crate::itemspec::definition) fn build(
        &self,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<ItemGeometry, ItemDiagnostic> {
        check_cancelled(&self.source, cancelled)?;
        let materials = self.materials()?;
        let mut meshes = Vec::new();
        let mut mapping = Vec::new();
        let mut vertices = 0usize;
        let mut source_vertices = 0usize;
        let mut triangles = 0usize;
        for (mesh_index, value) in self.array("meshes", 1024)?.iter().enumerate() {
            let pointer = format!("/meshes/{mesh_index}");
            let primitives = value["primitives"]
                .as_array()
                .filter(|values| !values.is_empty() && values.len() <= 256)
                .ok_or_else(|| {
                    self.error(
                        &pointer,
                        "Mesh requires a bounded, nonempty primitive array.",
                    )
                })?;
            let mut mapped = Vec::new();
            for (primitive_index, primitive) in primitives.iter().enumerate() {
                check_cancelled(&self.source, cancelled)?;
                if meshes.len() >= 4096 {
                    return Err(self.error(&pointer, "Item has too many mesh primitives."));
                }
                let pointer = format!("{pointer}/primitives/{primitive_index}");
                if primitive.get("targets").is_some()
                    || primitive
                        .get("extensions")
                        .is_some_and(|v| v.as_object().is_none_or(|v| !v.is_empty()))
                {
                    return Err(self.error(&pointer,"Morph targets and compressed/extended primitives are not supported by the static item importer."));
                }
                let attributes = primitive["attributes"].as_object().ok_or_else(|| {
                    self.error(&pointer, "Primitive attributes must be an object.")
                })?;
                if attributes
                    .keys()
                    .any(|key| key.starts_with("JOINTS_") || key.starts_with("WEIGHTS_"))
                {
                    return Err(self.error(
                        &pointer,
                        "Skin attributes require an animated item importer.",
                    ));
                }
                if attributes.keys().any(|key| {
                    !matches!(key.as_str(), "POSITION" | "NORMAL" | "TANGENT" | "COLOR_0")
                        && !key.starts_with("TEXCOORD_")
                }) {
                    return Err(self.error(
                        &pointer,
                        "Unsupported vertex attribute; import would discard its values.",
                    ));
                }
                let attribute = |name: &str| -> Result<Option<Data>, ItemDiagnostic> {
                    attributes
                        .get(name)
                        .map(|index| {
                            self.accessor(self.index(index, &pointer)?, 500_000, cancelled)
                        })
                        .transpose()
                };
                let positions = attribute("POSITION")?
                    .ok_or_else(|| self.error(&pointer, "Primitive has no POSITION attribute."))?;
                if positions.components != 3 || positions.kind != 5126 || positions.normalized {
                    return Err(self.error(&pointer, "Positions require a float VEC3 accessor."));
                }
                let positions: Vec<_> = positions
                    .values
                    .into_iter()
                    .map(|v| up([v[0], v[1], v[2]]))
                    .collect();
                if positions.iter().flatten().any(|v| v.abs() > 1e7) {
                    return Err(
                        self.error(&pointer, "Local vertex coordinates exceed their allowance.")
                    );
                }
                source_vertices = source_vertices
                    .checked_add(positions.len())
                    .filter(|n| *n <= 500_000)
                    .ok_or_else(|| self.error(&pointer, "Item exceeds 500000 local vertices."))?;
                let raw_indices = if let Some(index) = primitive.get("indices") {
                    let data = self.accessor(self.index(index, &pointer)?, 750_000, cancelled)?;
                    if data.components != 1
                        || data.normalized
                        || !matches!(data.kind, 5121 | 5123 | 5125)
                    {
                        return Err(
                            self.error(&pointer, "Indices require unsigned scalar components.")
                        );
                    }
                    data.values
                        .into_iter()
                        .map(|v| v[0] as u32)
                        .collect::<Vec<_>>()
                } else {
                    (0..positions.len() as u32).collect()
                };
                if raw_indices
                    .iter()
                    .any(|index| *index as usize >= positions.len())
                {
                    return Err(self.error(&pointer, "Primitive index exceeds its vertex range."));
                }
                let mode = primitive
                    .get("mode")
                    .map(|v| self.index(v, &pointer))
                    .transpose()?
                    .unwrap_or(4);
                let indices = match mode {
                    4 => {
                        if !raw_indices.len().is_multiple_of(3) {
                            return Err(self.error(&pointer, "Triangle indices are incomplete."));
                        }
                        raw_indices
                    }
                    5 | 6 => {
                        if raw_indices.len() < 3 || raw_indices.len() > 250_002 {
                            return Err(self.error(
                                &pointer,
                                "Triangle strip/fan count exceeds its allowance.",
                            ));
                        }
                        let mut triangles = Vec::with_capacity((raw_indices.len() - 2) * 3);
                        for index in 0..raw_indices.len() - 2 {
                            let triangle = if mode == 6 {
                                [
                                    raw_indices[0],
                                    raw_indices[index + 1],
                                    raw_indices[index + 2],
                                ]
                            } else if index.is_multiple_of(2) {
                                [
                                    raw_indices[index],
                                    raw_indices[index + 1],
                                    raw_indices[index + 2],
                                ]
                            } else {
                                [
                                    raw_indices[index + 1],
                                    raw_indices[index],
                                    raw_indices[index + 2],
                                ]
                            };
                            triangles.extend(triangle);
                        }
                        triangles
                    }
                    _ => return Err(self.error(
                        &pointer,
                        "Only triangle, triangle-strip and triangle-fan primitives are supported.",
                    )),
                };
                triangles = triangles
                    .checked_add(indices.len() / 3)
                    .filter(|n| *n <= 250_000)
                    .ok_or_else(|| self.error(&pointer, "Item exceeds 250000 source triangles."))?;
                let normals = if let Some(data) = attribute("NORMAL")? {
                    if data.components != 3
                        || data.kind != 5126
                        || data.values.len() != positions.len()
                    {
                        return Err(
                            self.error(&pointer, "Normals require matching float VEC3 attributes.")
                        );
                    }
                    data.values
                        .into_iter()
                        .map(|v| {
                            let n = up([v[0], v[1], v[2]]);
                            if length(n) < 1e-12 {
                                Err(self.error(&pointer, "Normal has zero length."))
                            } else {
                                Ok(n)
                            }
                        })
                        .collect::<Result<Vec<_>, _>>()?
                } else {
                    vec![]
                };
                let mut uvs = vec![[0.; 2]; positions.len()];
                let mut additional_uvs = BTreeMap::new();
                for key in attributes.keys().filter(|key| key.starts_with("TEXCOORD_")) {
                    let set = key[9..]
                        .parse::<u32>()
                        .ok()
                        .filter(|set| *set <= 7)
                        .ok_or_else(|| {
                            self.error(&pointer, "Texture coordinate set exceeds its allowance.")
                        })?;
                    let data = attribute(key)?.unwrap();
                    if data.components != 2
                        || data.values.len() != positions.len()
                        || !(data.kind == 5126
                            || data.normalized && matches!(data.kind, 5121 | 5123))
                    {
                        return Err(self.error(
                            &pointer,
                            "UVs require matching float or normalized unsigned VEC2 attributes.",
                        ));
                    }
                    let coordinates = data.values.into_iter().map(|v| [v[0], v[1]]).collect();
                    if set == 0 {
                        uvs = coordinates
                    } else {
                        additional_uvs.insert(set, coordinates);
                    }
                }
                let colors = if let Some(data) = attribute("COLOR_0")? {
                    if !matches!(data.components, 3 | 4)
                        || data.values.len() != positions.len()
                        || !(data.kind == 5126
                            || data.normalized && matches!(data.kind, 5121 | 5123))
                    {
                        return Err(self.error(&pointer,"Colors require matching float or normalized unsigned VEC3/VEC4 attributes."));
                    }
                    let count = data.components;
                    let colors: Vec<_> = data
                        .values
                        .into_iter()
                        .map(|v| [v[0], v[1], v[2], if count == 3 { 1. } else { v[3] }])
                        .collect();
                    if colors.iter().flatten().any(|v| !(0. ..=1.).contains(v)) {
                        return Err(
                            self.error(&pointer, "Vertex colors must be within zero and one.")
                        );
                    }
                    colors
                } else {
                    vec![]
                };
                let tangents = if let Some(data) = attribute("TANGENT")? {
                    if data.components != 4
                        || data.kind != 5126
                        || data.values.len() != positions.len()
                    {
                        return Err(self
                            .error(&pointer, "Tangents require matching float VEC4 attributes."));
                    }
                    data.values
                        .into_iter()
                        .map(|v| {
                            let n = up([v[0], v[1], v[2]]);
                            if length(n) < 1e-12 || !matches!(v[3], -1. | 1.) {
                                return Err(self
                                    .error(&pointer, "Invalid tangent direction or handedness."));
                            }
                            Ok([n[0], n[1], n[2], v[3]])
                        })
                        .collect::<Result<Vec<_>, _>>()?
                } else {
                    vec![]
                };
                let material_slot = if let Some(index) = primitive.get("material") {
                    format!("material/{}", self.index(index, &pointer)?)
                } else {
                    "material/default".into()
                };
                let material = materials.get(&material_slot).ok_or_else(|| {
                    self.error(&pointer, "Primitive references a missing material.")
                })?;
                for texture in material.textures.values() {
                    if !attributes.contains_key(&format!("TEXCOORD_{}", texture.tex_coord)) {
                        return Err(self.error(
                            &pointer,
                            "Material requires a texture coordinate set absent from its primitive.",
                        ));
                    }
                }
                mapped.push(meshes.len());
                let mut mesh = ItemMesh {
                    id: format!("mesh/{mesh_index}/primitive/{primitive_index}"),
                    positions,
                    normals,
                    uvs,
                    additional_uvs,
                    colors,
                    tangents,
                    indices,
                    material_slot,
                };
                if !attributes.contains_key("NORMAL") {
                    if mesh.indices.len() > 500_000 - vertices {
                        return Err(self.error(
                            &pointer,
                            "Flat-normal corners exceed 500000 local vertices.",
                        ));
                    }
                    flat_normals(&mut mesh);
                }
                vertices = vertices
                    .checked_add(mesh.positions.len())
                    .filter(|n| *n <= 500_000)
                    .ok_or_else(|| {
                        self.error(&pointer, "Item exceeds 500000 generated local vertices.")
                    })?;
                meshes.push(mesh);
            }
            mapping.push(mapped);
        }
        let source_nodes = self.array("nodes", 4096)?;
        let scenes = self.array("scenes", 256)?;
        let roots = if !scenes.is_empty() {
            let selected = self
                .scene
                .or(self
                    .document
                    .get("scene")
                    .map(|v| self.index(v, "/scene"))
                    .transpose()?)
                .unwrap_or(0);
            let scene = scenes
                .get(selected)
                .ok_or_else(|| self.error("/scene", "Selected GLB scene is missing."))?;
            scene["nodes"]
                .as_array()
                .ok_or_else(|| self.error("/scenes", "Scene roots must be an array."))?
                .iter()
                .map(|value| self.index(value, "/scenes/nodes"))
                .collect::<Result<Vec<_>, _>>()?
        } else {
            if self.scene.is_some() {
                return Err(self.error("/scene", "GLB has no selectable scenes."));
            }
            let mut children = BTreeSet::new();
            for (index, node) in source_nodes.iter().enumerate() {
                for child in self.children(node, &format!("/nodes/{index}"))? {
                    children.insert(child);
                }
            }
            (0..source_nodes.len())
                .filter(|index| !children.contains(index))
                .collect()
        };
        if roots.len() > 4096 || roots.is_empty() {
            return Err(self.error("/scenes", "GLB has no bounded, acyclic root hierarchy."));
        }
        let mut nodes = Vec::new();
        let mut visited = BTreeSet::new();
        for root in roots {
            self.walk(root, None, 0, &mapping, &mut nodes, &mut visited, cancelled)?;
        }
        check_cancelled(&self.source, cancelled)?;
        Ok(ItemGeometry {
            meshes,
            nodes,
            bounds: [[0.; 3]; 2],
            imported_materials: materials,
            images: self.images.clone(),
            parts: vec![],
            features: vec![],
            anchors: vec![],
        })
    }
    fn children(&self, node: &Value, pointer: &str) -> Result<Vec<usize>, ItemDiagnostic> {
        match node.get("children") {
            None => Ok(vec![]),
            Some(value) => value
                .as_array()
                .filter(|array| array.len() <= 4096)
                .ok_or_else(|| self.error(pointer, "Node children must be a bounded array."))?
                .iter()
                .map(|value| self.index(value, pointer))
                .collect(),
        }
    }
    #[allow(clippy::too_many_arguments)]
    fn walk(
        &self,
        index: usize,
        parent: Option<usize>,
        depth: usize,
        mapping: &[Vec<usize>],
        nodes: &mut Vec<ItemNode>,
        visited: &mut BTreeSet<usize>,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<(), ItemDiagnostic> {
        check_cancelled(&self.source, cancelled)?;
        let pointer = format!("/nodes/{index}");
        if depth >= 128 || nodes.len() >= 4096 || !visited.insert(index) {
            return Err(self.error(
                &pointer,
                "GLB hierarchy contains a cycle, repeated parent or excessive depth.",
            ));
        }
        let source_nodes = self.array("nodes", 4096)?;
        let node = source_nodes
            .get(index)
            .ok_or_else(|| self.error(&pointer, "Missing hierarchy node."))?;
        if !node.is_object()
            || node.get("skin").is_some()
            || node.get("weights").is_some()
            || node
                .get("extensions")
                .is_some_and(|v| v.as_object().is_none_or(|v| !v.is_empty()))
        {
            return Err(self.error(
                &pointer,
                "Unsupported static node shape, skin, morph or extension.",
            ));
        }
        let local = self.transform(node, &pointer)?;
        let converted = basis()
            .then(local)
            .then(basis().inverse().unwrap())
            .checked()
            .map_err(|e| self.error(&pointer, e))?;
        let meshes = if let Some(mesh) = node.get("mesh") {
            mapping
                .get(self.index(mesh, &pointer)?)
                .ok_or_else(|| self.error(&pointer, "Node references a missing mesh."))?
                .clone()
        } else {
            vec![]
        };
        let next = nodes.len();
        nodes.push(ItemNode {
            id: format!("node/{index}"),
            name: name(node.get("name")).map_err(|e| self.error(&pointer, e))?,
            parent,
            transform: converted.0,
            meshes,
            extras: node.get("extras").cloned(),
        });
        for child in self.children(node, &pointer)? {
            self.walk(
                child,
                Some(next),
                depth + 1,
                mapping,
                nodes,
                visited,
                cancelled,
            )?;
        }
        Ok(())
    }
    fn transform(&self, node: &Value, pointer: &str) -> Result<Matrix, ItemDiagnostic> {
        if let Some(matrix) = node.get("matrix") {
            if ["translation", "rotation", "scale"]
                .iter()
                .any(|key| node.get(*key).is_some())
            {
                return Err(self.error(pointer, "Node matrix and TRS must not be combined."));
            }
            let values = self.vector(Some(matrix), [0.; 16], pointer)?;
            if values[3] != 0. || values[7] != 0. || values[11] != 0. || values[15] != 1. {
                return Err(self.error(pointer, "Node matrix must be affine."));
            }
            return Matrix([
                [values[0], values[1], values[2]],
                [values[4], values[5], values[6]],
                [values[8], values[9], values[10]],
                [values[12], values[13], values[14]],
            ])
            .checked()
            .and_then(|matrix| {
                matrix.inverse()?;
                Ok(matrix)
            })
            .map_err(|e| self.error(pointer, e));
        }
        let [mut x, mut y, mut z, mut w] =
            self.vector(node.get("rotation"), [0., 0., 0., 1.], pointer)?;
        let length = (x * x + y * y + z * z + w * w).sqrt();
        if !length.is_finite() || length < 1e-12 {
            return Err(self.error(pointer, "Invalid node quaternion."));
        }
        x /= length;
        y /= length;
        z /= length;
        w /= length;
        let sizes = self.vector(node.get("scale"), [1.; 3], pointer)?;
        let translation = self.vector(node.get("translation"), [0.; 3], pointer)?;
        Matrix([
            scale(
                [
                    1. - 2. * (y * y + z * z),
                    2. * (x * y + w * z),
                    2. * (x * z - w * y),
                ],
                sizes[0],
            ),
            scale(
                [
                    2. * (x * y - w * z),
                    1. - 2. * (x * x + z * z),
                    2. * (y * z + w * x),
                ],
                sizes[1],
            ),
            scale(
                [
                    2. * (x * z + w * y),
                    2. * (y * z - w * x),
                    1. - 2. * (x * x + y * y),
                ],
                sizes[2],
            ),
            translation,
        ])
        .checked()
        .and_then(|matrix| {
            matrix.inverse()?;
            Ok(matrix)
        })
        .map_err(|e| self.error(pointer, e))
    }
}

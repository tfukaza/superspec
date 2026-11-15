use super::{Artifact, Diagnostic, Mapping, Output, Receiver, Request, Session};
use crate::{
    itemspec::definition::{self, Collision, CollisionIntent, CollisionShape, ItemDiagnostic},
    runtime::dependencies::{Node, Plan},
};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, sync::Arc};
use superspec_geometry::{
    Matrix, Solid, image,
    math::{cross, sub, unit},
};
use superspec_model::instances::{IDENTITY, ImageResource, Instance, LocalMesh, Model};
use superspec_types::{diagnostic::SourceLocation, material::Material};

impl From<ItemDiagnostic> for Diagnostic {
    fn from(value: ItemDiagnostic) -> Self {
        Self {
            code: value.code,
            source_name: value.source,
            line: value.line.unwrap_or(1),
            column: value.column.unwrap_or(1),
            offset: value.offset.unwrap_or(0),
            end: value.end,
            message: format!("{} {}", value.pointer, value.message),
        }
    }
}
fn matrix(value: Matrix) -> [f64; 16] {
    let m = value.0;
    [
        m[0][0], m[0][1], m[0][2], 0., m[1][0], m[1][1], m[1][2], 0., m[2][0], m[2][1], m[2][2],
        0., m[3][0], m[3][1], m[3][2], 1.,
    ]
}
fn location() -> SourceLocation {
    SourceLocation {
        line: 1,
        column: 1,
        offset: 0,
        end: None,
    }
}

impl Session {
    pub(super) fn item(
        &mut self,
        request: &Request,
        revision: u64,
        assets: &dyn Fn(&str) -> Result<Arc<[u8]>, String>,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Output, Vec<Diagnostic>> {
        let source = if request.kind == "import-glb" {
            json!({"format":"itemspec","version":1,"id":"imported-model","generator":{"kind":"glb","asset":request.entry},"collision":"none"}).to_string()
        } else {
            request.source.clone()
        };
        let item_failure = |mut error: ItemDiagnostic| {
            if error.source == request.entry {
                definition::locate_diagnostic(&source, &mut error);
            }
            vec![Diagnostic::from(error)]
        };
        let definition = definition::parse(&request.entry, &source).map_err(&item_failure)?;
        let prepared = definition::prepare(&definition, &request.parameters, assets, cancelled)
            .map_err(&item_failure)?;
        let fail = |message: String| vec![Diagnostic::message(&request.entry, message)];
        let plan = Plan::new(
            vec![Node {
                id: "item-geometry".into(),
                implementation: "itemspec-geometry-v1".into(),
                inputs: prepared.geometry_key.as_bytes().into(),
                dependencies: vec![],
                source: None,
            }],
            vec!["item-geometry".into()],
        )
        .map_err(|e| fail(e.to_string()))?;
        let mut item_error = None;
        let result = self
            .runtime
            .run(
                self.build_epoch(),
                &plan,
                cancelled,
                |_, _, stopped| match prepared.build(stopped) {
                    Ok(item) => {
                        let bytes = item.retained_bytes();
                        Ok((Artifact::Item(Arc::new(item)), bytes))
                    }
                    Err(error) => {
                        let message = error.to_string();
                        item_error = item_failure(error).pop();
                        Err(message)
                    }
                },
            )
            .map_err(|e| {
                item_error
                    .clone()
                    .map(|e| vec![e])
                    .unwrap_or_else(|| fail(e))
            })?;
        let Artifact::Item(cached) = result["item-geometry"].as_ref() else {
            unreachable!()
        };
        let generated = prepared
            .reuse(cached)
            .and_then(|item| item.with_material_overrides(&request.slot_overrides))
            .map_err(&item_failure)?;
        let placement = Matrix(
            request
                .placement_transform
                .unwrap_or_else(definition::identity),
        )
        .checked()
        .and_then(|matrix| matrix.inverse().map(|_| matrix))
        .map_err(&fail)?;
        let mut image_ids = BTreeMap::new();
        let mut material_ids = BTreeMap::new();
        let mut model = Model {
            id: format!("studio/item/{}", generated.id),
            name: generated.id.clone(),
            ..Default::default()
        };
        for image in &generated.geometry.images {
            let decoded = image::decode(&image.bytes).map_err(&fail)?;
            let id = format!("studio/import/image/{:x}", Sha256::digest(&image.bytes));
            image_ids.insert(image.id.clone(), id.clone());
            model.images.insert(
                id,
                ImageResource {
                    mime: decoded.mime,
                    bytes: image.bytes.clone(),
                    pixels: decoded.width * decoded.height,
                },
            );
        }
        for (id, material) in &generated.geometry.imported_materials {
            if generated.materials.contains_key(id)
                || !generated
                    .geometry
                    .nodes
                    .iter()
                    .flat_map(|node| &node.meshes)
                    .any(|index| generated.geometry.meshes[*index].material_slot == *id)
            {
                continue;
            }
            material.ensure_scene_compatible().map_err(&fail)?;
            let mut material = material.material();
            for image in [
                &mut material.texture,
                &mut material.normal_texture,
                &mut material.metallic_roughness_texture,
                &mut material.height_texture,
                &mut material.emissive_texture,
                &mut material.occlusion_texture,
            ] {
                if let Some(name) = image {
                    *name = image_ids
                        .get(name)
                        .ok_or_else(|| {
                            fail(format!(
                                "Imported material references unknown image {name}."
                            ))
                        })?
                        .clone();
                }
            }
            material.id.clear();
            material.id = format!(
                "studio/import/material/{:x}",
                Sha256::digest(serde_json::to_vec(&material).unwrap())
            );
            material_ids.insert(id.clone(), material.id.clone());
            model.materials.insert(material.id.clone(), material);
        }
        let imported = matches!(definition.generator, definition::Generator::Glb { .. });
        let selected_meshes: std::collections::BTreeSet<_> = generated
            .geometry
            .nodes
            .iter()
            .flat_map(|node| node.meshes.iter().copied())
            .collect();
        for (index, mesh) in generated.geometry.meshes.iter().enumerate() {
            if !selected_meshes.contains(&index) {
                continue;
            }
            if mesh.colors.iter().any(|rgba| rgba[3] != 1.) {
                return Err(fail(
                    "Per-vertex alpha is not supported by the scene delivery adapter.".into(),
                ));
            }
            let uvs = if mesh.uvs.is_empty() {
                vec![[0., 0.]; mesh.positions.len()]
            } else if imported {
                mesh.uvs.iter().map(|uv| [uv[0], 1. - uv[1]]).collect()
            } else {
                mesh.uvs.clone()
            };
            let tangents = if imported
                && (!mesh.tangents.is_empty()
                    || model
                        .materials
                        .get(
                            material_ids
                                .get(&mesh.material_slot)
                                .unwrap_or(&mesh.material_slot),
                        )
                        .is_some_and(|material| material.normal_texture.is_some()))
            {
                definition::delivery_tangents(mesh).map_err(&fail)?
            } else {
                vec![]
            };
            let local = LocalMesh {
                positions: mesh.positions.clone(),
                normals: mesh.normals.clone(),
                uvs,
                colors: if mesh.colors.is_empty() {
                    vec![[1.; 3]; mesh.positions.len()]
                } else {
                    mesh.colors.iter().map(|c| [c[0], c[1], c[2]]).collect()
                },
                indices: mesh.indices.clone(),
                tangents,
            };
            model.mesh_artifacts.insert(
                mesh.id.clone(),
                format!(
                    "itemspec-delivery-v1/{}/{}/tangents-{}",
                    generated.geometry_key,
                    mesh.id,
                    !local.tangents.is_empty()
                ),
            );
            model.meshes.insert(mesh.id.clone(), Arc::new(local));
            if !material_ids.contains_key(&mesh.material_slot) {
                material_ids.insert(mesh.material_slot.clone(), mesh.material_slot.clone());
                model.materials.insert(
                    mesh.material_slot.clone(),
                    Material {
                        id: mesh.material_slot.clone(),
                        ..Default::default()
                    },
                );
            }
        }
        let transforms = generated.geometry.world_transforms().map_err(&fail)?;
        let mut receiver_transforms = BTreeMap::new();
        let mut mesh_slots = BTreeMap::new();
        for (node, transform) in generated.geometry.nodes.iter().zip(&transforms) {
            for &index in &node.meshes {
                let mesh = &generated.geometry.meshes[index];
                let id = format!("{}/{}", node.id, mesh.id);
                let transform = matrix(placement.then(*transform));
                receiver_transforms.insert(id.clone(), transform);
                mesh_slots.insert(id.clone(), mesh.material_slot.clone());
                model.instances.push(Instance {
                    id,
                    object_id: generated.id.clone(),
                    mesh: mesh.id.clone(),
                    material: Some(material_ids[&mesh.material_slot].clone()),
                    transform,
                    source: location(),
                    visible: true,
                    collidable: matches!(
                        generated.collision,
                        Collision::Intent(CollisionIntent::Mesh)
                    ),
                });
            }
        }
        collision(&mut model, &generated, placement).map_err(&fail)?;
        if !generated.materials.is_empty() {
            let scene = model.scene(revision as u32).map_err(&fail)?;
            for (slot, material) in &generated.materials {
                let receivers: Vec<_> = scene
                    .manifest
                    .meshes
                    .iter()
                    .filter(|mesh| mesh.visible && mesh_slots.get(&mesh.id) == Some(slot))
                    .map(|mesh| Receiver {
                        id: mesh.id.clone(),
                        object_id: mesh.object_id.clone(),
                        positions: scene.positions
                            [mesh.vertex_start * 3..(mesh.vertex_start + mesh.vertex_count) * 3]
                            .iter()
                            .map(|v| *v as f64)
                            .collect(),
                        normals: scene.normals
                            [mesh.vertex_start * 3..(mesh.vertex_start + mesh.vertex_count) * 3]
                            .iter()
                            .map(|v| *v as f64)
                            .collect(),
                        uvs: scene.uvs
                            [mesh.vertex_start * 2..(mesh.vertex_start + mesh.vertex_count) * 2]
                            .iter()
                            .map(|v| *v as f64)
                            .collect(),
                        indices: scene.indices
                            [mesh.index_start..mesh.index_start + mesh.index_count]
                            .to_vec(),
                        object_to_world: receiver_transforms.get(&mesh.id).copied(),
                        contacts: Vec::new(),
                    })
                    .collect();
                let source = request.modules.get(&material.entry).ok_or_else(|| {
                    fail(format!(
                        "Material module {} was not supplied.",
                        material.entry
                    ))
                })?;
                let material_request = Request {
                    kind: "material".into(),
                    entry: material.entry.clone(),
                    source: source.clone(),
                    parameters: material.parameters.clone(),
                    seed: material.seed,
                    receivers,
                    mapping: Mapping::Object,
                    ..request.clone()
                };
                let baked = self.material(&material_request, revision, assets, cancelled)?;
                model.meshes.extend(baked.model.meshes);
                model.materials.extend(baked.model.materials);
                model.images.extend(baked.model.images);
                for replacement in baked.model.instances {
                    let existing = model
                        .instances
                        .iter_mut()
                        .find(|instance| instance.id == replacement.id)
                        .ok_or_else(|| {
                            fail("Baked item receiver has no matching placement.".into())
                        })?;
                    existing.mesh = replacement.mesh;
                    existing.material = replacement.material;
                    existing.transform = IDENTITY;
                }
            }
        }
        let parameters: Vec<_> = definition
            .parameters
            .iter()
            .map(|(name, parameter)| {
                let mut value = serde_json::to_value(parameter).unwrap();
                value["name"] = json!(name);
                value
            })
            .collect();
        let collision_name = match &generated.collision {
            Collision::Intent(CollisionIntent::None) => "none",
            Collision::Intent(CollisionIntent::Mesh) => "mesh",
            Collision::Intent(CollisionIntent::Bounds) => "bounds",
            Collision::Shape(CollisionShape::Box { .. }) => "box",
            Collision::Shape(CollisionShape::Sphere { .. }) => "sphere",
        };
        let attachments = generated
            .attachments
            .iter()
            .map(|attachment| {
                Ok(definition::Attachment {
                    name: attachment.name.clone(),
                    position: placement.point(attachment.position),
                    direction: unit(placement.direction(attachment.direction))?,
                })
            })
            .collect::<Result<Vec<_>, String>>()
            .map_err(&fail)?;
        Ok(Output {
            model,
            metadata: json!({"parameters":parameters,"attachments":attachments,"collision":collision_name,"materials":generated.materials,"meshSlots":mesh_slots,"receiverTransforms":receiver_transforms,"geometryKey":generated.geometry_key,"provenance":generated.provenance,"dependencies":generated.provenance.assets.keys().collect::<Vec<_>>()}),
        })
    }
}

fn collision(
    model: &mut Model,
    item: &definition::GeneratedItem,
    placement: Matrix,
) -> Result<(), String> {
    let (solid, transform) = match &item.collision {
        Collision::Intent(CollisionIntent::None | CollisionIntent::Mesh) => return Ok(()),
        Collision::Intent(CollisionIntent::Bounds) => {
            let b = item.geometry.bounds;
            let mut m = Matrix::default();
            m.0[3] = b[0];
            (Solid::cube(sub(b[1], b[0]))?, m)
        }
        Collision::Shape(CollisionShape::Box { center, size }) => {
            let mut m = Matrix::default();
            m.0[3] = std::array::from_fn(|i| center[i] - size[i] / 2.);
            (Solid::cube(*size)?, Matrix(item.transform).then(m))
        }
        Collision::Shape(CollisionShape::Sphere { center, radius }) => {
            let mut m = Matrix::default();
            m.0[3] = *center;
            (Solid::sphere(*radius, 24)?, Matrix(item.transform).then(m))
        }
    };
    let mesh = solid.mesh();
    let stride = mesh.num_prop as usize;
    let mut local = LocalMesh {
        positions: vec![],
        normals: vec![],
        uvs: vec![],
        colors: vec![],
        indices: vec![],
        tangents: vec![],
    };
    for indices in mesh.tri_verts.chunks_exact(3) {
        let p: [[_; 3]; 3] = std::array::from_fn(|i| {
            std::array::from_fn(|axis| {
                mesh.vert_properties[indices[i] as usize * stride + axis] as f64
            })
        });
        let normal = unit(cross(sub(p[1], p[0]), sub(p[2], p[0])))?;
        for point in p {
            local.positions.push(point);
            local.normals.push(normal);
            local.uvs.push([0., 0.]);
            local.colors.push([1.; 3]);
            local.indices.push(local.indices.len() as u32);
        }
    }
    let id = format!("studio/collision/{}", item.id);
    model.mesh_artifacts.insert(
        id.clone(),
        format!(
            "itemspec-collision-v1/{}/{:x}",
            item.geometry_key,
            Sha256::digest(
                serde_json::to_vec(&(&item.collision, &item.transform))
                    .map_err(|e| e.to_string())?
            )
        ),
    );
    model.meshes.insert(id.clone(), Arc::new(local));
    model.instances.push(Instance {
        id: id.clone(),
        object_id: item.id.clone(),
        mesh: id,
        material: None,
        transform: matrix(placement.then(transform)),
        source: location(),
        visible: false,
        collidable: true,
    });
    Ok(())
}

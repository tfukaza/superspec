//! Shader descriptions and receiver fields for live rendering, with binary float buffers.
use super::{archive, delivery, input::Input};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    borrow::Cow,
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};
use superspec_engine::authoring::{
    Diagnostic, FieldBake, Request, Session, occlusion_store::OcclusionStore,
};

#[derive(Deserialize)]
struct MeshFields {
    mesh: String,
    names: Vec<String>,
}

pub(super) fn floats(buffer: &mut Vec<u8>, values: &[f32]) {
    buffer.extend(values.iter().flat_map(|v| v.to_le_bytes()));
}

/// Every reference is content-addressed. A missed/expired browser baseline sends
/// an empty inventory and receives a self-contained packet using the same format.
#[derive(Default)]
struct ShadingBlocks {
    held: BTreeSet<String>,
    features: BTreeMap<String, Vec<usize>>,
    pack_features: bool,
    materials: BTreeMap<String, Value>,
    charts: Vec<(String, String, String)>,
    chart_keys: BTreeSet<String>,
    feature_bytes: Vec<u8>,
}

impl ShadingBlocks {
    fn feature(&mut self, values: &[f64], data: &mut Vec<u8>) -> Value {
        if values.is_empty() {
            return json!([]);
        }
        self.feature_bytes.clear();
        self.feature_bytes
            .extend(values.iter().flat_map(|v| v.to_le_bytes()));
        let key = format!(
            "f{}",
            URL_SAFE_NO_PAD.encode(Sha256::digest(&self.feature_bytes))
        );
        if !self.held.contains(&key) && !self.features.contains_key(&key) {
            let start = data.len();
            let mut range = vec![start, values.len()];
            if self.pack_features {
                // Four two-bit tags per byte. Each edge is twelve values: flat
                // normals and flags are often zero, and the next edge often
                // repeats a coordinate. Preserve each remaining float exactly.
                let tags = values.len().div_ceil(4);
                data.resize(start + tags, 0);
                for (i, &v) in values.iter().enumerate() {
                    let tag = if v.to_bits() == 0 {
                        0
                    } else if i >= 12 && v.to_bits() == values[i - 12].to_bits() {
                        1
                    } else if f64::from(v as f32).to_bits() == v.to_bits() {
                        data.extend_from_slice(&(v as f32).to_le_bytes());
                        2
                    } else {
                        data.extend_from_slice(&v.to_le_bytes());
                        3
                    };
                    data[start + i / 4] |= tag << (2 * (i % 4));
                }
                range.push(data.len() - start);
            } else {
                data.extend_from_slice(&self.feature_bytes);
            }
            self.features.insert(key.clone(), range);
        }
        Value::String(key)
    }

    fn chart(&mut self, mesh: &str, material: &str, digest: &[u8; 32]) -> bool {
        let key = format!("c{}", URL_SAFE_NO_PAD.encode(digest));
        self.charts
            .push((mesh.into(), material.into(), key.clone()));
        !self.chart_keys.insert(key.clone()) || self.held.contains(&key)
    }

    fn describe_materials(&mut self, descriptions: &mut Value) {
        for value in descriptions
            .as_object_mut()
            .into_iter()
            .flat_map(|m| m.values_mut())
        {
            let bytes = serde_json::to_vec(value).expect("admitted shader metadata");
            let key = format!("m{}", URL_SAFE_NO_PAD.encode(Sha256::digest(bytes)));
            let description = std::mem::replace(value, Value::String(key.clone()));
            if !self.held.contains(&key) {
                self.materials.entry(key).or_insert(description);
            }
        }
    }
}
fn fields(buffer: &mut Vec<u8>, bake: FieldBake) -> Value {
    let mut ranges = vec![];
    for plane in bake.fields {
        ranges.push([buffer.len(), plane.len()]);
        floats(buffer, &plane);
    }
    json!({"width":bake.width,"height":bake.height,"fields":ranges})
}
pub fn asset(assets: &BTreeMap<String, Arc<[u8]>>, name: &str) -> Result<Arc<[u8]>, String> {
    if name.starts_with("superspec-material:") {
        return crate::assets::read_confined(std::path::Path::new(""), name).map(Arc::from);
    }
    assets
        .get(name)
        .cloned()
        .ok_or_else(|| format!("Missing bundled companion {name}."))
}

pub fn build(
    session: &mut Session,
    store: &mut OcclusionStore,
    charts: &mut super::chart_delta::ChartHistory,
    (mut document, assets): archive::Contents,
    revision: u32,
    action: &str,
    cancelled: &dyn Fn() -> bool,
) -> Result<Vec<u8>, Vec<Diagnostic>> {
    let invalid = |error| vec![Diagnostic::message("project", error)];
    let mut buffer = Vec::new();
    let report = if action == "fields" {
        let names: Vec<String> =
            serde_json::from_value(document["names"].take()).map_err(|e| invalid(e.to_string()))?;
        let request: Request =
            serde_json::from_value(document).map_err(|e| invalid(e.to_string()))?;
        let baked = session.bake_fields(
            &request,
            revision.into(),
            &names,
            &|n| asset(&assets, n),
            cancelled,
        )?;
        json!({"fields":fields(&mut buffer,baked)})
    } else {
        let include_packet =
            document.get("includeScenePacket").and_then(Value::as_bool) != Some(false);
        let lattice_maps_only = document["latticeSurfaceMapsOnly"].as_bool() == Some(true);
        let pack_features = document["packedFeatureCoords"].as_bool() == Some(true);
        let chart_deltas = document["chartRecordDeltas"].as_bool() == Some(true);
        let mut blocks = if action == "shade-layouts" && !include_packet {
            document
                .get_mut("heldShadingBlocks")
                .map(|v| {
                    serde_json::from_value(v.take()).map(|held| ShadingBlocks {
                        held,
                        pack_features,
                        ..Default::default()
                    })
                })
                .transpose()
                .map_err(|e| invalid(e.to_string()))?
        } else {
            None
        };
        let want_planes = document["withOcclusion"].as_bool() == Some(true);
        let meshes = document.get_mut("fieldMeshes").map(Value::take);
        let input = Input::with_companions((document, assets)).map_err(invalid)?;
        if action == "project-fields" {
            let wanted: Vec<MeshFields> = serde_json::from_value(meshes.unwrap_or(Value::Null))
                .map_err(|e| invalid(e.to_string()))?;
            let ids: Vec<_> = wanted.iter().map(|r| r.mesh.clone()).collect();
            let (project, occlusion) = if want_planes {
                delivery::prepare_planes(session, store, &input, Some(&ids), revision, cancelled)?
            } else {
                (Cow::Borrowed(&input.project), None)
            };
            let read = |name: &str| {
                store
                    .get(name)
                    .cloned()
                    .map(Ok)
                    .unwrap_or_else(|| input.asset(name))
            };
            let prepared =
                session.prepare_project_fields(&project, revision.into(), &read, cancelled)?;
            let mut receivers = Vec::new();
            for receiver in wanted {
                if cancelled() {
                    return Err(invalid("Field generation cancelled.".into()));
                }
                let mut result = match session.bake_prepared_fields(
                    &prepared,
                    &receiver.mesh,
                    &receiver.names,
                    revision.into(),
                    &read,
                    cancelled,
                ) {
                    Ok(bake) => fields(&mut buffer, bake),
                    Err(diagnostics) => {
                        json!({"error":diagnostics.into_iter().map(|d|d.message).collect::<Vec<_>>().join("\n")})
                    }
                };
                if let Some(planes) = occlusion.as_ref().and_then(|r| r["planes"].as_array()) {
                    for plane in planes.iter().filter(|p| p["mesh"] == receiver.mesh) {
                        if let Some(name) = plane["name"].as_str() {
                            result["planes"][name] = plane["key"].clone();
                        }
                    }
                }
                result["mesh"] = json!(receiver.mesh);
                receivers.push(result);
            }
            json!({"projectFields":receivers,"occlusion":occlusion})
        } else {
            let started = std::time::Instant::now();
            let prepared = session.prepare_project(
                &input.project,
                revision.into(),
                &|n| input.asset(n),
                cancelled,
            )?;
            let prepared_at = std::time::Instant::now();
            let mut report = json!({});
            report["metadata"] = if let Some(blocks) = &mut blocks {
                let mut metadata = session.shade_prepared_with_features(
                    &prepared,
                    &mut |values| blocks.feature(values, &mut buffer),
                    cancelled,
                )?;
                blocks.describe_materials(&mut metadata["materials"]);
                metadata
            } else {
                session.shade_prepared(&prepared, cancelled)?
            };
            let described_at = std::time::Instant::now();
            if action == "shade-layouts" {
                let layout = if let Some(blocks) = &mut blocks {
                    session.changed_layouts_prepared(
                        &prepared,
                        lattice_maps_only,
                        &mut |id, material, key| blocks.chart(id, material, key),
                        cancelled,
                    )
                } else {
                    session.layouts_prepared_with_surface_maps(
                        &prepared,
                        lattice_maps_only,
                        cancelled,
                    )
                };
                match layout {
                    Ok(layout) => {
                        report["layouts"] = json!(layout);
                        report["layoutCount"] = json!(layout.records.len());
                        if blocks.is_some() {
                            buffer.resize(buffer.len().next_multiple_of(4), 0);
                            report["layoutOffset"] = json!(buffer.len());
                        }
                        if let Some(blocks) = blocks.as_ref().filter(|_| chart_deltas) {
                            report["chartRecordSpans"] = json!(charts.encode(
                                &layout,
                                &blocks.charts,
                                &blocks.held,
                                &mut buffer
                            ));
                        } else {
                            floats(&mut buffer, &layout.records);
                        }
                    }
                    Err(diagnostics) => {
                        report["metadata"]["layoutError"] = json!(
                            diagnostics
                                .into_iter()
                                .map(|finding| finding.message)
                                .collect::<Vec<_>>()
                                .join("\n")
                        );
                    }
                }
            }
            if let Some(blocks) = blocks {
                report["shadingBlocks"] = json!({"version":1, "features":blocks.features,
                    "materials":blocks.materials, "charts":blocks.charts});
                report["timings"] = json!({
                    "prepareMs": (prepared_at - started).as_secs_f64() * 1000.,
                    "describeMs": (described_at - prepared_at).as_secs_f64() * 1000.,
                    "layoutsMs": described_at.elapsed().as_secs_f64() * 1000.,
                });
            }
            let mut scene = prepared.scene;
            superspec_export::add_project_batches(&mut scene, &input.batches).map_err(invalid)?;
            if include_packet {
                report["packetOffset"] = json!(buffer.len());
                buffer.extend(scene.packet());
            } else {
                report["packetOffset"] = Value::Null;
            }
            report
        }
    };
    delivery::editor_packet(&report, buffer).map_err(invalid)
}

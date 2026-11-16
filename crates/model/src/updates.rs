//! Ordered scene deltas. Reuse is established by exact buffers, independent of hash collisions.
use crate::Scene;
use serde::Serialize;
use std::{
    collections::{BTreeMap, hash_map::DefaultHasher},
    hash::{Hash, Hasher},
};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Header<'a> {
    manifest: &'a crate::Manifest,
    base_revision: Option<u32>,
    meshes: Vec<Option<usize>>,
    assets: Vec<Option<usize>>,
}
fn same_floats(a: &[f32], b: &[f32]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(a, b)| a.to_bits() == b.to_bits())
}
fn same_mesh(a: &Scene, ai: usize, b: &Scene, bi: usize) -> bool {
    let x = &a.manifest.meshes[ai];
    let y = &b.manifest.meshes[bi];
    if x.vertex_count != y.vertex_count || x.index_count != y.index_count {
        return false;
    }
    let slice = |start: usize, count: usize, components: usize| {
        start * components..(start + count) * components
    };
    let (xs, xc, ys, yc) = (
        x.vertex_start,
        x.vertex_count,
        y.vertex_start,
        y.vertex_count,
    );
    same_floats(
        &a.positions[slice(xs, xc, 3)],
        &b.positions[slice(ys, yc, 3)],
    ) && same_floats(&a.normals[slice(xs, xc, 3)], &b.normals[slice(ys, yc, 3)])
        && same_floats(&a.uvs[slice(xs, xc, 2)], &b.uvs[slice(ys, yc, 2)])
        && same_floats(&a.colors[slice(xs, xc, 3)], &b.colors[slice(ys, yc, 3)])
        && a.indices[x.index_start..x.index_start + x.index_count]
            == b.indices[y.index_start..y.index_start + y.index_count]
}
fn hash(bytes: &[u8]) -> u64 {
    let mut h = DefaultHasher::new();
    bytes.hash(&mut h);
    h.finish()
}

pub fn encode(scene: &Scene, previous: Option<&Scene>) -> Vec<u8> {
    let meshes = if let Some(old) = previous {
        let ids = old
            .manifest
            .meshes
            .iter()
            .enumerate()
            .map(|(i, m)| (m.id.as_str(), i))
            .collect::<BTreeMap<_, _>>();
        scene
            .manifest
            .meshes
            .iter()
            .enumerate()
            .map(|(i, m)| {
                ids.get(m.id.as_str())
                    .copied()
                    .filter(|&j| same_mesh(scene, i, old, j))
            })
            .collect::<Vec<_>>()
    } else {
        vec![None; scene.manifest.meshes.len()]
    };
    let mut asset_lookup: BTreeMap<(u64, usize), Vec<usize>> = BTreeMap::new();
    if let Some(old) = previous {
        for (i, a) in old.manifest.assets.iter().enumerate() {
            let bytes = &old.asset_bytes[a.byte_start..a.byte_start + a.byte_length];
            asset_lookup
                .entry((hash(bytes), bytes.len()))
                .or_default()
                .push(i);
        }
    }
    let assets = scene
        .manifest
        .assets
        .iter()
        .map(|a| {
            let bytes = &scene.asset_bytes[a.byte_start..a.byte_start + a.byte_length];
            previous.and_then(|old| {
                asset_lookup
                    .get(&(hash(bytes), bytes.len()))
                    .and_then(|candidates| {
                        candidates.iter().copied().find(|&i| {
                            let p = &old.manifest.assets[i];
                            p.mime == a.mime
                                && old.asset_bytes[p.byte_start..p.byte_start + p.byte_length]
                                    == *bytes
                        })
                    })
            })
        })
        .collect::<Vec<_>>();
    let header = Header {
        manifest: &scene.manifest,
        base_revision: previous.map(|s| s.manifest.revision),
        meshes,
        assets,
    };
    let mut json = serde_json::to_vec(&header).expect("finite scene update");
    while !json.len().is_multiple_of(4) {
        json.push(b' ');
    }
    let mut bytes = b"LSU1".to_vec();
    bytes.extend_from_slice(&(json.len() as u32).to_le_bytes());
    bytes.extend(json);
    for (i, m) in scene.manifest.meshes.iter().enumerate() {
        if header.meshes[i].is_some() {
            continue;
        }
        for array in [
            &scene.positions[m.vertex_start * 3..(m.vertex_start + m.vertex_count) * 3],
            &scene.normals[m.vertex_start * 3..(m.vertex_start + m.vertex_count) * 3],
            &scene.uvs[m.vertex_start * 2..(m.vertex_start + m.vertex_count) * 2],
            &scene.colors[m.vertex_start * 3..(m.vertex_start + m.vertex_count) * 3],
        ] {
            for value in array {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
        }
        for value in &scene.indices[m.index_start..m.index_start + m.index_count] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
    }
    for (i, a) in scene.manifest.assets.iter().enumerate() {
        if header.assets[i].is_none() {
            bytes.extend_from_slice(&scene.asset_bytes[a.byte_start..a.byte_start + a.byte_length]);
        }
    }
    // Instanced batches are small beside what they draw and are sent whole with every update,
    // after everything else (as `Scene::packet` lays them out).
    scene.batches.write(&mut bytes);
    bytes
}

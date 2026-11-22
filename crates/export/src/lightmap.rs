//! UV1 generation preserves source ownership, material coordinates and canonical triangles.
use superspec_model::{LightmapAtlas, Scene};
use superspec_types::limits;

/// Prepares independent lightmap buffers without modifying the canonical scene.
pub fn prepare_lightmap(
    scene: &Scene,
    cancelled: &(dyn Fn() -> bool + Sync),
) -> Result<Scene, String> {
    prepare_lightmap_sized(scene, 4096, cancelled)
}

pub fn prepare_lightmap_sized(
    scene: &Scene,
    page_size: u32,
    cancelled: &(dyn Fn() -> bool + Sync),
) -> Result<Scene, String> {
    if cancelled() {
        return Err("Lightmap generation cancelled.".into());
    }
    let mut derived = scene.clone();
    unwrap_lightmap_sized(&mut derived, page_size, cancelled)?;
    if cancelled() {
        return Err("Lightmap generation cancelled.".into());
    }
    Ok(derived)
}

fn unwrap_lightmap_sized(
    scene: &mut Scene,
    page_size: u32,
    cancelled: &(dyn Fn() -> bool + Sync),
) -> Result<(), String> {
    if !(128..=4096).contains(&page_size) || !page_size.is_power_of_two() {
        return Err("Lightmap resolution must be a power of two from 128 through 4096.".into());
    }
    if cancelled() {
        return Err("Lightmap generation cancelled.".into());
    }
    validate_ranges(scene)?;
    let visible: Vec<_> = scene
        .manifest
        .meshes
        .iter()
        .enumerate()
        .filter(|(_, m)| m.visible)
        .map(|(i, _)| i)
        .collect();
    if visible.is_empty() {
        return Ok(());
    }
    let positions: Vec<Vec<[f32; 3]>> = visible
        .iter()
        .map(|i| {
            let m = &scene.manifest.meshes[*i];
            scene.positions[m.vertex_start * 3..(m.vertex_start + m.vertex_count) * 3]
                .chunks_exact(3)
                .map(|p| [p[0], p[1], p[2]])
                .collect()
        })
        .collect();
    let inputs: Vec<_> = visible
        .iter()
        .enumerate()
        .map(|(slot, i)| {
            let m = &scene.manifest.meshes[*i];
            uvunwrap::MeshInput::new(
                &positions[slot],
                &scene.indices[m.index_start..m.index_start + m.index_count],
            )
        })
        .collect();
    let atlas = uvunwrap::generate_with_progress(
        &inputs,
        &uvunwrap::Options {
            density: uvunwrap::Density::Auto { page_size },
            packing: uvunwrap::Packing::Fast,
            padding: 2,
            ..Default::default()
        },
        &|_| !cancelled(),
    )
    .map_err(|e| e.to_string())?;
    if atlas.page_count != 1 {
        return Err("Lightmap needs more than one atlas page. Use --no-uv for this export.".into());
    }
    let mut output_positions = vec![];
    let mut normals = vec![];
    let mut colors = vec![];
    let mut uvs = vec![];
    let mut uv1 = vec![];
    let mut charts = vec![];
    let mut indices = vec![];
    let mut ranges = vec![];
    let mut tangents = scene.manifest.mesh_tangents.clone();
    let mut slot = 0;
    for m in &scene.manifest.meshes {
        if cancelled() {
            return Err("Lightmap generation cancelled.".into());
        }
        let vertex_start = output_positions.len() / 3;
        let index_start = indices.len();
        if m.visible {
            let out = &atlas.meshes[slot];
            slot += 1;
            if let Some(source) = scene.manifest.mesh_tangents.get(&m.id) {
                tangents.insert(
                    m.id.clone(),
                    out.vertices
                        .iter()
                        .map(|vertex| source[vertex.xref as usize])
                        .collect(),
                );
            }
            for v in &out.vertices {
                let source = m.vertex_start + v.xref as usize;
                output_positions.extend_from_slice(&scene.positions[source * 3..source * 3 + 3]);
                normals.extend_from_slice(&scene.normals[source * 3..source * 3 + 3]);
                colors.extend_from_slice(&scene.colors[source * 3..source * 3 + 3]);
                uvs.extend_from_slice(&scene.uvs[source * 2..source * 2 + 2]);
                uv1.extend(v.uv);
            }
            indices.extend_from_slice(&out.indices);
            charts.extend(
                out.indices
                    .chunks_exact(3)
                    .map(|triangle| out.vertices[triangle[0] as usize].chart),
            );
        } else {
            output_positions.extend_from_slice(
                &scene.positions[m.vertex_start * 3..(m.vertex_start + m.vertex_count) * 3],
            );
            normals.extend_from_slice(
                &scene.normals[m.vertex_start * 3..(m.vertex_start + m.vertex_count) * 3],
            );
            colors.extend_from_slice(
                &scene.colors[m.vertex_start * 3..(m.vertex_start + m.vertex_count) * 3],
            );
            uvs.extend_from_slice(
                &scene.uvs[m.vertex_start * 2..(m.vertex_start + m.vertex_count) * 2],
            );
            uv1.resize(uv1.len() + m.vertex_count * 2, 0.);
            indices.extend_from_slice(&scene.indices[m.index_start..m.index_start + m.index_count]);
            charts.resize(charts.len() + m.index_count / 3, None);
        }
        ranges.push((
            vertex_start,
            output_positions.len() / 3 - vertex_start,
            index_start,
            indices.len() - index_start,
        ));
    }
    if output_positions.len() / 3 > limits::MAX_WORLD_VERTICES {
        return Err("Lightmap seam vertices exceed scene budget.".into());
    }
    if cancelled() {
        return Err("Lightmap generation cancelled.".into());
    }
    for (m, (a, b, c, d)) in scene.manifest.meshes.iter_mut().zip(ranges) {
        m.vertex_start = a;
        m.vertex_count = b;
        m.index_start = c;
        m.index_count = d;
    }
    scene.positions = output_positions;
    scene.normals = normals;
    scene.colors = colors;
    scene.uvs = uvs;
    scene.indices = indices;
    scene.lightmap_uvs = Some(uv1);
    scene.lightmap_charts = Some(charts);
    scene.manifest.vertex_count = scene.positions.len() / 3;
    scene.manifest.index_count = scene.indices.len();
    scene.manifest.mesh_tangents = tangents;
    scene.manifest.lightmap = Some(LightmapAtlas {
        width: atlas.width,
        height: atlas.height,
        charts: atlas.chart_count,
        texels_per_meter: atlas.texels_per_unit,
    });
    Ok(())
}

fn validate_ranges(scene: &Scene) -> Result<(), String> {
    let count = scene.positions.len() / 3;
    if !scene.positions.len().is_multiple_of(3)
        || count > limits::MAX_WORLD_VERTICES
        || scene.indices.len() > limits::MAX_WORLD_INDICES
        || scene.normals.len() != count * 3
        || scene.colors.len() != count * 3
        || scene.uvs.len() != count * 2
    {
        return Err("Invalid lightmap scene buffers.".into());
    }
    for mesh in &scene.manifest.meshes {
        if mesh
            .vertex_start
            .checked_add(mesh.vertex_count)
            .is_none_or(|end| end > count)
            || mesh
                .index_start
                .checked_add(mesh.index_count)
                .is_none_or(|end| end > scene.indices.len())
            || !mesh.index_count.is_multiple_of(3)
            || scene
                .manifest
                .mesh_tangents
                .get(&mesh.id)
                .is_some_and(|t| t.len() != mesh.vertex_count)
        {
            return Err("Invalid lightmap mesh range or tangent attributes.".into());
        }
        if scene.indices[mesh.index_start..mesh.index_start + mesh.index_count]
            .iter()
            .any(|index| *index as usize >= mesh.vertex_count)
        {
            return Err("Invalid lightmap triangle index.".into());
        }
    }
    Ok(())
}

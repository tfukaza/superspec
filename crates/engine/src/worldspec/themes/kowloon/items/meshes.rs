//! Material-slot partition of a Kowloon drawing for portable ItemSpec delivery.
use super::{ItemMesh, Mesh};
use std::collections::BTreeMap;

fn has_area(mesh: &Mesh<'_>, corners: &[u32; 3]) -> bool {
    let [a, b, c] = [0, 1, 2].map(|k| {
        let p = mesh.vertices[corners[k] as usize].position;
        [f64::from(p[0]), f64::from(p[1]), f64::from(p[2])]
    });
    let (u, v) = (
        [b[0] - a[0], b[1] - a[1], b[2] - a[2]],
        [c[0] - a[0], c[1] - a[1], c[2] - a[2]],
    );
    let cross = [
        u[1] * v[2] - u[2] * v[1],
        u[2] * v[0] - u[0] * v[2],
        u[0] * v[1] - u[1] * v[0],
    ];
    cross.iter().map(|x| x * x).sum::<f64>().sqrt() >= 1e-9
}

/// Slot and vertex order follow the first nondegenerate triangle that uses them.
pub fn meshes(mesh: &Mesh<'_>) -> Result<Vec<ItemMesh>, String> {
    let mut surface_groups = vec![None; mesh.surfaces.len()];
    let mut slot_groups = BTreeMap::new();
    let mut groups: Vec<(&str, Vec<&[u32; 3]>)> = Vec::new();
    for (triangle, corners) in mesh.indices.as_chunks::<3>().0.iter().enumerate() {
        let &surface = mesh
            .triangle_surfaces
            .get(triangle)
            .ok_or("Unassigned triangle.")?;
        let cached = surface_groups.get_mut(surface).ok_or("Unknown surface.")?;
        if !has_area(mesh, corners) {
            continue;
        }
        let group = *cached.get_or_insert_with(|| {
            let slot = mesh.surfaces[surface].slot.as_str();
            *slot_groups.entry(slot).or_insert_with(|| {
                groups.push((slot, Vec::new()));
                groups.len() - 1
            })
        });
        groups[group].1.push(corners);
    }

    // A source vertex may appear under several materials. Mark its last group so the
    // same dense remap can serve every slot without clearing or copying the source mesh.
    let mut remap = vec![(usize::MAX, 0_u32); mesh.vertices.len()];
    let mut output = Vec::with_capacity(groups.len());
    for (group, (slot, triangles)) in groups.into_iter().enumerate() {
        let mut target = ItemMesh {
            id: slot.to_owned(),
            positions: vec![],
            normals: vec![],
            uvs: vec![],
            additional_uvs: BTreeMap::new(),
            colors: vec![],
            tangents: vec![],
            indices: Vec::with_capacity(triangles.len() * 3),
            material_slot: slot.to_owned(),
            roles: vec![],
        };
        for corners in triangles {
            for &corner in corners {
                let source = corner as usize;
                let (last_group, index) = &mut remap[source];
                if *last_group != group {
                    let vertex = mesh.vertices[source];
                    *last_group = group;
                    *index = target.positions.len() as u32;
                    target.positions.push(vertex.position);
                    target.normals.push(vertex.normal);
                    target.uvs.push(vertex.uv);
                    target
                        .colors
                        .push([vertex.color[0], vertex.color[1], vertex.color[2], 1.]);
                }
                target.indices.push(*index);
            }
        }
        output.push(target);
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::worldspec::themes::kowloon::mesh::{Mode, Surface, Vertex};

    #[test]
    fn split_preserves_each_slots_first_live_triangle_and_shared_vertices() {
        let mut source = Mesh::new(Mode::Render, 100, &|| false).unwrap();
        for slot in ["glass", "metal", "glass"] {
            source.surfaces.push(Surface {
                part: "fixture".into(),
                slot: slot.into(),
            });
        }
        source.vertices = [[0., 0., 0.], [1., 0., 0.], [0., 0., 1.], [1., 0., 1.]]
            .map(|p| Vertex {
                position: p,
                normal: [0., -1., 0.],
                uv: [p[0], p[2]],
                color: [p[0], p[2], 1.],
            })
            .to_vec();
        source.indices = vec![0, 0, 0, 3, 2, 1, 1, 2, 0, 1, 3, 2, 0, 2, 1];
        source.triangle_surfaces = vec![0, 1, 2, 0, 1];
        let out = meshes(&source).unwrap();
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].material_slot, "metal");
        assert_eq!(out[0].indices, [0, 1, 2, 3, 1, 2]);
        assert_eq!(out[1].material_slot, "glass");
        assert_eq!(out[1].indices, [0, 1, 2, 0, 3, 1]);
        for (part, order) in out.iter().zip([[3, 2, 1, 0], [1, 2, 0, 3]]) {
            for (i, from) in order.into_iter().enumerate() {
                let v = source.vertices[from];
                assert_eq!(part.positions[i], v.position);
                assert_eq!(part.normals[i], v.normal);
                assert_eq!(part.uvs[i], v.uv);
                assert_eq!(part.colors[i], [v.color[0], v.color[1], v.color[2], 1.]);
            }
        }
    }
}

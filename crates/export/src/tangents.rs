use crate::Options;
use superspec_geometry::math::{V3, add, cross, dot, length, scale, sub, unit};
use superspec_model::{Mesh, Scene};

/// Preserves the authored +Y normal-map basis while the exporter flips image-space V.
/// UV seams and hard normals already have distinct vertices in the producer's mesh.
pub(crate) fn material_tangents(
    scene: &Scene,
    mesh: &Mesh,
    options: &Options<'_>,
) -> Result<Vec<f32>, String> {
    if let Some(tangents) = scene.manifest.mesh_tangents.get(&mesh.id) {
        if tangents.len() != mesh.vertex_count
            || tangents.iter().any(|t| {
                t.iter().any(|v| !v.is_finite())
                    || !matches!(t[3], -1. | 1.)
                    || ((t[0] * t[0] + t[1] * t[1] + t[2] * t[2]) - 1.).abs() > 1e-3
            })
        {
            return Err("Invalid authored export tangent basis.".into());
        }
        for (i, tangent) in tangents.iter().enumerate() {
            let normal = unit(std::array::from_fn(|axis| {
                scene.normals[(mesh.vertex_start + i) * 3 + axis] as f64
            }))?;
            if dot(
                normal,
                [tangent[0] as f64, tangent[1] as f64, tangent[2] as f64],
            )
            .abs()
                > 1e-3
            {
                return Err("Authored tangent must be perpendicular to its vertex normal.".into());
            }
        }
        return Ok(tangents.iter().flatten().copied().collect());
    }
    let mut along = vec![[0.; 3]; mesh.vertex_count];
    let mut across = vec![[0.; 3]; mesh.vertex_count];
    let point = |i: usize| -> V3 {
        std::array::from_fn(|a| f64::from(scene.positions[(mesh.vertex_start + i) * 3 + a]))
    };
    let uv = |i: usize| -> [f64; 2] {
        std::array::from_fn(|a| f64::from(scene.uvs[(mesh.vertex_start + i) * 2 + a]))
    };
    for (t, triangle) in scene.indices[mesh.index_start..mesh.index_start + mesh.index_count]
        .as_chunks::<3>()
        .0
        .iter()
        .enumerate()
    {
        if t.is_multiple_of(1024) {
            options.check(0)?;
        }
        let [a, b, c] = triangle.map(|i| i as usize);
        let e1 = sub(point(b), point(a));
        let e2 = sub(point(c), point(a));
        let du1 = uv(b)[0] - uv(a)[0];
        let dv1 = uv(b)[1] - uv(a)[1];
        let du2 = uv(c)[0] - uv(a)[0];
        let dv2 = uv(c)[1] - uv(a)[1];
        let determinant = du1 * dv2 - du2 * dv1;
        if determinant.abs() < 1e-12 {
            continue;
        }
        let tangent = scale(sub(scale(e1, dv2), scale(e2, dv1)), 1. / determinant);
        let bitangent = scale(sub(scale(e2, du1), scale(e1, du2)), 1. / determinant);
        for i in [a, b, c] {
            along[i] = add(along[i], tangent);
            across[i] = add(across[i], bitangent);
        }
    }
    let mut output = Vec::with_capacity(mesh.vertex_count * 4);
    for i in 0..mesh.vertex_count {
        if i.is_multiple_of(1024) {
            options.check(0)?;
        }
        let normal = unit(std::array::from_fn(|a| {
            f64::from(scene.normals[(mesh.vertex_start + i) * 3 + a])
        }))?;
        let mut tangent = sub(along[i], scale(normal, dot(normal, along[i])));
        if length(tangent) < 1e-12 {
            tangent = cross(
                if normal[2].abs() < 0.9 {
                    [0., 0., 1.]
                } else {
                    [0., 1., 0.]
                },
                normal,
            );
        }
        tangent = unit(tangent)?;
        output.extend(tangent.map(|v| v as f32));
        output.push(if dot(cross(normal, tangent), across[i]) < 0. {
            -1.
        } else {
            1.
        });
    }
    Ok(output)
}

use super::{ImportedMaterial, ItemGeometry, ItemMesh, ItemNode};
use crate::itemspec::{
    mesh::{Mesh, Mode},
    primitives::face,
};
use std::collections::BTreeMap;
use superspec_geometry::{
    Matrix,
    math::{cos, sin},
};

fn output(mesh: Mesh<'_>) -> ItemGeometry {
    let material_slot = "surface".to_owned();
    let mut fallback = ImportedMaterial::plain(&material_slot);
    fallback.metallic_factor = 0.;
    fallback.roughness_factor = 0.8;
    ItemGeometry {
        meshes: vec![ItemMesh {
            id: "mesh/0".into(),
            positions: mesh.vertices.iter().map(|v| v.position).collect(),
            normals: mesh.vertices.iter().map(|v| v.normal).collect(),
            uvs: mesh.vertices.iter().map(|v| v.uv).collect(),
            additional_uvs: BTreeMap::new(),
            colors: vec![],
            tangents: vec![],
            indices: mesh.indices,
            material_slot,
            roles: vec![],
        }],
        nodes: vec![ItemNode {
            id: "node/0".into(),
            name: Some("item".into()),
            parent: None,
            transform: Matrix::default().0,
            meshes: vec![0],
            extras: None,
        }],
        bounds: [[0.; 3]; 2],
        imported_materials: BTreeMap::from([("surface".into(), fallback)]),
        images: vec![],
        parts: vec![],
        features: vec![],
        anchors: vec![],
        prototypes: BTreeMap::new(),
        placements: vec![],
    }
}
fn emit(
    mesh: &mut Mesh<'_>,
    points: &[[f64; 3]],
    normal: [f64; 3],
    uv: Option<&[[f64; 2]]>,
    normals: Option<&[[f64; 3]]>,
) -> Result<(), String> {
    face(
        mesh,
        Matrix::default(),
        "item",
        "surface",
        points,
        normal,
        1.,
        uv,
        normals,
        &|_, _, _| Ok([1.; 3]),
    )
}

pub(super) fn pipe(
    length: f64,
    radius: f64,
    thickness: f64,
    segments: usize,
    capped: bool,
    cancelled: &dyn Fn() -> bool,
) -> Result<ItemGeometry, String> {
    let mut mesh = Mesh::new(Mode::Render, segments * 8 + 8, cancelled)?;
    let inner = if thickness > 0. {
        radius - thickness
    } else {
        0.
    };
    let circumference = std::f64::consts::TAU * radius;
    for segment in 0..segments {
        let a = segment as f64 / segments as f64 * std::f64::consts::TAU;
        let b = (segment + 1) as f64 / segments as f64 * std::f64::consts::TAU;
        let radial = [[cos(a), sin(a), 0.], [cos(b), sin(b), 0.]];
        let point = |r: f64, k: usize, z: f64| [r * radial[k][0], r * radial[k][1], z];
        let uv0 = segment as f64 / segments as f64 * circumference;
        let uv1 = (segment + 1) as f64 / segments as f64 * circumference;
        emit(
            &mut mesh,
            &[
                point(radius, 0, 0.),
                point(radius, 1, 0.),
                point(radius, 1, length),
                point(radius, 0, length),
            ],
            radial[0],
            Some(&[[uv0, 0.], [uv1, 0.], [uv1, length], [uv0, length]]),
            Some(&[radial[0], radial[1], radial[1], radial[0]]),
        )?;
        if inner > 0. {
            let normals = radial.map(|n| n.map(|v| -v));
            emit(
                &mut mesh,
                &[
                    point(inner, 0, 0.),
                    point(inner, 1, 0.),
                    point(inner, 1, length),
                    point(inner, 0, length),
                ],
                normals[0],
                Some(&[[uv0, 0.], [uv1, 0.], [uv1, length], [uv0, length]]),
                Some(&[normals[0], normals[1], normals[1], normals[0]]),
            )?;
        }
        if capped {
            for (z, nz) in [(0., -1.), (length, 1.)] {
                if inner > 0. {
                    emit(
                        &mut mesh,
                        &[
                            point(radius, 0, z),
                            point(radius, 1, z),
                            point(inner, 1, z),
                            point(inner, 0, z),
                        ],
                        [0., 0., nz],
                        None,
                        None,
                    )?;
                } else {
                    emit(
                        &mut mesh,
                        &[[0., 0., z], point(radius, 0, z), point(radius, 1, z)],
                        [0., 0., nz],
                        None,
                        None,
                    )?;
                }
            }
        }
    }
    Ok(output(mesh))
}

pub(super) fn duct(
    length: f64,
    width: f64,
    height: f64,
    thickness: f64,
    capped: bool,
    cancelled: &dyn Fn() -> bool,
) -> Result<ItemGeometry, String> {
    let mut mesh = Mesh::new(Mode::Render, 64, cancelled)?;
    let ring = |w: f64, h: f64| {
        [
            [-w / 2., -h / 2.],
            [w / 2., -h / 2.],
            [w / 2., h / 2.],
            [-w / 2., h / 2.],
        ]
    };
    let outer = ring(width, height);
    let inner = (thickness > 0.).then(|| ring(width - 2. * thickness, height - 2. * thickness));
    let normals = [[0., -1., 0.], [1., 0., 0.], [0., 1., 0.], [-1., 0., 0.]];
    for side in 0..4 {
        let next = (side + 1) % 4;
        let point = |ring: [[f64; 2]; 4], k: usize, z: f64| [ring[k][0], ring[k][1], z];
        emit(
            &mut mesh,
            &[
                point(outer, side, 0.),
                point(outer, next, 0.),
                point(outer, next, length),
                point(outer, side, length),
            ],
            normals[side],
            None,
            None,
        )?;
        if let Some(inner) = inner {
            emit(
                &mut mesh,
                &[
                    point(inner, side, 0.),
                    point(inner, next, 0.),
                    point(inner, next, length),
                    point(inner, side, length),
                ],
                normals[side].map(|v| -v),
                None,
                None,
            )?;
        }
        if capped && let Some(inner) = inner {
            for (z, nz) in [(0., -1.), (length, 1.)] {
                emit(
                    &mut mesh,
                    &[
                        point(outer, side, z),
                        point(outer, next, z),
                        point(inner, next, z),
                        point(inner, side, z),
                    ],
                    [0., 0., nz],
                    None,
                    None,
                )?;
            }
        }
    }
    if capped && inner.is_none() {
        for (z, nz) in [(0., -1.), (length, 1.)] {
            emit(
                &mut mesh,
                &outer.map(|p| [p[0], p[1], z]),
                [0., 0., nz],
                None,
                None,
            )?;
        }
    }
    Ok(output(mesh))
}

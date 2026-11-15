use super::*;
use crate::worldspec::themes::kowloon::{
    mesh::Mode,
    surface::{Layout, RasterSettings},
};

fn mesh(maximum: usize) -> Mesh<'static> {
    let mut mesh = Mesh::new(Mode::Render, maximum, &|| false).unwrap();
    for (i, shift) in [0., 3., 6.].into_iter().enumerate() {
        mesh.triangle_precise(
            &format!("part-{i}"),
            "paint",
            [[0., 0., 0.], [2., 0., 0.], [0., 2., 0.]].map(|p| Vertex {
                position: [p[0] + shift, p[1], p[2]],
                normal: [p[0], 0., 2.],
                uv: [p[0], p[1]],
                color: [0.2, 0.3, 0.4],
            }),
        )
        .unwrap();
    }
    mesh.vertices.push(Vertex {
        position: [1., 0., 0.],
        normal: [1., 0., 0.],
        uv: [9., 8.],
        color: [1., 0., 0.],
    });
    mesh
}
fn layout() -> Layout {
    Layout::new(RasterSettings {
        bounds: [0., 0., 2., 2.],
        texels_per_meter: 4.,
        preserve: true,
        repeat: false,
        uv_coordinates: true,
        bump: true,
    })
    .unwrap()
}
fn snapshot(mesh: &Mesh<'_>) -> String {
    format!(
        "{:?}/{:?}/{:?}/{:?}",
        mesh.vertices, mesh.indices, mesh.surfaces, mesh.triangle_surfaces
    )
}
#[test]
fn conformed_receivers_keep_their_own_attributes_and_separate_bindings() {
    for shared in [false, true] {
        let mut mesh = mesh(20);
        let before = mesh.vertices.clone();
        let layout = layout();
        let bindings = [
            RasterBinding {
                first_triangle: 0,
                triangle_count: 1,
                slot: "first",
                layout: &layout,
            },
            RasterBinding {
                first_triangle: 2,
                triangle_count: 1,
                slot: "third",
                layout: &layout,
            },
        ];
        let shared = if shared {
            Some(std::sync::Arc::new(
                Edges::new(
                    mesh.vertices[..mesh.vertices.len() - 1]
                        .iter()
                        .map(|v| v.position)
                        .chain([[7., 0., 0.]]),
                    &mut Budget::new(1_000_000, &|| false).unwrap(),
                )
                .unwrap(),
            ))
        } else {
            None
        };
        let patch = Patch::prepare(
            &mesh,
            &bindings,
            &[0, 1],
            &[[7., 0., 0.]],
            shared,
            &mut Budget::new(1_000_000, &|| false).unwrap(),
        )
        .unwrap()
        .unwrap();
        mesh.bind_rasters(&bindings).unwrap();
        patch.commit(&mut mesh);
        assert_eq!(mesh.triangle_count(), 5);
        let slots: Vec<_> = mesh
            .triangle_surfaces
            .iter()
            .map(|i| mesh.surfaces[*i].slot.as_str())
            .collect();
        assert_eq!(slots, ["first", "first", "paint", "third", "third"]);
        for (&id, original) in mesh.indices[6..9].iter().zip(&before[3..6]) {
            assert_eq!(
                format!("{:?}", mesh.vertices[id as usize]),
                format!("{:?}", original)
            );
        }
        let inserted = &mesh.vertices[3..];
        assert!(
            inserted
                .iter()
                .all(|v| (dot(v.normal, v.normal) - 1.).abs() < 1e-12 && v.color == [1.; 3])
        );
        let midpoint = inserted
            .iter()
            .find(|v| v.position == [1., 0., 0.])
            .unwrap();
        assert_eq!(midpoint.uv, [0.5, 1. / 6.]);
        assert_eq!(midpoint.normal, [1. / 5f64.sqrt(), 0., 2. / 5f64.sqrt()]);
        assert_eq!(mesh.surfaces[mesh.triangle_surfaces[4]].part, "part-2");
    }
}
#[test]
fn failed_conformance_never_rebinds_or_mutates_its_drawing() {
    let mut mesh = mesh(3);
    let layout = layout();
    let bindings = [RasterBinding {
        first_triangle: 0,
        triangle_count: 1,
        slot: "first",
        layout: &layout,
    }];
    let before = snapshot(&mesh);
    assert!(
        Patch::prepare(
            &mesh,
            &bindings,
            &[0],
            &[],
            None,
            &mut Budget::new(1_000_000, &|| false).unwrap()
        )
        .is_err()
    );
    assert_eq!(snapshot(&mesh), before);
    mesh.maximum_triangles = 20;
    let calls = std::cell::Cell::new(0);
    let cancelled = || {
        calls.set(calls.get() + 1);
        calls.get() > 20
    };
    assert!(
        Patch::prepare(
            &mesh,
            &bindings,
            &[0],
            &[],
            None,
            &mut Budget::new(1_000_000, &cancelled).unwrap()
        )
        .err()
        .unwrap()
        .contains("cancelled")
    );
    assert_eq!(snapshot(&mesh), before);
    let patch = Patch::prepare(
        &mesh,
        &bindings,
        &[0],
        &[],
        None,
        &mut Budget::new(1_000_000, &|| false).unwrap(),
    )
    .unwrap()
    .unwrap();
    mesh.bind_rasters(&bindings).unwrap();
    patch.commit(&mut mesh);
    assert_eq!(mesh.triangle_count(), 4);
}

#[test]
fn patch_storage_charges_retained_tail_and_commit_scratch_before_allocating() {
    let bytes_per_triangle = 12 + 2 * std::mem::size_of::<usize>();
    let selected_bytes = 3 * (std::mem::size_of::<Vertex>() + 4);
    let retained = (MAX_PATCH_BYTES - selected_bytes) / bytes_per_triangle;
    patch_storage(3, retained, 0, usize::MAX).unwrap();
    assert!(patch_storage(3, retained + 1, 0, usize::MAX).is_err());
    assert!(patch_storage(3, retained, 8, usize::MAX).is_err());
    assert!(patch_storage(usize::MAX, 1, 1, usize::MAX).is_err());
}

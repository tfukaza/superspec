//! Contracts for native shop composition; independent prop and finish oracles live beside their owners.
use superspec_engine::worldspec::themes::kowloon::mesh::Mesh;
use superspec_geometry::{Matrix, V3};

pub fn positions(mesh: &Mesh<'_>) -> Vec<V3> {
    mesh.indices
        .iter()
        .map(|&i| mesh.vertices[i as usize].position)
        .collect()
}

pub fn same_mesh(a: &Mesh<'_>, b: &Mesh<'_>) {
    assert_eq!(a.indices, b.indices);
    assert_eq!(a.triangle_surfaces, b.triangle_surfaces);
    assert_eq!(
        a.surfaces
            .iter()
            .map(|s| (&s.part, &s.slot))
            .collect::<Vec<_>>(),
        b.surfaces
            .iter()
            .map(|s| (&s.part, &s.slot))
            .collect::<Vec<_>>()
    );
    assert_eq!(a.vertices.len(), b.vertices.len());
    for (a, b) in a.vertices.iter().zip(&b.vertices) {
        assert_eq!(
            (a.position, a.normal, a.uv, a.color),
            (b.position, b.normal, b.uv, b.color)
        );
    }
}

pub fn different_detail(a: &Mesh<'_>, b: &Mesh<'_>) {
    assert!(
        a.vertices.len() != b.vertices.len()
            || a.vertices
                .iter()
                .zip(&b.vertices)
                .any(|(a, b)| (a.position, a.color, a.uv) != (b.position, b.color, b.uv)),
        "seed must alter stock or its appearance"
    );
}

pub fn valid(mesh: &Mesh<'_>) {
    assert!(!mesh.indices.is_empty());
    assert_eq!(mesh.indices.len(), mesh.triangle_surfaces.len() * 3);
    for vertex in &mesh.vertices {
        assert!(
            vertex
                .position
                .iter()
                .chain(&vertex.normal)
                .chain(&vertex.uv)
                .chain(&vertex.color)
                .all(|v| v.is_finite())
        );
        let length: f64 = vertex.normal.iter().map(|v| v * v).sum();
        // Each unit-normal component is delivered to four decimal places.
        // |length² - 1| <= 2 sqrt(3) * 0.00005 + 3 * 0.00005².
        assert!((length - 1.).abs() < 0.000174, "non-unit normal {length}");
        assert!(vertex.color.iter().all(|v| *v >= 0.));
    }
    for &surface in &mesh.triangle_surfaces {
        assert!(!mesh.surfaces[surface].part.is_empty());
        assert!(!mesh.surfaces[surface].slot.is_empty());
    }
}

pub fn same_positions(actual: &[V3], expected: &[V3]) {
    assert_eq!(actual.len(), expected.len());
    for (corner, (a, b)) in actual.iter().zip(expected).enumerate() {
        for axis in 0..3 {
            assert!(
                (a[axis] - b[axis]).abs() < 2e-9,
                "corner {corner}, axis {axis}: {} != {}",
                a[axis],
                b[axis]
            );
        }
    }
}

pub fn part_points(mesh: &Mesh<'_>, part: &str, frame: Matrix) -> Vec<V3> {
    let inverse = frame.inverse().unwrap();
    let points: Vec<_> = mesh
        .indices
        .chunks_exact(3)
        .zip(&mesh.triangle_surfaces)
        .filter(|(_, surface)| mesh.surfaces[**surface].part == part)
        .flat_map(|(triangle, _)| {
            triangle
                .iter()
                .map(|&i| inverse.point(mesh.vertices[i as usize].position))
        })
        .collect();
    assert!(!points.is_empty(), "missing composition part {part}");
    points
}

pub fn contains(mesh: &Mesh<'_>, expected: V3) {
    // Authored positions are quantized to 10 micrometers at mesh emission.
    assert!(
        mesh.vertices.iter().any(|v| v
            .position
            .iter()
            .zip(expected)
            .all(|(a, b)| (*a - b).abs() < 0.0000051)),
        "missing support corner {expected:?}"
    );
}

pub fn bounds(points: &[V3], low: V3, high: V3) {
    assert!(!points.is_empty());
    for p in points {
        for axis in 0..3 {
            // Inverse rigid placement combines three independently quantized axes.
            assert!(
                p[axis] >= low[axis] - 0.000009 && p[axis] <= high[axis] + 0.000009,
                "point {p:?} outside {low:?} .. {high:?}"
            );
        }
    }
}

pub fn open_room(mesh: &Mesh<'_>, part: &str, frame: Matrix, x: [f64; 2], depth: f64, z: [f64; 2]) {
    let points = part_points(mesh, part, frame);
    bounds(&points, [x[0], -depth, z[0]], [x[1], 0., z[1]]);
    for px in x {
        for py in [-depth, 0.] {
            for pz in z {
                contains(mesh, frame.point([px, py, pz]));
            }
        }
    }
    let inverse = frame.inverse().unwrap();
    let planes = [
        (2, z[0], [0., 0., 1.]),
        (2, z[1], [0., 0., -1.]),
        (0, x[0], [1., 0., 0.]),
        (0, x[1], [-1., 0., 0.]),
        (1, -depth, [0., 1., 0.]),
    ];
    let mut areas = [0.; 5];
    let triangles = mesh
        .indices
        .chunks_exact(3)
        .zip(&mesh.triangle_surfaces)
        .filter(|(_, surface)| mesh.surfaces[**surface].part == part);
    for (t, (indices, _)) in points.chunks_exact(3).zip(triangles) {
        let matching: Vec<_> = planes
            .iter()
            .enumerate()
            .filter(|(_, (axis, at, _))| t.iter().all(|p| (p[*axis] - at).abs() < 0.000009))
            .collect();
        assert_eq!(
            matching.len(),
            1,
            "room triangle must belong to one enclosure plane: {t:?}"
        );
        let (plane, (_, _, normal)) = matching[0];
        for &index in indices {
            let actual = inverse.direction(mesh.vertices[index as usize].normal);
            assert!(
                actual
                    .iter()
                    .zip(normal)
                    .all(|(a, b)| (a - b).abs() < 0.0002),
                "wrong interior normal {actual:?} != {normal:?}"
            );
        }
        let a = std::array::from_fn::<_, 3, _>(|i| t[1][i] - t[0][i]);
        let b = std::array::from_fn::<_, 3, _>(|i| t[2][i] - t[0][i]);
        let n = [
            a[1] * b[2] - a[2] * b[1],
            a[2] * b[0] - a[0] * b[2],
            a[0] * b[1] - a[1] * b[0],
        ];
        areas[plane] += n.iter().map(|v| v * v).sum::<f64>().sqrt() / 2.;
    }
    let width = x[1] - x[0];
    let height = z[1] - z[0];
    for (plane, (area, expected)) in areas
        .into_iter()
        .zip([
            width * depth,
            width * depth,
            depth * height,
            depth * height,
            width * height,
        ])
        .enumerate()
    {
        // Each plane must cover its complete authored rectangle. The allowance
        // accounts for corner quantization, independently of triangle subdivision.
        assert!(
            (area - expected).abs() < 0.001,
            "room plane {plane} coverage {area} != {expected}"
        );
    }
}

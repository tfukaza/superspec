//! The one native grocer in the building corpus has an independent room plan.
use serde::Deserialize;
use superspec_engine::worldspec::themes::kowloon::mesh::{Mesh, Mode};
use superspec_geometry::Matrix;

#[derive(Deserialize)]
pub struct Shop {
    trade: String,
    matrix: [f64; 16],
    plan: Plan,
}
#[derive(Deserialize)]
struct Plan {
    x0: f64,
    x1: f64,
    floor: f64,
    head: f64,
    depth: f64,
}

pub fn structural(mesh: &Mesh<'_>, shop: &Shop) -> (Mesh<'static>, Vec<usize>) {
    assert_eq!(shop.trade, "grocer");
    let frame = Matrix(std::array::from_fn(|column| {
        std::array::from_fn(|axis| shop.matrix[column * 4 + axis])
    }));
    let inverse = frame.inverse().unwrap();
    let local = |point| {
        let p = inverse.point(point);
        [p[0] - shop.plan.x0, -p[1], p[2] - shop.plan.floor]
    };
    let mut result = Mesh::new(Mode::Render, mesh.triangle_count() + 1, &|| false).unwrap();
    result.vertices = mesh.vertices.clone();
    result.surfaces = mesh.surfaces.clone();
    let mut selected = vec![];
    let mut body = vec![];
    let mut services = vec![];
    let mut register = vec![];
    for (index, (triangle, &surface)) in mesh
        .indices
        .chunks_exact(3)
        .zip(&mesh.triangle_surfaces)
        .enumerate()
    {
        let part = mesh.surfaces[surface].part.as_str();
        let native = matches!(
            part,
            "shop-grocer" | "shop-grocer/services" | "shop-grocer/checkout/register"
        );
        if !native {
            assert!(
                !part.starts_with("shop-grocer/"),
                "unreviewed native descendant {part}"
            );
            selected.push(index);
            result.indices.extend_from_slice(triangle);
            result.triangle_surfaces.push(surface);
            continue;
        }
        assert!(!mesh.surfaces[surface].slot.is_empty());
        for &vertex in triangle {
            let vertex = &mesh.vertices[vertex as usize];
            let p = local(vertex.position);
            for (coordinate, upper) in p.into_iter().zip([
                shop.plan.x1 - shop.plan.x0,
                shop.plan.depth,
                shop.plan.head - shop.plan.floor,
            ]) {
                assert!(
                    (-0.00002..=upper + 0.00002).contains(&coordinate),
                    "{part} escapes independently planned room: {p:?}"
                );
            }
            assert!(
                vertex
                    .normal
                    .iter()
                    .chain(&vertex.uv)
                    .chain(&vertex.color)
                    .all(|v| v.is_finite())
            );
            assert!(vertex.color.iter().all(|v| *v >= 0.));
            if part == "shop-grocer" {
                body.push(p);
            }
            if part == "shop-grocer/services" {
                services.push(p);
            }
            if part == "shop-grocer/checkout/register" {
                register.push(p);
            }
        }
    }
    assert!(!body.is_empty());
    assert!(
        body.iter().any(|p| p[2].abs() < 0.00002),
        "grocer fittings need floor support"
    );
    assert!(
        body.iter().any(|p| p[2] > 0.6),
        "grocer stock must retain vertical extent"
    );
    assert!(!register.is_empty(), "grocer checkout must remain present");
    assert!(register.iter().any(|p| (p[2] - 0.9).abs() < 0.00002));
    assert!(
        register.iter().any(|p| (p[2] - 1.08684932).abs() < 0.00002),
        "checkout must retain the authored register's display height"
    );
    if shop.plan.head - shop.plan.floor >= 2.3 {
        assert!(!services.is_empty());
        assert!(
            services
                .iter()
                .all(|p| p[2] >= shop.plan.head - shop.plan.floor - 0.13)
        );
        for edge in [0.008, shop.plan.x1 - shop.plan.x0 - 0.008] {
            assert!(
                services.iter().any(|p| (p[0] - edge).abs() < 0.00002),
                "service boxes must reach both walls"
            );
        }
    }
    (result, selected)
}

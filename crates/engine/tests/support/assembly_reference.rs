//! Shared complete assembly comparison against frozen migration evidence.
use super::reference;
use serde::Deserialize;
use serde_json::json;
use superspec_engine::{
    materialspec::surface::Sample,
    worldspec::themes::kowloon::{
        mesh::Mesh,
        weathered::{self, Recipe},
    },
};
#[derive(Deserialize)]
pub struct Geometry {
    pub indices: Vec<u32>,
    positions: Vec<f64>,
    normals: Vec<f64>,
    uv: Vec<f64>,
    colors: Vec<f64>,
    slots: Vec<String>,
}
#[derive(Deserialize)]
pub struct Chart {
    triangles: Vec<usize>,
    point: [f64; 2],
    sample: Sample,
}

/// Retains complete independent reference coverage for one structural component
/// while a containing native room has its own composition contract.
#[allow(dead_code)]
pub fn check_part(mesh: &Mesh<'_>, part: &str, geometry: &Geometry, label: &str) {
    let mut selected = Mesh::new(
        superspec_engine::worldspec::themes::kowloon::mesh::Mode::Render,
        mesh.triangle_count() + 1,
        &|| false,
    )
    .unwrap();
    selected.vertices = mesh.vertices.clone();
    selected.surfaces = mesh.surfaces.clone();
    for (triangle, &surface) in mesh.indices.chunks_exact(3).zip(&mesh.triangle_surfaces) {
        if mesh.surfaces[surface].part == part {
            selected.indices.extend_from_slice(triangle);
            selected.triangle_surfaces.push(surface);
        }
    }
    check_triangles(&selected, geometry, label);
}

/// Partitions frozen source triangles and remaps their receiver coverage into a host's order.
#[allow(dead_code)]
pub fn partition(
    geometry: &Geometry,
    charts: &[Chart],
    selected: &[usize],
) -> (Geometry, Vec<Chart>) {
    let mut part = Geometry {
        indices: vec![],
        positions: vec![],
        normals: vec![],
        uv: vec![],
        colors: vec![],
        slots: vec![],
    };
    let mut local = std::collections::BTreeMap::new();
    for (next, &triangle) in selected.iter().enumerate() {
        local.insert(triangle, next);
        part.slots.push(geometry.slots[triangle].clone());
        for &index in &geometry.indices[triangle * 3..triangle * 3 + 3] {
            let index = index as usize;
            part.indices.push(part.indices.len() as u32);
            part.positions
                .extend(&geometry.positions[index * 3..index * 3 + 3]);
            part.normals
                .extend(&geometry.normals[index * 3..index * 3 + 3]);
            part.colors
                .extend(&geometry.colors[index * 3..index * 3 + 3]);
            part.uv.extend(&geometry.uv[index * 2..index * 2 + 2]);
        }
    }
    let charts = charts
        .iter()
        .filter_map(|chart| {
            let triangles: Vec<_> = chart
                .triangles
                .iter()
                .filter_map(|t| local.get(t).copied())
                .collect();
            if triangles.is_empty() {
                return None;
            }
            assert_eq!(
                triangles.len(),
                chart.triangles.len(),
                "A source receiver crossed host ownership"
            );
            Some(Chart {
                triangles,
                point: chart.point,
                sample: chart.sample.clone(),
            })
        })
        .collect();
    (part, charts)
}

/// Geometry-only surveys must retain every expanded source triangle position.
#[allow(dead_code)]
pub fn check_survey(mesh: &Mesh<'_>, geometry: &Geometry, label: &str) {
    assert!(mesh.surveying());
    assert_eq!(
        mesh.survey.len(),
        geometry.indices.len(),
        "{label} survey count"
    );
    for (corner, (actual, &index)) in mesh.survey.iter().zip(&geometry.indices).enumerate() {
        for (axis, value) in actual.iter().enumerate() {
            assert!(
                (value - geometry.positions[index as usize * 3 + axis]).abs() <= 2e-10,
                "{label} survey corner {corner} axis {axis}"
            );
        }
    }
}

/// Compares delivered triangle corners independently of equivalent vertex sharing.
#[allow(dead_code)]
pub fn check_triangles(mesh: &Mesh<'_>, geometry: &Geometry, label: &str) {
    for (corner, (&actual, &expected)) in mesh.indices.iter().zip(&geometry.indices).enumerate() {
        let v = &mesh.vertices[actual as usize];
        let expected = expected as usize;
        for (name, got, want) in [
            (
                "position",
                &v.position[..],
                &geometry.positions[expected * 3..expected * 3 + 3],
            ),
            (
                "normal",
                &v.normal[..],
                &geometry.normals[expected * 3..expected * 3 + 3],
            ),
            (
                "uv",
                &v.uv[..],
                &geometry.uv[expected * 2..expected * 2 + 2],
            ),
            (
                "color",
                &v.color[..],
                &geometry.colors[expected * 3..expected * 3 + 3],
            ),
        ] {
            for (a, b) in got.iter().zip(want) {
                assert!(
                    (a - b).abs() <= 2e-10,
                    "{label} triangle {} part {} slot {} expected slot {} {name}: {a} != {b}",
                    corner / 3,
                    mesh.surfaces[mesh.triangle_surfaces[corner / 3]].part,
                    mesh.surfaces[mesh.triangle_surfaces[corner / 3]].slot,
                    geometry.slots[corner / 3]
                );
            }
        }
    }
    assert_eq!(mesh.indices.len(), geometry.indices.len(), "{label} count");
    assert_eq!(mesh.triangle_surfaces.len(), geometry.slots.len());
    for (triangle, (surface, slot)) in mesh
        .triangle_surfaces
        .iter()
        .zip(&geometry.slots)
        .enumerate()
    {
        assert_eq!(
            &mesh.surfaces[*surface].slot, slot,
            "{label} triangle {triangle} slot"
        );
    }
}

pub fn check(mesh: &mut Mesh<'_>, geometry: &Geometry, charts: &[Chart], label: &str) {
    assert_eq!(mesh.indices.len(), geometry.indices.len(), "{label} count");
    assert_eq!(
        mesh.indices
            .iter()
            .zip(&geometry.indices)
            .position(|(a, b)| a != b),
        None,
        "{label} indices"
    );
    for (attribute, got, want) in [
        (
            "position",
            mesh.vertices
                .iter()
                .flat_map(|v| v.position)
                .collect::<Vec<_>>(),
            &geometry.positions,
        ),
        (
            "normal",
            mesh.vertices.iter().flat_map(|v| v.normal).collect(),
            &geometry.normals,
        ),
        (
            "uv",
            mesh.vertices.iter().flat_map(|v| v.uv).collect(),
            &geometry.uv,
        ),
        (
            "color",
            mesh.vertices.iter().flat_map(|v| v.color).collect(),
            &geometry.colors,
        ),
    ] {
        assert_eq!(got.len(), want.len(), "{label} {attribute}");
        if let Some(j) = got
            .iter()
            .zip(want)
            .position(|(a, b)| (a - b).abs() > 2e-10)
        {
            panic!("{label} {attribute} {j}: {} != {}", got[j], want[j]);
        }
    }
    assert_eq!(
        mesh.triangle_surfaces.len(),
        geometry.slots.len(),
        "{label} slots"
    );
    for (triangle, (surface, expected)) in mesh
        .triangle_surfaces
        .iter()
        .zip(&geometry.slots)
        .enumerate()
    {
        assert_eq!(
            &mesh.surfaces[*surface].slot, expected,
            "{label} triangle {triangle} slot"
        );
    }
    check_finishes(mesh, geometry, charts, label);
}

pub fn check_finishes(mesh: &mut Mesh<'_>, geometry: &Geometry, charts: &[Chart], label: &str) {
    check_selected_finishes(
        mesh,
        &(0..geometry.indices.len() / 3).collect::<Vec<_>>(),
        charts,
        label,
    );
}

/// Keeps independent receiver samples and exact coverage after enclosing native
/// components change their triangle count. Selection uses emitted part ownership.
#[allow(dead_code)]
pub fn check_part_finishes(
    mesh: &mut Mesh<'_>,
    parts: &std::collections::BTreeMap<String, Vec<Chart>>,
    label: &str,
) {
    let mut selected = vec![];
    let mut charts = vec![];
    for (part, expected) in parts {
        let offset = selected.len();
        selected.extend(
            mesh.triangle_surfaces
                .iter()
                .enumerate()
                .filter_map(|(t, &s)| (mesh.surfaces[s].part == *part).then_some(t)),
        );
        charts.extend(expected.iter().map(|c| Chart {
            triangles: c.triangles.iter().map(|t| t + offset).collect(),
            point: c.point,
            sample: c.sample.clone(),
        }));
    }
    check_selected_finishes(mesh, &selected, &charts, label);
}

#[allow(dead_code)]
pub fn check_selected_finishes(
    mesh: &mut Mesh<'_>,
    selected: &[usize],
    charts: &[Chart],
    label: &str,
) {
    let plan = weathered::finish(mesh, &|| false).unwrap();
    let local: std::collections::BTreeMap<_, _> =
        selected.iter().enumerate().map(|(i, &t)| (t, i)).collect();
    assert_eq!(local.len(), selected.len());
    let mut covered = vec![false; selected.len()];
    for r in plan.receivers() {
        let triangles: Vec<_> = (r.first_triangle..r.first_triangle + r.triangle_count)
            .filter_map(|t| local.get(&t).copied())
            .collect();
        if triangles.is_empty() {
            continue;
        }
        assert_eq!(
            triangles.len(),
            r.triangle_count,
            "{label}: receiver crosses selected ownership"
        );
        let chart = charts
            .iter()
            .find(|c| c.triangles.contains(&triangles[0]))
            .unwrap_or_else(|| {
                panic!(
                    "{label}: unexpected receiver at triangle {}",
                    r.first_triangle
                )
            });
        for t in triangles {
            assert!(chart.triangles.contains(&t));
            assert!(!covered[t]);
            covered[t] = true;
        }
        let job = &plan.jobs()[r.job];
        let runoff = job.runoff_recipe().map(|r| r.build(&|| false).unwrap());
        let f = |u, v| runoff.as_ref().unwrap().sample(u, v);
        let [u, v] = chart.point;
        use superspec_engine::worldspec::themes::kowloon::{
            frosted::Frosted, metal::Metal, painted::Paint,
        };
        let sample = match &job.recipe {
            Recipe::Metal { settings, .. } => Metal::new(
                settings,
                runoff
                    .as_ref()
                    .map(|_| &f as &dyn Fn(f64, f64) -> Result<f64, String>),
            )
            .unwrap()
            .sample(u, v),
            Recipe::Paint(r) => Paint::new(r).unwrap().sample(u, v),
            Recipe::Frosted(r) => Frosted::new(r).unwrap().sample(u, v),
            Recipe::Facade(r) => {
                superspec_engine::worldspec::themes::kowloon::facade_material::Facade::new(r)
                    .unwrap()
                    .sample(u, v)
            }
            Recipe::FacadeDeposits(r) => {
                superspec_engine::worldspec::themes::kowloon::skin::Deposited::new(r)
                    .unwrap()
                    .sample(u, v)
            }
            Recipe::Stained(r) => {
                superspec_engine::worldspec::themes::kowloon::stains::Field::new(r)
                    .unwrap()
                    .sample(u, v)
            }
            Recipe::Pavement(r) => {
                superspec_engine::worldspec::themes::kowloon::pavement::raster::Field::new(
                    r,
                    &|| false,
                )
                .unwrap()
                .sample(u, v)
            }
            Recipe::Tile(r) => superspec_engine::worldspec::themes::kowloon::tile::Field::new(r)
                .unwrap()
                .sample(u, v),
            Recipe::TiledSkin(r) => {
                superspec_engine::worldspec::themes::kowloon::tiled_skin::Field::new(r)
                    .unwrap()
                    .sample(u, v)
            }
            Recipe::Sign(r) => {
                superspec_engine::worldspec::themes::kowloon::signs::weathering::Field::new(r)
                    .unwrap()
                    .sample(u, v)
            }
        }
        .unwrap();
        reference::close_at(
            &json!(sample),
            &json!(chart.sample),
            &format!(
                "{label} triangle {} source {}",
                r.first_triangle,
                job.source()
            ),
        );
    }
    assert_eq!(
        covered.iter().filter(|v| **v).count(),
        charts.iter().map(|c| c.triangles.len()).sum::<usize>()
    );
}

//! Source ownership and material roles for span fixings, before raster rebinding.
use super::anchors::Face;
use std::collections::BTreeMap;
use superspec_geometry::V3;
use superspec_geometry::obstructions::{Budget, Triangle};

pub struct Host {
    pub id: String,
    pub walls: Vec<String>,
    pub glazing: Vec<String>,
}
pub struct Surface<'a> {
    pub object: &'a str,
    pub host: Option<&'a str>,
    pub material: &'a str,
    pub visible: bool,
    pub dynamic: bool,
}
pub struct Facet<'a> {
    pub surface: Surface<'a>,
    pub points: Triangle,
    pub normal: V3,
}
/// Uses actual source material IDs. Direct host ownership precedes provenance;
/// generated panes require both the glazing role and the exact openings part.
pub fn faces<'a>(
    hosts: &[Host],
    triangles: impl IntoIterator<Item = Facet<'a>>,
    budget: &mut Budget<'_>,
) -> Result<Vec<Face>, String> {
    if hosts.len() > 512 {
        return Err("Span classification exceeds host allowance.".into());
    }
    let mut lookup = BTreeMap::new();
    let mut identity_bytes = 0;
    for (i, host) in hosts.iter().enumerate() {
        budget.spend(1)?;
        if host.walls.len() > 16
            || host.glazing.len() > 16
            || std::iter::once(&host.id)
                .chain(&host.walls)
                .chain(&host.glazing)
                .any(|s| s.is_empty() || s.len() > 4096)
            || lookup.insert(host.id.as_str(), i).is_some()
        {
            return Err(
                "Span classification requires distinct bounded hosts and material identities."
                    .into(),
            );
        }
        identity_bytes += host.id.len()
            + host
                .walls
                .iter()
                .chain(&host.glazing)
                .map(String::len)
                .sum::<usize>();
        if identity_bytes > 4 * 1024 * 1024 {
            return Err("Span classification exceeds identity allowance.".into());
        }
    }
    let mut faces = vec![];
    for (index, triangle) in triangles.into_iter().enumerate() {
        budget.spend(1)?;
        if index >= 1_048_576 {
            return Err("Span classification exceeds inspected triangle allowance.".into());
        }
        let surface = triangle.surface;
        if !surface.visible || surface.dynamic {
            continue;
        }
        let (host, glazing) = if let Some(&i) = lookup.get(surface.object) {
            (i, false)
        } else {
            let Some(&i) = surface.host.and_then(|host| lookup.get(host)) else {
                continue;
            };
            let host = &hosts[i];
            if host.walls.iter().any(|m| m == surface.material) {
                (i, false)
            } else if surface.object.ends_with("/$dressing/openings")
                && host.glazing.iter().any(|m| m == surface.material)
            {
                (i, true)
            } else {
                continue;
            }
        };
        faces.push(Face {
            host,
            glazing,
            points: triangle.points,
            stored_normal: triangle.normal,
        });
    }
    Ok(faces)
}

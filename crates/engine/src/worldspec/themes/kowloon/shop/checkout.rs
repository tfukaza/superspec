//! Countertop register with reflected-host compensation for printed controls.
use super::super::{fixture::Fixture, pen::Tint};
use serde::Deserialize;
use std::sync::OnceLock;
use superspec_geometry::{V3, math::add};

pub const SIZE: V3 = [0.30, 0.28, 0.27];

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Register {
    faces: Vec<Vec<[f64; 11]>>,
    #[serde(default)]
    painted_faces: Vec<Vec<[f64; 11]>>,
}

pub fn draw(d: &mut Fixture<'_, '_>, at: V3, scale: f64, textured: bool) -> Result<(), String> {
    static ASSET: OnceLock<Register> = OnceLock::new();
    let asset = ASSET.get_or_init(|| {
        serde_json::from_str(include_str!("assets/shop-register.json"))
            .expect("shipped shop register")
    });
    let pen = d.pen.moved([
        at[0] + SIZE[0] * scale / 2.,
        at[1] + SIZE[1] * scale / 2.,
        at[2],
    ])?;
    let sign = if pen.mirrored { -1. } else { 1. };
    let detail = d.mesh.graphics_detail();
    if detail.reduced() {
        // The register's massing in its own sampled colours.
        let points: Vec<_> = asset
            .faces
            .iter()
            .chain(&asset.painted_faces)
            .flatten()
            .map(|v| {
                (
                    [v[0] * sign * scale, v[1] * scale, v[2] * scale],
                    [v[8], v[9], v[10]],
                )
            })
            .collect();
        return super::grocer_props::mass(d, &pen, &points, detail, false);
    }
    for (faces, textured) in [(&asset.faces, textured), (&asset.painted_faces, false)] {
        let slot = if textured { "shop-register" } else { "paint" };
        for face in faces {
            let points: Vec<_> = face
                .iter()
                .map(|v| [v[0] * sign * scale, v[1] * scale, v[2] * scale])
                .collect();
            let normals: Vec<_> = face.iter().map(|v| [v[3] * sign, v[4], v[5]]).collect();
            let hint = normals.iter().copied().fold([0.; 3], add);
            let uv: Vec<_> = face.iter().map(|v| [v[6], v[7]]).collect();
            let color = |p| {
                points
                    .iter()
                    .position(|v| *v == p)
                    .map(|i| [face[i][8], face[i][9], face[i][10]])
                    .ok_or_else(|| "Missing register corner tint.".into())
            };
            let tint = if textured {
                Tint::Color([1.; 3])
            } else {
                Tint::Field(&color)
            };
            pen.face(
                d.mesh,
                slot,
                &points,
                hint,
                &tint,
                d.repeat(slot),
                Some(&uv),
                Some(&normals),
            )?;
        }
    }
    Ok(())
}

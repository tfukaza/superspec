//! Delivered albedo sampled at frozen source triangle centroids, independently of bake identities.
use serde::Deserialize;
use superspec_engine::Scene;
#[derive(Deserialize)]
struct Probe {
    host: String,
    part: String,
    positions: Vec<f64>,
    normal: [f64; 3],
    rgba: [u8; 4],
    size: [usize; 2],
}
#[derive(Deserialize)]
struct Reference {
    samples: Vec<Probe>,
}
pub fn check(scene: &Scene) {
    let reference: Reference = serde_json::from_str(include_str!(
        "../../../../tests/fixtures/rust/multiple-building-color-samples.json"
    ))
    .unwrap();
    assert_eq!(reference.samples.len(), 6);
    for probe in reference.samples {
        assert!(
            probe
                .part
                .starts_with(&format!("{}/$dressing/", probe.host))
        );
        let mut matches = 0;
        for mesh in scene
            .manifest
            .meshes
            .iter()
            .filter(|m| m.object_id == probe.part)
        {
            for triangle in scene.indices[mesh.index_start..mesh.index_start + mesh.index_count]
                .as_chunks::<3>()
                .0
            {
                let indices = triangle.map(|v| mesh.vertex_start + v as usize);
                if !probe
                    .normal
                    .iter()
                    .enumerate()
                    .all(|(a, v)| scene.normals[indices[0] * 3 + a] == *v as f32)
                {
                    continue;
                }
                if !(0..3).any(|offset| {
                    (0..3).all(|corner| {
                        (0..3).all(|a| {
                            scene.positions[indices[(corner + offset) % 3] * 3 + a]
                                == probe.positions[corner * 3 + a] as f32
                        })
                    })
                }) {
                    continue;
                }
                matches += 1;
                let material = scene
                    .manifest
                    .materials
                    .iter()
                    .find(|m| Some(&m.id) == mesh.material.as_ref())
                    .unwrap();
                let texture = material.texture.as_ref().unwrap();
                assert_eq!(material.color, "#ffffff");
                assert!(
                    indices
                        .iter()
                        .all(|i| scene.colors[i * 3..i * 3 + 3] == [1.; 3])
                );
                let asset = scene
                    .manifest
                    .assets
                    .iter()
                    .find(|a| &a.id == texture)
                    .unwrap();
                let image = superspec_geometry::image::decode(
                    &scene.asset_bytes[asset.byte_start..asset.byte_start + asset.byte_length],
                )
                .unwrap();
                assert_eq!(
                    [image.width, image.height],
                    probe.size,
                    "{} raster dimensions",
                    probe.part
                );
                let uv: [f64; 2] = std::array::from_fn(|a| {
                    indices
                        .iter()
                        .map(|i| f64::from(scene.uvs[i * 2 + a]))
                        .sum::<f64>()
                        / 3.
                });
                assert!(uv.iter().all(|v| v.is_finite() && (0. ..=1.).contains(v)));
                let x = (uv[0] * image.width as f64)
                    .floor()
                    .clamp(0., (image.width - 1) as f64) as usize;
                let y = ((1. - uv[1]) * image.height as f64)
                    .floor()
                    .clamp(0., (image.height - 1) as f64) as usize;
                assert_eq!(
                    &image.rgba[(y * image.width + x) * 4..(y * image.width + x) * 4 + 4],
                    &probe.rgba,
                    "{} delivered color at {uv:?}",
                    probe.part
                );
            }
        }
        assert_eq!(
            matches, 1,
            "{} requires one unambiguous delivered triangle",
            probe.part
        );
    }
}

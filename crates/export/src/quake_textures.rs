use crate::Options;
use std::collections::BTreeMap;
use superspec_geometry::image::{self, Image};
use superspec_model::Scene;
use superspec_types::material::Material;

pub(crate) struct Texture {
    pub name: String,
    pub width: usize,
    pub height: usize,
}
pub(crate) struct Wad {
    pub bytes: Vec<u8>,
    pub textures: BTreeMap<String, Texture>,
}

fn linear(x: f64) -> f64 {
    if x <= 0.04045 {
        x / 12.92
    } else {
        ((x + 0.055) / 1.055).powf(2.4)
    }
}
fn srgb(x: f64) -> f64 {
    if x <= 0.0031308 {
        x * 12.92
    } else {
        1.055 * x.powf(1. / 2.4) - 0.055
    }
}

pub(crate) fn encode(scene: &Scene, palette: &[u8], options: &Options<'_>) -> Result<Wad, String> {
    if palette.len() != 768 {
        return Err("Quake palette must contain exactly 768 RGB bytes.".into());
    }
    let mut materials = scene.manifest.materials.clone();
    if !materials.iter().any(|m| m.id == "default") {
        materials.push(Material::default());
    }
    materials.push(Material {
        id: "__enclosure_sky".into(),
        color: "#829caf".into(),
        ..Default::default()
    });
    materials.sort_by(|a, b| a.id.cmp(&b.id));
    let mut chunks = vec![];
    let mut names = vec![];
    let mut textures = BTreeMap::new();
    let mut nearest = BTreeMap::<[u8; 3], u8>::new();
    let mut total = 12;
    for material in materials {
        options.check(total)?;
        if material.normal_texture.is_some()
            || material.metallic_roughness_texture.is_some()
            || material.emissive.is_some()
            || material.alpha_cutoff.is_some()
            || material.opacity.is_some_and(|x| x < 1.)
        {
            return Err(format!(
                "{} uses material channels outside Quake palette delivery. Use GLB or glTF for this material.",
                material.id
            ));
        }
        let image = if let Some(path) = &material.texture {
            let asset = scene
                .manifest
                .assets
                .iter()
                .find(|a| &a.id == path)
                .ok_or_else(|| format!("Missing Quake texture {path}."))?;
            image::decode(
                &scene.asset_bytes[asset.byte_start..asset.byte_start + asset.byte_length],
            )?
        } else {
            Image {
                width: 16,
                height: 16,
                rgba: vec![255; 1024],
                mime: "image/png",
            }
        };
        let tint: Vec<_> = [1, 3, 5]
            .iter()
            .map(|i| {
                u8::from_str_radix(&material.color[*i..*i + 2], 16).map(|v| linear(v as f64 / 255.))
            })
            .collect::<Result<_, _>>()
            .map_err(|e| e.to_string())?;
        let sky = material.id == "__enclosure_sky";
        let (width, height) = if sky {
            (256, 128)
        } else {
            (
                image.width.div_ceil(16).clamp(1, 32) * 16,
                image.height.div_ceil(16).clamp(1, 32) * 16,
            )
        };
        let name = if sky {
            "sky_ls".into()
        } else {
            format!("ls{:012x}", names.len())
        };
        let mut chunk = vec![0u8; 40];
        chunk[..name.len()].copy_from_slice(name.as_bytes());
        chunk[16..20].copy_from_slice(&(width as u32).to_le_bytes());
        chunk[20..24].copy_from_slice(&(height as u32).to_le_bytes());
        for lod in 0..4 {
            let offset = chunk.len() as u32;
            chunk[24 + lod * 4..28 + lod * 4].copy_from_slice(&offset.to_le_bytes());
            let (w, h) = (width >> lod, height >> lod);
            for y in 0..h {
                options.check(total + chunk.len() + w)?;
                for x in 0..w {
                    let sx = (((x as f64 + 0.5) * image.width as f64 / w as f64) as usize)
                        .min(image.width - 1);
                    let sy = (((y as f64 + 0.5) * image.height as f64 / h as f64) as usize)
                        .min(image.height - 1);
                    let p = (sy * image.width + sx) * 4;
                    let color = std::array::from_fn(|c| {
                        (srgb(linear(image.rgba[p + c] as f64 / 255.) * tint[c]) * 255.)
                            .round()
                            .clamp(0., 255.) as u8
                    });
                    let index = *nearest.entry(color).or_insert_with(|| {
                        // The final 32 palette entries are fullbright in Quake.
                        (0..224)
                            .min_by_key(|i| {
                                (0..3)
                                    .map(|c| {
                                        let d = color[c] as i32 - palette[i * 3 + c] as i32;
                                        d * d
                                    })
                                    .sum::<i32>()
                            })
                            .unwrap() as u8
                    });
                    chunk.push(index);
                }
            }
        }
        total += chunk.len() + 32;
        options.check(total)?;
        textures.insert(
            material.id,
            Texture {
                name: name.clone(),
                width,
                height,
            },
        );
        names.push(name);
        chunks.push(chunk);
        // Bound the lookup independently of output textures.
        if nearest.len() > 262144 {
            nearest.clear();
        }
    }
    let mut bytes = b"WAD2".to_vec();
    bytes.extend((chunks.len() as u32).to_le_bytes());
    bytes.extend((12u32 + chunks.iter().map(|c| c.len() as u32).sum::<u32>()).to_le_bytes());
    let mut directory = vec![];
    for (chunk, name) in chunks.into_iter().zip(names) {
        directory.extend((bytes.len() as u32).to_le_bytes());
        directory.extend((chunk.len() as u32).to_le_bytes());
        directory.extend((chunk.len() as u32).to_le_bytes());
        directory.extend([68, 0, 0, 0]);
        let mut field = [0; 16];
        field[..name.len()].copy_from_slice(name.as_bytes());
        directory.extend(field);
        bytes.extend(chunk);
    }
    bytes.extend(directory);
    Ok(Wad { bytes, textures })
}

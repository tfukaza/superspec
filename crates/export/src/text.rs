//! Portable Z-up OBJ/MTL bundles and architectural SVG plans.
use crate::{Options, Purpose};
use std::{collections::BTreeMap, fmt::Write};
use superspec_geometry::math::{cross, sub};
use superspec_model::Scene;

pub struct Obj {
    pub geometry: Vec<u8>,
    pub materials: Vec<u8>,
    pub textures: Vec<(String, Vec<u8>)>,
}

/// Keeps source coordinates and material UVs. Unsupported PBR channels fail explicitly.
pub fn obj(scene: &Scene, options: &Options<'_>) -> Result<Obj, String> {
    let meshes: Vec<_> = scene
        .manifest
        .meshes
        .iter()
        .filter(|m| match options.purpose {
            Purpose::Visual => m.visible,
            Purpose::Collision => scene.manifest.colliders.contains(&m.id),
        })
        .collect();
    if meshes.is_empty() {
        return Err("Cannot export an empty OBJ model.".into());
    }
    let mut geometry = String::from("# SuperSpec Z-up metres\nmtllib scene.mtl\n");
    let mut materials = String::new();
    let mut textures = vec![];
    let mut used = BTreeMap::new();
    let mut bytes = 0;
    for mesh in &meshes {
        let Some(id) = &mesh.material else { continue };
        if used.contains_key(id) {
            continue;
        }
        let name = format!("material_{}", used.len());
        used.insert(id.clone(), name.clone());
        let material = scene
            .manifest
            .materials
            .iter()
            .find(|m| &m.id == id)
            .ok_or("Unknown OBJ material.")?;
        if options.purpose == Purpose::Visual
            && (material.normal_texture.is_some()
                || material.metallic_roughness_texture.is_some()
                || material.alpha_cutoff.is_some()
                || material.emissive.is_some() && material.emissive_strength.unwrap_or(1.) > 0.
                || material.opacity.is_some_and(|a| a < 1.))
        {
            return Err(format!(
                "OBJ cannot represent all PBR channels of material {id}. Use GLB or glTF."
            ));
        }
        let rgb: Vec<_> = [1, 3, 5]
            .iter()
            .map(|i| u8::from_str_radix(&material.color[*i..*i + 2], 16).map(|v| v as f64 / 255.))
            .collect::<Result<_, _>>()
            .map_err(|e| e.to_string())?;
        writeln!(
            materials,
            "newmtl {name}\nKd {} {} {}\nNs {}",
            rgb[0],
            rgb[1],
            rgb[2],
            (1. - material.roughness) * 1000.
        )
        .unwrap();
        if options.purpose == Purpose::Visual {
            if let Some(id) = &material.texture {
                let asset = scene
                    .manifest
                    .assets
                    .iter()
                    .find(|a| &a.id == id)
                    .ok_or("Missing OBJ texture.")?;
                let texture = scene
                    .asset_bytes
                    .get(asset.byte_start..asset.byte_start + asset.byte_length)
                    .ok_or("Invalid OBJ texture range.")?;
                let extension = if asset.mime == "image/png" {
                    "png"
                } else {
                    "jpg"
                };
                let file = format!("texture-{}.{extension}", textures.len());
                writeln!(
                    materials,
                    "map_Kd -s {} {} 1 {file}",
                    material.repeat, material.repeat
                )
                .unwrap();
                bytes += texture.len();
                options.check(bytes)?;
                textures.push((file, texture.to_vec()));
            }
        }
        materials.push('\n');
    }
    let mut offset = 1;
    for mesh in meshes {
        options.check(bytes + geometry.len() + materials.len())?;
        writeln!(geometry, "g mesh_{}", offset).unwrap();
        if let Some(name) = mesh.material.as_ref().and_then(|id| used.get(id)) {
            writeln!(geometry, "usemtl {name}").unwrap();
        }
        for vertex in mesh.vertex_start..mesh.vertex_start + mesh.vertex_count {
            let p = &scene.positions[vertex * 3..vertex * 3 + 3];
            let n = &scene.normals[vertex * 3..vertex * 3 + 3];
            let uv = &scene.uvs[vertex * 2..vertex * 2 + 2];
            writeln!(
                geometry,
                "v {} {} {}\nvt {} {}\nvn {} {} {}",
                p[0], p[1], p[2], uv[0], uv[1], n[0], n[1], n[2]
            )
            .unwrap();
            options.check(bytes + geometry.len() + materials.len())?;
        }
        for triangle in
            scene.indices[mesh.index_start..mesh.index_start + mesh.index_count].chunks_exact(3)
        {
            let [a, b, c] = [
                triangle[0] as usize + offset,
                triangle[1] as usize + offset,
                triangle[2] as usize + offset,
            ];
            writeln!(geometry, "f {a}/{a}/{a} {b}/{b}/{b} {c}/{c}/{c}").unwrap();
            options.check(bytes + geometry.len() + materials.len())?;
        }
        offset += mesh.vertex_count;
    }
    Ok(Obj {
        geometry: geometry.into_bytes(),
        materials: materials.into_bytes(),
        textures,
    })
}

/// Projects authored floor loops and physical wall caps into a north-up SVG drawing.
pub fn svg_plan(scene: &Scene, options: &Options<'_>) -> Result<Vec<u8>, String> {
    let context = scene
        .manifest
        .architecture
        .as_ref()
        .ok_or("An SVG plan requires architectural context.")?;
    let mut bounds = [[0., 0.], [1., 1.]];
    let mut body = String::new();
    for (water, floors) in [(false, &context.floors), (true, &context.water)] {
        for floor in floors {
            options.check(body.len())?;
            let id = superspec_types::format::escape(&floor.id);
            write!(body,"<path data-object=\"{id}\" fill=\"{}\" fill-rule=\"evenodd\" stroke=\"#829cae\" stroke-width=\"0.04\" d=\"",if water {"#398cc9"}else{"#bbcbd8"}).unwrap();
            for ring in &floor.loops {
                for (i, p) in ring.iter().enumerate() {
                    for axis in 0..2 {
                        bounds[0][axis] = f64::min(bounds[0][axis], p[axis]);
                        bounds[1][axis] = f64::max(bounds[1][axis], p[axis]);
                    }
                    write!(
                        body,
                        "{} {} {} ",
                        if i == 0 { "M" } else { "L" },
                        p[0],
                        -p[1]
                    )
                    .unwrap();
                }
                body.push_str("Z ");
            }
            write!(body, "\"><title>{id}</title></path>").unwrap();
        }
    }
    for mesh in &scene.manifest.meshes {
        if !scene.manifest.colliders.contains(&mesh.id)
            || !context
                .objects
                .iter()
                .any(|o| o.id == mesh.object_id && o.kind == "wall")
        {
            continue;
        }
        let id = superspec_types::format::escape(&mesh.object_id);
        write!(body, "<path data-object=\"{id}\" fill=\"#233749\" d=\"").unwrap();
        for triangle in
            scene.indices[mesh.index_start..mesh.index_start + mesh.index_count].chunks_exact(3)
        {
            let indices: [u32; 3] = triangle.try_into().unwrap();
            let points = indices.map(|i| {
                std::array::from_fn::<_, 3, _>(|axis| {
                    scene.positions[(mesh.vertex_start + i as usize) * 3 + axis] as f64
                })
            });
            if cross(sub(points[1], points[0]), sub(points[2], points[0]))[2] <= 1e-10 {
                continue;
            }
            for (i, p) in points.iter().enumerate() {
                for axis in 0..2 {
                    bounds[0][axis] = f64::min(bounds[0][axis], p[axis]);
                    bounds[1][axis] = f64::max(bounds[1][axis], p[axis]);
                }
                write!(
                    body,
                    "{} {} {} ",
                    if i == 0 { "M" } else { "L" },
                    p[0],
                    -p[1]
                )
                .unwrap();
            }
            body.push_str("Z ");
            options.check(body.len())?;
        }
        body.push_str("\"/>");
    }
    let width = bounds[1][0] - bounds[0][0] + 2.;
    let height = bounds[1][1] - bounds[0][1] + 2.;
    let output = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"{} {} {width} {height}\" role=\"img\" aria-label=\"Architectural floor plan\">{body}</svg>",
        bounds[0][0] - 1.,
        -bounds[1][1] - 1.
    );
    options.check(output.len())?;
    Ok(output.into_bytes())
}

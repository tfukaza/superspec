//! Valve 220 map and WAD2 encoding. Native adapters own the external BSP toolchain.
use crate::{Options, brushes, quake_textures};
use serde::Serialize;
use std::{collections::BTreeMap, fmt::Write};
use superspec_geometry::{Geometry, Matrix, Operation, Solid, math::*};
use superspec_model::{ExportFace, ExportSolid, Scene};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    pub brushes: usize,
    pub spawns: Vec<V3>,
    pub units_per_meter: u32,
    pub warnings: Vec<&'static str>,
}
pub struct Files {
    pub map: Vec<u8>,
    pub wad: Vec<u8>,
    pub report: Report,
}
fn quoted(value: &str) -> String {
    format!("\"{}\"", value.replace(['\"', '\\', '\r', '\n'], "_"))
}
fn box_source(min: V3, max: V3, material: &str) -> Result<ExportSolid, String> {
    let solid = Solid::cube(sub(max, min))?.transform(Matrix::translation(min))?;
    let mesh = solid.mesh();
    let points: Vec<V3> = mesh
        .vert_properties
        .chunks_exact(mesh.num_prop as usize)
        .map(|p| [p[0] as f64, p[1] as f64, p[2] as f64])
        .collect();
    let triangles: Vec<[usize; 3]> = mesh
        .tri_verts
        .chunks_exact(3)
        .map(|t| [t[0] as usize, t[1] as usize, t[2] as usize])
        .collect();
    let faces = triangles
        .iter()
        .map(|t| ExportFace {
            points: t.map(|i| points[i]),
            material: material.into(),
            visible: true,
            collidable: true,
            dynamic: false,
            object: "__enclosure".into(),
        })
        .collect();
    Ok(ExportSolid {
        geometry: Geometry { points, triangles },
        faces,
    })
}
fn emit(
    input: &ExportSolid,
    textures: &BTreeMap<String, quake_textures::Texture>,
    scene: &Scene,
    options: &Options<'_>,
) -> Result<Vec<String>, String> {
    let first = input.faces.first().ok_or("Empty brush source.")?;
    if input
        .faces
        .iter()
        .any(|f| (f.collidable, f.dynamic) != (first.collidable, first.dynamic))
    {
        return Err("A combined brush has conflicting collision or dynamic ownership.".into());
    }
    let mut output = vec![];
    for brush in brushes::decompose(input, options)? {
        let mut text = format!("// source {}\n{{\n", quoted(&first.object));
        for plane in brush.planes {
            let face = plane.face.map(|i| &input.faces[i]).unwrap_or(first);
            let material = textures
                .get(&face.material)
                .or_else(|| textures.get("default"))
                .unwrap();
            let name = if face.visible { &material.name } else { "skip" };
            let repeat = scene
                .manifest
                .materials
                .iter()
                .find(|m| m.id == face.material)
                .map_or(1., |m| m.repeat);
            let u = unit(cross(
                if plane.normal[2].abs() > 0.9 {
                    [0., 1., 0.]
                } else {
                    [0., 0., 1.]
                },
                plane.normal,
            ))?;
            let v = cross(plane.normal, u);
            for p in plane.points.iter().rev() {
                write!(
                    text,
                    "( {:.7} {:.7} {:.7} ) ",
                    p[0] * 32.,
                    p[1] * 32.,
                    p[2] * 32.
                )
                .unwrap();
            }
            let u = scale(u, material.width as f64 * repeat / 32.);
            let v = scale(v, -(material.height as f64) * repeat / 32.);
            writeln!(
                text,
                "{name} [ {:.7} {:.7} {:.7} 0 ] [ {:.7} {:.7} {:.7} 0 ] 0 1 1",
                u[0], u[1], u[2], v[0], v[1], v[2]
            )
            .unwrap();
        }
        text.push_str("}\n");
        output.push(text);
    }
    Ok(output)
}
fn spawn_height(scene: &Scene, p: V3, owner: Option<&str>) -> Option<f64> {
    let mut result: Option<f64> = None;
    for floor in &scene.manifest.architecture.as_ref()?.floors {
        if owner.is_some_and(|id| id != floor.id && id != floor.area) {
            continue;
        }
        for t in &floor.triangles {
            let q = t.map(|i| floor.points[i]);
            let n = cross(sub(q[1], q[0]), sub(q[2], q[0]));
            if n[2].abs() < 1e-9
                || (0..3).any(|i| n[2].signum() * orient(q[i], q[(i + 1) % 3], p) < -1e-7)
            {
                continue;
            }
            let z = q[0][2] - (n[0] * (p[0] - q[0][0]) + n[1] * (p[1] - q[0][1])) / n[2];
            if (z - p[2]).abs() <= 0.1 {
                result = Some(result.map_or(z, |old| old.max(z)));
            }
        }
    }
    result
}

pub fn encode(scene: &Scene, palette: &[u8], options: &Options<'_>) -> Result<Files, String> {
    options.check(0)?;
    if scene.export_solids.is_empty() {
        return Err("BSP requires explicit closed-solid retention.".into());
    }
    let wad = quake_textures::encode(scene, palette, options)?;
    let context = scene
        .manifest
        .architecture
        .as_ref()
        .ok_or("Missing architectural context.")?;
    let mut collision = vec![];
    let mut world = vec![];
    let mut entities = vec![];
    let mut bounds = [[f64::INFINITY; 3], [f64::NEG_INFINITY; 3]];
    let mut count = 0;
    let mut map_bytes = 0;
    for input in &scene.export_solids {
        let face = input.faces.first().ok_or("Empty brush source.")?;
        for p in &input.geometry.points {
            for i in 0..3 {
                bounds[0][i] = bounds[0][i].min(p[i]);
                bounds[1][i] = bounds[1][i].max(p[i]);
            }
        }
        if !input.faces.iter().any(|f| f.visible || f.collidable) {
            continue;
        }
        if face.collidable {
            collision.push(Solid::from_geometry(
                &input.geometry,
                Solid::reserve_owner(),
            )?);
        }
        let brushes = emit(input, &wad.textures, scene, options)?;
        count += brushes.len();
        map_bytes += brushes.iter().map(String::len).sum::<usize>();
        options.check(map_bytes + wad.bytes.len())?;
        if count > 100000 {
            return Err("BSP exceeds 100000 brushes.".into());
        }
        if face.collidable && !face.dynamic {
            world.extend(brushes);
        } else {
            let class = if face.collidable {
                "func_wall"
            } else {
                "func_detail_illusionary"
            };
            entities.push(format!(
                "{{\n\"classname\" \"{class}\"\n\"targetname\" {}\n{}\n}}\n",
                quoted(&face.object),
                brushes.join("")
            ));
        }
    }
    for i in 0..3 {
        bounds[0][i] -= 8.;
        bounds[1][i] += 8.;
    }
    for axis in 0..3 {
        for side in 0..2 {
            let mut min = bounds[0].map(|x| x - 1.);
            let mut max = bounds[1].map(|x| x + 1.);
            if side == 0 {
                max[axis] = bounds[0][axis];
            } else {
                min[axis] = bounds[1][axis];
            }
            let material = if axis == 2 && side == 1 {
                "__enclosure_sky"
            } else {
                "default"
            };
            let brushes = emit(
                &box_source(min, max, material)?,
                &wad.textures,
                scene,
                options,
            )?;
            count += brushes.len();
            world.extend(brushes);
        }
    }
    let mut spawns = vec![];
    for marker in context.metadata.iter().filter(|n| n.kind == "marker") {
        let kind = marker.text("kind").unwrap_or("");
        if !(kind == "spawn"
            || kind.ends_with("_spawn")
            || kind == "info_player_start"
            || kind == "info_player_deathmatch")
        {
            continue;
        }
        options.check(map_bytes + wad.bytes.len())?;
        let p = marker.vector("position", [0.; 3]);
        let feet = spawn_height(scene, p, marker.text("element")).ok_or_else(|| {
            format!(
                "{} must lie on its authored floor for BSP delivery.",
                marker.id
            )
        })? + 1. / 32.;
        let hull = Solid::cube([1., 1., 56. / 32.])?.transform(Matrix::translation([
            p[0] - 0.5,
            p[1] - 0.5,
            feet,
        ]))?;
        for solid in &collision {
            if solid.boolean(&hull, Operation::Intersection)?.volume() > 1e-7 {
                return Err(format!("{} has a blocked Quake player hull.", marker.id));
            }
        }
        let origin = [p[0] * 32., p[1] * 32., feet * 32. + 24.];
        let class = if spawns.is_empty() {
            "info_player_start"
        } else {
            "info_player_deathmatch"
        };
        entities.push(format!(
            "{{\n\"classname\" \"{class}\"\n\"targetname\" {}\n\"origin\" \"{} {} {}\"\n}}\n",
            quoted(&marker.id),
            origin[0],
            origin[1],
            origin[2]
        ));
        spawns.push(origin);
    }
    if spawns.is_empty() {
        return Err("BSP requires an authored spawn marker.".into());
    }
    if spawns.len() == 1 {
        let p = spawns[0];
        entities.push(format!(
            "{{\n\"classname\" \"info_player_deathmatch\"\n\"origin\" \"{} {} {}\"\n}}\n",
            p[0], p[1], p[2]
        ));
    }
    let map = format!(
        "// SuperSpec / Valve 220 / Z-up / 32 units per meter\n{{\n\"classname\" \"worldspawn\"\n\"message\" {}\n\"wad\" \"scene.wad\"\n\"_minlight\" \"80\"\n{}\n}}\n{}",
        quoted(&scene.manifest.name),
        world.join(""),
        entities.join("")
    );
    options.check(map.len() + wad.bytes.len())?;
    Ok(Files {
        map: map.into_bytes(),
        wad: wad.bytes,
        report: Report {
            brushes: count,
            spawns,
            units_per_meter: 32,
            warnings: vec![
                "Quake uses palette textures, classic collision hulls and neutral minimum lighting.",
                "BSP faces use world-space physical texture scale. Authored lightmap and unfolded UVs remain in the portable mesh exports.",
                "Dynamic solids become static func_wall entities. Gameplay metadata is retained in scene.json.",
                "An eight-meter enclosure surrounds the authored scene with a sky ceiling.",
            ],
        },
    })
}

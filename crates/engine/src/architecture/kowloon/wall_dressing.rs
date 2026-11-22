//! Visual wall finishes over authored exposed triangles, with optional window fixtures.
use super::*;
use crate::worldspec::themes::kowloon::{
    building::materials::{Appearance, Settings},
    facade_material::{self, Coat},
    fixture::Fixture,
    kit_materials::Library,
    mesh::{Mesh as Drawing, Mode},
    metal::Binding,
    palette::srgb,
    pavement::field,
    pen::Pen,
    tile, tiled_skin, wall, windows,
};
use superspec_aml::rng::SeededRng;

pub(super) struct Recipe {
    index: usize,
    rule: Node,
}
pub(super) struct Job {
    host: String,
    rule: Node,
    appearance: Appearance,
    drawing: Drawing<'static>,
    materials: BTreeMap<String, Material>,
}
struct Plane {
    frame: Matrix,
    faces: Vec<[V3; 3]>,
}
impl Compiler<'_> {
    pub(super) fn wall_recipes(&self, materials: &[Material]) -> Result<Vec<Recipe>, Diagnostic> {
        let Some(settings) = self.dressing_settings() else {
            return Ok(vec![]);
        };
        let mut result = vec![];
        for rule in settings.children.iter().filter(|r| r.kind == "wall") {
            let host = rule.text("host").unwrap();
            let fail = |s| rule.fail("AML_DRESSING", s);
            let index = *self
                .resolution
                .ids
                .get(host)
                .ok_or_else(|| fail("Unknown wall dressing host."))?;
            let node = &self.resolution.nodes[index];
            if !["wall", "retaining-wall", "box", "extrusion"].contains(&node.source.kind.as_str())
            {
                return Err(fail("Wall dressing requires a wall or solid host."));
            }
            if !materials
                .iter()
                .any(|m| m.id == rule.text("material").unwrap())
            {
                return Err(fail("Unknown wall dressing material."));
            }
            result.push(Recipe {
                index,
                rule: rule.clone(),
            });
        }
        Ok(result)
    }

    pub(super) fn dress_walls(
        &self,
        geometry: &mut BTreeMap<u32, Geometry>,
        recipes: Vec<Recipe>,
    ) -> Result<Vec<Job>, Diagnostic> {
        if recipes.is_empty() {
            return Ok(vec![]);
        }
        let settings = self.dressing_settings().unwrap();
        let mut library = Library::new(
            &self.materials()?,
            settings.text("finishes"),
            settings.text("time-of-day") == Some("night"),
        )
        .map_err(|e| settings.fail("AML_DRESSING", e))?;
        let substrates: Vec<_> = settings
            .text("wall-materials")
            .unwrap_or("")
            .split_whitespace()
            .map(str::to_owned)
            .collect();
        let mut jobs = vec![];
        for Recipe { index, rule } in recipes {
            let node = &self.resolution.nodes[index];
            let fail = |e: String| rule.fail("AML_DRESSING", e);
            let mut planes = BTreeMap::<[i64; 4], Plane>::new();
            for (owner, geometry) in geometry.iter_mut() {
                let surface = &self.surfaces[owner];
                if surface.node != index || surface.assigned || !surface.visible {
                    continue;
                }
                let mut retained = vec![];
                for triangle in &geometry.triangles {
                    let points = triangle.map(|i| geometry.points[i]);
                    let Ok(normal) =
                        unit(cross(sub(points[1], points[0]), sub(points[2], points[0])))
                    else {
                        retained.push(*triangle);
                        continue;
                    };
                    let local = unit([
                        dot(node.matrix.0[0], normal),
                        dot(node.matrix.0[1], normal),
                        dot(node.matrix.0[2], normal),
                    ])
                    .map_err(fail)?;
                    let receives = match rule.text("face").unwrap_or("side") {
                        "front" => local[1] < -0.99999,
                        "back" => local[1] > 0.99999,
                        "left" => local[0] < -0.99999,
                        "right" => local[0] > 0.99999,
                        _ => local[2].abs() < 1e-5,
                    };
                    if !receives {
                        retained.push(*triangle);
                        continue;
                    }
                    if normal[2].abs() > 1e-5 {
                        return Err(fail(
                            "Wall finish currently requires vertical world-space faces.".into(),
                        ));
                    }
                    let normal = unit([normal[0], normal[1], 0.]).map_err(fail)?;
                    let offset = dot(points[0], normal);
                    let key = [normal[0], normal[1], normal[2], offset]
                        .map(|v| (v * 100000.).round() as i64);
                    let plane = planes.entry(key).or_insert_with(|| Plane {
                        frame: Matrix([
                            [normal[1], -normal[0], 0.],
                            normal,
                            [0., 0., 1.],
                            normal.map(|v| v * offset),
                        ]),
                        faces: vec![],
                    });
                    let inverse = plane.frame.inverse().map_err(fail)?;
                    plane.faces.push(points.map(|p| inverse.point(p)));
                }
                geometry.triangles = retained;
            }
            if planes.is_empty() {
                continue;
            }
            let mut appearance =
                Appearance::new(&mut library, rule.text("material").unwrap(), &substrates)
                    .map_err(fail)?;
            if let Some(color) = rule.text("tile-color") {
                let id = format!("$wall/{}/ceramic", node.id);
                let mut material = library.materials["$kowloon/wall-ceramic"].clone();
                material.id = id.clone();
                material.color = color.into();
                material.validate().map_err(fail)?;
                library.materials.insert(id.clone(), material.clone());
                material.id = "$kowloon/wall-ceramic".into();
                appearance.inputs.insert(material.id.clone(), material);
                appearance
                    .bindings
                    .insert("$kowloon/wall-ceramic".into(), id);
            }
            let identity = format!("{}/{}", settings.text("seed").unwrap(), node.id);
            let grime = rule.number("grime", settings.number("grime", 0.4));
            let wear = rule.number("wear", settings.number("wear", 0.2));
            let density = rule.number("texture-density", 64.);
            let kit = appearance
                .kit(&Settings {
                    seed: &identity,
                    grime,
                    wear,
                    rust: wear,
                    density,
                    detail: 1,
                })
                .map_err(fail)?;
            let mut drawing = Drawing::new(Mode::Render, 2_000_000, &|| false).map_err(fail)?;
            drawing
                .bind_source_materials(&appearance.bindings)
                .map_err(fail)?;
            for (number, plane) in planes.into_values().enumerate() {
                let bounds = plane.faces.iter().flatten().fold(
                    [
                        f64::INFINITY,
                        f64::INFINITY,
                        f64::NEG_INFINITY,
                        f64::NEG_INFINITY,
                    ],
                    |mut b, p| {
                        b[0] = b[0].min(p[0]);
                        b[1] = b[1].min(p[2]);
                        b[2] = b[2].max(p[0]);
                        b[3] = b[3].max(p[2]);
                        b
                    },
                );
                let seed = wall::finish_seed(&identity);
                let backing = if substrates.is_empty() {
                    "concrete"
                } else {
                    "substrate-0"
                };
                let finish = facade_material::Settings {
                    concrete: rule.text("material").unwrap().contains("concrete"),
                    coat: Coat {
                        material: "wall".into(),
                        repeat: appearance.repeat("wall"),
                        tint: [1.; 3],
                    },
                    backing: Binding {
                        material: backing.into(),
                        repeat: appearance.repeat(backing),
                    },
                    wear,
                    density,
                    variant: seed % 2,
                };
                let ceramic = &appearance.inputs["$kowloon/wall-ceramic"];
                let bed = &appearance.inputs["$kowloon/wall-tile-bed"];
                let recipe = tiled_skin::Recipe {
                    bounds,
                    finish,
                    condition: wall::Settings {
                        base: Some(bounds[1]),
                        ledges: vec![],
                        openings: vec![],
                        grime,
                        wear,
                        seed: f64::from(seed),
                    },
                    tiles: tiled_skin::Settings {
                        seed,
                        field: tile::Settings {
                            seed,
                            share: rule.number("tile-share", 0.),
                            wear: rule.number("tile-wear", wear),
                            grime: rule.number("tile-grime", grime),
                            size: rule.optional_number("tile-size"),
                            aspect: rule.optional_number("tile-aspect"),
                        },
                        materials: tile::Materials {
                            ceramic: "$kowloon/wall-ceramic".into(),
                            bed: "$kowloon/wall-tile-bed".into(),
                            grime: "$kowloon/wall-tile-grime".into(),
                            repeat: ceramic.repeat,
                            bed_repeat: bed.repeat,
                            bed_roughness: bed.roughness,
                            roughness: ceramic.roughness,
                        },
                        repair: Binding {
                            material: "render-repair".into(),
                            repeat: appearance.repeat("render-repair"),
                        },
                    },
                    washes: vec![],
                    openings: vec![],
                    mapping_seed: seed,
                };
                let pen = Pen::new(plane.frame, "wall").map_err(fail)?;
                let mut fixture = Fixture {
                    pen,
                    mesh: &mut drawing,
                    repeats: &|slot| appearance.repeat(slot),
                };
                tiled_skin::faces(
                    &mut fixture,
                    &plane.faces,
                    &recipe,
                    &format!("wall/{number}"),
                )
                .map_err(fail)?;
                if rule.boolean("windows", false) {
                    fixture.pen = fixture.pen.into_part("openings").map_err(fail)?;
                    let count = ((bounds[2] - bounds[0] - 0.6) / 2.2).floor().max(0.) as usize;
                    let height = (bounds[3] - bounds[1] - 1.3).min(1.25);
                    if height < 0.6 {
                        continue;
                    }
                    for bay in 0..count.min(128) {
                        let x =
                            bounds[0] + (bounds[2] - bounds[0]) * (bay as f64 + 0.5) / count as f64;
                        let span = [
                            x - 0.6,
                            x + 0.6,
                            bounds[1] + 1.05,
                            bounds[1] + 1.05 + height,
                        ];
                        if !covered(&plane.faces, span) {
                            continue;
                        }
                        let look = windows::Look {
                            weathering: Some(kit.settings().weathering.clone()),
                            kind: windows::Kind::Slider,
                            frame: srgb("#53645d"),
                            trim: srgb("#c7c3b4"),
                            curtain: srgb("#cfbea1"),
                            reach: 0.25,
                            hood: true,
                            lit: false,
                            boarded: false,
                            fixed: false,
                            recess: 0.,
                            transom_ratio: None,
                            lower_pane_bars: None,
                            timber_textured: true,
                        };
                        windows::window(
                            &mut fixture,
                            span,
                            &look,
                            &mut SeededRng::from_text(&format!("{identity}/{bay}")),
                        )
                        .map_err(fail)?;
                    }
                }
            }
            jobs.push(Job {
                host: node.id.clone(),
                rule,
                appearance,
                drawing,
                materials: library.materials.clone(),
            });
        }
        Ok(jobs)
    }
    pub(super) fn render_walls(
        &self,
        visuals: &mut VisualOutput<'_>,
        jobs: Vec<Job>,
        theme: &mut ThemeState,
        execution: &Execution<'_>,
    ) -> Result<(), Diagnostic> {
        for mut job in jobs {
            if !theme.building_hosts.is_empty() {
                theme.render_edge_vertices.extend(
                    job.drawing
                        .indices
                        .iter()
                        .map(|i| job.drawing.vertices[*i as usize].position),
                );
            }
            self.bake_visual(
                execution,
                visuals,
                &mut job.drawing,
                &job.host,
                &job.rule,
                &job.appearance.inputs,
                &job.materials,
                job.appearance.bindings,
                None,
            )?;
        }
        Ok(())
    }
}
fn covered(faces: &[[V3; 3]], b: [f64; 4]) -> bool {
    let rectangle = vec![
        [b[0], b[2], 0.],
        [b[1], b[2], 0.],
        [b[1], b[3], 0.],
        [b[0], b[3], 0.],
    ];
    let area: f64 = faces
        .iter()
        .map(|t| field::area(&field::intersect(&t.map(|p| [p[0], p[2], 0.]), &rectangle)).abs())
        .sum();
    area >= (b[1] - b[0]) * (b[3] - b[2]) - 1e-5
}

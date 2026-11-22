//! Authored pavement receivers and architectural contacts; physical geometry remains untouched.
use super::*;
use crate::worldspec::themes::kowloon::{
    kit_materials::Library,
    mesh::{Mesh as Drawing, Mode},
    pavement::{
        self,
        context::Wall,
        layout::{self, Neighbor},
        plan::Settings,
        render::Site,
    },
    seams::{self, Contacts, Facet, Kind},
};
use superspec_geometry::obstructions::Budget;

pub(super) struct Prepared {
    index: usize,
    faces: Vec<Vec<V3>>,
    layout: layout::Layout,
    seams: Vec<seams::Seam>,
    walls: Vec<Wall>,
}

pub(super) fn settings(rule: &Node, host_kind: &str) -> Result<Option<Settings>, Diagnostic> {
    let options = [
        "patch-size",
        "material-size",
        "relief",
        "wetness",
        "litter",
        "repairs",
        "tile-share",
        "tile-size",
        "tile-aspect",
        "tile-wear",
        "tile-grime",
        "tile-color",
        "timber-share",
        "joined-pours",
        "asphalt",
        "manholes",
        "breakup",
        "asphalt-material",
        "texture-density",
    ];
    let Some(mode) = rule.text("ground") else {
        if options.iter().any(|key| rule.attributes.contains_key(*key)) {
            return Err(rule.fail("AML_DRESSING", "Ground controls require ground=patched."));
        }
        return Ok(None);
    };
    if mode != "patched" || !["slab", "landing"].contains(&host_kind) {
        return Err(rule.fail(
            "AML_DRESSING",
            "Patched ground requires a horizontal slab or landing.",
        ));
    }
    let defaults = Settings::default();
    if rule
        .optional_number("tile-size")
        .is_some_and(|v| !(0.01..=10.).contains(&v))
        || rule
            .optional_number("tile-aspect")
            .is_some_and(|v| !(0.1..=10.).contains(&v))
    {
        return Err(rule.fail(
            "AML_DRESSING",
            "Ground tile size or aspect exceeds supported bounds.",
        ));
    }
    let result = Settings {
        patch_size: rule.number("patch-size", defaults.patch_size),
        material_size: rule.number("material-size", defaults.material_size),
        relief: rule.number("relief", defaults.relief),
        wetness: rule.number("wetness", defaults.wetness),
        litter: rule.number("litter", defaults.litter),
        repairs: rule.number("repairs", defaults.repairs),
        tile_share: rule.number("tile-share", defaults.tile_share),
        timber_share: rule.number("timber-share", defaults.timber_share),
        joined_pours: rule.number("joined-pours", defaults.joined_pours),
        asphalt: rule.number("asphalt", defaults.asphalt),
        manholes: rule.number("manholes", defaults.manholes),
        breakup: rule.number("breakup", defaults.breakup),
    };
    result
        .validate()
        .map_err(|e| rule.fail("AML_DRESSING", e))?;
    Ok(Some(result))
}

/// The same local top-face predicate is used for admission and render replacement.
pub(super) fn receives(matrix: Matrix, normal: V3) -> bool {
    unit([
        dot(matrix.0[0], normal),
        dot(matrix.0[1], normal),
        dot(matrix.0[2], normal),
    ])
    .is_ok_and(|local| local[2] > 0.7)
}

fn assets(library: &mut Library, rule: &Node) -> Result<pavement::render::Assets, Diagnostic> {
    let fail = |e: String| rule.fail("AML_DRESSING", e);
    let declared = rule
        .text("asphalt-material")
        .map(|id| BTreeMap::from([("asphalt".into(), id.into())]))
        .unwrap_or_default();
    let authored = library.authored_ids().cloned().collect();
    let colors = rule
        .text("tile-color")
        .map(|color| BTreeMap::from([("tile".into(), color.into())]))
        .unwrap_or_default();
    let assets = pavement::materials::assets(
        &pavement::materials::Settings {
            scope: rule.text("host").unwrap(),
            base: rule.text("material").unwrap(),
            declared: &declared,
            colors: &colors,
            authored: &authored,
        },
        &mut library.materials,
    )
    .map_err(fail)?;
    for source in assets.sources.values().chain([
        &assets.tile_bed,
        &assets.edge,
        &assets.water,
        &assets.paper,
        &assets.brick,
        &assets.can,
    ]) {
        if library.materials[&source.id].height_texture.is_some() {
            return Err(fail(
                "Patched ground displacement is not connected yet.".into(),
            ));
        }
    }
    Ok(assets)
}
pub(super) fn validate_materials(
    materials: &[Material],
    settings: &Node,
    rule: &Node,
) -> Result<(), Diagnostic> {
    if rule.text("host").unwrap().len() > 512 || settings.text("seed").unwrap().len() > 1024 {
        return Err(rule.fail(
            "AML_COMPLEXITY",
            "Ground hosts and seeds exceed procedural identity allowance.",
        ));
    }
    let mut library = Library::new(
        materials,
        settings.text("finishes"),
        settings.text("time-of-day") == Some("night"),
    )
    .map_err(|e| rule.fail("AML_DRESSING", e))?;
    assets(&mut library, rule)?;
    Ok(())
}

fn account_points(
    points: usize,
    total: &mut usize,
    budget: &mut Budget<'_>,
    rule: &Node,
) -> Result<(), Diagnostic> {
    budget
        .spend(points)
        .map_err(|e| rule.fail("AML_COMPLEXITY", e))?;
    *total = total
        .checked_add(points)
        .ok_or_else(|| rule.fail("AML_COMPLEXITY", "Ground context size overflow."))?;
    if *total > 1_048_576 {
        return Err(rule.fail("AML_COMPLEXITY", "Ground context point allowance exceeded."));
    }
    Ok(())
}

impl Compiler<'_> {
    pub(super) fn plan_ground(
        &self,
        output: &Product,
        render: &BTreeMap<u32, Geometry>,
        theme: &ThemeState,
    ) -> Result<Vec<Prepared>, Diagnostic> {
        if !theme.floor_dressing.values().any(|p| p.ground.is_some()) {
            return Ok(vec![]);
        }
        let fail = |e: String| self.dressing_settings().unwrap().fail("AML_DRESSING", e);
        let mut budget = Budget::new(16_000_000, &|| false).map_err(fail)?;
        let mut selected = BTreeMap::<usize, Vec<Vec<V3>>>::new();
        let mut inspected = 0;
        for (owner, geometry) in render {
            let surface = &self.surfaces[owner];
            let Some(plan) = theme
                .floor_dressing
                .get(&surface.node)
                .filter(|p| p.ground.is_some())
            else {
                continue;
            };
            if !surface.visible || surface.assigned {
                continue;
            }
            let matrix = self.resolution.nodes[surface.node].matrix;
            for t in &geometry.triangles {
                budget.spend(1).map_err(fail)?;
                inspected += 1;
                if inspected > 1_048_576 {
                    return Err(plan.source.fail(
                        "AML_COMPLEXITY",
                        "Ground receiver triangle allowance exceeded.",
                    ));
                }
                let points = t.map(|i| geometry.points[i]);
                let Ok(normal) = unit(cross(sub(points[1], points[0]), sub(points[2], points[0])))
                else {
                    continue;
                };
                if !receives(matrix, normal) {
                    continue;
                }
                if normal[2] < 0.99999 || points.iter().any(|p| (p[2] - points[0][2]).abs() > 1e-6)
                {
                    return Err(plan.source.fail(
                        "AML_DRESSING",
                        "Patched ground requires horizontal receiving faces in world coordinates.",
                    ));
                }
                let faces = selected.entry(surface.node).or_default();
                if faces
                    .first()
                    .is_some_and(|f| (f[0][2] - points[0][2]).abs() > 1e-6)
                {
                    return Err(plan.source.fail(
                        "AML_DRESSING",
                        "Patched ground requires one horizontal receiving elevation per host.",
                    ));
                }
                if faces.len() >= 65_536 {
                    return Err(plan
                        .source
                        .fail("AML_COMPLEXITY", "Ground receiver allowance exceeded."));
                }
                faces.push(points.to_vec());
            }
        }
        if selected.is_empty() {
            return Ok(vec![]);
        }
        let physical = owned_geometry(&output.solids, &output.patches);
        let mut contacts = seams::contacts(
            physical.iter().flat_map(|(owner, g)| {
                let surface = &self.surfaces[owner];
                let node = &self.resolution.nodes[surface.node];
                g.triangles.iter().map(move |t| {
                    let points = t.map(|i| g.points[i]);
                    Facet {
                        object: &node.id,
                        kind: match node.source.kind.as_str() {
                            "slab" | "landing" => Kind::Floor,
                            "stairs" | "ramp" => Kind::Stairs,
                            "wall" => Kind::Wall,
                            _ => Kind::Other,
                        },
                        visible: surface.visible,
                        collidable: surface.collidable,
                        points,
                        normal_z: unit(cross(sub(points[1], points[0]), sub(points[2], points[0])))
                            .map_or(0., |n| n[2]),
                    }
                })
            }),
            &mut budget,
        )
        .map_err(fail)?;
        let mut result = vec![];
        for (index, faces) in selected {
            let plan = &theme.floor_dressing[&index];
            let options = plan.ground.as_ref().unwrap();
            let node = &self.resolution.nodes[index];
            let mut context_points = 0;
            let neighbors =
                self.ground_neighbors(index, &contacts, &mut budget, &mut context_points)?;
            let mut connections = vec![];
            for n in &self.resolution.document.metadata {
                budget.spend(1).map_err(fail)?;
                if n.kind != "connection"
                    || ![n.text("from"), n.text("to")].contains(&Some(node.id.as_str()))
                {
                    continue;
                }
                if let Some(id) = n.text("path") {
                    if connections.len() >= 20_000 {
                        return Err(plan
                            .source
                            .fail("AML_COMPLEXITY", "Ground connection allowance exceeded."));
                    }
                    let path = &self.resolution.paths[id];
                    account_points(path.len(), &mut context_points, &mut budget, &plan.source)?;
                    connections.push(path.clone());
                }
            }
            let layout = layout::authored(
                layout::Authored {
                    loops: &self.floors[&index].loops,
                    origin: node.matrix.0[3],
                    receiving: &faces,
                    neighbors: &neighbors,
                    connections: &connections,
                },
                options.patch_size,
                options.material_size,
                &mut budget,
            )
            .map_err(|e| plan.source.fail("AML_DRESSING", e))?;
            let walls = contacts
                .walls_by_floor
                .remove(&node.id)
                .unwrap_or_default()
                .into_iter()
                .flat_map(|c| {
                    c.seams.into_iter().map(move |seam| Wall {
                        host: c.host.clone(),
                        seam,
                    })
                })
                .collect();
            result.push(Prepared {
                index,
                faces,
                layout,
                seams: contacts.by_object.remove(&node.id).unwrap_or_default(),
                walls,
            });
        }
        Ok(result)
    }

    fn ground_neighbors(
        &self,
        index: usize,
        contacts: &Contacts,
        budget: &mut Budget<'_>,
        points: &mut usize,
    ) -> Result<Vec<Neighbor>, Diagnostic> {
        let host = &self.resolution.nodes[index].id;
        let mut neighbors = vec![];
        for (other, node) in self.resolution.nodes.iter().enumerate() {
            budget
                .spend(1)
                .map_err(|e| node.source.fail("AML_COMPLEXITY", e))?;
            if other == index
                || !node.source.boolean("visible", true)
                || !node.source.boolean("collidable", true)
            {
                continue;
            }
            if node.source.kind == "wall" {
                let Some(walls) = contacts.walls_by_floor.get(host) else {
                    continue;
                };
                budget
                    .spend(walls.len())
                    .map_err(|e| node.source.fail("AML_COMPLEXITY", e))?;
                let Some(wall) = walls.iter().find(|w| w.host == node.id) else {
                    continue;
                };
                let path = &self.resolution.paths[node.source.text("path").unwrap()];
                account_points(
                    path.len() + wall.seams.len() * 2,
                    points,
                    budget,
                    &node.source,
                )?;
                let path = path.iter().map(|p| node.matrix.point(*p)).collect();
                neighbors.push(Neighbor::Wall {
                    path,
                    thickness: node.source.number("thickness", 0.22)
                        * dot(node.matrix.0[0], node.matrix.0[0])
                            .sqrt()
                            .max(dot(node.matrix.0[1], node.matrix.0[1]).sqrt()),
                    contacts: wall.seams.clone(),
                });
            } else if ["slab", "landing"].contains(&node.source.kind.as_str()) {
                account_points(
                    self.floors[&other].loops.iter().map(Vec::len).sum(),
                    points,
                    budget,
                    &node.source,
                )?;
                neighbors.push(Neighbor::Floor {
                    loops: self.floors[&other].loops.clone(),
                });
            }
            if neighbors.len() > 20_000 {
                return Err(node
                    .source
                    .fail("AML_COMPLEXITY", "Ground neighbor allowance exceeded."));
            }
        }
        Ok(neighbors)
    }

    pub(super) fn render_ground(
        &self,
        visuals: &mut VisualOutput<'_>,
        prepared: Vec<Prepared>,
        theme: &mut ThemeState,
        execution: &Execution<'_>,
    ) -> Result<(), Diagnostic> {
        if prepared.is_empty() {
            return Ok(());
        }
        let settings = self.dressing_settings().unwrap().clone();
        let fail = |e: String| settings.fail("AML_DRESSING", e);
        let mut library = Library::new(
            &self.materials()?,
            settings.text("finishes"),
            settings.text("time-of-day") == Some("night"),
        )
        .map_err(fail)?;
        let mut budget = Budget::new(128 * 1024 * 1024, &|| false).map_err(fail)?;
        for prepared in prepared {
            let plan = &theme.floor_dressing[&prepared.index];
            let rule = &plan.source;
            let fail = |e: String| rule.fail("AML_DRESSING", e);
            let host = &self.resolution.nodes[prepared.index].id;
            let assets = assets(&mut library, rule)?;
            let mut drawing = Drawing::new(Mode::Render, 2_000_000, &|| false).map_err(fail)?;
            let regions = pavement::render::draw_preserved(
                &mut drawing,
                &Site {
                    host,
                    part: "ground",
                    seed: &format!("{}/{host}/ground", settings.text("seed").unwrap()),
                    layout: &prepared.layout,
                    faces: &prepared.faces,
                    seams: &prepared.seams,
                    walls: &prepared.walls,
                    settings: plan.ground.as_ref().unwrap(),
                    grime: plan.finish.grime,
                    wear: plan.finish.wear,
                    sampling: 0.18,
                    tile_size: rule.number("tile-size", 0.4),
                    tile_aspect: rule.optional_number("tile-aspect"),
                    tile_wear: Some(rule.number("tile-wear", 0.9)),
                    tile_grime: Some(rule.number("tile-grime", plan.finish.grime)),
                    density: rule.optional_number("texture-density"),
                    asphalt_density: Some(160.),
                    fine_marks: true,
                    flush: false,
                },
                &assets,
                &mut budget,
            )
            .map_err(fail)?;
            if !theme.building_hosts.is_empty() {
                if theme.render_edge_vertices.len() + drawing.indices.len() > 6_000_000 {
                    return Err(fail(
                        "Ground receiver edges exceed scene vertex allowance.".into(),
                    ));
                }
                theme.render_edge_vertices.extend(
                    drawing
                        .indices
                        .iter()
                        .map(|i| drawing.vertices[*i as usize].position),
                );
            }
            let bindings = library
                .materials
                .keys()
                .map(|id| (id.clone(), id.clone()))
                .collect();
            self.bake_visual(
                execution,
                visuals,
                &mut drawing,
                host,
                rule,
                &library.materials,
                &library.materials,
                bindings,
                None,
            )?;
            visuals.ground_plans().push(pavement::render::Plan {
                host: host.clone(),
                anchors: prepared
                    .layout
                    .anchors
                    .into_iter()
                    .map(|a| layout::Anchor {
                        point: prepared.layout.world.point(a.point),
                        kind: a.kind,
                    })
                    .collect(),
                regions,
            });
        }
        Ok(())
    }
}

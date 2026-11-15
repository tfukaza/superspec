//! Connects admitted AML hosts to detached procedural jobs without changing canonical collision.
use super::*;
use crate::worldspec::themes::kowloon::{
    building::{
        self,
        assembly::{Job, connect as connect_spans},
        drawings,
        materials::Appearance,
    },
    clearance::{Collision, CollisionStorage, RenderTriangle},
    kit_materials::Library,
    massing::{ROOF_RESERVE, default_stories},
    signs::book::Book,
    spans::walking::{Surface as WalkingSurface, Walking},
};
use superspec_geometry::obstructions::{Budget, Obstructions};
mod base_walls;

pub(super) struct Recipe {
    pub(super) node: usize,
    rule: Node,
    request: building::Request,
}
pub(super) struct Prepared {
    plan: building::Plan,
    rules: Vec<(usize, Node)>,
    walking: Walking,
    budget: Budget<'static>,
}

impl Compiler<'_> {
    pub(super) fn dressing_settings(&self) -> Option<&Node> {
        self.resolution
            .document
            .metadata
            .iter()
            .find(|n| n.kind == "dressing")
    }
    pub(super) fn building_recipes(
        &self,
        materials: &[Material],
    ) -> Result<Vec<Recipe>, Diagnostic> {
        let Some(settings) = self.dressing_settings() else {
            return Ok(vec![]);
        };
        let mut used = BTreeSet::new();
        for rule in &settings.children {
            if !used.insert(rule.string("host", "")) {
                return Err(rule.fail("AML_DRESSING", "Use one dressing rule per host."));
            }
        }
        let substrates = settings
            .text("wall-materials")
            .map(|s| s.split_whitespace().collect::<Vec<_>>());
        if let Some(ids) = substrates.as_ref()
            && (ids.is_empty()
                || ids.len() > 8
                || ids.iter().copied().collect::<BTreeSet<_>>().len() != ids.len()
                || ids.iter().any(|id| !materials.iter().any(|m| m.id == *id)))
        {
            return Err(settings.fail(
                "AML_DRESSING",
                "wall-materials requires one to eight distinct declared materials.",
            ));
        }
        if let Some(prefix) = settings.text("finishes") {
            let roles = [
                "wall-posters",
                "wall-paper",
                "wall-edge",
                "wall-rust",
                "wall-metal",
                "wall-cable",
                "wall-ceramic",
                "wall-tile-plaster",
                "concrete",
                "lettering",
                "signboard",
                "rust",
                "metal",
                "brushed-metal",
                "kitchen-metal",
                "vent-metal",
                "vent-edge",
                "shop-register",
                "market-trays",
                "wall-tile-grime",
                "sheet",
                "timber",
                "shop-timber",
                "restaurant-surfaces",
                "restaurant-tiles",
                "restaurant-food",
                "roofing",
                "balcony-plants",
                "balcony-cloth",
                "cloth",
                "tarp",
                "aircon",
                "market-apple",
                "snack-packet",
                "grocer-props",
                "render-weathered",
                "render-repair",
            ];
            if prefix.is_empty()
                || prefix.len() > 512
                || !roles
                    .iter()
                    .any(|role| materials.iter().any(|m| m.id == format!("{prefix}-{role}")))
            {
                return Err(settings.fail(
                    "AML_DRESSING",
                    "No recognized kit finish is declared under this prefix.",
                ));
            }
            if materials
                .iter()
                .any(|m| m.id == format!("{prefix}-lettering") && m.alpha_cutoff.is_none())
            {
                return Err(settings.fail(
                    "AML_DRESSING",
                    "The lettering distance-field atlas requires alpha-cutoff.",
                ));
            }
        }
        let mut recipes = vec![];
        for rule in settings.children.iter().filter(|r| r.kind == "building") {
            let host = rule.text("host").unwrap();
            let fail = |message: &str| {
                let mut d = rule.fail("AML_DRESSING", message);
                d.objects = vec![host.into()];
                d
            };
            if settings.text("seed").unwrap().len() > 1024 {
                return Err(settings.fail(
                    "AML_COMPLEXITY",
                    "Building seeds are limited to 1024 UTF-8 bytes.",
                ));
            }
            if host.len() > 512 {
                return Err(fail(
                    "Building host identities are limited to 512 UTF-8 bytes.",
                ));
            }
            let index = *self
                .resolution
                .ids
                .get(host)
                .ok_or_else(|| fail("Unknown dressing host."))?;
            let node = &self.resolution.nodes[index];
            if !["box", "extrusion"].contains(&node.source.kind.as_str())
                || node.source.boolean("collidable", true)
                || node.source.boolean("dynamic", false)
                || node.parent.is_some()
            {
                return Err(fail(
                    "A building requires a standalone, static, non-collidable box or convex extrusion.",
                ));
            }
            if (0..3).any(|i| (length(node.matrix.0[i]) - 1.).abs() > 1e-6)
                || (node.matrix.0[2][2] - 1.).abs() > 1e-6
            {
                return Err(fail(
                    "Buildings require level, unscaled placement; yaw and horizontal reflection are supported.",
                ));
            }
            if !materials
                .iter()
                .any(|m| Some(m.id.as_str()) == rule.text("material"))
            {
                return Err(fail("Unknown dressing material."));
            }
            let (shape, width, depth, height) = if node.source.kind == "box" {
                let width = node.source.number("width", 1.);
                let depth = node.source.number("depth", 1.);
                let height = node.source.number("height", 1.);
                (
                    building::Shape::Box {
                        width,
                        depth,
                        height,
                        bottom_center: node.source.text("anchor") == Some("bottom-center"),
                    },
                    width,
                    depth,
                    height,
                )
            } else {
                if rule.number("shops", 0.) > 0. || rule.text("base-wall").is_some() {
                    return Err(fail(
                        "Polygon buildings require closed ground frontages and cannot own base walls.",
                    ));
                }
                let profile = self.profile(&node.source)?;
                if profile.loops.len() != 1 {
                    return Err(fail(
                        "Building profiles require one convex loop without holes.",
                    ));
                }
                let elevation = self.resolution.elevation(&node.source)?;
                let points = profile.loops[0]
                    .iter()
                    .map(|p| {
                        [
                            p[0],
                            p[1],
                            if node.source.boolean("flat", false) {
                                elevation
                            } else {
                                p[2] + elevation
                            },
                        ]
                    })
                    .collect::<Vec<_>>();
                let height = node.source.number("distance", 0.);
                let footprint =
                    building::footprint::Footprint::new(&points, height).map_err(|e| fail(&e))?;
                let extent = |axis: usize| {
                    points
                        .iter()
                        .map(|p| p[axis])
                        .fold(f64::NEG_INFINITY, f64::max)
                        - points.iter().map(|p| p[axis]).fold(f64::INFINITY, f64::min)
                };
                (
                    building::Shape::Polygon(footprint),
                    extent(0),
                    extent(1),
                    height,
                )
            };
            let stories = rule.number("stories", default_stories(height) as f64) as usize;
            if rule.text("model") == Some("reference-corner")
                && (!matches!(shape, building::Shape::Box { .. })
                    || (width - 9.8).abs() > 1e-6
                    || (depth - 7.).abs() > 1e-6
                    || (height - 13.1).abs() > 1e-6
                    || ["stories", "shops", "base-wall"]
                        .iter()
                        .any(|key| rule.attributes.contains_key(*key)))
            {
                return Err(fail(
                    "The reference-corner model requires a 9.8 × 7 × 13.1 m box and supplies its own stories, shops and walls.",
                ));
            }
            if !(3. ..=40.).contains(&width)
                || !(3. ..=40.).contains(&depth)
                || height > 40.
                || stories == 0
                || (height - ROOF_RESERVE) / (stories as f64) < 2.8
            {
                return Err(fail(
                    "Building dimensions require 3–40 m width/depth, height at most 40 m and 2.8 m stories plus the roof reserve.",
                ));
            }
            let mut source = rule.clone();
            source.id = host.into();
            recipes.push(Recipe {
                node: index,
                rule: source,
                request: building::Request {
                    host: host.into(),
                    matrix: node.matrix,
                    shape,
                    stories: Some(stories),
                    shops: rule.optional_number("shops"),
                    base_wall: rule.text("base-wall").map(str::to_owned),
                },
            });
        }
        if recipes.len() > 512 {
            return Err(settings.fail(
                "AML_COMPLEXITY",
                "Dressing supports at most 512 building hosts.",
            ));
        }
        if !recipes.is_empty() {
            let mut library = Library::new(
                materials,
                settings.text("finishes"),
                settings.text("time-of-day") == Some("night"),
            )
            .map_err(|e| settings.fail("AML_DRESSING", e))?;
            let substrates = substrates
                .unwrap_or_default()
                .into_iter()
                .map(str::to_owned)
                .collect::<Vec<_>>();
            for recipe in &recipes {
                Appearance::new(
                    &mut library,
                    recipe.rule.text("material").unwrap(),
                    &substrates,
                )
                .map_err(|e| recipe.rule.fail("AML_UNSUPPORTED", e))?;
            }
        }
        Ok(recipes)
    }

    /// Copies final CSG surfaces before dressing, keeping closed solids separate from open patches.
    fn building_collision(&self, output: &Product) -> Result<Vec<Collision>, String> {
        let mut triangles = vec![];
        let mut storage = CollisionStorage::default();
        for (solid, shape) in output.solids.iter().enumerate() {
            let solid = format!("physical/{solid}");
            for (owner, geometry) in owned_geometry(std::slice::from_ref(shape), &[]) {
                let surface = &self.surfaces[&owner];
                if !surface.collidable {
                    continue;
                }
                let object = &self.resolution.nodes[surface.node].id;
                storage.admit(geometry.triangles.len(), object, Some(&solid))?;
                triangles.extend(geometry.triangles.iter().map(|t| Collision {
                    points: t.map(|i| geometry.points[i]),
                    object: object.clone(),
                    solid: Some(solid.clone()),
                }));
            }
        }
        for (geometry, owner) in &output.patches {
            let surface = &self.surfaces[owner];
            if surface.collidable {
                let object = &self.resolution.nodes[surface.node].id;
                storage.admit(geometry.triangles.len(), object, None)?;
                triangles.extend(geometry.triangles.iter().map(|t| Collision {
                    points: t.map(|i| geometry.points[i]),
                    object: object.clone(),
                    solid: None,
                }));
            }
        }
        Ok(triangles)
    }

    pub(super) fn plan_buildings(
        &mut self,
        scene: &mut Scene,
        output: &Product,
        recipes: Vec<Recipe>,
        authored: &BTreeMap<u32, Geometry>,
        theme: &mut ThemeState,
    ) -> Result<Option<Prepared>, Diagnostic> {
        let replaced = self.base_walls(&recipes, authored)?;
        theme.building_hosts.extend(replaced);
        let recipes = recipes
            .into_iter()
            .filter(|r| {
                self.resolution.nodes[r.node]
                    .source
                    .boolean("visible", true)
            })
            .collect::<Vec<_>>();
        if recipes.is_empty() {
            return Ok(None);
        }
        let settings = self.dressing_settings().unwrap();
        let fail = |e: String| settings.fail("AML_DRESSING", e);
        let mut budget = Budget::new(128_000_000, &|| false).map_err(fail)?;
        let physical = self.building_collision(output).map_err(fail)?;
        let ground = authored
            .iter()
            .flat_map(|(owner, g)| {
                let visible = self.surfaces[owner].visible;
                g.triangles.iter().map(move |t| RenderTriangle {
                    points: t.map(|i| g.points[i]),
                    visible,
                })
            })
            .collect::<Vec<_>>();
        let walking = Walking::new(
            &physical
                .iter()
                .map(|t| WalkingSurface {
                    points: t.points,
                    collidable: true,
                    floor_or_stairs: self.resolution.ids.get(&t.object).is_some_and(|i| {
                        ["slab", "landing", "stairs", "ramp", "terrain"]
                            .contains(&self.resolution.nodes[*i].source.kind.as_str())
                    }),
                })
                .collect::<Vec<_>>(),
            &mut budget,
        )
        .map_err(fail)?;
        let rules: Vec<_> = recipes.iter().map(|r| (r.node, r.rule.clone())).collect();
        let plan = building::Plan::new(
            recipes.into_iter().map(|r| r.request).collect(),
            settings.text("seed").unwrap(),
            physical,
            &ground,
            &mut budget,
        )
        .map_err(fail)?;
        for warning in &plan.warnings {
            let rule = &rules
                .iter()
                .find(|(_, r)| r.text("host") == Some(warning.host.as_str()))
                .unwrap()
                .1;
            let mut diagnostic = rule.fail(
                warning.code,
                "Shops require at least 4.8 m width and 5 m depth; this building receives none.",
            );
            diagnostic.severity = "warning";
            diagnostic.objects = vec![warning.host.clone()];
            diagnostic.spatial = Some(Box::new(superspec_aml::diagnostic::SpatialEvidence {
                bounds: [
                    std::array::from_fn(|a| {
                        warning
                            .corners
                            .iter()
                            .map(|p| p[a])
                            .fold(f64::INFINITY, f64::min)
                    }),
                    std::array::from_fn(|a| {
                        warning
                            .corners
                            .iter()
                            .map(|p| p[a])
                            .fold(f64::NEG_INFINITY, f64::max)
                    }),
                ],
                inspection_target: warning.host.clone(),
                evidence: "The requested host is smaller than the supported stocked-shop frontage."
                    .into(),
            }));
            scene.manifest.diagnostics.push(diagnostic);
        }
        Ok(Some(Prepared {
            plan,
            rules,
            walking,
            budget,
        }))
    }
    pub(super) fn render_buildings(
        &mut self,
        scene: &mut Scene,
        prepared: Option<Prepared>,
        theme: &mut ThemeState,
        execution: &Execution<'_>,
    ) -> Result<(), Diagnostic> {
        let Some(Prepared {
            plan,
            rules,
            walking,
            mut budget,
        }) = prepared
        else {
            return Ok(());
        };
        let mut edge_vertices = std::mem::take(&mut theme.render_edge_vertices);
        let mut obstacles = std::mem::take(&mut theme.sign_survey);
        let mut jobs = vec![];
        let mut book = Book::default();
        let settings = self.dressing_settings().unwrap();
        let fail = |e: String| settings.fail("AML_DRESSING", e);
        let mut library = Library::new(
            &self.materials()?,
            settings.text("finishes"),
            settings.text("time-of-day") == Some("night"),
        )
        .map_err(fail)?;
        let substrates = settings
            .text("wall-materials")
            .unwrap_or("")
            .split_whitespace()
            .map(str::to_owned)
            .collect::<Vec<_>>();
        let appearances = rules
            .iter()
            .map(|(_, rule)| {
                Appearance::new(&mut library, rule.text("material").unwrap(), &substrates)
                    .map_err(|e| rule.fail("AML_DRESSING", e))
            })
            .collect::<Result<Vec<_>, _>>()?;
        (execution.observe)("building-drawings");
        let drawings = drawings::run(
            &plan,
            &rules,
            &appearances,
            settings,
            budget.remaining(),
            self.cancelled,
            execution.workers,
        )
        .map_err(fail)?;
        for (i, (appearance, mut drawn)) in appearances.into_iter().zip(drawings).enumerate() {
            budget.spend(drawn.work).map_err(fail)?;
            if obstacles.len() + drawn.survey.len() / 3 > 1_048_576 {
                return Err(settings.fail(
                    "AML_COMPLEXITY",
                    "Building survey exceeds scene triangle allowance.",
                ));
            }
            obstacles.extend(drawn.survey.as_chunks::<3>().0.iter().copied());
            book.append(&mut drawn.signs).map_err(fail)?;
            jobs.push(Job {
                index: i,
                appearance,
                drawing: drawn.drawing.bind(&|| false),
            });
        }
        (execution.observe)("building-signs");
        let obstacles = Obstructions::new(&obstacles, &mut budget).map_err(fail)?;
        let sign_plans = book
            .settle(&obstacles, &mut budget, |host, p, s| {
                let job = jobs
                    .iter_mut()
                    .find(|j| plan.sites()[j.index].host == host)
                    .ok_or("Unknown sign drawing host.")?;
                let before = job.drawing.triangle_count();
                p.draw(&mut job.drawing, &|slot| job.appearance.repeat(slot), s)?;
                Ok(job.drawing.triangle_count() - before)
            })
            .map_err(fail)?;
        scene.manifest.sign_plans.extend(sign_plans);
        (execution.observe)("building-spans");
        let contributors =
            if rules.len() >= 2 || settings.children.iter().any(|rule| rule.kind == "spans") {
                connect_spans(
                    &mut jobs,
                    &plan,
                    settings,
                    &walking,
                    &edge_vertices,
                    &mut budget,
                )
                .map_err(fail)?
            } else {
                BTreeMap::new()
            };
        if jobs
            .iter()
            .map(|j| j.drawing.triangle_count())
            .sum::<usize>()
            > 2_000_000
        {
            return Err(settings.fail(
                "AML_COMPLEXITY",
                "Building drawings exceed scene triangle allowance.",
            ));
        }
        (execution.observe)("building-edges");
        for job in &jobs {
            if edge_vertices.len() + job.drawing.vertices.len() > 6_000_000 {
                return Err(settings.fail(
                    "AML_COMPLEXITY",
                    "Building receiver edges exceed scene vertex allowance.",
                ));
            }
            edge_vertices.extend(job.drawing.vertices.iter().map(|v| v.position));
        }
        let edge_index = std::sync::Arc::new(
            crate::worldspec::themes::kowloon::conform::Edges::new(edge_vertices, &mut budget)
                .map_err(fail)?,
        );
        for Job {
            index: i,
            appearance,
            mut drawing,
        } in jobs
        {
            let rule = &rules[i].1;
            self.bake_visual(
                execution,
                scene,
                &mut drawing,
                &plan.sites()[i].host,
                rule,
                &appearance.inputs,
                &library.materials,
                appearance.bindings,
                Some(edge_index.clone()),
            )?;
        }
        (execution.observe)("building-credit");
        if let Some(context) = scene.manifest.architecture.as_mut() {
            context.credit_visuals(&contributors).map_err(fail)?;
        }
        Ok(())
    }
}

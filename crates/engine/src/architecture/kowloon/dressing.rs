use super::*;
use crate::worldspec::themes::kowloon::{floor::Finish, mesh::Vertex};
use superspec_aml::rng::SeededRng;

use crate::worldspec::themes::kowloon::assembly::FloorPlan;

impl Compiler<'_> {
    pub(super) fn dressing_settings(&self) -> Option<&Node> {
        self.resolution
            .document
            .metadata
            .iter()
            .find(|node| node.kind == "dressing")
    }

    pub(super) fn check_dressing_settings(&self, materials: &[Material]) -> Result<(), Diagnostic> {
        let Some(settings) = self.dressing_settings() else {
            return Ok(());
        };
        let mut hosts = BTreeSet::new();
        for rule in &settings.children {
            if !hosts.insert(rule.text("host").unwrap()) {
                return Err(rule.fail("AML_DRESSING", "Use one dressing rule per host."));
            }
        }
        if let Some(palette) = settings.text("wall-materials") {
            let ids: Vec<_> = palette.split_whitespace().collect();
            if !(1..=8).contains(&ids.len())
                || ids.iter().copied().collect::<BTreeSet<_>>().len() != ids.len()
                || ids.iter().any(|id| !materials.iter().any(|m| m.id == *id))
            {
                return Err(settings.fail(
                    "AML_DRESSING",
                    "wall-materials requires one to eight distinct declared materials.",
                ));
            }
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
                "kitchen-worktop",
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
            let matches = |role: &str| {
                let id = format!("{prefix}-{role}");
                materials.iter().find(|m| m.id == id)
            };
            if prefix.is_empty()
                || prefix.len() > 512
                || !roles.iter().any(|role| matches(role).is_some())
            {
                return Err(settings.fail(
                    "AML_DRESSING",
                    "No recognized kit finish is declared under this prefix.",
                ));
            }
            if matches("lettering").is_some_and(|material| material.alpha_cutoff.is_none()) {
                return Err(settings.fail(
                    "AML_DRESSING",
                    "The lettering distance-field atlas requires alpha-cutoff.",
                ));
            }
        }
        Ok(())
    }

    pub(super) fn plan_dressing(
        &self,
        materials: &[Material],
    ) -> Result<BTreeMap<usize, FloorPlan>, Diagnostic> {
        let Some(settings) = self.dressing_settings() else {
            return Ok(BTreeMap::new());
        };
        let mut plans = BTreeMap::new();
        for rule in &settings.children {
            if rule.kind != "floor" {
                continue;
            }
            let fail = |message| {
                let mut diagnostic = rule.fail("AML_DRESSING", message);
                diagnostic.objects = vec![rule.string("host", "")];
                diagnostic
            };
            let host = rule.text("host").unwrap();
            let index = *self
                .resolution
                .ids
                .get(host)
                .ok_or_else(|| fail("Unknown dressing host."))?;
            if plans.contains_key(&index) {
                return Err(fail("Use one dressing rule per host."));
            }
            let node = &self.resolution.nodes[index];
            if !["slab", "landing", "stairs", "ramp"].contains(&node.source.kind.as_str()) {
                return Err(fail("Floor dressing requires a walking surface."));
            }
            let material = materials
                .iter()
                .find(|m| m.id == rule.text("material").unwrap())
                .ok_or_else(|| fail("Unknown dressing material."))?;
            let mut source = rule.clone();
            source.id = host.to_owned();
            let ground = ground::settings(rule, &node.source.kind)?;
            if ground.is_some() {
                ground::validate_materials(materials, settings, rule)?;
            }
            plans.insert(
                index,
                FloorPlan {
                    source,
                    ground,
                    material: material.id.clone(),
                    finish: Finish {
                        grime: rule.number("grime", settings.number("grime", 0.4)),
                        wear: rule.number("wear", settings.number("wear", 0.2)),
                        phase: SeededRng::from_text(&format!(
                            "{}/{host}",
                            settings.text("seed").unwrap()
                        ))
                        .float(0., 100.),
                        repeat: material.repeat,
                    },
                },
            );
        }
        Ok(plans)
    }

    pub(super) fn dress_floors(
        &mut self,
        scene: &mut Scene,
        geometry: &mut BTreeMap<u32, Geometry>,
        theme: &ThemeState,
    ) -> Result<(), Diagnostic> {
        if theme.floor_dressing.is_empty() {
            return Ok(());
        }
        let mut output = BTreeMap::<usize, Vec<Vertex>>::new();
        let mut work = 0;
        for (owner, geometry) in geometry.iter_mut() {
            let surface = &self.surfaces[owner];
            let Some(plan) = theme.floor_dressing.get(&surface.node) else {
                continue;
            };
            if !surface.visible || surface.assigned {
                continue;
            }
            let matrix = self.resolution.nodes[surface.node].matrix;
            let mut retained = Vec::new();
            for t in &geometry.triangles {
                work += 1;
                if work > 2_000_000 {
                    return Err(plan
                        .source
                        .fail("AML_COMPLEXITY", "Floor dressing work budget exceeded."));
                }
                let points = t.map(|i| geometry.points[i]);
                let Ok(normal) = unit(cross(sub(points[1], points[0]), sub(points[2], points[0])))
                else {
                    retained.push(*t);
                    continue;
                };
                if !ground::receives(matrix, normal) {
                    retained.push(*t);
                    continue;
                }
                if plan.ground.is_some() {
                    continue;
                }
                plan.finish
                    .triangle(
                        points,
                        normal,
                        &mut work,
                        output.entry(surface.node).or_default(),
                    )
                    .at(&plan.source)?;
            }
            geometry.triangles = retained;
        }
        for (index, vertices) in output {
            let plan = &theme.floor_dressing[&index];
            let node = &self.resolution.nodes[index];
            if scene.positions.len() + vertices.len() * 3 > 18_000_000
                || scene.indices.len() + vertices.len() > 6_000_000
            {
                return Err(plan
                    .source
                    .fail("AML_COMPLEXITY", "Scene geometry budget exceeded."));
            }
            let vertex_start = scene.positions.len() / 3;
            let index_start = scene.indices.len();
            let mut bounds = [[f64::INFINITY; 3], [f64::NEG_INFINITY; 3]];
            for (i, vertex) in vertices.iter().enumerate() {
                scene.positions.extend(vertex.position.map(|v| v as f32));
                scene.normals.extend(vertex.normal.map(|v| v as f32));
                scene.uvs.extend(vertex.uv.map(|v| v as f32));
                scene.colors.extend(vertex.color.map(|v| v as f32));
                scene.indices.push(i as u32);
                for axis in 0..3 {
                    let value = f64::from(vertex.position[axis] as f32);
                    bounds[0][axis] = bounds[0][axis].min(value);
                    bounds[1][axis] = bounds[1][axis].max(value);
                }
            }
            scene.manifest.meshes.push(Mesh {
                id: format!("{}/$finish/floor", node.id),
                object_id: node.id.clone(),
                source: plan.source.source.clone(),
                material: Some(plan.material.clone()),
                visible: true,
                vertex_start,
                vertex_count: vertices.len(),
                index_start,
                index_count: vertices.len(),
                bounds,
            });
        }
        Ok(())
    }
}

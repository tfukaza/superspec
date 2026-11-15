//! Explicit Kowloon bridge to private physical receiving surfaces and provenance.
use super::build::{PhysicalBuild, PhysicalOutput, RenderSurfaces};
use super::*;
use crate::worldspec::{
    build::ThemeAdapter,
    themes::kowloon::{
        assembly::{Execution, ThemeState},
        cache::Cache,
    },
};
use std::sync::Mutex;
mod buildings;
mod dressing;
mod ground;
mod visual;
mod wall_dressing;

pub(crate) struct Kowloon<'a> {
    execution: Execution<'a>,
    state: ThemeState,
    buildings: Vec<buildings::Recipe>,
    walls: Vec<wall_dressing::Recipe>,
    building_plan: Option<buildings::Prepared>,
    ground_plan: Vec<ground::Prepared>,
    wall_plan: Vec<wall_dressing::Job>,
    source: Option<Node>,
}
impl<'a> Kowloon<'a> {
    pub(crate) fn new(
        cache: Option<&'a Mutex<Cache>>,
        workers: usize,
        observe: &'a dyn Fn(&str),
    ) -> Self {
        Self {
            execution: Execution {
                cache,
                workers,
                observe,
            },
            state: ThemeState::default(),
            buildings: vec![],
            walls: vec![],
            building_plan: None,
            ground_plan: vec![],
            wall_plan: vec![],
            source: None,
        }
    }
}
impl ThemeAdapter for Kowloon<'_> {
    fn prepare(
        &mut self,
        physical: &PhysicalBuild<'_>,
        materials: &[Material],
    ) -> Result<(), Diagnostic> {
        let compiler = &physical.compiler;
        let floor_dressing = compiler.plan_dressing(materials)?;
        let buildings = compiler.building_recipes(materials)?;
        let walls = compiler.wall_recipes(materials)?;
        let building_hosts = buildings.iter().map(|recipe| recipe.node).collect();
        self.source = compiler.dressing_settings().cloned();
        self.state = ThemeState {
            floor_dressing,
            building_hosts,
            ..Default::default()
        };
        self.buildings = buildings;
        self.walls = walls;
        self.building_plan = None;
        self.ground_plan.clear();
        self.wall_plan.clear();
        Ok(())
    }
    fn needs_surfaces(&self) -> bool {
        !self.buildings.is_empty()
            || self
                .state
                .floor_dressing
                .values()
                .any(|plan| plan.ground.is_some())
    }
    fn plan(
        &mut self,
        physical: &mut PhysicalBuild<'_>,
        scene: &mut Scene,
        output: &PhysicalOutput,
        render: &RenderSurfaces,
    ) -> Result<(), Diagnostic> {
        (self.execution.observe)("building-plan");
        self.building_plan = physical.compiler.plan_buildings(
            scene,
            &output.product,
            std::mem::take(&mut self.buildings),
            &render.geometry,
            &mut self.state,
        )?;
        (self.execution.observe)("ground-plan");
        self.ground_plan =
            physical
                .compiler
                .plan_ground(&output.product, &render.geometry, &self.state)?;
        Ok(())
    }
    fn render_structure(
        &mut self,
        physical: &mut PhysicalBuild<'_>,
        scene: &mut Scene,
        render: RenderSurfaces,
    ) -> Result<(), Diagnostic> {
        self.wall_plan = physical.compiler.emit_kowloon_render(
            scene,
            render.geometry,
            std::mem::take(&mut self.walls),
            &mut self.state,
        )?;
        Ok(())
    }
    fn publish(
        &mut self,
        physical: &mut PhysicalBuild<'_>,
        scene: &mut Scene,
    ) -> Result<(), Diagnostic> {
        (self.execution.observe)("ground-render");
        physical.compiler.render_ground(
            scene,
            std::mem::take(&mut self.ground_plan),
            &mut self.state,
            &self.execution,
        )?;
        (self.execution.observe)("wall-render");
        physical.compiler.render_walls(
            scene,
            std::mem::take(&mut self.wall_plan),
            &mut self.state,
            &self.execution,
        )?;
        (self.execution.observe)("building-render");
        physical.compiler.render_buildings(
            scene,
            self.building_plan.take(),
            &mut self.state,
            &self.execution,
        )?;
        Ok(())
    }
    fn fallback_source(&self) -> Option<&Node> {
        self.source.as_ref()
    }
}

impl Compiler<'_> {
    fn emit_kowloon_render(
        &mut self,
        scene: &mut Scene,
        mut by_owner: BTreeMap<u32, Geometry>,
        wall_recipes: Vec<wall_dressing::Recipe>,
        theme: &mut ThemeState,
    ) -> Result<Vec<wall_dressing::Job>, Diagnostic> {
        if !theme.building_hosts.is_empty() {
            for (owner, geometry) in &by_owner {
                let surface = &self.surfaces[owner];
                if !surface.visible || theme.building_hosts.contains(&surface.node) {
                    continue;
                }
                if theme.sign_survey.len() + geometry.triangles.len() > 1_048_576 {
                    return Err(self.resolution.nodes[surface.node].source.fail(
                        "AML_COMPLEXITY",
                        "Authored sign survey exceeds scene triangle allowance.",
                    ));
                }
                theme.sign_survey.extend(
                    geometry
                        .triangles
                        .iter()
                        .map(|t| t.map(|i| geometry.points[i])),
                );
            }
        }
        self.dress_floors(scene, &mut by_owner, theme)?;
        let walls = self.dress_walls(&mut by_owner, wall_recipes)?;
        by_owner.retain(|owner, _| !theme.building_hosts.contains(&self.surfaces[owner].node));
        self.append_owned(
            scene,
            by_owner,
            false,
            (!theme.building_hosts.is_empty()).then_some(&mut theme.render_edge_vertices),
        )?;
        Ok(walls)
    }
}

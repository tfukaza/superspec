//! Explicit Kowloon bridge to private physical receiving surfaces and provenance.
use super::build::{PhysicalBuild, PhysicalOutput, RenderSurfaces};
use super::*;
use crate::worldspec::{
    build::{ItemOutput, ThemeAdapter, VisualOutput},
    themes::kowloon::{
        assembly::{Execution, ThemeState},
        cache::Cache,
    },
};
use std::sync::Mutex;
mod dressing;
mod ground;
mod visual;
mod wall_dressing;
mod windows;

pub(crate) struct Kowloon<'a> {
    execution: Execution<'a>,
    state: ThemeState,
    walls: Vec<wall_dressing::Recipe>,
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
            walls: vec![],
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
        compiler.check_dressing_settings(materials)?;
        let walls = compiler.wall_recipes(materials)?;
        self.source = compiler.dressing_settings().cloned();
        self.state = ThemeState { floor_dressing };
        self.walls = walls;
        self.ground_plan.clear();
        self.wall_plan.clear();
        Ok(())
    }
    fn needs_surfaces(&self) -> bool {
        self.state
            .floor_dressing
            .values()
            .any(|plan| plan.ground.is_some())
    }
    fn plan(
        &mut self,
        physical: &PhysicalBuild<'_>,
        _diagnostics: &mut Vec<Diagnostic>,
        output: &PhysicalOutput,
        render: &RenderSurfaces,
    ) -> Result<(), Diagnostic> {
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
            &self.state,
        )?;
        Ok(())
    }
    fn publish(
        &mut self,
        physical: &PhysicalBuild<'_>,
        visuals: &mut VisualOutput<'_>,
    ) -> Result<(), Diagnostic> {
        (self.execution.observe)("ground-render");
        physical.compiler.render_ground(
            visuals,
            std::mem::take(&mut self.ground_plan),
            &self.state,
            &self.execution,
        )?;
        (self.execution.observe)("wall-render");
        physical.compiler.render_walls(
            visuals,
            std::mem::take(&mut self.wall_plan),
            &self.execution,
        )
    }
    fn place_items<'build>(
        &self,
        physical: &'build PhysicalBuild<'_>,
        items: ItemOutput<'build>,
    ) -> Result<ItemOutput<'build>, Diagnostic> {
        use crate::worldspec::themes::kowloon::city as catalog;
        let items = physical.compiler.place_windows(items)?;
        let placements = physical
            .compiler
            .resolution
            .document
            .metadata
            .iter()
            .filter(|node| node.kind == "city")
            .flat_map(|city| &city.children);
        let mut items = items.place_city(
            || {
                self.execution
                    .cache
                    .map(|cache| cache.lock().expect("Kowloon cache").city_items())
                    .unwrap_or_default()
            },
            placements
                .clone()
                .filter(|n| !matches!(n.kind.as_str(), "tree" | "shrub")),
            catalog::request,
            |node, path| {
                if superspec_catalog::image_name(path).is_some() {
                    physical.compiler.asset(path)
                } else if node.kind == "grass-field" {
                    physical.compiler.assets.read(path)
                } else {
                    catalog::asset(path)
                }
            },
            physical.compiler.cancelled,
        )?;
        catalog::finish(
            &mut items,
            placements,
            physical.compiler.cancelled,
            self.execution.cache,
        )?;
        Ok(items)
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
        theme: &ThemeState,
    ) -> Result<Vec<wall_dressing::Job>, Diagnostic> {
        self.dress_floors(scene, &mut by_owner, theme)?;
        let walls = self.dress_walls(&mut by_owner, wall_recipes)?;
        self.append_owned(scene, by_owner, false)?;
        Ok(walls)
    }
}

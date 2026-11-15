//! Completed procedural artifacts are published through the shared scene contract.
use super::*;
use crate::worldspec::themes::kowloon::{
    mesh::Mesh as Drawing, visual_baking::VisualBaking, weathered,
};

impl Compiler<'_> {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn bake_visual(
        &self,
        execution: &Execution<'_>,
        scene: &mut Scene,
        drawing: &mut Drawing<'_>,
        host: &str,
        rule: &Node,
        inputs: &BTreeMap<String, Material>,
        delivery: &BTreeMap<String, Material>,
        mut bindings: BTreeMap<String, String>,
        edges: Option<std::sync::Arc<crate::worldspec::themes::kowloon::conform::Edges>>,
    ) -> Result<(), Diagnostic> {
        (execution.observe)("surface-planning");
        let fail = |e: String| rule.fail("AML_DRESSING", e);
        let baking = weathered::finish(drawing, self.cancelled).map_err(fail)?;
        let baking = if let Some(edges) = edges {
            baking.with_edge_index(edges)
        } else {
            baking
        };
        let bakes = VisualBaking {
            cache: execution.cache,
            asset: &|path| self.asset(path),
            workers: execution.workers,
            cancelled: self.cancelled,
            observe: execution.observe,
        }
        .bake(baking, host, rule, inputs)?;
        (execution.observe)("surface-publication");
        for material in delivery.values() {
            if !scene.manifest.materials.iter().any(|m| m.id == material.id) {
                scene.manifest.materials.push(material.clone());
            }
        }
        bindings.extend(
            bakes
                .iter()
                .map(|b| (b.material().id.clone(), b.material().id.clone())),
        );
        let result = scene
            .append_baked_visual(host, drawing, &bindings, &bakes)
            .map_err(fail);
        (execution.observe)("producer-continuation");
        result
    }
}

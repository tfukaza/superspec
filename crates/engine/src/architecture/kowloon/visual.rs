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
        visuals: &mut VisualOutput<'_>,
        drawing: &mut Drawing<'_>,
        host: &str,
        rule: &Node,
        inputs: &BTreeMap<String, Material>,
        delivery: &BTreeMap<String, Material>,
        mut bindings: BTreeMap<String, String>,
    ) -> Result<(), Diagnostic> {
        (execution.observe)("surface-planning");
        let fail = |e: String| rule.fail("AML_DRESSING", e);
        let baking = weathered::finish(drawing, self.cancelled).map_err(fail)?;
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
            visuals.ensure_material(material).map_err(fail)?;
        }
        bindings.extend(
            bakes
                .iter()
                .map(|b| (b.material().id.clone(), b.material().id.clone())),
        );
        let result = visuals
            .append(host, drawing, &bindings, &bakes)
            .map_err(fail);
        (execution.observe)("producer-continuation");
        result
    }
}

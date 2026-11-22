//! Structural build order with a supplied, compiled-in theme adapter.
use crate::{
    Scene,
    architecture::build::{PhysicalBuild, PhysicalOutput, RenderSurfaces},
};
use std::collections::BTreeMap;
use superspec_model::{
    theme::{ground, signs},
    visual::{BakedVisualInput, VisualInput},
};
use superspec_types::{architecture::Node, diagnostic::Diagnostic, material::Material};

mod items;
pub(crate) use items::ItemOutput;

/// Connects compiled-in themes to receiving surfaces and delivery.
pub(crate) trait ThemeAdapter {
    fn prepare(
        &mut self,
        _physical: &PhysicalBuild<'_>,
        _materials: &[Material],
    ) -> Result<(), Diagnostic> {
        Ok(())
    }
    fn needs_surfaces(&self) -> bool {
        false
    }
    fn plan(
        &mut self,
        _physical: &PhysicalBuild<'_>,
        _diagnostics: &mut Vec<Diagnostic>,
        _output: &PhysicalOutput,
        _render: &RenderSurfaces,
    ) -> Result<(), Diagnostic> {
        Ok(())
    }
    fn render_structure(
        &mut self,
        physical: &mut PhysicalBuild<'_>,
        scene: &mut Scene,
        render: RenderSurfaces,
    ) -> Result<(), Diagnostic> {
        physical.render_structure(scene, render)
    }
    fn publish(
        &mut self,
        _physical: &PhysicalBuild<'_>,
        _visuals: &mut VisualOutput<'_>,
    ) -> Result<(), Diagnostic> {
        Ok(())
    }
    /// Compose admitted item models, including any physical contributions, after architecture.
    fn place_items<'build>(
        &self,
        _physical: &'build PhysicalBuild<'_>,
        items: ItemOutput<'build>,
    ) -> Result<ItemOutput<'build>, Diagnostic> {
        Ok(items)
    }
    fn fallback_source(&self) -> Option<&Node> {
        None
    }
}

/// Post-review dressing can add draw data and visual provenance. Canonical mesh ranges,
/// colliders and physical findings stay under the build coordinator's ownership.
pub(crate) struct VisualOutput<'a>(&'a mut Scene);

impl VisualOutput<'_> {
    pub(crate) fn ensure_material(&mut self, material: &Material) -> Result<(), String> {
        let materials = &mut self.0.manifest.materials;
        if let Some(existing) = materials.iter().find(|existing| existing.id == material.id) {
            if existing != material {
                return Err(format!("Conflicting visual material {}.", material.id));
            }
        } else {
            material.validate()?;
            materials.push(material.clone());
        }
        Ok(())
    }

    pub(crate) fn append(
        &mut self,
        host: &str,
        drawing: &impl VisualInput,
        bindings: &BTreeMap<String, String>,
        bakes: &[impl BakedVisualInput],
    ) -> Result<(), String> {
        self.0.append_baked_visual(host, drawing, bindings, bakes)
    }

    pub(crate) fn ground_plans(&mut self) -> &mut Vec<ground::Plan> {
        &mut self.0.manifest.ground_plans
    }

    pub(crate) fn sign_plans(&mut self) -> &mut Vec<signs::Plan> {
        &mut self.0.manifest.sign_plans
    }

    pub(crate) fn credit(
        &mut self,
        contributors: &BTreeMap<String, Vec<String>>,
    ) -> Result<(), String> {
        self.0
            .manifest
            .architecture
            .as_mut()
            .expect("post-review visuals have architectural context")
            .credit_visuals(contributors)
    }
}

#[cfg(test)]
mod tests;

#[allow(clippy::too_many_arguments)]
pub(crate) fn compile_prepared(
    prepared: crate::source::PreparedDocument,
    revision: u32,
    assets: &dyn Fn(&str) -> Result<Vec<u8>, String>,
    visual: bool,
    validate_image: &dyn Fn(&[u8]) -> Result<crate::assets::image_admission::ImageInfo, String>,
    cancelled: &(dyn Fn() -> bool + Sync),
    retain_solids: bool,
    observe: &dyn Fn(&str),
    theme: &mut dyn ThemeAdapter,
) -> Result<Scene, Diagnostic> {
    let (mut physical, mut scene) =
        PhysicalBuild::new(prepared, revision, assets, validate_image, cancelled)?;
    theme.prepare(&physical, &scene.manifest.materials)?;
    physical.check_cancelled()?;
    physical.validate_relationships()?;
    observe("physical-solids");
    let output = physical.lower(&mut scene)?;
    observe("structural-validation");
    physical.validate_structure(&scene, &output)?;
    observe("face-materials");
    let render = physical.surfaces(&output, visual || theme.needs_surfaces())?;
    if retain_solids {
        physical.retain_solids(&mut scene, &output)?;
    }
    theme.plan(&physical, &mut scene.manifest.diagnostics, &output, &render)?;
    physical.check_cancelled()?;
    observe("architectural-render");
    if visual {
        theme.render_structure(&mut physical, &mut scene, render)?;
    }
    physical.check_cancelled()?;
    observe("physical-context");
    scene.manifest.architecture = Some(physical.context(&output)?);
    observe("architectural-review");
    let findings = crate::validation::review_structure(&scene, cancelled).map_err(|message| {
        physical.root().fail(
            if cancelled() {
                "CANCELLED"
            } else {
                "AML_REVIEW"
            },
            message,
        )
    })?;
    let review = crate::review::bind(&scene, findings, cancelled)?;
    scene.manifest.reviews = review.findings;
    scene.manifest.diagnostics.extend(review.diagnostics);
    scene.manifest.success = true;
    scene.manifest.vertex_count = scene.positions.len() / 3;
    scene.manifest.index_count = scene.indices.len();
    if visual {
        theme.publish(&physical, &mut VisualOutput(&mut scene))?;
    }
    physical.check_cancelled()?;
    observe("asset-publication");
    scene.manifest.materials.retain(|material| {
        scene
            .manifest
            .meshes
            .iter()
            .any(|mesh| mesh.visible && mesh.material.as_deref() == Some(&material.id))
    });
    physical.publish_assets(&mut scene, theme.fallback_source())?;
    physical.check_cancelled()?;
    if visual {
        let items = ItemOutput::new(scene, physical.root());
        scene = theme.place_items(&physical, items)?.finish(&physical)?;
    }
    observe("complete");
    physical.check_cancelled()?;
    Ok(scene)
}

//! Structural build order with a supplied, compiled-in theme adapter.
use crate::{
    Scene,
    architecture::build::{PhysicalBuild, PhysicalOutput, RenderSurfaces},
};
use superspec_types::{architecture::Node, diagnostic::Diagnostic, material::Material};

/// Scoped bridge for compiled-in visual producers. Physical maps remain private.
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
        _physical: &mut PhysicalBuild<'_>,
        _scene: &mut Scene,
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
        _physical: &mut PhysicalBuild<'_>,
        _scene: &mut Scene,
    ) -> Result<(), Diagnostic> {
        Ok(())
    }
    fn fallback_source(&self) -> Option<&Node> {
        None
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
    theme.plan(&mut physical, &mut scene, &output, &render)?;
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
        theme.publish(&mut physical, &mut scene)?;
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
    scene.manifest.success = true;
    scene.manifest.vertex_count = scene.positions.len() / 3;
    scene.manifest.index_count = scene.indices.len();
    observe("complete");
    physical.check_cancelled()?;
    Ok(scene)
}

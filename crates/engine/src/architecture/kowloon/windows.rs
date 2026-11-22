//! Bind the visual casement to the aperture surveyed during physical lowering.
use super::*;
use crate::worldspec::themes::kowloon::city as catalog;

impl Compiler<'_> {
    pub(super) fn place_windows<'build>(
        &self,
        mut items: ItemOutput<'build>,
    ) -> Result<ItemOutput<'build>, Diagnostic> {
        let visible = self.window_fits.iter().filter(|(i, _)| {
            let node = &self.resolution.nodes[**i];
            let mut parent = node.parent;
            while let Some(p) = parent {
                if self.resolution.nodes[p].source.kind == "collision" {
                    return false;
                }
                parent = self.resolution.nodes[p].parent;
            }
            node.source.boolean("visible", true)
        });
        let revision = items.revision();
        let mut session = None;
        for (&index, fit) in visible {
            let node = &self.resolution.nodes[index];
            let mut request =
                catalog::hosted_window(fit.width, fit.height, node.source.boolean("open", false));
            request.placement_transform = Some(fit.matrix.0);
            let mut model = session
                .get_or_insert_with(crate::Session::default)
                .build(
                    &request,
                    revision.into(),
                    &|path| {
                        if superspec_catalog::image_name(path).is_some() {
                            self.asset(path)
                        } else {
                            catalog::asset(path)
                        }
                    },
                    self.cancelled,
                )
                .map_err(|errors| {
                    node.source.fail(
                        "AML_WINDOW",
                        errors
                            .into_iter()
                            .map(|e| e.message)
                            .collect::<Vec<_>>()
                            .join("\n"),
                    )
                })?
                .model;
            model.instances.retain(|part| part.visible);
            for part in &mut model.instances {
                part.collidable = false;
                part.id = format!("{}/{}", node.id, part.id);
                part.object_id = node.id.clone();
                part.source = node.source.source.clone();
            }
            let fail = |e| node.source.fail("AML_WINDOW", e);
            items = items.attach_visual(&node.id, model).map_err(fail)?;
        }
        Ok(items)
    }
}

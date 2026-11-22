//! Item placement operations on an already reviewed world. Only the build coordinator
//! can open or finish this scope.
use super::PhysicalBuild;
use crate::{Scene, Session, authoring::Request, worldspec::city};
use std::{
    collections::BTreeSet,
    sync::{Arc, Mutex},
};
use superspec_model::{
    batches::{Collision, InstanceBatch},
    composition,
    instances::{LocalMesh, Model},
};
use superspec_types::{
    architecture::Node, diagnostic::Diagnostic, lighting::Lighting, material::Material,
};

pub(crate) struct ItemOutput<'build> {
    scene: Scene,
    root: &'build Node,
    first_item_mesh: usize,
    repeats: Vec<city::Repeats>,
    asset_source: Option<&'build Node>,
}

impl<'build> ItemOutput<'build> {
    pub(super) fn new(scene: Scene, root: &'build Node) -> Self {
        let first_item_mesh = scene.manifest.meshes.len();
        Self {
            scene,
            root,
            first_item_mesh,
            repeats: Vec::new(),
            asset_source: None,
        }
    }

    pub(super) fn finish(mut self, physical: &PhysicalBuild<'_>) -> Result<Scene, Diagnostic> {
        // Each pending group names templates admitted by this placement scope. Their batches
        // must inherit any slot bindings the theme supplied after placing them.
        for repeats in self.repeats {
            repeats
                .deliver(&mut self.scene)
                .map_err(|e| self.asset_source.unwrap_or(self.root).fail("AML_CITY", e))?;
        }
        if let Some(source) = self.asset_source {
            physical.publish_material_assets(&mut self.scene, Some(source))?;
        }
        Ok(self.scene)
    }

    pub(crate) fn revision(&self) -> u32 {
        self.scene.manifest.revision
    }

    /// The AML host already owns collision and provenance. A hosted item may add
    /// visible draws for that owner without invalidating a physical review.
    pub(crate) fn attach_visual(mut self, host: &str, model: Model) -> Result<Self, String> {
        if self
            .scene
            .manifest
            .architecture
            .as_ref()
            .and_then(|c| c.source(host))
            .is_none()
        {
            return Err(format!("Unknown visual attachment host {host}."));
        }
        if model
            .instances
            .iter()
            .any(|m| m.object_id != host || m.collidable || !m.visible)
            || model
                .batches
                .iter()
                .any(|b| b.object_id != host || b.collision != Collision::None || !b.visible)
        {
            return Err(format!(
                "Visual attachment for {host} must contain only its visible, render-only parts."
            ));
        }
        let mut visual = model.scene(self.revision())?;
        visual.manifest.architecture = None;
        let validation = self.scene.manifest.validation.take();
        let geometry_status = self.scene.manifest.geometry_validation;
        let navigation_status = self.scene.manifest.navigation_validation;
        self.scene = composition::append(self.scene, visual)?;
        self.scene.manifest.validation = validation;
        self.scene.manifest.geometry_validation = geometry_status;
        self.scene.manifest.navigation_validation = navigation_status;
        Ok(self)
    }

    pub(crate) fn place_city<'a>(
        mut self,
        item_session: impl FnOnce() -> Arc<Mutex<Session>>,
        placements: impl Iterator<Item = &'a Node>,
        item: impl Fn(&Node) -> Result<Request, String>,
        assets: impl Fn(&Node, &str) -> Result<Arc<[u8]>, String>,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Self, Diagnostic> {
        let (scene, repeats) = city::place(
            self.scene,
            item_session,
            placements,
            item,
            assets,
            cancelled,
        )?;
        self.scene = scene;
        self.repeats.push(repeats);
        Ok(self)
    }

    pub(crate) fn add_batch(
        &mut self,
        batch: &InstanceBatch,
        mesh: &LocalMesh,
    ) -> Result<(), String> {
        self.scene.add_batch(batch, mesh)
    }

    pub(crate) fn project_foliage(&mut self) -> Result<(), String> {
        self.scene.project_foliage_lods()
    }

    pub(crate) fn default_lighting(
        &mut self,
        default: impl FnOnce() -> Lighting,
    ) -> Result<(), String> {
        if self.scene.manifest.lighting.is_none() {
            let lighting = default();
            if let Some(invalid) = lighting.validate().first() {
                return Err(format!("Invalid {}: {}.", invalid.field, invalid.message));
            }
            self.scene.manifest.lighting = Some(lighting);
        }
        Ok(())
    }

    /// Bind catalog slots on item meshes appended during this phase. Include only
    /// materials that were actually used, in deterministic material-ID order.
    /// An existing scene definition owns its ID, including an authored override.
    /// The resolver supplies definitions only for IDs missing from the scene.
    pub(crate) fn bind_slots(
        &mut self,
        source: &'build Node,
        owners: &BTreeSet<String>,
        slot_material: impl Fn(&str) -> Option<String>,
        resolve: impl Fn(&str) -> Option<Material>,
    ) -> Result<(), String> {
        let meshes = &mut self.scene.manifest.meshes[self.first_item_mesh..];
        let mut bindings = Vec::new();
        for (index, mesh) in meshes.iter().enumerate() {
            if owners.contains(&mesh.object_id)
                && let Some(id) = mesh.id.rsplit('/').next().and_then(&slot_material)
            {
                bindings.push((index, id));
            }
        }
        let used: BTreeSet<_> = bindings.iter().map(|(_, id)| id).collect();
        let mut additions = Vec::new();
        for id in used {
            if !self.scene.manifest.materials.iter().any(|m| &m.id == id) {
                let material = resolve(id).ok_or_else(|| format!("Unknown item material {id}."))?;
                if &material.id != id {
                    return Err(format!("Item material identity does not match {id}."));
                }
                material.validate()?;
                additions.push(material);
            }
        }
        for (index, id) in bindings {
            meshes[index].material = Some(id);
        }
        self.scene.manifest.materials.extend(additions);
        self.asset_source.get_or_insert(source);
        Ok(())
    }
}

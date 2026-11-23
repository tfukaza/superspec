//! Persistent inputs and ordered visual edits; expansion belongs to delivery.
use super::admission::{Admission, Limits};
use crate::{
    Scene,
    instances::{LocalMesh, Model},
    visual::GraphicsTier,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{Arc, Weak},
};

#[derive(Clone, Debug)]
enum Base {
    Scene(Arc<Scene>),
    Instances(Arc<Model>),
}

/// Authoring order is significant for visual replacement and resource delivery order.
#[derive(Clone, Debug)]
pub enum Layer {
    Append(Arc<Model>),
    Replace {
        model: Arc<Model>,
        receivers: BTreeSet<String>,
    },
}

/// A canonical physical base and immutable local resources with separate placements.
/// Calling `scene` creates independently owned compatibility buffers.
#[derive(Clone, Debug)]
pub struct WorldModel {
    base: Base,
    layers: Vec<Layer>,
    meshes: BTreeMap<String, Weak<LocalMesh>>,
    admission: Admission,
    limits: Limits,
    retained_bytes: usize,
    render_visibility: BTreeMap<String, GraphicsTier>,
}

impl WorldModel {
    pub fn from_scene(mut scene: Scene) -> Result<Self, String> {
        // Retain compact metadata ownership, not spare allocations left by producer rollback.
        // Geometry, images and retained solid buffers move without copying.
        scene.manifest = scene.manifest.clone();
        super::ranges(&scene)?;
        let mut pixels = 0usize;
        for asset in &scene.manifest.assets {
            let count = super::image_pixels(&scene, asset)?;
            pixels = pixels
                .checked_add(count)
                .ok_or("Image pixel allowance overflow.")?;
            scene.image_sizes.insert(asset.id.clone(), count);
        }
        if pixels > scene.image_pixels() {
            scene.reserve_image_pixels(pixels - scene.image_pixels())?;
        }
        let scene = Arc::new(scene);
        let admission = Admission::from_validated_scene(scene.clone())?;
        let mut output = Self {
            base: Base::Scene(scene),
            layers: vec![],
            meshes: BTreeMap::new(),
            admission,
            limits: Limits::default(),
            retained_bytes: 0,
            render_visibility: BTreeMap::new(),
        };
        output.admission.check(output.limits)?;
        output.retained_bytes = output.charge(&output.admission, None, None)?;
        Ok(output)
    }

    pub fn from_instances(model: Model) -> Result<Self, String> {
        let mut output = Self {
            base: Base::Instances(Arc::new(Model::default())),
            layers: vec![],
            meshes: BTreeMap::new(),
            admission: Admission::default(),
            limits: Limits::default(),
            retained_bytes: 0,
            render_visibility: BTreeMap::new(),
        };
        let model = output.share_meshes(model)?;
        output.admission = Admission::model(&model)?;
        output.admission.check(output.limits)?;
        output.publish_meshes(&model);
        output.base = Base::Instances(Arc::new(model));
        output.retained_bytes = output.charge(&output.admission, None, None)?;
        Ok(output)
    }

    /// Lowers admission limits without allocating a delivery view.
    pub fn with_limits(mut self, limits: Limits) -> Result<Self, String> {
        limits.validate()?;
        self.admission.check(limits)?;
        if self.retained_bytes > limits.retained_bytes {
            return Err("Retained world resource allowance exceeded.".into());
        }
        self.limits = limits;
        Ok(self)
    }

    /// Conservative retained ownership charge, including superseded layers; not process telemetry.
    pub fn retained_bytes(&self) -> usize {
        self.retained_bytes
    }

    pub fn base_scene(&self) -> Option<&Arc<Scene>> {
        match &self.base {
            Base::Scene(scene) => Some(scene),
            _ => None,
        }
    }

    pub fn base_instances(&self) -> Option<&Arc<Model>> {
        match &self.base {
            Base::Instances(model) => Some(model),
            _ => None,
        }
    }

    pub fn layers(&self) -> &[Layer] {
        &self.layers
    }

    /// Attach validated WorldSpec appearance choices to this delivery.
    pub fn set_render_visibility(
        &mut self,
        visibility: BTreeMap<String, GraphicsTier>,
    ) -> Result<(), String> {
        if visibility.len() > superspec_types::limits::MAX_MODEL_RECORDS_PER_KIND {
            return Err("WorldSpec render visibility allowance exceeded.".into());
        }
        let mut base_objects = BTreeSet::new();
        if let Some(scene) = self.base_scene().filter(|_| !visibility.is_empty()) {
            base_objects.extend(scene.manifest.meshes.iter().map(|mesh| mesh.object_id.as_str()));
            base_objects.extend(scene.manifest.batches.iter().map(|batch| batch.object_id.as_str()));
        }
        for id in visibility.keys() {
            if id.is_empty()
                || id.len() > 4096
                || (!self.admission.has_owner(id) && !base_objects.contains(id.as_str()))
            {
                return Err(format!("Unknown render visibility object {id}."));
            }
        }
        self.render_visibility = visibility;
        Ok(())
    }

    pub fn append(&mut self, model: Model) -> Result<(), String> {
        let model = self.share_meshes(model)?;
        let admission = self
            .admission
            .append(Admission::model(&model)?, self.limits)?;
        let retained = self.charge(&admission, Some(&model), None)?;
        self.publish_meshes(&model);
        self.layers.push(Layer::Append(Arc::new(model)));
        self.admission = admission;
        self.retained_bytes = retained;
        Ok(())
    }

    pub fn replace_receivers(
        &mut self,
        model: Model,
        receivers: BTreeSet<String>,
    ) -> Result<(), String> {
        let model = self.share_meshes(model)?;
        let admission =
            self.admission
                .replace(Admission::model(&model)?, &receivers, self.limits)?;
        let retained = self.charge(&admission, Some(&model), Some(&receivers))?;
        self.publish_meshes(&model);
        self.layers.push(Layer::Replace {
            model: Arc::new(model),
            receivers,
        });
        self.admission = admission;
        self.retained_bytes = retained;
        Ok(())
    }

    /// Admission and expansion use the same contracts as the existing scene adapters.
    /// Failure or cancellation cannot alter this model or another consumer's buffers.
    pub fn scene(&self, revision: u32) -> Result<Scene, String> {
        self.scene_cancellable(revision, &|| false)
    }

    pub fn scene_cancellable(
        &self,
        revision: u32,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Scene, String> {
        let check = || {
            if cancelled() {
                Err("World delivery cancelled.".to_owned())
            } else {
                Ok(())
            }
        };
        check()?;
        let mut scene = match &self.base {
            Base::Scene(scene) => scene.as_ref().clone(),
            Base::Instances(model) => model.scene(revision)?,
        };
        scene.manifest.revision = revision;
        for layer in &self.layers {
            check()?;
            scene = match layer {
                Layer::Append(model) => super::append(scene, model.scene(revision)?)?,
                Layer::Replace { model, receivers } => {
                    super::replace_receivers(scene, model.scene(revision)?, receivers)?
                }
            };
        }
        scene.manifest.render_visibility.extend(self.render_visibility.clone());
        check()?;
        Ok(scene)
    }

    fn share_meshes(&self, mut model: Model) -> Result<Model, String> {
        let mut admitted = BTreeMap::<String, Arc<LocalMesh>>::new();
        for (key, identity) in &model.mesh_artifacts {
            if identity.is_empty() || identity.len() > 4096 {
                return Err("Invalid local mesh artifact identity.".into());
            }
            let mesh = model
                .meshes
                .get_mut(key)
                .ok_or("Mesh artifact references an unknown local resource.")?;
            if let Some(shared) = admitted
                .get(identity)
                .cloned()
                .or_else(|| self.meshes.get(identity).and_then(Weak::upgrade))
            {
                if shared.as_ref() != mesh.as_ref() {
                    return Err(format!("Conflicting local mesh artifact {identity}."));
                }
                *mesh = shared;
            } else {
                admitted.insert(identity.clone(), mesh.clone());
            }
        }
        Ok(model)
    }

    fn publish_meshes(&mut self, model: &Model) {
        for (key, identity) in &model.mesh_artifacts {
            self.meshes
                .insert(identity.clone(), Arc::downgrade(&model.meshes[key]));
        }
    }

    fn charge(
        &self,
        admission: &Admission,
        extra: Option<&Model>,
        receivers: Option<&BTreeSet<String>>,
    ) -> Result<usize, String> {
        let models: Vec<&Model> = self
            .base_instances()
            .map(Arc::as_ref)
            .into_iter()
            .chain(self.layers.iter().map(|layer| match layer {
                Layer::Append(model) | Layer::Replace { model, .. } => model.as_ref(),
            }))
            .chain(extra)
            .collect();
        let identities: BTreeSet<_> = models
            .iter()
            .flat_map(|model| model.mesh_artifacts.values().map(String::as_str))
            .collect();
        let mut bytes = super::retention::retained_bytes(
            self.base_scene().map(|scene| scene.as_ref()),
            models.iter().copied(),
        );
        bytes = bytes
            .saturating_add(admission.retained_bytes())
            .saturating_add(512);
        for identity in identities {
            bytes = bytes.saturating_add(identity.len() + 256);
        }
        for layer in &self.layers {
            bytes = bytes.saturating_add(256);
            if let Layer::Replace { receivers, .. } = layer {
                for id in receivers {
                    bytes = bytes.saturating_add(id.capacity() + 128);
                }
            }
        }
        if extra.is_some() {
            bytes = bytes.saturating_add(256);
        }
        if let Some(receivers) = receivers {
            for id in receivers {
                bytes = bytes.saturating_add(id.capacity() + 128);
            }
        }
        if bytes > self.limits.retained_bytes {
            return Err("Retained world resource allowance exceeded.".into());
        }
        Ok(bytes)
    }
}

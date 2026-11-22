//! Atlas layouts of a scene project's receivers, for a renderer that bakes materials in UV space
//! itself (the Studio's GPU preview). The layout is the one a CPU bake of the same project uses:
//! the same [`Atlas`], sized by the same rule, so a texel of a GPU bake samples the point the CPU
//! bake samples there and CPU-baked fields ([`Session::bake_project_fields`]) land on its grid.

use super::{
    Diagnostic, FieldBake, Project, Request, Session,
    project::{ContactScene, layout_settings, receiver_request, receivers},
    receiver::{Atlas, GUTTER},
};
use crate::{runtime::store::Retention, session::Artifact};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

/// Numbers per triangle in [`ProjectLayouts::records`]:
/// - 0..4 the triangle's chart rectangle in atlas texels, rows from the upper edge: x, y, width,
///   height (gutters included);
/// - 4 the quarter turns from the triangle's legs to the atlas axes ([`Atlas::turn`]), 5..7 the
///   chart's distinct samples along its width and height, 7 the triangle's role in its chart
///   (0 alone, 1 the first of a pair, 2 the second, 3 one of a flat face on a pattern anchor's
///   lattice, which lies in the chart where its atlas UVs put it);
/// - 8..17 world positions of the corner, the first leg's end and the second leg's end;
/// - 17..26 their unit normals; 26..32 their surface UVs; 32..38 their atlas UVs (V up).
pub const LAYOUT_RECORD: usize = 38;

/// One receiver's atlas: its size and where its triangle records start.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReceiverLayout {
    /// The mesh identity in the composed scene.
    pub id: String,
    /// The material assignment that covers it.
    pub material: String,
    pub width: usize,
    pub height: usize,
    /// Texels per meter along every chart.
    pub density: f64,
    pub triangles: usize,
    /// Index of the receiver's first number in [`ProjectLayouts::records`].
    pub offset: usize,
    /// How each chart maps atlas texels to the material's surface domain, ten numbers a chart
    /// in chart order: the chart rectangle's x and y in atlas texels; the surface position (in
    /// meters, the `surface` a program reads) of the first inner texel's center, at
    /// (x + gutter, y + gutter); the change of that position one texel along atlas +x and one
    /// along +y (rows down), which also says how the chart's frame is turned; the sample
    /// footprint in meters; and 1 where the first inner texel is exactly on the lattice of its
    /// footprint (surface = (whole numbers + 0.5) * footprint), 0 otherwise. Intrinsic values
    /// (see `Shader::reads`) are shared between receivers only at lattice points with equal
    /// footprints, so a chart with a 0 here shares nothing with the CPU bake's rasters.
    pub surface: Vec<f64>,
}

/// A scene project prepared for describing, laying out and field-baking
/// ([`Session::prepare_project`]).
pub struct PreparedProject {
    /// The project with its item models resolved.
    pub(super) project: Project,
    /// What each item reports about itself, by model identity; `None` without item models.
    pub(super) items: Option<serde_json::Map<String, serde_json::Value>>,
    /// The composed models.
    pub model: superspec_model::composition::WorldModel,
    /// The models' delivery view for the revision the project was prepared at.
    pub scene: superspec_model::Scene,
    /// Object-to-world transforms of imported and placed receivers.
    pub(super) transforms: BTreeMap<String, [f64; 16]>,
    /// The scene's triangles as contacts are found among them, made when first asked for.
    pub(super) touching: std::cell::OnceCell<ContactScene>,
}

/// The name [`PreparedProject`] had while only field bakes used it.
pub type ProjectFields = PreparedProject;

/// One chart's entry of [`ReceiverLayout::surface`].
fn chart_surface(atlas: &Atlas, chart: &super::receiver::Chart) -> [f64; 10] {
    let at = |x: usize, y: usize| {
        let (triangle, u, v) = atlas.texel(chart, x, y);
        atlas.sample(triangle, u, v).ok()
    };
    let inner = GUTTER.min(chart.width - 1).min(chart.height - 1);
    let (Some(first), Some(along), Some(down)) = (
        at(inner, inner),
        at((inner + 1).min(chart.width - 1), inner),
        at(inner, (inner + 1).min(chart.height - 1)),
    ) else {
        return [
            chart.x as f64,
            chart.y as f64,
            0.,
            0.,
            0.,
            0.,
            0.,
            0.,
            0.,
            0.,
        ];
    };
    let step = first.footprint;
    let on = |position: f64| {
        let index = (position / step - 0.5).round();
        step > 0. && (index + 0.5) * step == position
    };
    [
        chart.x as f64,
        chart.y as f64,
        first.surface[0],
        first.surface[1],
        along.surface[0] - first.surface[0],
        along.surface[1] - first.surface[1],
        down.surface[0] - first.surface[0],
        down.surface[1] - first.surface[1],
        step,
        f64::from(u8::from(on(first.surface[0]) && on(first.surface[1]))),
    ]
}

/// The atlases of every assigned receiver of a scene project.
#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectLayouts {
    /// Texels of chart margin on every side ([`GUTTER`]).
    pub gutter: usize,
    /// Numbers per triangle record ([`LAYOUT_RECORD`]).
    pub record: usize,
    pub receivers: Vec<ReceiverLayout>,
    /// Triangle records of every receiver, end to end.
    #[serde(skip)]
    pub records: Vec<f32>,
}

// Keep only the transferable atlas data between edits. Full CPU atlases also own
// source vertices and, on anchored faces, a triangle owner for every texture pixel.
const PREVIEW_LAYOUTS: Retention = Retention {
    name: "authoring-preview-layouts",
    bytes: 64 * 1024 * 1024,
    entries: 2048,
};

pub(crate) struct ChartRecords {
    width: usize,
    height: usize,
    density: f64,
    records: Vec<f32>,
    surface: Vec<f64>,
    content_key: std::sync::OnceLock<[u8; 32]>,
}

impl ChartRecords {
    fn new(mut atlas: Atlas, lattice_maps_only: bool) -> Self {
        let mut records = Vec::new();
        write_records(&atlas, &mut records);
        // A GPU chart carries three samples of the lattice plane. When every triangle
        // supplies the same bits at those three points, its owner cannot affect the
        // result. Skip the CPU rasterization of potentially millions of owner texels.
        for index in 0..atlas.charts.len() {
            let chart = &atlas.charts[index];
            if chart.lattice.is_none() || chart.others.is_empty() {
                continue;
            }
            let probes = [(GUTTER, GUTTER), (GUTTER + 1, GUTTER), (GUTTER, GUTTER + 1)]
                .map(|(x, y)| atlas.texel(chart, x, y));
            let values = |triangle, (_, u, v)| {
                atlas
                    .sample(triangle, u, v)
                    .ok()
                    .map(|p| (p.surface.map(f64::to_bits), p.footprint.to_bits()))
            };
            let same = probes.iter().all(|probe| {
                let first = values(chart.first, *probe);
                chart
                    .others
                    .iter()
                    .all(|&other| values(other, *probe) == first)
            });
            if !same {
                atlas.fill_chart_owners(index);
            }
        }
        Self {
            width: atlas.width,
            height: atlas.height,
            density: atlas.density,
            records,
            content_key: Default::default(),
            surface: atlas
                .charts
                .iter()
                .filter(|chart| !lattice_maps_only || chart.lattice.is_some())
                .flat_map(|chart| chart_surface(&atlas, chart))
                .collect(),
        }
    }

    fn content_key(&self) -> &[u8; 32] {
        self.content_key.get_or_init(|| {
            let mut hash = Sha256::new();
            hash.update(b"receiver-chart-records-v1");
            for value in [
                self.width,
                self.height,
                self.records.len(),
                self.surface.len(),
            ] {
                hash.update((value as u64).to_le_bytes());
            }
            hash.update(self.density.to_le_bytes());
            let mut bytes = Vec::with_capacity(4096);
            for chunk in self.records.chunks(1024) {
                bytes.clear();
                bytes.extend(chunk.iter().flat_map(|v| v.to_le_bytes()));
                hash.update(&bytes);
            }
            for value in &self.surface {
                hash.update(value.to_le_bytes());
            }
            hash.finalize().into()
        })
    }
}

/// Hash the GPU layout's admitted inputs before allocating another atlas. Unlike a
/// CPU material bake it does not sample contact fields. Named features are
/// admitted even on hits, and do not affect chart coordinates or texel density.
fn chart_key(receiver: &super::Receiver, request: &Request, lattice_maps_only: bool) -> [u8; 32] {
    let id = &receiver.id;
    let mut digest = Sha256::new();
    digest.update(b"preview-chart-records-v3");
    digest.update([u8::from(lattice_maps_only)]);
    digest.update(
        serde_json::to_vec(&(
            request.resolution,
            request.profile.max_resolution(),
            request.mapping,
            request.domain,
            request.gravity,
            request.density_of(id).map(f64::to_bits),
            receiver.object_to_world,
            request
                .anchors
                .get(id)
                .filter(|_| request.mapping == super::Mapping::Anchored),
        ))
        .expect("admitted receiver layout inputs"),
    );
    // Batch the bytes of the admitted arrays. Formatting every vertex as a decimal
    // string or cloning a Receiver to blank its identity makes cache hits costly.
    let mut words = Vec::<u8>::with_capacity(4096);
    for values in [&receiver.positions, &receiver.normals, &receiver.uvs] {
        digest.update((values.len() as u64).to_le_bytes());
        for chunk in values.chunks(512) {
            words.clear();
            words.extend(chunk.iter().flat_map(|value| value.to_le_bytes()));
            digest.update(&words);
        }
    }
    digest.update((receiver.indices.len() as u64).to_le_bytes());
    for chunk in receiver.indices.chunks(1024) {
        words.clear();
        words.extend(chunk.iter().flat_map(|value| value.to_le_bytes()));
        digest.update(&words);
    }
    digest.finalize().into()
}

/// Appends an atlas's triangle records (see [`LAYOUT_RECORD`]) in triangle order.
fn write_records(atlas: &Atlas, records: &mut Vec<f32>) {
    let receiver = &atlas.receiver;
    let triangles = receiver.indices.len() / 3;
    let mut owner = vec![(0usize, 0f32); triangles];
    for (index, chart) in atlas.charts.iter().enumerate() {
        owner[chart.first] = (index, if chart.second.is_some() { 1. } else { 0. });
        if let Some(second) = chart.second {
            owner[second] = (index, 2.);
        }
        // A whole flat face on a pattern anchor's lattice (role 3): each triangle draws in the
        // chart where its atlas UVs put it, not along its own legs.
        if chart.lattice.is_some() {
            for triangle in chart.triangles() {
                owner[triangle] = (index, 3.);
            }
        }
    }
    records.reserve(triangles * LAYOUT_RECORD);
    for triangle in 0..triangles {
        let (chart, role) = owner[triangle];
        let chart = &atlas.charts[chart];
        records.extend([chart.x, chart.y, chart.width, chart.height].map(|texels| texels as f32));
        records.extend([
            f32::from(atlas.turn(triangle)),
            chart.samples[0] as f32,
            chart.samples[1] as f32,
            role,
        ]);
        let corners = atlas.corners(triangle).map(|index| index as usize);
        // The atlas mesh holds each triangle's corners in this order, with unit normals.
        for position in &atlas.mesh.positions[triangle * 3..triangle * 3 + 3] {
            records.extend(position.map(|v| v as f32));
        }
        for normal in &atlas.mesh.normals[triangle * 3..triangle * 3 + 3] {
            records.extend(normal.map(|v| v as f32));
        }
        for corner in corners {
            records.extend(
                receiver.uvs[corner * 2..corner * 2 + 2]
                    .iter()
                    .map(|v| *v as f32),
            );
        }
        for uv in &atlas.mesh.uvs[triangle * 3..triangle * 3 + 3] {
            records.extend(uv.map(|v| v as f32));
        }
    }
}

impl Session {
    /// The atlas of every assigned receiver of a version 2 material project, as the CPU bake of
    /// that project lays it out. Nothing is compiled or baked.
    pub fn project_layouts(
        &mut self,
        project: &Project,
        revision: u64,
        assets: &dyn Fn(&str) -> Result<Arc<[u8]>, String>,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<ProjectLayouts, Vec<Diagnostic>> {
        let fail = |message: String| vec![Diagnostic::message(&project.entry, message)];
        if project.schema_version != 2 || project.kind != "material" {
            return Err(fail(
                "Only version 2 material projects have receiver layouts.".into(),
            ));
        }
        let prepared = self.prepare_project(project, revision, assets, cancelled)?;
        self.layouts_prepared(&prepared, cancelled)
    }

    /// [`Session::project_layouts`] of a prepared project ([`Session::prepare_project`]).
    pub fn layouts_prepared(
        &mut self,
        prepared: &PreparedProject,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<ProjectLayouts, Vec<Diagnostic>> {
        self.layouts_prepared_with_surface_maps(prepared, false, cancelled)
    }

    /// A real-time GPU baker interpolates ordinary triangle charts from their vertex records.
    /// It only samples the extra texel-to-surface map on a flat face snapped to a pattern lattice.
    /// Other consumers can still request every chart's map with [`Self::layouts_prepared`].
    pub fn layouts_prepared_with_surface_maps(
        &mut self,
        prepared: &PreparedProject,
        lattice_maps_only: bool,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<ProjectLayouts, Vec<Diagnostic>> {
        self.collect_layouts(prepared, lattice_maps_only, None, cancelled)
    }

    /// Visit the content key of every admitted chart in binding order. Two different
    /// input receivers can give the same layout. A caller holding its exact records
    /// and surface maps needs only the new material and placement binding.
    pub fn changed_layouts_prepared(
        &mut self,
        prepared: &PreparedProject,
        lattice_maps_only: bool,
        retained: &mut dyn FnMut(&str, &str, &[u8; 32]) -> bool,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<ProjectLayouts, Vec<Diagnostic>> {
        self.collect_layouts(prepared, lattice_maps_only, Some(retained), cancelled)
    }

    fn collect_layouts(
        &mut self,
        prepared: &PreparedProject,
        lattice_maps_only: bool,
        mut retained: Option<&mut dyn FnMut(&str, &str, &[u8; 32]) -> bool>,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<ProjectLayouts, Vec<Diagnostic>> {
        let PreparedProject {
            project,
            scene,
            transforms,
            ..
        } = prepared;
        let fail = |message: String| vec![Diagnostic::message(&project.entry, message)];
        if project.schema_version != 2 || project.kind != "material" {
            return Err(fail(
                "Only version 2 material projects have receiver layouts.".into(),
            ));
        }
        let mut layouts = ProjectLayouts {
            gutter: GUTTER,
            record: LAYOUT_RECORD,
            ..Default::default()
        };
        for assignment in &project.materials {
            let ids: BTreeSet<String> = assignment.meshes.iter().cloned().collect();
            let mut request = layout_settings(project, assignment);
            for id in &ids {
                if let Some(density) = project.texel_densities.get(id) {
                    request.texel_densities.insert(id.clone(), *density);
                }
                if request.mapping == super::Mapping::Anchored
                    && let Some(anchor) = project.anchors.get(id)
                {
                    request.anchors.insert(id.clone(), *anchor);
                }
            }
            for receiver in receivers(scene, &ids, transforms) {
                if cancelled() {
                    return Err(fail("Project authoring cancelled.".into()));
                }
                let id = receiver.id.clone();
                let key = chart_key(&receiver, &request, lattice_maps_only);
                let cached = self.runtime.lookup(PREVIEW_LAYOUTS, &key);
                let named = project
                    .features
                    .get(&id)
                    .map(Vec::as_slice)
                    .unwrap_or_default();
                let check_named = || {
                    crate::materialspec::program::NamedFeatures::validate(named)
                        .map_err(|message| fail(format!("{id}: {message}")))
                };
                let charts = match cached.as_deref() {
                    Some(Artifact::Layout(charts)) => {
                        check_named()?;
                        Arc::clone(charts)
                    }
                    _ => {
                        let atlas = Atlas::layout_for_request(receiver, &request)
                            .map_err(|message| fail(format!("{id}: {message}")))?;
                        check_named()?;
                        let charts = Arc::new(ChartRecords::new(atlas, lattice_maps_only));
                        let bytes = charts.records.capacity() * 4
                            + charts.surface.capacity() * 8
                            + size_of::<ChartRecords>()
                            + 128;
                        self.runtime.retain(
                            PREVIEW_LAYOUTS,
                            key,
                            Artifact::Layout(Arc::clone(&charts)),
                            bytes,
                            false,
                        );
                        charts
                    }
                };
                // A fresh or evicted native session still validates the receiver even
                // when the client advertises a key that it acquired on another session.
                if retained
                    .as_mut()
                    .is_some_and(|held| held(&id, &assignment.id, charts.content_key()))
                {
                    continue;
                }
                let offset = layouts.records.len();
                layouts.records.extend_from_slice(&charts.records);
                layouts.receivers.push(ReceiverLayout {
                    id,
                    material: assignment.id.clone(),
                    width: charts.width,
                    height: charts.height,
                    density: charts.density,
                    triangles: charts.records.len() / LAYOUT_RECORD,
                    offset,
                    surface: charts.surface.clone(),
                });
            }
        }
        Ok(layouts)
    }

    /// Evaluates `names` (fields of the material assigned to `mesh`) at every texel of that
    /// receiver's atlas, as [`Session::bake_fields`] does for a single request: the receiver, its
    /// contacts and the request are the ones `build_project` bakes that mesh with. A name
    /// `stack.<k>/<field>` is a field of the slot's `k`-th layer (from 1), baked with that layer's
    /// module, parameters and seed, and the receiver's named features as the layer follows them
    /// (a GPU host composing the layers itself asks for the fields of a layer's plan this way).
    pub fn bake_project_fields(
        &mut self,
        project: &Project,
        mesh: &str,
        names: &[String],
        revision: u64,
        assets: &dyn Fn(&str) -> Result<Arc<[u8]>, String>,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<FieldBake, Vec<Diagnostic>> {
        let prepared = self.prepare_project_fields(project, revision, assets, cancelled)?;
        self.bake_prepared_fields(&prepared, mesh, names, revision, assets, cancelled)
    }

    /// What describing, laying out and field-baking a scene project share: the project with its
    /// item models resolved (and what they report), the composed models and their scene. A host
    /// that wants several of those for one project prepares it once
    /// ([`Session::shade_prepared`], [`Session::layouts_prepared`],
    /// [`Session::bake_prepared_fields`]) instead of placing every item and composing every
    /// model again for each. It describes the project and the assets as they were when it was
    /// made.
    pub fn prepare_project(
        &mut self,
        project: &Project,
        revision: u64,
        assets: &dyn Fn(&str) -> Result<Arc<[u8]>, String>,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<PreparedProject, Vec<Diagnostic>> {
        let fail = |message: String| vec![Diagnostic::message(&project.entry, message)];
        self.begin_authoring(revision, &project.entry)?;
        // Item models bind their slots' meshes to materials (`Session::resolve_items`).
        let (resolved, placements) = self.resolve_items(project, assets, cancelled)?;
        let (project, items) = match resolved {
            Some(items) => (items.project, Some(items.metadata)),
            None => (project.clone(), None),
        };
        let (model, transforms) =
            self.compose_models(&project, placements, revision, assets, cancelled)?;
        let scene = model
            .scene_cancellable(revision as u32, cancelled)
            .map_err(&fail)?;
        Ok(PreparedProject {
            project,
            items,
            model,
            scene,
            transforms,
            touching: std::cell::OnceCell::new(),
        })
    }

    /// [`Session::prepare_project`] for field bakes, which only version 2 material projects have.
    pub fn prepare_project_fields(
        &mut self,
        project: &Project,
        revision: u64,
        assets: &dyn Fn(&str) -> Result<Arc<[u8]>, String>,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<PreparedProject, Vec<Diagnostic>> {
        if project.schema_version != 2 || project.kind != "material" {
            return Err(vec![Diagnostic::message(
                &project.entry,
                "Only version 2 material projects bake fields per mesh.",
            )]);
        }
        self.prepare_project(project, revision, assets, cancelled)
    }

    /// [`Session::bake_project_fields`] for one mesh of a prepared project.
    pub fn bake_prepared_fields(
        &mut self,
        prepared: &PreparedProject,
        mesh: &str,
        names: &[String],
        revision: u64,
        assets: &dyn Fn(&str) -> Result<Arc<[u8]>, String>,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<FieldBake, Vec<Diagnostic>> {
        let PreparedProject {
            project,
            scene,
            transforms,
            ..
        } = prepared;
        let touching = prepared.touching.get_or_init(|| ContactScene::new(scene));
        let fail = |message: String| vec![Diagnostic::message(&project.entry, message)];
        let assignment = project
            .materials
            .iter()
            .find(|assignment| assignment.meshes.iter().any(|id| id == mesh))
            .ok_or_else(|| fail(format!("Mesh {mesh} has no material.")))?;
        let source = project.modules.get(&assignment.entry).ok_or_else(|| {
            fail(format!(
                "Material {} uses missing module {}.",
                assignment.id, assignment.entry
            ))
        })?;
        let ids = BTreeSet::from([mesh.to_owned()]);
        let mut found = receivers(scene, &ids, transforms);
        let Some(mut receiver) = found.pop() else {
            return Err(fail(format!(
                "Mesh {mesh} is not a visible mesh of the project's models."
            )));
        };
        receiver.contacts = touching.contacts(scene, &receiver);
        let request = receiver_request(project, assignment, source, receiver);
        // The names by the stack's material they are fields of: 0 the slot's own, k its k-th layer.
        let mut groups: BTreeMap<usize, Vec<(usize, String)>> = BTreeMap::new();
        for (at, name) in names.iter().enumerate() {
            let (material, field) = match name
                .strip_prefix("stack.")
                .and_then(|rest| rest.split_once('/'))
                .and_then(|(k, field)| Some((k.parse::<usize>().ok()?, field)))
            {
                Some((k, field)) if k >= 1 => (k, field.to_owned()),
                _ => (0, name.clone()),
            };
            groups.entry(material).or_default().push((at, field));
        }
        if groups.len() == 1 && groups.contains_key(&0) {
            return self.bake_fields(&request, revision, names, assets, cancelled);
        }
        let mut fields: Vec<Vec<f32>> = vec![Vec::new(); names.len()];
        let (mut width, mut height) = (0, 0);
        for (material, wanted) in groups {
            let request = if material == 0 {
                request.clone()
            } else {
                let layer = assignment.layers.get(material - 1).ok_or_else(|| {
                    fail(format!(
                        "Material {} has no layer stack.{material}.",
                        assignment.id
                    ))
                })?;
                let text = project.modules.get(&layer.entry).ok_or_else(|| {
                    fail(format!(
                        "Material {} uses missing module {}.",
                        assignment.id, layer.entry
                    ))
                })?;
                let mut features = request.features.clone();
                if let Some(own) = features.get_mut(mesh) {
                    *own = crate::materialspec::program::followed_features(own, &layer.follow);
                }
                Request {
                    entry: layer.entry.clone(),
                    source: text.clone(),
                    parameters: layer.parameters.clone(),
                    seed: layer.seed,
                    layers: Vec::new(),
                    features,
                    ..request.clone()
                }
            };
            let field_names: Vec<String> = wanted.iter().map(|(_, field)| field.clone()).collect();
            let baked = self.bake_fields(&request, revision, &field_names, assets, cancelled)?;
            (width, height) = (baked.width, baked.height);
            for ((at, _), texels) in wanted.iter().zip(baked.fields) {
                fields[*at] = texels;
            }
        }
        Ok(FieldBake {
            width,
            height,
            fields,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::authoring::{Mapping, receiver};

    fn atlas(domain: [f64; 2], density: Option<f64>) -> Atlas {
        Atlas::new(
            receiver::swatch(domain),
            64,
            Mapping::Object,
            domain,
            [0., 0., -1.],
            512,
            density,
        )
        .unwrap()
    }

    #[test]
    fn anchored_face_resizes_crop_the_existing_pattern_block() {
        let density = 16.;
        let prepare = |low: f64, high: f64| {
            let mut wall = receiver::swatch([(high - low) / density, 1.]);
            for vertex in wall.positions.chunks_exact_mut(3) {
                vertex[0] += low / density;
            }
            let request = Request {
                resolution: 256,
                texel_density: Some(density),
                mapping: Mapping::Anchored,
                ..Default::default()
            };
            let atlas = Atlas::for_request(wall, &request).unwrap();
            assert_eq!(
                (atlas.width, atlas.height, atlas.density),
                (128, 20, density)
            );
            for (xyz, uv) in atlas.mesh.positions.iter().zip(&atlas.mesh.uvs) {
                assert!(
                    (xyz[0] - low / density).abs() < 1e-12
                        || (xyz[0] - high / density).abs() < 1e-12
                );
                // One more meter still covers exactly 16 more texels, including a subpixel crop.
                let origin = chart_surface(&atlas, &atlas.charts[0])[2] * density - 0.5;
                assert!(
                    (uv[0] * atlas.width as f64 - (xyz[0] * density - origin + GUTTER as f64))
                        .abs()
                        < 1e-10
                );
            }
            atlas
        };
        let original = prepare(-14.1, 63.2);
        let grown = prepare(-15.5, 63.9);
        let shrunk = prepare(-13.4, 62.2);
        for wall in [&original, &grown, &shrunk] {
            let chart = &wall.charts[0];
            assert_eq!(chart.width, 80 + 2 * GUTTER);
            for x in 0..chart.width {
                let (tri, u, v) = wall.texel(chart, x, GUTTER);
                let sample = wall.sample(tri, u, v).unwrap();
                // Check the unused reserve and both filter gutters against physical lattice coordinates.
                assert_eq!(
                    sample.surface[0],
                    (x as f64 - GUTTER as f64 - 16. + 0.5) / density
                );
                assert_eq!(sample.footprint, 1. / density);
            }
        }
        // Growth past cell 64 needs new space. Filling the entire next block speculatively
        // would exceed the work budget, so the chart takes its tight 81 cells.
        assert_eq!(prepare(-15.5, 64.1).charts[0].width, 81 + 2 * GUTTER);
        // Completing the block ending at cell 128 would exceed the existing texture.
        let tight = prepare(0., 121.5);
        assert_eq!(tight.charts[0].width, 122 + 2 * GUTTER);
    }

    #[test]
    fn seams_and_different_uv_gradients_keep_the_tight_lattice_chart() {
        let request = Request {
            resolution: 256,
            texel_density: Some(16.),
            mapping: Mapping::Anchored,
            ..Default::default()
        };
        let wall = receiver::swatch([4.8125, 1.]);
        let chart_width = |wall| {
            let atlas = Atlas::for_request(wall, &request).unwrap();
            assert_eq!(atlas.charts.len(), 1);
            atlas.charts[0].width
        };
        assert_eq!(chart_width(wall.clone()), 80 + 2 * GUTTER);
        // A continuous diagonal can join triangles with different rates of UV change.
        let mut bent = wall.clone();
        bent.uvs[6] = 0.5;
        assert_eq!(chart_width(bent), 77 + 2 * GUTTER);
        // Split vertices on that same diagonal and move the second island.
        let mut split = wall;
        split.positions.extend_from_within(0..3);
        split.positions.extend_from_within(6..9);
        split.normals.extend([0., -1., 0.].repeat(2));
        split.uvs.extend([1., 0., 2., 1.]);
        split.indices[3..].copy_from_slice(&[4, 5, 3]);
        assert_eq!(chart_width(split), 77 + 2 * GUTTER);
    }

    #[test]
    fn gpu_lattice_surface_matches_cpu_pixel_owners_with_varying_normals() {
        for smooth in [false, true] {
            let mut wall = receiver::swatch([1., 1.]);
            if smooth {
                // Split the diagonal's two vertices so the triangles can carry
                // independent normals and project the same point along different axes.
                wall.positions.extend_from_within(0..3);
                wall.positions.extend_from_within(6..9);
                wall.uvs.extend_from_within(0..2);
                wall.uvs.extend_from_within(4..6);
                wall.normals.truncate(9);
                wall.normals.extend([1., 0., 0.].repeat(3));
                wall.indices[3..].copy_from_slice(&[4, 5, 3]);
            }
            let request = Request {
                resolution: 64,
                texel_density: Some(32.),
                mapping: Mapping::Anchored,
                anchors: BTreeMap::from([(
                    wall.id.clone(),
                    crate::authoring::Anchor {
                        origin: [0.; 3],
                        u: [1., 0., 0.],
                        v: [0., 0., 1.],
                    },
                )]),
                ..Default::default()
            };
            let cpu = Atlas::for_request(wall.clone(), &request).unwrap();
            assert!(cpu.charts.iter().any(|c| !c.owners.is_empty()));
            let expected: Vec<_> = cpu
                .charts
                .iter()
                .flat_map(|chart| chart_surface(&cpu, chart))
                .collect();
            let unowned = Atlas::layout_for_request(wall, &request).unwrap();
            let first_triangle: Vec<_> = unowned
                .charts
                .iter()
                .flat_map(|chart| chart_surface(&unowned, chart))
                .collect();
            assert_eq!(first_triangle != expected, smooth);
            let gpu = ChartRecords::new(unowned, true);
            assert_eq!(gpu.surface, expected);
        }
    }

    /// A layer that follows `base` as its `follow` measures to the receiver's base by its own
    /// name; the receiver's other names stay, and a name the layer takes as its own is not also
    /// the receiver's.
    #[test]
    fn a_layer_sees_the_features_it_follows_by_its_own_names() {
        let feature = |name: &str| crate::materialspec::program::NamedFeature {
            name: name.into(),
            points: vec![[0., 0., 0.], [1., 0., 0.]],
            closed: false,
            zone: None,
        };
        let features = [feature("base"), feature("sill"), feature("follow")];
        use crate::materialspec::program::{Follow, followed_features};
        let follow = BTreeMap::from([("follow".to_owned(), Follow::from("base"))]);
        let names: Vec<String> = followed_features(&features, &follow)
            .into_iter()
            .map(|feature| feature.name)
            .collect();
        assert_eq!(names, ["base", "follow", "sill"]);
        assert_eq!(followed_features(&features, &BTreeMap::new()).len(), 3);
        // A list follows several: the layer's one name holds the polylines of all of them.
        let both = BTreeMap::from([(
            "follow".to_owned(),
            Follow::Many(vec!["sill".into(), "base".into()]),
        )]);
        let names: Vec<String> = followed_features(&features, &both)
            .into_iter()
            .map(|feature| feature.name)
            .collect();
        assert_eq!(names, ["base", "follow", "sill", "follow"]);
        // One name in a list is the name itself, and a list's order and repeats say nothing.
        assert_eq!(
            Follow::Many(vec!["base".into()]).normalized(),
            Follow::from("base")
        );
        assert_eq!(
            Follow::Many(vec!["sill".into(), "base".into(), "sill".into()]).normalized(),
            Follow::Many(vec!["base".into(), "sill".into()])
        );
        assert_eq!(
            serde_json::to_string(&follow).unwrap(),
            r#"{"follow":"base"}"#
        );
    }

    /// Reads a record the way a renderer does and checks it against the atlas's own sampling.
    #[test]
    fn records_reproduce_the_atlas_texels() {
        for (domain, density) in [
            ([1., 1.], None),
            ([2., 0.5], None),
            ([0.25, 0.25], Some(64.)),
        ] {
            let atlas = atlas(domain, density);
            let mut records = Vec::new();
            write_records(&atlas, &mut records);
            assert_eq!(records.len(), 2 * LAYOUT_RECORD);
            for chart in &atlas.charts {
                for y in 0..chart.height {
                    for x in 0..chart.width {
                        let (triangle, u, v) = atlas.texel(chart, x, y);
                        let expected = atlas.sample(triangle, u, v).unwrap();
                        let record = &records[triangle * LAYOUT_RECORD..][..LAYOUT_RECORD];
                        assert_eq!(
                            &record[..4],
                            &[chart.x, chart.y, chart.width, chart.height].map(|n| n as f32)
                        );
                        // The renderer's texel-to-leg mapping (see atlas.ts), independent of `Atlas::texel`.
                        let inner = [record[2] - 4., record[3] - 4.];
                        let place = |texel: usize, axis: usize| {
                            let at = ((texel as f32 + 0.5 - 2.) / inner[axis]).clamp(0., 1.);
                            let count = record[5 + axis];
                            if count < inner[axis] {
                                ((at * count).floor().min(count - 1.) + 0.5) / count
                            } else {
                                at
                            }
                        };
                        let (s, t) = (place(x, 0), place(y, 1));
                        let turn = record[4] as u8;
                        let (mut a, mut b) = if turn & 1 == 1 { (t, 1. - s) } else { (s, t) };
                        // A texel on the diagonal belongs to either triangle of the pair.
                        let second = a + b > 1.;
                        if (a + b - 1.).abs() > 1e-5 {
                            assert_eq!(second, record[7] == 2., "role at {x},{y}");
                        }
                        if turn >= 2 {
                            (a, b) = (1. - a, 1. - b);
                        }
                        for axis in 0..3 {
                            let world = record[8 + axis] * (1. - a - b)
                                + record[11 + axis] * a
                                + record[14 + axis] * b;
                            assert!(
                                (f64::from(world) - expected.world[axis]).abs() < 1e-4,
                                "world at {x},{y}"
                            );
                        }
                    }
                }
            }
            // Atlas UVs are the baked mesh's own.
            for (triangle, uvs) in atlas.mesh.uvs.chunks_exact(3).enumerate() {
                let record = &records[triangle * LAYOUT_RECORD..][..LAYOUT_RECORD];
                for (corner, uv) in uvs.iter().enumerate() {
                    assert_eq!(record[32 + corner * 2], uv[0] as f32);
                    assert_eq!(record[33 + corner * 2], uv[1] as f32);
                }
            }
        }
    }
}

//! Immutable raster jobs, bounded execution and atomic final drawing binding.
use super::{
    bake::BakedMaterial,
    facade_material::{self, Facade},
    frosted::{self, Frosted},
    mesh::{Mesh, RasterBinding},
    metal::{self, Metal},
    painted::{self, Paint},
    surface::{Inputs, Layout, RasterSettings, rasterize},
    weathered::raster_settings,
};
#[derive(Clone, serde::Serialize)]
pub enum Recipe {
    Metal {
        settings: Box<metal::Settings>,
        runoff: Option<super::pipe_runoff::Recipe>,
    },
    Paint(painted::Recipe),
    Frosted(frosted::Recipe),
    Facade(facade_material::Settings),
    FacadeDeposits(super::skin::Deposits),
    Tile(super::tile::Recipe),
    TiledSkin(Box<super::tiled_skin::Recipe>),
    Sign(Box<super::signs::weathering::Recipe>),
    Stained(Box<super::stains::Recipe>),
    Pavement(Box<super::pavement::raster::Recipe>),
}
pub struct Job {
    pub recipe: Recipe,
    pub density: f64,
}
pub(crate) type CachedBatch = crate::runtime::batch::Batch<BakedMaterial>;
impl Job {
    fn cache_key(&self, inputs: &[u8]) -> Result<Vec<u8>, String> {
        let mut key = b"levelspec-material-bake-v3".to_vec();
        key.extend_from_slice(&(inputs.len() as u64).to_le_bytes());
        key.extend_from_slice(inputs);
        key.extend(serde_json::to_vec(&(self.density, &self.recipe)).map_err(|e| e.to_string())?);
        Ok(key)
    }

    fn conforms_edges(&self) -> bool {
        match &self.recipe {
            Recipe::Sign(_) => true,
            Recipe::Stained(recipe) => recipe.conforms_edges(),
            _ => false,
        }
    }
    fn validate_geometry_sources(
        &self,
        materials: &std::collections::BTreeMap<String, superspec_aml::material::Material>,
    ) -> Result<(), String> {
        if self.conforms_edges()
            && self.structure()?.layers.iter().any(|layer| {
                materials
                    .get(&layer.material)
                    .is_some_and(|m| m.height_texture.is_some())
            })
        {
            return Err("Height-bearing sign bases require geometric refinement, which is not implemented yet.".into());
        }
        Ok(())
    }
    pub fn metal_settings(&self) -> Option<&metal::Settings> {
        match &self.recipe {
            Recipe::Metal { settings, .. } => Some(settings),
            _ => None,
        }
    }
    pub fn runoff_recipe(&self) -> Option<&super::pipe_runoff::Recipe> {
        match &self.recipe {
            Recipe::Metal { runoff, .. } => runoff.as_ref(),
            _ => None,
        }
    }
    pub fn bounds(&self) -> [f64; 4] {
        match &self.recipe {
            Recipe::Metal { settings, .. } => settings.bounds,
            Recipe::Paint(recipe) => recipe.finish.bounds,
            Recipe::Frosted(recipe) => recipe.bounds,
            Recipe::Facade(_) => [0., 0., facade_material::PERIOD, facade_material::PERIOD],
            Recipe::FacadeDeposits(recipe) => recipe.bounds,
            Recipe::Tile(recipe) => recipe.bounds,
            Recipe::TiledSkin(recipe) => recipe.bounds,
            Recipe::Sign(recipe) => [0., 0., recipe.width, recipe.height],
            Recipe::Stained(recipe) => recipe.bounds(),
            Recipe::Pavement(recipe) => recipe.bounds,
        }
    }
    pub fn source(&self) -> &str {
        match &self.recipe {
            Recipe::Metal { settings, .. } => &settings.substrate.material,
            Recipe::Paint(recipe) => &recipe.binding.material,
            Recipe::Frosted(recipe) => &recipe.material,
            Recipe::Facade(recipe) => &recipe.coat.material,
            Recipe::FacadeDeposits(recipe) => &recipe.finish.coat.material,
            Recipe::Tile(recipe) => &recipe.materials.bed,
            Recipe::TiledSkin(recipe) => &recipe.finish.coat.material,
            Recipe::Sign(recipe) => &recipe.paints[0].slot,
            Recipe::Stained(recipe) => recipe.source(),
            Recipe::Pavement(recipe) => recipe.source(),
        }
    }
    pub(super) fn validate(&self) -> Result<(), String> {
        match &self.recipe {
            Recipe::Pavement(recipe) => {
                super::pavement::raster::Field::new(recipe, &|| false)?;
                if recipe.density != self.density {
                    return Err("Ground field and raster densities must agree.".into());
                }
            }
            Recipe::Sign(recipe) => {
                super::signs::weathering::Field::new(recipe)?;
            }
            Recipe::Stained(recipe) => {
                super::stains::Field::new(recipe)?;
            }
            Recipe::Metal { settings, runoff } => {
                Metal::new(settings, None)?;
                if let Some(recipe) = runoff {
                    recipe.working_bytes()?;
                }
            }
            Recipe::Paint(recipe) => {
                recipe.validate()?;
            }
            Recipe::Frosted(recipe) => {
                Frosted::new(recipe)?;
            }
            Recipe::Facade(recipe) => {
                Facade::new(recipe)?;
                if recipe.density != self.density {
                    return Err("Facade field and raster densities must agree.".into());
                }
            }
            Recipe::FacadeDeposits(recipe) => {
                super::skin::Deposited::new(recipe)?;
                if self.density != recipe.finish.density.min(128.) {
                    return Err("Facade deposits require the capped intrinsic density.".into());
                }
            }
            Recipe::Tile(recipe) => {
                super::tile::Field::new(recipe)?;
                if self.density != recipe.density {
                    return Err("Ceramic field and raster density must agree.".into());
                }
            }
            Recipe::TiledSkin(recipe) => {
                super::tiled_skin::Field::new(recipe)?;
                if self.density != recipe.finish.density {
                    return Err("Tiled wall field and raster densities must agree.".into());
                }
            }
        }
        Layout::new(self.raster_settings())?;
        Ok(())
    }
    pub(super) fn raster_settings(&self) -> RasterSettings {
        let mut settings = raster_settings(self.bounds(), self.density);
        settings.repeat = matches!(self.recipe, Recipe::Facade(_));
        if matches!(
            self.recipe,
            Recipe::Facade(_)
                | Recipe::FacadeDeposits(_)
                | Recipe::Tile(_)
                | Recipe::TiledSkin(_)
                | Recipe::Sign(_)
                | Recipe::Stained(_)
                | Recipe::Pavement(_)
        ) {
            settings.uv_coordinates = false;
        }
        settings
    }
    fn structure(&self) -> Result<super::surface::Sample, String> {
        let zero = |_, _| Ok(0.);
        Ok(match &self.recipe {
            Recipe::Metal { settings, runoff } => Metal::new(
                settings,
                runoff
                    .as_ref()
                    .map(|_| &zero as &dyn Fn(f64, f64) -> Result<f64, String>),
            )?
            .structure(),
            Recipe::Paint(recipe) => Paint::new(recipe)?.structure(),
            Recipe::Frosted(recipe) => Frosted::new(recipe)?.structure(),
            Recipe::Facade(recipe) => Facade::new(recipe)?.structure(),
            Recipe::FacadeDeposits(recipe) => super::skin::Deposited::new(recipe)?.structure(),
            Recipe::Tile(recipe) => super::tile::Field::new(recipe)?.structure(),
            Recipe::TiledSkin(recipe) => super::tiled_skin::Field::new(recipe)?.structure(),
            Recipe::Sign(recipe) => super::signs::weathering::Field::new(recipe)?.structure(),
            Recipe::Stained(recipe) => super::stains::Field::new(recipe)?.structure(),
            Recipe::Pavement(recipe) => {
                super::pavement::raster::Field::new(recipe, &|| false)?.structure()?
            }
        })
    }
    fn required_materials(&self) -> Result<std::collections::BTreeSet<String>, String> {
        let mut required = std::collections::BTreeSet::from([self.source().to_owned()]);
        for layer in self.structure()?.layers {
            required.insert(layer.material);
            required.extend(layer.coatings.into_iter().map(|coat| coat.material));
        }
        Ok(required)
    }
    /// Conservative owned raster/row allowance; immutable source images are shared separately.
    fn raster_budget(&self) -> Result<usize, String> {
        self.validate()?;
        let layout = Layout::new(self.raster_settings())?;
        let structure = self.structure()?;
        let scratch =
            (layout.width + 2) * 3 * (super::surface::sample_storage(&structure)? * 2 + 8);
        let field_storage = match &self.recipe {
            // Includes the admitted condition snapshot, bounded repair campaigns and nine tile-damage blobs.
            Recipe::TiledSkin(recipe) => recipe.owned_bytes() + 1024 * 1024,
            Recipe::Stained(recipe) => recipe.owned_bytes() + 1024 * 1024,
            Recipe::Pavement(recipe) => recipe.owned_bytes() + 8 * 1024 * 1024,
            Recipe::Tile(_) => 8192,
            Recipe::Sign(recipe) => {
                recipe.owned_bytes() + recipe.paints.len() * std::mem::size_of::<usize>()
            }
            Recipe::FacadeDeposits(recipe) => {
                recipe.condition.ledges.len() * std::mem::size_of::<super::wall::Ledge>()
                    + recipe.condition.openings.len() * std::mem::size_of::<super::wall::Opening>()
            }
            _ => 0,
        };
        let bytes = layout.width * layout.height * 12 + scratch + field_storage + 1024;
        if bytes > 64 * 1024 * 1024 {
            return Err("Raster job exceeds its 64 MiB raster allowance.".into());
        }
        Ok(bytes)
    }
    #[cfg(any(not(target_arch = "wasm32"), feature = "wasm-threads"))]
    fn working_bytes(&self) -> Result<usize, String> {
        let layout = Layout::new(self.raster_settings())?;
        Ok(self.raster_budget()?
            + superspec_geometry::image::encoding_working_bytes(layout.width, layout.height)?
            + self
                .runoff_recipe()
                .map_or(Ok(0), super::pipe_runoff::Recipe::working_bytes)?)
    }
    fn bake(
        &self,
        index: usize,
        inputs: &Inputs<'_>,
        id: &str,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<BakedMaterial, String> {
        let budget = self.raster_budget()?;
        let runoff = self
            .runoff_recipe()
            .map(|r| r.build(cancelled))
            .transpose()?;
        let runoff_sample = |u, v| runoff.as_ref().unwrap().sample(u, v);
        let settings = self.raster_settings();
        let raster = match &self.recipe {
            Recipe::Pavement(recipe) => {
                let field = super::pavement::raster::Field::new(recipe, cancelled)?;
                rasterize(
                    settings,
                    &|u, v| field.sample(u, v),
                    inputs,
                    budget,
                    cancelled,
                )?
            }
            Recipe::Stained(recipe) => {
                let field = super::stains::Field::new(recipe)?;
                rasterize(
                    settings,
                    &|u, v| field.sample(u, v),
                    inputs,
                    budget,
                    cancelled,
                )?
            }
            Recipe::Sign(recipe) => {
                let field = super::signs::weathering::Field::new(recipe)?;
                rasterize(
                    settings,
                    &|u, v| field.sample(u, v),
                    inputs,
                    budget,
                    cancelled,
                )?
            }
            Recipe::Metal {
                settings: metal_settings,
                ..
            } => {
                let field = Metal::new(
                    metal_settings,
                    runoff
                        .as_ref()
                        .map(|_| &runoff_sample as &dyn Fn(f64, f64) -> Result<f64, String>),
                )?;
                rasterize(
                    settings,
                    &|u, v| field.sample(u, v),
                    inputs,
                    budget,
                    cancelled,
                )?
            }
            Recipe::Paint(recipe) => {
                let field = Paint::new(recipe)?;
                rasterize(
                    settings,
                    &|u, v| field.sample(u, v),
                    inputs,
                    budget,
                    cancelled,
                )?
            }
            Recipe::Frosted(recipe) => {
                let field = Frosted::new(recipe)?;
                rasterize(
                    settings,
                    &|u, v| field.sample(u, v),
                    inputs,
                    budget,
                    cancelled,
                )?
            }
            Recipe::Facade(recipe) => {
                let field = Facade::new(recipe)?;
                rasterize(
                    settings,
                    &|u, v| field.sample(u, v),
                    inputs,
                    budget,
                    cancelled,
                )?
            }
            Recipe::FacadeDeposits(recipe) => {
                let field = super::skin::Deposited::new(recipe)?;
                rasterize(
                    settings,
                    &|u, v| field.sample(u, v),
                    inputs,
                    budget,
                    cancelled,
                )?
            }
            Recipe::Tile(recipe) => {
                let field = super::tile::Field::new(recipe)?;
                rasterize(
                    settings,
                    &|u, v| field.sample(u, v),
                    inputs,
                    budget,
                    cancelled,
                )?
            }
            Recipe::TiledSkin(recipe) => {
                let field = super::tiled_skin::Field::new(recipe)?;
                rasterize(
                    settings,
                    &|u, v| field.sample(u, v),
                    inputs,
                    budget,
                    cancelled,
                )?
            }
        };
        let source = inputs
            .materials
            .get(self.source())
            .ok_or("Missing raster source material.")?;
        BakedMaterial::encode(&format!("{id}/{index}"), raster, source, cancelled)
    }
}

/// Replaces a flat face's appearance and stores local-meter chart coordinates for final binding.
pub(super) fn face(
    drawing: &mut super::fixture::Fixture<'_, '_>,
    slot: &str,
    points: &[superspec_geometry::V3],
    job: Job,
    group: String,
) -> Result<(), String> {
    face_with_fallback(drawing, slot, points, job, group, None)
}
pub(super) fn face_with_fallback(
    drawing: &mut super::fixture::Fixture<'_, '_>,
    slot: &str,
    points: &[superspec_geometry::V3],
    job: Job,
    group: String,
    fallback: Option<super::skin::Deposits>,
) -> Result<(), String> {
    use super::{mesh::rounded, pen::Tint};
    if drawing.mesh.surveying()
        || points.len() < 3
        || points.len() > 4096
        || points
            .iter()
            .flatten()
            .any(|v| !v.is_finite() || v.abs() > 10000.)
        || group.len() > 4096
        || slot.is_empty()
        || slot.len() > 4096
    {
        return Err("Raster face requires bounded render geometry and identity.".into());
    }
    job.validate()?;
    let first = drawing.mesh.triangle_count();
    let inverse = drawing.pen.matrix.inverse()?;
    let uv: Vec<_> = points
        .iter()
        .map(|p| {
            let local = inverse.point(drawing.pen.world(*p).map(|v| rounded(v, 1e5)));
            [local[0], local[2]]
        })
        .collect();
    drawing.pen.face_raw(
        drawing.mesh,
        slot,
        points,
        [0., 1., 0.],
        &Tint::Color([1.; 3]),
        1.,
        Some(&uv),
        None,
    )?;
    let spatial = (!job.raster_settings().uv_coordinates).then(|| super::stains::Receiver {
        frame: super::stains::Frame::pen(&drawing.pen, points[0][1]),
        fallback,
    });
    super::weathered::receive_surface(drawing.mesh, first, job, group, spatial)
}

/// Native coarse material-job execution. Source images and retained encoded results are separate budgets.
#[cfg(not(target_arch = "wasm32"))]
pub struct ParallelBaking {
    pub workers: usize,
    pub working_bytes: usize,
}
#[cfg(not(target_arch = "wasm32"))]
impl Default for ParallelBaking {
    fn default() -> Self {
        Self {
            workers: 4,
            working_bytes: 128 * 1024 * 1024,
        }
    }
}
pub struct ReceiverRange {
    pub first_triangle: usize,
    pub triangle_count: usize,
    pub job: usize,
}
/// Immutable jobs in first-use order; all contacts/folds and material identities participate in sharing.
pub struct Plan<'mesh, 'cancel> {
    pub(super) jobs: Vec<Job>,
    pub(super) receivers: Vec<ReceiverRange>,
    pub(super) mesh: &'mesh mut Mesh<'cancel>,
    pub(super) edge_vertices: std::sync::Arc<[superspec_geometry::V3]>,
    pub(super) edge_index: Option<std::sync::Arc<super::conform::Edges>>,
}
impl Plan<'_, '_> {
    /// Includes raw geometry, recipes, material inputs and every potentially incident external edge.
    fn binding_key(&self, bakes: &[BakedMaterial], prefix: &[u8]) -> Result<[u8; 32], String> {
        use sha2::{Digest, Sha256};
        let mut hash = Sha256::new();
        hash.update(b"levelspec-bound-geometry-v1");
        hash.update((prefix.len() as u64).to_le_bytes());
        hash.update(prefix);
        self.mesh.binding_hash(&mut hash);
        hash.update((self.jobs.len() as u64).to_le_bytes());
        for job in &self.jobs {
            hash.update(
                serde_json::to_vec(&(job.density, &job.recipe)).map_err(|e| e.to_string())?,
            );
        }
        hash.update((self.receivers.len() as u64).to_le_bytes());
        for receiver in &self.receivers {
            for value in [
                receiver.first_triangle,
                receiver.triangle_count,
                receiver.job,
            ] {
                hash.update((value as u64).to_le_bytes());
            }
        }
        for bake in bakes {
            hash.update(
                serde_json::to_vec(&(bake.material().id.as_str(), bake.layout()))
                    .map_err(|e| e.to_string())?,
            );
        }
        let mut bounds = [[f64::INFINITY; 3], [f64::NEG_INFINITY; 3]];
        for vertex in &self.mesh.vertices {
            for i in 0..3 {
                bounds[0][i] = bounds[0][i].min(vertex.position[i]);
                bounds[1][i] = bounds[1][i].max(vertex.position[i]);
            }
        }
        hash.update((self.edge_vertices.len() as u64).to_le_bytes());
        for point in self.edge_vertices.iter() {
            for value in point {
                hash.update(value.to_bits().to_le_bytes());
            }
        }
        hash.update([u8::from(self.edge_index.is_some())]);
        if let Some(edges) = &self.edge_index {
            edges.binding_hash(&mut hash, bounds);
        }
        Ok(hash.finalize().into())
    }

    pub(crate) fn bind_cached(
        mut self,
        bakes: Vec<BakedMaterial>,
        prefix: &[u8],
        cache: &std::sync::Mutex<super::cache::Cache>,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Vec<BakedMaterial>, String> {
        let key = self.binding_key(&bakes, prefix)?;
        if cancelled() {
            return Err("Geometry binding cancelled.".into());
        }
        let bound = cache.lock().expect("bake cache").binding(&key);
        if let Some(bound) = bound {
            self.mesh.restore_binding(&bound)?;
            return Ok(bakes);
        }
        self.bind_geometry(&bakes, cancelled)?;
        cache
            .lock()
            .expect("bake cache")
            .insert_binding(key, self.mesh.binding_snapshot());
        Ok(bakes)
    }
    /// A fully warm host does not need to decode source images again.
    pub fn cached_bakes(
        &self,
        id: &str,
        prefix: &[u8],
        cache: &std::sync::Mutex<super::cache::Cache>,
    ) -> Result<Option<Vec<BakedMaterial>>, String> {
        if id.is_empty() || id.len() > 1024 {
            return Err("Raster bake requires a bounded identity.".into());
        }
        let mut bakes = Vec::with_capacity(self.jobs.len());
        for (i, job) in self.jobs.iter().enumerate() {
            let key = job.cache_key(prefix)?;
            let Some(bake) = cache.lock().expect("bake cache").get(&key) else {
                return Ok(None);
            };
            bakes.push(bake.renamed(&format!("{id}/{i}")));
        }
        Ok(Some(bakes))
    }
    pub fn bind_completed(
        self,
        bakes: Vec<BakedMaterial>,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Vec<BakedMaterial>, String> {
        self.bind(bakes, cancelled)
    }
    /// Raster identity excludes placement; binding identity retains current geometry and edge inputs.
    pub fn bake_cached(
        self,
        inputs: &Inputs<'_>,
        id: &str,
        prefix: &[u8],
        cache: &std::sync::Mutex<super::cache::Cache>,
        cancelled: &(dyn Fn() -> bool + Sync),
    ) -> Result<Vec<BakedMaterial>, String> {
        self.bake_cached_with_workers(inputs, id, prefix, cache, 4, cancelled)
    }
    pub(crate) fn bake_cached_with_workers(
        self,
        inputs: &Inputs<'_>,
        id: &str,
        prefix: &[u8],
        cache: &std::sync::Mutex<super::cache::Cache>,
        workers: usize,
        cancelled: &(dyn Fn() -> bool + Sync),
    ) -> Result<Vec<BakedMaterial>, String> {
        self.validate_materials(inputs.materials)?;
        let batch = self.prepare_cached(id, |_| prefix, cache, cancelled)?;
        self.bake_prepared(inputs, id, prefix, batch, cache, workers, cancelled)
    }
    pub(crate) fn prepare_cached<'a>(
        &self,
        id: &str,
        input_key: impl Fn(usize) -> &'a [u8],
        cache: &std::sync::Mutex<super::cache::Cache>,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<CachedBatch, String> {
        if id.is_empty() || id.len() > 1024 {
            return Err("Raster bake requires a bounded identity.".into());
        }
        let mut keys = Vec::with_capacity(self.jobs.len());
        for (i, job) in self.jobs.iter().enumerate() {
            if cancelled() {
                return Err("Raster baking cancelled.".into());
            }
            keys.push(job.cache_key(input_key(i))?);
        }
        CachedBatch::prepare(
            keys,
            |i, key| {
                cache
                    .lock()
                    .expect("bake cache")
                    .get(key)
                    .map(|b| b.renamed(&format!("{id}/{i}")))
            },
            cancelled,
            "Raster baking cancelled.",
        )
    }
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn bake_prepared(
        self,
        inputs: &Inputs<'_>,
        id: &str,
        binding_prefix: &[u8],
        batch: CachedBatch,
        cache: &std::sync::Mutex<super::cache::Cache>,
        workers: usize,
        cancelled: &(dyn Fn() -> bool + Sync),
    ) -> Result<Vec<BakedMaterial>, String> {
        #[cfg(all(target_arch = "wasm32", not(feature = "wasm-threads")))]
        let _ = workers;
        let CachedBatch {
            mut keys,
            mut ordered,
            missing,
            shared,
        } = batch;
        if missing.is_empty() {
            let bakes = ordered
                .into_iter()
                .map(|b| b.ok_or_else(|| "Raster cache omitted a job.".to_string()))
                .collect::<Result<Vec<_>, _>>()?;
            return self.bind_cached(bakes, binding_prefix, cache, cancelled);
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let workers = self
                .worker_count(&ParallelBaking {
                    workers,
                    ..Default::default()
                })?
                .min(missing.len());
            let results = crate::runtime::dispatch::selected(&missing, workers, &|i| {
                self.jobs[i].bake(i, inputs, id, cancelled)
            });
            for result in results {
                for (i, bake) in result.map_err(|_| "Native raster worker panicked.")? {
                    ordered[i] = Some(bake?);
                }
            }
        }
        #[cfg(all(target_arch = "wasm32", not(feature = "wasm-threads")))]
        for &i in &missing {
            ordered[i] = Some(self.jobs[i].bake(i, inputs, id, cancelled)?);
        }
        #[cfg(all(target_arch = "wasm32", feature = "wasm-threads"))]
        {
            let maximum = self
                .jobs
                .iter()
                .map(Job::working_bytes)
                .collect::<Result<Vec<_>, _>>()?
                .into_iter()
                .max()
                .unwrap_or(1);
            let allowance = 128 * 1024 * 1024;
            if maximum > allowance {
                return Err("Raster job exceeds the shared-memory working allowance.".into());
            }
            let lanes = workers.min(allowance / maximum).min(missing.len());
            let results = crate::runtime::dispatch::lanes(&missing, lanes, &|i| {
                self.jobs[i].bake(i, inputs, id, cancelled)
            });
            for lane in results {
                for (i, bake) in lane {
                    ordered[i] = Some(bake?);
                }
            }
        }
        for &(i, representative) in &shared {
            ordered[i] = Some(
                ordered[representative]
                    .as_ref()
                    .ok_or("Raster worker omitted a shared job.")?
                    .renamed(&format!("{id}/{i}")),
            );
        }
        let bakes = ordered
            .into_iter()
            .map(|b| b.ok_or_else(|| "Raster worker omitted a job.".to_string()))
            .collect::<Result<Vec<_>, _>>()?;
        let bakes = self.bind_cached(bakes, binding_prefix, cache, cancelled)?;
        cache.lock().expect("bake cache").record_baking(
            missing.len(),
            missing.iter().map(|&i| bakes[i].pixels).sum(),
            shared.len(),
        );
        for i in missing {
            cache
                .lock()
                .expect("bake cache")
                .insert(std::mem::take(&mut keys[i]), &bakes[i]);
        }
        Ok(bakes)
    }
    /// Rejects unsupported displacement before any source image decoding or raster work.
    pub fn validate_materials(
        &self,
        materials: &std::collections::BTreeMap<String, superspec_aml::material::Material>,
    ) -> Result<(), String> {
        for job in &self.jobs {
            job.validate_geometry_sources(materials)?;
        }
        Ok(())
    }
    /// Immutable positions outside this detached host also participate in receiver adjacency.
    pub fn with_edge_vertices(
        mut self,
        vertices: std::sync::Arc<[superspec_geometry::V3]>,
    ) -> Self {
        self.edge_vertices = vertices;
        self.edge_index = None;
        self
    }
    pub(crate) fn with_edge_index(mut self, index: std::sync::Arc<super::conform::Edges>) -> Self {
        self.edge_vertices = std::sync::Arc::from([]);
        self.edge_index = Some(index);
        self
    }
    /// Every layer and optional coating participates, including those with zero coverage at the chart origin.
    pub fn required_materials(&self) -> Result<std::collections::BTreeSet<String>, String> {
        Ok(self
            .material_dependencies()?
            .into_iter()
            .flatten()
            .collect())
    }
    pub(crate) fn material_dependencies(
        &self,
    ) -> Result<Vec<std::collections::BTreeSet<String>>, String> {
        self.jobs.iter().map(Job::required_materials).collect()
    }
    pub fn jobs(&self) -> &[Job] {
        &self.jobs
    }
    pub fn receivers(&self) -> &[ReceiverRange] {
        &self.receivers
    }
    /// Bake all jobs before changing the drawing. Returned material IDs are also the new drawing slots.
    pub fn bake(
        self,
        inputs: &Inputs<'_>,
        id: &str,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Vec<BakedMaterial>, String> {
        if id.len() > 1024 || id.is_empty() {
            return Err("Raster plan requires its unchanged drawing and bounded identity.".into());
        }
        let mut bakes = Vec::new();
        for (i, job) in self.jobs.iter().enumerate() {
            job.validate_geometry_sources(inputs.materials)?;
            bakes.push(job.bake(i, inputs, id, cancelled)?);
        }
        self.bind(bakes, cancelled)
    }
    /// Reports the CPU/memory-capped worker count before starting native threads.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn worker_count(&self, options: &ParallelBaking) -> Result<usize, String> {
        if options.workers == 0
            || options.workers > 16
            || options.working_bytes == 0
            || options.working_bytes > 256 * 1024 * 1024
        {
            return Err(
                "Native baking requires 1–16 workers and at most 256 MiB working allowance.".into(),
            );
        }
        let maximum = self
            .jobs
            .iter()
            .map(Job::working_bytes)
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .max()
            .unwrap_or(1);
        if maximum > options.working_bytes {
            return Err("Raster job exceeds the native working allowance.".into());
        }
        let cpus = std::thread::available_parallelism().map_or(1, usize::from);
        Ok(options
            .workers
            .min(cpus)
            .min(self.jobs.len().max(1))
            .min(options.working_bytes / maximum))
    }
    /// Executes independent, immutable recipes on bounded native workers, then binds in source order.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn bake_parallel(
        self,
        inputs: &Inputs<'_>,
        id: &str,
        options: &ParallelBaking,
        cancelled: &(dyn Fn() -> bool + Sync),
    ) -> Result<Vec<BakedMaterial>, String> {
        if id.is_empty() || id.len() > 1024 {
            return Err("Raster bake requires a bounded identity.".into());
        }
        let workers = self.worker_count(options)?;
        for job in &self.jobs {
            job.validate_geometry_sources(inputs.materials)?;
        }
        if workers == 1 {
            return self.bake(inputs, id, cancelled);
        }
        let (results, start_error) =
            crate::runtime::dispatch::named(self.jobs.len(), workers, "raster", &|i| {
                self.jobs[i].bake(i, inputs, id, cancelled)
            });
        if let Some(error) = start_error {
            return Err(format!("Cannot start native raster worker: {error}"));
        }
        let mut ordered: Vec<_> = (0..self.jobs.len()).map(|_| None).collect();
        for result in results {
            for (i, value) in result.map_err(|_| "Native raster worker panicked.")? {
                ordered[i] = Some(value);
            }
        }
        let bakes = ordered
            .into_iter()
            .map(|v| v.ok_or("Native raster worker omitted a job.")?)
            .collect::<Result<Vec<_>, String>>()?;
        self.bind(bakes, cancelled)
    }
    fn bind(
        mut self,
        bakes: Vec<BakedMaterial>,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Vec<BakedMaterial>, String> {
        self.bind_geometry(&bakes, cancelled)?;
        Ok(bakes)
    }

    fn bind_geometry(
        &mut self,
        bakes: &[BakedMaterial],
        cancelled: &dyn Fn() -> bool,
    ) -> Result<(), String> {
        let bytes = bakes
            .iter()
            .flat_map(|b| &b.images)
            .map(|(_, bytes)| bytes.len())
            .sum::<usize>();
        if bytes > 128 * 1024 * 1024 {
            return Err("Raster bake exceeds encoded image budget.".into());
        }
        let bindings: Vec<_> = self
            .receivers
            .iter()
            .map(|r| RasterBinding {
                first_triangle: r.first_triangle,
                triangle_count: r.triangle_count,
                slot: &bakes[r.job].material().id,
                layout: bakes[r.job].layout(),
            })
            .collect();
        if cancelled() {
            return Err("Raster binding cancelled.".into());
        }
        let selected: Vec<_> = self
            .receivers
            .iter()
            .enumerate()
            .filter_map(|(i, r)| self.jobs[r.job].conforms_edges().then_some(i))
            .collect();
        let patch = super::conform::Patch::prepare(
            self.mesh,
            &bindings,
            &selected,
            &self.edge_vertices,
            self.edge_index.clone(),
            &mut superspec_geometry::obstructions::Budget::new(128 * 1024 * 1024, cancelled)?,
        )?;
        self.mesh.bind_rasters(&bindings)?;
        if let Some(patch) = patch {
            patch.commit(self.mesh);
        }
        Ok(())
    }
}

#[cfg(test)]
mod cache_tests;

//! Product entry points explicitly compose the shipped source profile and theme.
use super::{build, themes::kowloon::cache::Cache};
use crate::{Scene, Session, architecture::kowloon::Kowloon};
use std::sync::Mutex;
use superspec_types::diagnostic::Diagnostic;

pub(crate) use super::themes::kowloon::city::stock_images;

/// The shipped theme's fixtures as ItemSpec native generators (`kowloon/...`).
pub fn item_generators() -> std::sync::Arc<dyn crate::itemspec::definition::NativeGenerators> {
    super::themes::kowloon::items::natives()
}

/// Compiles the architectural model with the shipped profile and portable kernel.
pub fn compile(source: &str, revision: u32) -> Result<Scene, Diagnostic> {
    compile_with_assets(source, revision, &|path| {
        Err(format!("Asset {path} was not supplied."))
    })
}

/// Asset paths remain authored identities. Host adapters supply bounded bytes explicitly.
pub fn compile_with_assets(
    source: &str,
    revision: u32,
    assets: &dyn Fn(&str) -> Result<Vec<u8>, String>,
) -> Result<Scene, Diagnostic> {
    compile_target(
        source,
        revision,
        assets,
        true,
        &crate::assets::image_admission::validate_image,
        None,
        &|| false,
        false,
        &|_| {},
        4,
    )
}

/// Uses a host-selected source profile with the shipped physical and visual adapter.
pub fn compile_with_registry(
    source: &str,
    revision: u32,
    assets: &dyn Fn(&str) -> Result<Vec<u8>, String>,
    registry: &superspec_aml::extensions::Registry,
) -> Result<Scene, Diagnostic> {
    let prepared = crate::source::prepare(source, registry)?;
    let mut theme = Kowloon::new(None, 4, &|_| {});
    build::compile_prepared(
        prepared,
        revision,
        assets,
        true,
        &crate::assets::image_admission::validate_image,
        &|| false,
        false,
        &|_| {},
        &mut theme,
    )
}

/// Compiles collision and source findings without loading visual material images.
/// Geometric dependencies such as terrain heightmaps remain required.
pub fn compile_collision_with_assets(
    source: &str,
    revision: u32,
    assets: &dyn Fn(&str) -> Result<Vec<u8>, String>,
) -> Result<Scene, Diagnostic> {
    compile_target(
        source,
        revision,
        assets,
        false,
        &crate::assets::image_admission::validate_image,
        None,
        &|| false,
        false,
        &|_| {},
        4,
    )
}

#[allow(clippy::too_many_arguments)]
fn compile_target(
    source: &str,
    revision: u32,
    assets: &dyn Fn(&str) -> Result<Vec<u8>, String>,
    visual: bool,
    validate_image: &dyn Fn(&[u8]) -> Result<crate::assets::image_admission::ImageInfo, String>,
    cache: Option<&Mutex<Cache>>,
    cancelled: &(dyn Fn() -> bool + Sync),
    retain_solids: bool,
    observe: &dyn Fn(&str),
    workers: usize,
) -> Result<Scene, Diagnostic> {
    observe("parse-resolve");
    let prepared = crate::source::prepare(source, superspec_profiles::default_registry())?;
    let mut theme = Kowloon::new(cache, workers, observe);
    build::compile_prepared(
        prepared,
        revision,
        assets,
        visual,
        validate_image,
        cancelled,
        retain_solids,
        observe,
        &mut theme,
    )
}

impl Session {
    /// Shipped raster and binding producer statistics; reading them creates no theme state.
    pub fn bake_stats(&self) -> crate::BakeStats {
        self.producer_states
            .inspect::<Mutex<Cache>, _>(|cache| cache.lock().expect("bake cache").stats())
            .unwrap_or_default()
    }

    pub fn compile(
        &self,
        source: &str,
        revision: u32,
        assets: &dyn Fn(&str) -> Result<Vec<u8>, String>,
    ) -> Result<Scene, Diagnostic> {
        self.compile_request(source, revision, assets, true, &|| false, false, &|_| {})
    }

    pub fn compile_collision(
        &self,
        source: &str,
        revision: u32,
        assets: &dyn Fn(&str) -> Result<Vec<u8>, String>,
    ) -> Result<Scene, Diagnostic> {
        self.compile_request(source, revision, assets, false, &|| false, false, &|_| {})
    }

    pub fn compile_cancellable(
        &self,
        source: &str,
        revision: u32,
        assets: &dyn Fn(&str) -> Result<Vec<u8>, String>,
        visual: bool,
        cancelled: &(dyn Fn() -> bool + Sync),
    ) -> Result<Scene, Diagnostic> {
        self.compile_request(source, revision, assets, visual, cancelled, false, &|_| {})
    }

    /// Retains closed brush sources only for explicit native delivery.
    pub fn compile_for_brushes(
        &self,
        source: &str,
        revision: u32,
        assets: &dyn Fn(&str) -> Result<Vec<u8>, String>,
        cancelled: &(dyn Fn() -> bool + Sync),
    ) -> Result<Scene, Diagnostic> {
        self.compile_request(source, revision, assets, true, cancelled, true, &|_| {})
    }

    /// Reports host-clock phases without embedding timings in deterministic packets.
    pub fn compile_observed(
        &self,
        source: &str,
        revision: u32,
        assets: &dyn Fn(&str) -> Result<Vec<u8>, String>,
        cancelled: &(dyn Fn() -> bool + Sync),
        observe: &dyn Fn(&str),
    ) -> Result<Scene, Diagnostic> {
        self.compile_request(source, revision, assets, true, cancelled, false, observe)
    }

    #[allow(clippy::too_many_arguments)]
    fn compile_request(
        &self,
        source: &str,
        revision: u32,
        assets: &dyn Fn(&str) -> Result<Vec<u8>, String>,
        visual: bool,
        cancelled: &(dyn Fn() -> bool + Sync),
        retain_solids: bool,
        observe: &dyn Fn(&str),
    ) -> Result<Scene, Diagnostic> {
        let epoch = self.begin_build();
        let runtime = &self.runtime;
        self.producer_states.with(
            || Mutex::new(Cache::with_runtime(self.runtime.clone())),
            |cache| {
                compile_target(
                    source,
                    revision,
                    assets,
                    visual,
                    &|bytes| self.validate_image(bytes),
                    Some(cache),
                    &|| cancelled() || !runtime.current(epoch),
                    retain_solids,
                    observe,
                    self.workers(),
                )
            },
        )
    }
}

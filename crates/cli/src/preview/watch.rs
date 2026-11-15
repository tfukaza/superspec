//! Content-based dependency watching. Only complete, still-current compilations are published.
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    cell::RefCell,
    collections::BTreeMap,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Instant,
};
use superspec_aml::diagnostic::Diagnostic;

type Source = Result<String, String>;
type Assets = BTreeMap<String, Result<Vec<u8>, String>>;

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub session: String,
    pub revision: u32,
    pub scene_revision: Option<u32>,
    pub state: &'static str,
    pub diagnostics: Vec<Diagnostic>,
    pub error: Option<String>,
    pub compile_ms: f64,
    pub bake_cache: superspec_engine::BakeStats,
    pub scene_source_hash: Option<String>,
    pub scene_asset_hashes: BTreeMap<String, String>,
}

pub struct Published {
    pub status: Status,
    pub packet: Option<Arc<Vec<u8>>>,
}

impl Default for Published {
    fn default() -> Self {
        Self {
            status: Status {
                session: String::new(),
                revision: 0,
                scene_revision: None,
                state: "loading",
                diagnostics: vec![],
                error: None,
                compile_ms: 0.0,
                bake_cache: Default::default(),
                scene_source_hash: None,
                scene_asset_hashes: BTreeMap::new(),
            },
            packet: None,
        }
    }
}

pub struct Watcher {
    path: PathBuf,
    source: Option<Source>,
    assets: Assets,
    revision: u32,
    dirty: bool,
}

impl Watcher {
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            source: None,
            assets: BTreeMap::new(),
            revision: 0,
            dirty: true,
        }
    }

    /// Reads bytes, not timestamps, so atomic saves and same-size/mtime edits remain observable.
    fn source(&self) -> Source {
        super::super::read_source(&self.path).map_err(|error| error.to_string())
    }

    fn asset(&self, name: &str) -> Result<Vec<u8>, String> {
        super::super::assets::read_confined(
            self.path.parent().expect("absolute source parent"),
            name,
        )
    }

    /// The callback is the shared compiler; tests can gate its completion without timing races.
    pub fn refresh<F>(&mut self, published: &Mutex<Published>, compile: F)
    where
        F: FnOnce(
            &str,
            u32,
            &dyn Fn(&str) -> Result<Vec<u8>, String>,
        ) -> Result<superspec_engine::Scene, Diagnostic>,
    {
        let source = self.source();
        let changed = self.source.as_ref() != Some(&source)
            || self
                .assets
                .iter()
                .any(|(name, bytes)| self.asset(name) != *bytes);
        if !self.dirty && !changed {
            return;
        }
        self.dirty = false;
        self.revision = self
            .revision
            .checked_add(1)
            .expect("preview revision exhausted");
        {
            let mut output = published.lock().expect("preview state");
            output.status.revision = self.revision;
            output.status.state = "compiling";
            output.status.diagnostics.clear();
            output.status.error = None;
        }
        let start = Instant::now();
        let inputs = RefCell::new(Assets::new());
        let read = |name: &str| {
            let mut inputs = inputs.borrow_mut();
            if let Some(bytes) = inputs.get(name) {
                return bytes.clone();
            }
            let total: usize = inputs
                .values()
                .filter_map(|result| result.as_ref().ok())
                .map(Vec::len)
                .sum();
            let bytes = self.asset(name);
            let over_budget = inputs.len() >= 256
                || total + bytes.as_ref().map_or(0, Vec::len) > 128 * 1024 * 1024;
            inputs.insert(name.to_owned(), bytes.clone());
            if over_budget {
                Err("Preview asset bundle exceeds the count or byte limit.".into())
            } else {
                bytes
            }
        };
        let result = source.as_ref().ok().map(|source| {
            compile(source, self.revision, &read)
                .map(|scene| (Arc::new(scene.packet()), scene.manifest.diagnostics))
        });
        let inputs = inputs.into_inner();
        // A changed source or dependency invalidates both successful output and diagnostics.
        if source != self.source()
            || inputs
                .iter()
                .any(|(name, bytes)| self.asset(name) != *bytes)
        {
            self.dirty = true;
            return;
        }
        self.source = Some(source.clone());
        self.assets = inputs;
        let mut output = published.lock().expect("preview state");
        match result {
            Some(Ok((packet, diagnostics))) => {
                output.status.scene_source_hash = source
                    .as_ref()
                    .ok()
                    .map(|source| format!("{:x}", Sha256::digest(source.as_bytes())));
                output.status.scene_asset_hashes = self
                    .assets
                    .iter()
                    .filter_map(|(name, bytes)| {
                        bytes
                            .as_ref()
                            .ok()
                            .map(|bytes| (name.clone(), format!("{:x}", Sha256::digest(bytes))))
                    })
                    .collect();
                output.packet = Some(packet);
                output.status.scene_revision = Some(self.revision);
                output.status.diagnostics = diagnostics;
                output.status.state = "ready";
            }
            Some(Err(diagnostic)) => {
                output.status.diagnostics = vec![diagnostic];
                output.status.state = "error";
            }
            None => {
                output.status.error = source.err();
                output.status.state = "error";
            }
        }
        output.status.compile_ms = start.elapsed().as_secs_f64() * 1000.0;
    }
}

//! Native/browser compilation orchestration and session-owned artifact lifetimes.
pub use crate::assets::image_admission::CacheStats;
use crate::assets::image_admission::{ImageCache, ImageInfo};
use crate::runtime::{dependencies::Runtime, producer_states::ProducerStates};
use std::{
    cell::{Cell, RefCell},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

pub(crate) enum Artifact {
    Source,
    Program(Arc<crate::materialspec::program::Program>),
    Inputs(Arc<crate::materialspec::program::Inputs>),
    /// A decoded source image, keyed by its encoded bytes so parameter edits reuse it.
    Image(Arc<crate::materialspec::program::Image>),
    Receiver(Arc<crate::authoring::receiver::Atlas>),
    Material(Arc<crate::authoring::material::BakedReceiver>),
    Item(Arc<crate::itemspec::definition::GeneratedItem>),
    Raster(crate::materialspec::bake::BakedMaterial),
    Binding(Arc<crate::itemspec::mesh::BoundMesh>),
}

/// Bounded in-memory artifact reuse shared by native preview and the browser worker.
/// Entries never survive a compiler/decoder version change because the session owns them in memory.
pub struct Session {
    workers: usize,
    pub(crate) programs: RefCell<Vec<crate::authoring::CachedProgram>>,
    pub(crate) program_bytes: Cell<usize>,
    pub(crate) authoring_revision: Cell<Option<u64>>,
    pub(crate) runtime: Arc<Runtime<Artifact>>,
    epoch: AtomicU64,
    images: RefCell<ImageCache>,
    pub(crate) producer_states: ProducerStates,
    /// Rows of material bakes evaluated by other sessions, when a host distributes them.
    pub(crate) distribution: crate::authoring::material::Distribution,
    /// Mip levels of decoded material images, shared by every material that samples them.
    pub(crate) mips: crate::materialspec::program::MipCache,
    /// The items a scene project placed last, by model identity, so that an edit regenerates
    /// only the parts whose geometry key changed.
    pub(crate) placed_items:
        RefCell<std::collections::BTreeMap<String, crate::authoring::PlacedItemState>>,
}
impl Default for Session {
    fn default() -> Self {
        Self::new(64 * 1024 * 1024).expect("default cache bound")
    }
}
impl Session {
    /// Zero disables image-admission reuse; other producer partitions keep their independent limits.
    pub fn new(encoded_byte_limit: usize) -> Result<Self, String> {
        let images = ImageCache::new(encoded_byte_limit)?;
        let runtime = Arc::new(Runtime::new(128 * 1024 * 1024));
        Ok(Self {
            workers: 4,
            programs: RefCell::new(vec![]),
            program_bytes: Cell::new(0),
            authoring_revision: Cell::new(None),
            epoch: AtomicU64::new(0),
            producer_states: ProducerStates::default(),
            distribution: Default::default(),
            mips: Default::default(),
            placed_items: RefCell::new(Default::default()),
            runtime,
            images: RefCell::new(images),
        })
    }
    pub fn stats(&self) -> CacheStats {
        self.images.borrow().stats()
    }
    /// Controls native execution while the portable single-worker path stays available.
    pub fn set_workers(&mut self, workers: usize) -> Result<(), String> {
        if !(1..=16).contains(&workers) {
            return Err("Worker count must be between 1 and 16.".into());
        }
        self.workers = workers;
        Ok(())
    }
    pub fn clear(&self) {
        self.begin_build();
        self.runtime.clear();
        self.producer_states.clear();
        self.images.borrow_mut().clear();
        self.programs.borrow_mut().clear();
        self.placed_items.borrow_mut().clear();
        self.program_bytes.set(0);
        self.authoring_revision.set(None);
    }
    pub(crate) fn workers(&self) -> usize {
        self.workers
    }
    pub(crate) fn validate_image(&self, bytes: &[u8]) -> Result<ImageInfo, String> {
        self.images.borrow_mut().validate(bytes)
    }
    pub(crate) fn begin_build(&self) -> u64 {
        let epoch = self.epoch.fetch_add(1, Ordering::AcqRel) + 1;
        self.runtime.begin(epoch).expect("session epochs increase");
        epoch
    }
    pub(crate) fn build_epoch(&self) -> u64 {
        self.epoch.load(Ordering::Acquire)
    }
}

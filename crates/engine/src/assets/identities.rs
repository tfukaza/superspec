//! Exact-byte source identities and optional decoded-size admission metadata.
use std::{
    collections::{VecDeque, hash_map::DefaultHasher},
    hash::{Hash, Hasher},
};
struct Image {
    hash: u64,
    bytes: Box<[u8]>,
    id: u64,
    pixels: Option<usize>,
}
#[derive(Default)]
pub(crate) struct Identities {
    images: VecDeque<Image>,
    image_bytes: usize,
    next_image: u64,
}
impl Identities {
    pub(crate) fn image(&mut self, bytes: &[u8]) -> u64 {
        let mut hasher = DefaultHasher::new();
        bytes.hash(&mut hasher);
        let hash = hasher.finish();
        if let Some(i) = self
            .images
            .iter()
            .position(|e| e.hash == hash && e.bytes.as_ref() == bytes)
        {
            let entry = self.images.remove(i).unwrap();
            let id = entry.id;
            self.images.push_back(entry);
            return id;
        }
        self.next_image += 1;
        let id = self.next_image;
        if bytes.len() <= 128 * 1024 * 1024 {
            while self.image_bytes + bytes.len() > 128 * 1024 * 1024 || self.images.len() >= 256 {
                let old = self.images.pop_front().unwrap();
                self.image_bytes -= old.bytes.len();
            }
            self.image_bytes += bytes.len();
            self.images.push_back(Image {
                hash,
                bytes: bytes.into(),
                id,
                pixels: None,
            });
        }
        id
    }
    pub(crate) fn source_pixels(&self, identity: u64) -> Option<usize> {
        self.images
            .iter()
            .find(|image| image.id == identity)
            .and_then(|image| image.pixels)
    }
    pub(crate) fn record_pixels(&mut self, identity: u64, pixels: usize) {
        if let Some(image) = self.images.iter_mut().find(|image| image.id == identity) {
            image.pixels = Some(pixels);
        }
    }
}

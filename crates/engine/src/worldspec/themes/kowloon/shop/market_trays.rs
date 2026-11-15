//! Baked produce tray assortment fitted into metric rack slots.
use super::super::{fixture::Fixture, pen::Tint};
use serde::Deserialize;
use std::sync::OnceLock;
use superspec_geometry::{
    V3,
    math::{cross, sub},
};
use superspec_types::rng::SeededRng;

#[derive(Deserialize)]
pub(super) struct Tray {
    size: String,
    depth: f64,
    height: f64,
    vertices: Vec<[f64; 8]>,
    indices: Vec<usize>,
}

pub(super) struct Assortment {
    trays: Vec<&'static Tray>,
    next: usize,
}

impl Assortment {
    pub fn new(size: &str, rng: &mut SeededRng) -> Self {
        static DATA: OnceLock<Vec<Tray>> = OnceLock::new();
        let assets = DATA.get_or_init(|| {
            serde_json::from_str(include_str!("assets/market-trays.json"))
                .expect("shipped market trays")
        });
        let mut trays: Vec<_> = assets.iter().filter(|tray| tray.size == size).collect();
        for index in (1..trays.len()).rev() {
            trays.swap(index, rng.integer(0, index));
        }
        Self { trays, next: 0 }
    }

    pub fn pick(&mut self, slot: &mut SeededRng) -> Option<&'static Tray> {
        if self.trays.is_empty() || !slot.chance(0.46) {
            return None;
        }
        let tray = self.trays[self.next % self.trays.len()];
        self.next += 1;
        Some(tray)
    }
}

impl Tray {
    pub fn draw(&self, d: &mut Fixture<'_, '_>, [width, depth, height]: V3) -> Result<(), String> {
        let fitted = width
            .min(depth / self.depth)
            .min((height + 0.035) / self.height);
        let left = (width - fitted) / 2.;
        let slot = "market-trays";
        d.mesh.check_work(self.indices.len() / 3)?;
        for triangle in self.indices.as_chunks::<3>().0 {
            let vertices = triangle.map(|index| self.vertices[index]);
            let points = vertices.map(|v| {
                [
                    left + v[0] * fitted,
                    v[1] * fitted * self.depth,
                    v[2] * fitted,
                ]
            });
            let outward = cross(sub(points[1], points[0]), sub(points[2], points[0]));
            let normals = vertices.map(|v| [v[3], v[4], v[5]]);
            let uv = vertices.map(|v| [v[6], v[7]]);
            d.pen.face(
                d.mesh,
                slot,
                &points,
                outward,
                &Tint::Color([1.; 3]),
                d.repeat(slot),
                Some(&uv),
                Some(&normals),
            )?;
        }
        Ok(())
    }
}

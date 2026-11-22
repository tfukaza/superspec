use super::PreviewBase;
use crate::Scene;
use superspec_types::batch::{FoliageMass, FoliagePalette};

pub(super) fn references(scene: &Scene, base: &PreviewBase<'_>) -> Vec<[usize; 2]> {
    scene
        .manifest
        .batches
        .iter()
        .enumerate()
        .filter_map(|(target, batch)| {
            let mass = batch.foliage.as_ref()?.mass.as_ref()?;
            match base {
                PreviewBase::Retained(_) => Some([target, target]),
                PreviewBase::Compare(Some(previous)) => {
                    // Batch IDs are unique and delivered in ascending order.
                    let batches = &previous.manifest.batches;
                    let index = batches.binary_search_by(|old| old.id.cmp(&batch.id)).ok()?;
                    let old = batches[index].foliage.as_ref()?.mass.as_ref()?;
                    identical(mass, old).then_some([target, index])
                }
                PreviewBase::Compare(None) => None,
            }
        })
        .collect()
}

fn identical(a: &FoliageMass, b: &FoliageMass) -> bool {
    fn settings(m: &FoliageMass) -> ([u64; 21], bool, &str) {
        let FoliageMass {
            sites: _,
            twigs: _,
            radius,
            cell,
            level,
            card_keep,
            form,
            jitter,
            leaf_scale,
            soft_edges,
            spray_scale,
            card_scale,
            shapes,
            variation,
            sparkles,
            palette,
            look,
        } = m;
        let FoliagePalette {
            hue,
            saturation,
            value,
            shade,
            shade_hue,
            contrast,
            depth,
            bands,
            deep,
        } = palette;
        (
            [
                radius,
                cell,
                level,
                card_keep,
                form,
                jitter,
                leaf_scale,
                spray_scale,
                card_scale,
                shapes,
                variation,
                sparkles,
                hue,
                saturation,
                value,
                shade,
                shade_hue,
                contrast,
                depth,
                bands,
                deep,
            ]
            .map(|x| x.to_bits()),
            *soft_edges,
            look,
        )
    }
    fn same_points<const N: usize>(a: &[[f64; N]], b: &[[f64; N]]) -> bool {
        a.len() == b.len()
            && a.iter()
                .zip(b)
                .all(|(x, y)| x.map(f64::to_bits) == y.map(f64::to_bits))
    }
    settings(a) == settings(b)
        && same_points(&a.sites, &b.sites)
        && a.twigs.len() == b.twigs.len()
        && a.twigs.iter().zip(&b.twigs).all(|(x, y)| same_points(x, y))
}

/// Borrow the full manifest for serialization, temporarily moving out only the referenced
/// masses. A large city should not allocate a duplicate copy of its manifest to make a delta.
pub(super) struct Omitted<'a> {
    scene: &'a mut Scene,
    removed: Vec<(usize, FoliageMass)>,
}

impl<'a> Omitted<'a> {
    pub(super) fn new(scene: &'a mut Scene, refs: &[[usize; 2]]) -> Self {
        let mut guard = Self {
            scene,
            removed: Vec::with_capacity(refs.len()),
        };
        for &[target, _] in refs {
            let foliage = guard.scene.manifest.batches[target]
                .foliage
                .as_mut()
                .expect("matched foliage");
            guard
                .removed
                .push((target, foliage.mass.take().expect("matched mass")));
        }
        guard
    }
    pub(super) fn scene(&self) -> &Scene {
        self.scene
    }
}

impl Drop for Omitted<'_> {
    fn drop(&mut self) {
        for (index, mass) in self.removed.drain(..) {
            self.scene.manifest.batches[index]
                .foliage
                .as_mut()
                .expect("matched foliage")
                .mass = Some(mass);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        batches::{InstanceBatch, Placement},
        instances::LocalMesh,
        updates::encode_preview,
    };
    use serde_json::{Value, json};

    fn add(scene: &mut Scene, id: &str) {
        let mesh = LocalMesh {
            positions: vec![[0., 0., 0.], [0., 1., 0.], [1., 0., 0.]],
            normals: vec![[0., 0., 1.]; 3],
            uvs: vec![[0., 0.]; 3],
            colors: vec![[1.; 3]; 3],
            indices: vec![0, 2, 1],
            tangents: vec![],
        };
        let foliage = serde_json::from_value(json!({"family":"shadow","patch":id,"far":20,
            "mass":{"sites":[[0.0,0.0,1.0,0.4]], "twigs":[[[0.0,0.0,0.0],[0.1,0.2,0.3]]]}}))
        .unwrap();
        scene
            .add_batch(
                &InstanceBatch {
                    id: id.into(),
                    object_id: id.into(),
                    mesh: id.into(),
                    placements: vec![Placement::default()],
                    foliage: Some(foliage),
                    ..Default::default()
                },
                &mesh,
            )
            .unwrap();
    }
    fn header(bytes: &[u8]) -> Value {
        let size = u32::from_le_bytes(bytes[4..8].try_into().unwrap()) as usize;
        serde_json::from_slice(&bytes[8..8 + size]).unwrap()
    }
    fn mass(batch: &mut crate::batches::Batch) -> &mut FoliageMass {
        batch.foliage.as_mut().unwrap().mass.as_mut().unwrap()
    }
    #[test]
    fn foliage_references_require_identity_exact_bits_and_independent_opt_in() {
        let mut before = Scene::empty(1, "test");
        for id in ["b", "c", "d", "e"] {
            add(&mut before, id);
        }
        let mut next = before.clone();
        next.manifest.revision = 2;
        add(&mut next, "a"); // Every surviving batch moves one record.
        mass(&mut next.manifest.batches[2]).sites[0][0] = -0.;
        mass(&mut next.manifest.batches[3]).twigs[0][1][2] = 0.5;
        mass(&mut next.manifest.batches[4]).palette.hue = -0.;
        next.manifest.batches[1].foliage.as_mut().unwrap().far = 30.; // Non-mass data still travels.
        let full = next.packet();
        let sparse = header(&encode_preview(
            &mut next,
            PreviewBase::Compare(Some(&before)),
            false,
            true,
        ));
        assert_eq!(sparse["foliageMassRefs"], json!([[1, 0]]));
        assert!(sparse.get("batchSpans").is_none());
        assert_eq!(sparse["manifest"]["batches"][1]["foliage"]["far"], 30.);
        for i in 0..5 {
            assert_eq!(
                sparse["manifest"]["batches"][i]["foliage"]
                    .get("mass")
                    .is_some(),
                i != 1
            );
        }
        for (base, capable) in [(Some(&before), false), (None, true)] {
            let legacy = header(&encode_preview(
                &mut next,
                PreviewBase::Compare(base),
                true,
                capable,
            ));
            assert!(legacy.get("foliageMassRefs").is_none());
            assert_eq!(
                legacy["manifest"]["batches"][1]["foliage"]["mass"]["sites"][0][3],
                0.4
            );
        }
        assert_eq!(next.packet(), full);
        next.manifest.revision = 3;
        next.manifest.name = "renamed".into();
        let renamed = next.packet();
        let retained = header(&encode_preview(
            &mut next,
            PreviewBase::Retained(2),
            true,
            true,
        ));
        assert_eq!(
            retained["foliageMassRefs"],
            json!([[0, 0], [1, 1], [2, 2], [3, 3], [4, 4]])
        );
        assert!(retained["batchSpans"].is_array());
        assert_eq!(next.packet(), renamed);
    }
}

//! Last accepted AML preview, shared by the native editor and the Wasm worker.
use crate::{Manifest, Scene, Session};
use sha2::{Digest, Sha256};
use std::{cell::RefCell, collections::BTreeMap};
use superspec_aml::{
    extensions::Element,
    source::{AmlSource, SourceLimits, parse_source},
};
use superspec_model::updates::{PreviewBase, encode_preview};
use superspec_types::diagnostic::{Diagnostic, SourceLocation};

struct Accepted {
    source: String,
    inputs: BTreeMap<String, Option<[u8; 32]>>,
    scene: Scene,
}

#[derive(Default)]
pub struct Preview {
    accepted: Option<Accepted>,
    /// Opted in per request by a client that retains and can patch its batch channels.
    pub batch_spans: bool,
    /// Independent client capability to restore exact prior foliage masses by batch ID.
    pub foliage_mass_refs: bool,
}

impl Preview {
    pub fn revision(&self) -> Option<u32> {
        self.accepted.as_ref().map(|a| a.scene.manifest.revision)
    }

    /// A mismatched baseline produces a self-contained update. Rejected drafts do not replace it.
    pub fn update(
        &mut self,
        session: &Session,
        source: &str,
        revision: u32,
        base_revision: Option<u32>,
        assets: &dyn Fn(&str) -> Result<Vec<u8>, String>,
        cancelled: &(dyn Fn() -> bool + Sync),
    ) -> Result<Vec<u8>, Diagnostic> {
        if let Some(previous) = self.accepted.as_mut()
            && let Some(mut manifest) =
                metadata_edit(&previous.source, source, &previous.scene.manifest)
            && previous.inputs.iter().all(|(name, digest)| {
                digest.is_some_and(|d| {
                    assets(name).is_ok_and(|bytes| <[u8; 32]>::from(Sha256::digest(&bytes)) == d)
                })
            })
            && !cancelled()
        {
            let base = previous.scene.manifest.revision;
            manifest.revision = revision;
            previous.scene.manifest = manifest;
            previous.source = source.to_owned();
            return Ok(if base_revision == Some(base) {
                encode_preview(
                    &mut previous.scene,
                    PreviewBase::Retained(base),
                    self.batch_spans,
                    self.foliage_mass_refs,
                )
            } else {
                superspec_model::updates::encode(&previous.scene, None)
            });
        }
        let inputs = RefCell::new(BTreeMap::new());
        let mut scene = session.compile_cancellable(
            source,
            revision,
            &|name| {
                let bytes = assets(name);
                let digest = bytes
                    .as_ref()
                    .ok()
                    .map(|bytes| Sha256::digest(bytes).into());
                let mut reads = inputs.borrow_mut();
                let entry = reads.entry(name.to_owned()).or_insert(digest);
                if *entry != digest {
                    // A changing dependency cannot certify reuse of this compilation.
                    *entry = None;
                }
                bytes
            },
            true,
            cancelled,
        )?;
        let base = self
            .accepted
            .as_ref()
            .filter(|a| Some(a.scene.manifest.revision) == base_revision);
        let packet = encode_preview(
            &mut scene,
            PreviewBase::Compare(base.map(|a| &a.scene)),
            self.batch_spans,
            self.foliage_mass_refs,
        );
        self.accepted = Some(Accepted {
            source: source.to_owned(),
            inputs: inputs.into_inner(),
            scene,
        });
        Ok(packet)
    }
}

/// Compare admitted XML trees before lowering. For the shipped profile only the root title
/// and ID are displayed metadata. Any other attribute or topology change takes the normal path.
fn metadata_edit(before: &str, after: &str, manifest: &Manifest) -> Option<Manifest> {
    if before == after {
        return Some(manifest.clone());
    }
    let new = parse_source(after, SourceLimits::default()).ok()?;
    let old = parse_source(before, SourceLimits::default()).ok()?;
    let mut ranges = BTreeMap::new();
    if !match_nodes((&old, old.root()), (&new, new.root()), true, &mut ranges) {
        return None;
    }
    let id = new.root().attribute("id")?;
    let mut next = manifest.clone();
    next.id = id.to_owned();
    next.name = new.root().attribute("name").unwrap_or(id).to_owned();
    next.visit_sources_mut(|location| {
        // Generated records can have a synthetic origin without an authored range.
        if location.end.is_none() && (location.line, location.column, location.offset) == (1, 1, 0)
        {
            return Ok(());
        }
        let (old, new) = ranges.get(&location.offset).ok_or(())?;
        if old != location {
            return Err(());
        }
        *location = new.clone();
        Ok(())
    })
    .ok()?;
    Some(next)
}

fn match_nodes(
    (old, a): (&AmlSource<'_>, Element<'_, '_>),
    (new, b): (&AmlSource<'_>, Element<'_, '_>),
    root: bool,
    ranges: &mut BTreeMap<usize, (SourceLocation, SourceLocation)>,
) -> bool {
    let keep = |key: &str| !(root && matches!(key, "name" | "id"));
    if a.tag_name().name() != b.tag_name().name()
        || !a
            .attributes()
            .filter(|v| keep(v.name()))
            .map(|v| (v.name(), v.value()))
            .eq(b
                .attributes()
                .filter(|v| keep(v.name()))
                .map(|v| (v.name(), v.value())))
    {
        return false;
    }
    let source = old.location(a);
    ranges.insert(source.offset, (source, new.location(b)));
    let mut children = b.children().filter(Element::is_element);
    for child in a.children().filter(Element::is_element) {
        let Some(other) = children.next() else {
            return false;
        };
        if !match_nodes((old, child), (new, other), false, ranges) {
            return false;
        }
    }
    children.next().is_none()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use superspec_geometry::image::{Image, encode};

    fn header(bytes: &[u8]) -> Value {
        let length = u32::from_le_bytes(bytes[4..8].try_into().unwrap()) as usize;
        serde_json::from_slice(&bytes[8..8 + length]).unwrap()
    }

    #[test]
    fn retained_city_items_match_fresh_after_move_resize_and_revision_restart() {
        let session = Session::default();
        let load = |id: &str| {
            let file = superspec_catalog::image_name(id).ok_or_else(|| id.to_string())?;
            std::fs::read(
                std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../../assets/materials")
                    .join(file),
            )
            .map_err(|e| e.to_string())
        };
        for (revision, x, width) in [(7, 0, 1), (8, 2, 1), (9, 2, 2), (2, 4, 1)] {
            let aml = format!(
                r#"<architecture version="1" id="study"><city><pillar id="post" position="{x} 0 0" width="{width}" depth="1" height="3"/></city></architecture>"#
            );
            let retained = session.compile(&aml, revision, &load).unwrap();
            assert_eq!(
                retained.packet(),
                crate::compile_with_assets(&aml, revision, &load)
                    .unwrap()
                    .packet()
            );
        }
    }

    #[test]
    fn formatting_rename_and_recovery_match_independent_compilation() {
        let original = r#"<architecture version="1" id="one"><material id="m" texture="grain.png"/><group id="g"><box id="box" material="m" width="2" depth="1" height="1"/></group></architecture>"#;
        let edited = original
            .replace(r#"id="one""#, r#"name="香港 🌿" id="different""#)
            .replace("<group", "\r\n<!-- 🌿 -->\n  <group");
        let session = Session::default();
        let mut preview = Preview::default();
        let png = |red| {
            encode(&Image {
                width: 1,
                height: 1,
                rgba: vec![red, 24, 36, 255],
                mime: "image/png",
            })
            .unwrap()
        };
        let load = |_: &str| Ok(png(12));
        preview
            .update(&session, original, 1, None, &load, &|| false)
            .unwrap();
        assert!(
            preview
                .update(&session, "<", 2, Some(1), &load, &|| false)
                .is_err()
        );
        assert!(
            preview
                .update(
                    &session,
                    &edited,
                    3,
                    Some(1),
                    &|_| Err("missing".into()),
                    &|| false
                )
                .is_err()
        );
        assert_eq!(preview.revision(), Some(1));
        let update = preview
            .update(&session, &edited, 4, Some(1), &load, &|| false)
            .unwrap();
        assert_eq!(header(&update)["retainedScene"], true);
        assert_eq!(
            preview.accepted.as_ref().unwrap().scene.packet(),
            crate::compile_with_assets(&edited, 4, &load)
                .unwrap()
                .packet()
        );
        let recolored = |_: &str| Ok(png(54));
        let update = preview
            .update(&session, &edited, 5, Some(4), &recolored, &|| false)
            .unwrap();
        assert!(header(&update).get("retainedScene").is_none());
        assert_eq!(
            preview.accepted.as_ref().unwrap().scene.packet(),
            crate::compile_with_assets(&edited, 5, &recolored)
                .unwrap()
                .packet()
        );
        let moved = edited.replace(r#"width="2""#, r#"width="1""#);
        let update = preview
            .update(&session, &moved, 6, Some(5), &recolored, &|| false)
            .unwrap();
        assert!(header(&update).get("retainedScene").is_none());
        let replacement = preview
            .update(&session, &moved, 7, Some(1), &recolored, &|| false)
            .unwrap();
        assert!(header(&replacement)["baseRevision"].is_null());
    }
}

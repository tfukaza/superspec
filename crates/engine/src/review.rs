//! Geometry-bound review decisions remain inspectable when a source acknowledgment is stale.
use crate::Scene;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use superspec_aml::diagnostic::Diagnostic;

pub use superspec_model::review::{Finding, Suppression};

pub(crate) struct Review {
    pub findings: Vec<Finding>,
    pub diagnostics: Vec<Diagnostic>,
}

fn fingerprint(
    scene: &Scene,
    diagnostic: &Diagnostic,
    cancelled: &dyn Fn() -> bool,
) -> Option<String> {
    let mut hash = Sha256::new();
    hash.update(b"levelspec-review-v1");
    hash.update(
        serde_json::to_vec(&(
            diagnostic.code,
            &diagnostic.objects,
            &diagnostic.message,
            &diagnostic.spatial,
        ))
        .unwrap(),
    );
    let physical: std::collections::BTreeSet<_> = scene
        .manifest
        .meshes
        .iter()
        .filter(|mesh| scene.manifest.colliders.contains(&mesh.id))
        .map(|mesh| mesh.object_id.as_str())
        .collect();
    let mut meshes: Vec<_> = scene
        .manifest
        .meshes
        .iter()
        .filter(|mesh| {
            scene.manifest.colliders.contains(&mesh.id)
                || !physical.contains(mesh.object_id.as_str()) && mesh.visible
        })
        .collect();
    meshes.sort_by(|a, b| a.id.cmp(&b.id));
    for mesh in meshes {
        if cancelled() {
            return None;
        }
        if let Some(spatial) = &diagnostic.spatial {
            if (0..3).any(|i| {
                mesh.bounds[1][i] < spatial.bounds[0][i] - 0.3
                    || mesh.bounds[0][i] > spatial.bounds[1][i] + 0.3
            }) {
                continue;
            }
        }
        hash.update((mesh.object_id.len() as u64).to_le_bytes());
        hash.update(mesh.object_id.as_bytes());
        for (index, triangle) in scene.indices
            [mesh.index_start..mesh.index_start + mesh.index_count]
            .chunks_exact(3)
            .enumerate()
        {
            if index.is_multiple_of(4096) && cancelled() {
                return None;
            }
            for vertex in triangle {
                let first = (mesh.vertex_start + *vertex as usize) * 3;
                for value in &scene.positions[first..first + 3] {
                    hash.update(value.to_bits().to_le_bytes());
                }
            }
        }
    }
    Some(format!("{:x}", hash.finalize()))
}

pub(crate) fn bind(
    scene: &Scene,
    diagnostics: Vec<Diagnostic>,
    cancelled: &dyn Fn() -> bool,
) -> Result<Review, Diagnostic> {
    let mut acknowledgments = BTreeMap::new();
    if let Some(context) = &scene.manifest.architecture {
        for node in context
            .metadata
            .iter()
            .filter(|n| n.kind == "review-suppression")
        {
            let id = node.text("suggestion").unwrap_or("");
            let fingerprint = node.text("fingerprint").unwrap_or("");
            let reason = node.text("reason").unwrap_or("");
            if id.is_empty()
                || fingerprint.len() != 64
                || !fingerprint.bytes().all(|b| b.is_ascii_hexdigit())
                || reason.trim().is_empty()
                || reason.len() > 4096
                || acknowledgments.insert(id, (fingerprint, reason)).is_some()
            {
                return Err(node.fail("AML_REVIEW","A review acknowledgment requires a unique suggestion, a SHA-256 fingerprint and a nonempty reason."));
            }
        }
    }
    let mut review = Review {
        findings: vec![],
        diagnostics: vec![],
    };
    for diagnostic in diagnostics {
        let id = format!("{}:{}", diagnostic.code, diagnostic.objects.join("|"));
        let Some(fingerprint) = fingerprint(scene, &diagnostic, cancelled) else {
            let mut error = diagnostic;
            error.code = "CANCELLED";
            error.message = "Review fingerprinting cancelled.".into();
            return Err(error);
        };
        let suppression = acknowledgments
            .get(id.as_str())
            .map(|(old, reason)| Suppression {
                reason: (*reason).to_owned(),
                stale: !old.eq_ignore_ascii_case(&fingerprint),
            });
        let suppressed = suppression.as_ref().is_some_and(|s| !s.stale);
        if !suppressed {
            review.diagnostics.push(diagnostic.clone());
        }
        review.findings.push(Finding {
            id,
            fingerprint,
            state: if suppressed { "suppressed" } else { "active" },
            diagnostic,
            suppression,
        });
    }
    Ok(review)
}

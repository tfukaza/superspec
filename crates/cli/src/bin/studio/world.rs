use serde::Deserialize;
use serde_json::json;
use std::path::Path;
use superspec_engine::{Session, authoring::Diagnostic};

#[derive(Deserialize)]
struct Draft {
    source: String,
    revision: u32,
    #[serde(rename = "baseRevision")]
    base_revision: Option<u32>,
    #[serde(default, rename = "batchSpans")]
    batch_spans: bool,
    #[serde(default, rename = "foliageMassRefs")]
    foliage_mass_refs: bool,
}

pub fn build(
    session: &Session,
    (document, assets): super::archive::Contents,
    action: &str,
    preview: &mut superspec_engine::worldspec::preview::Preview,
    cancelled: &(dyn Fn() -> bool + Sync),
) -> Result<Vec<u8>, Vec<Diagnostic>> {
    let invalid = |message| vec![Diagnostic::message("scene.aml", message)];
    let draft: Draft = serde_json::from_value(document).map_err(|e| invalid(e.to_string()))?;
    let asset = |name: &str| {
        if name.starts_with("superspec-material:") {
            return crate::assets::read_confined(Path::new(""), name);
        }
        assets
            .get(name)
            .map(|bytes| bytes.to_vec())
            .ok_or_else(|| format!("Missing bundled companion {name}."))
    };
    if action == "aml-update" {
        preview.batch_spans = draft.batch_spans;
        preview.foliage_mass_refs = draft.foliage_mass_refs;
        let packet = preview.update(
            session,
            &draft.source,
            draft.revision,
            draft.base_revision,
            &asset,
            cancelled,
        );
        return Ok(packet
            .unwrap_or_else(|error| superspec_engine::result_packet(Err(error), draft.revision)));
    }
    let result = session.compile_cancellable(
        &draft.source,
        draft.revision,
        &asset,
        action != "aml-validate",
        cancelled,
    );
    if action == "aml-scene" {
        return Ok(superspec_engine::result_packet(result, draft.revision));
    }
    let mut scene = match result {
        Ok(scene) => scene,
        Err(diagnostic) if action == "aml-validate" => {
            return Ok(json!({"success":false,"diagnostics":[diagnostic]})
                .to_string()
                .into_bytes());
        }
        Err(diagnostic) => {
            return Err(invalid(format!(
                "{}:{} {} — {}",
                diagnostic.source.line,
                diagnostic.source.column,
                diagnostic.code,
                diagnostic.message
            )));
        }
    };
    superspec_engine::validation::audit(&mut scene, cancelled).map_err(invalid)?;
    if action == "aml-validate" {
        return serde_json::to_vec(&scene.manifest.validation).map_err(|e| invalid(e.to_string()));
    }
    if !scene.manifest.success {
        return Err(invalid("Declared validation requirements failed.".into()));
    }
    let scene = superspec_export::prepare_lightmap(&scene, cancelled).map_err(invalid)?;
    superspec_export::glb(
        &scene,
        &superspec_export::Options {
            textures: if action == "aml-glb-png" {
                superspec_export::Textures::Png
            } else {
                superspec_export::Textures::Ktx2
            },
            cancelled,
            ..Default::default()
        },
    )
    .map_err(invalid)
}

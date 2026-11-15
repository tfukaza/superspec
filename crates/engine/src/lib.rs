//! Portable architectural compilation. No filesystem, renderer or JavaScript dependencies.
pub mod architecture;
pub(crate) mod assets;
pub mod authoring;
pub mod itemspec;
pub mod materialspec;
pub mod reconstruction;
mod resolve;
pub mod review;
pub mod runtime;
mod session;
pub mod source;
pub mod updates;
pub use runtime::cache::Stats as BakeStats;
pub use session::{CacheStats, Session};
pub mod tracing;
pub mod validation;
pub mod visibility;
pub mod worldspec;
use superspec_aml::diagnostic::Diagnostic;

pub use superspec_model::{Asset, ExportFace, ExportSolid, LightmapAtlas, Manifest, Mesh, Scene};
use superspec_model::{MAX_SCENE_IMAGE_BYTES, MAX_SCENE_IMAGE_PIXELS};

pub use worldspec::shipped::{
    compile, compile_collision_with_assets, compile_with_assets, compile_with_registry,
};

/// Both targets return the same framed result, including source diagnostics on failure.
pub fn compile_packet(source: &str, revision: u32) -> Vec<u8> {
    result_packet(compile(source, revision), revision)
}

/// Frames both successful output and source failures without publishing partial geometry.
pub fn result_packet(result: Result<Scene, Diagnostic>, revision: u32) -> Vec<u8> {
    match result {
        Ok(scene) => scene.packet(),
        Err(diagnostic) => {
            let mut scene = Scene::empty(revision, "superspec-rust-architecture-v1");
            scene.manifest.diagnostics.push(diagnostic);
            scene.packet()
        }
    }
}

pub fn library_assets(source: &str) -> String {
    let Ok(prepared) = source::prepare(source, superspec_profiles::default_registry()) else {
        return "[]".into();
    };
    let document = prepared.document();
    let mut images = std::collections::BTreeSet::new();
    if document.metadata.iter().any(|n| n.kind == "dressing") {
        images.extend(superspec_catalog::images());
    } else {
        for node in document
            .declarations
            .values()
            .filter(|n| n.kind == "material")
        {
            if let Ok(material) = superspec_aml::material::Material::read(node) {
                images.extend(
                    material
                        .companions()
                        .into_iter()
                        .filter(|p| p.starts_with("superspec-material:"))
                        .map(str::to_owned),
                );
            }
        }
    }
    serde_json::to_string(&images).expect("asset identities")
}

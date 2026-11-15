//! Native preview-ready measurements, including real edit sources and packet assembly.
use sha2::{Digest, Sha256};
use std::{
    cell::RefCell,
    collections::BTreeMap,
    path::{Path, PathBuf},
    time::Instant,
};
use superspec_engine::Session;

pub fn run(source: &Path, edits: &[PathBuf], workers: usize) -> Result<serde_json::Value, String> {
    let source = std::fs::canonicalize(source).map_err(|e| e.to_string())?;
    let parent = source.parent().ok_or("Source has no directory.")?;
    let mut session = Session::default();
    session.set_workers(workers)?;
    let inputs: Vec<_> = std::iter::once(("cold", source.clone()))
        .chain(std::iter::once(("warm-full", source.clone())))
        .chain(edits.iter().cloned().map(|p| ("edit", p)))
        .collect();
    let mut samples = vec![];
    for (label, path) in inputs {
        let text = super::read_source(&path).map_err(|e| e.to_string())?;
        eprintln!(
            "Benchmarking {label} on {} with {workers} requested workers…",
            path.display()
        );
        let start = Instant::now();
        let asset_hashes = RefCell::new(BTreeMap::new());
        let phase = RefCell::new((String::new(), Instant::now(), BTreeMap::new()));
        let observe = |name: &str| {
            let now = Instant::now();
            let mut p = phase.borrow_mut();
            if !p.0.is_empty() {
                let elapsed = now.duration_since(p.1).as_secs_f64() * 1000.;
                let old = p.0.clone();
                *p.2.entry(old).or_insert(0.) += elapsed;
            }
            p.0 = name.into();
            p.1 = now;
        };
        let scene = session
            .compile_observed(
                &text,
                0,
                &|name| {
                    let bytes = crate::assets::read(parent, name)?;
                    asset_hashes
                        .borrow_mut()
                        .insert(name.to_owned(), format!("{:x}", Sha256::digest(&bytes)));
                    Ok(bytes)
                },
                &super::cancelled,
                &observe,
            )
            .map_err(|e| e.to_string())?;
        let compiled = start.elapsed().as_secs_f64() * 1000.;
        let packet = scene.packet();
        let ready = start.elapsed().as_secs_f64() * 1000.;
        let packet_hash = format!("{:x}", Sha256::digest(&packet));
        eprintln!("Preview ready in {ready:.1} ms.");
        samples.push(serde_json::json!({"case":label,"source":path,"compileMs":compiled,
            "sourceHash":format!("{:x}",Sha256::digest(text.as_bytes())),"assetHashes":asset_hashes.into_inner(),
            "previewReadyMs":ready,"packetBytes":packet.len(),"packetHash":packet_hash,
            "vertices":scene.manifest.vertex_count,"triangles":scene.manifest.index_count/3,
            "phases":phase.into_inner().2,"imageCache":session.stats(),"bakeCache":session.bake_stats()}));
    }
    Ok(
        serde_json::json!({"boundary":"source request through complete preview packet","workers":workers,"platform":std::env::consts::OS,"architecture":std::env::consts::ARCH,"samples":samples}),
    )
}

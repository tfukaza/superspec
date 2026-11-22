//! Optional offline delivery adapter. The exporter remains the sole codec owner.
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicUsize, Ordering},
};
use superspec_geometry::{image, ktx2};

// Also used by the optional publisher. Bump when Kind::Data's codec/quality,
// mip generation, channel interpretation or source decoding changes.
const RECIPE: &[u8] = b"superspec-city-data-uastc-q75-e2-clamp-v1\0";
const MAX_JOBS: usize = 4096;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    version: u32,
    workers: Option<usize>,
    jobs: Vec<Job>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Job {
    input: PathBuf,
    output: PathBuf,
    key: String,
}

struct Work {
    key: String,
    inputs: BTreeSet<PathBuf>,
    outputs: BTreeSet<PathBuf>,
}

#[derive(Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Receipt {
    version: u32,
    source_key: String,
    artifact_sha256: String,
    width: usize,
    height: usize,
    bytes: usize,
}

fn receipt_path(path: &Path) -> PathBuf {
    let mut filename = path.file_name().unwrap().to_os_string();
    filename.push(".json");
    path.with_file_name(filename)
}

fn read_bounded(path: &Path, limit: usize) -> Result<Vec<u8>, String> {
    let file = fs::File::open(path).map_err(|error| format!("{}: {error}", path.display()))?;
    let mut bytes = Vec::new();
    file.take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("{}: {error}", path.display()))?;
    if bytes.len() > limit {
        return Err(format!("{}: file exceeds {limit} bytes", path.display()));
    }
    Ok(bytes)
}

fn key(source: &[u8]) -> String {
    let mut digest = Sha256::new();
    digest.update(RECIPE);
    digest.update(source);
    format!("{:x}", digest.finalize())
}

fn ordinary_file(path: &Path) -> Result<bool, String> {
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(format!("{}: {error}", path.display())),
        Ok(meta) if !meta.is_file() || meta.file_type().is_symlink() => {
            return Err(format!(
                "{}: cache output must be an ordinary file",
                path.display()
            ));
        }
        Ok(_) => {}
    }
    Ok(true)
}

fn receipt(
    path: &Path,
    source_key: &str,
    width: usize,
    height: usize,
) -> Result<Option<Receipt>, String> {
    let path = receipt_path(path);
    if !ordinary_file(&path)? {
        return Ok(None);
    }
    let record: Receipt = serde_json::from_slice(&read_bounded(&path, 4096)?)
        .map_err(|error| format!("{}: invalid cache receipt: {error}", path.display()))?;
    if record.version != 1
        || record.source_key != source_key
        || (record.width, record.height) != (width, height)
        || record.bytes == 0
        || record.bytes > image::MAX_IMAGE_BYTES
        || record.artifact_sha256.len() != 64
        || !record
            .artifact_sha256
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(format!(
            "{}: cache receipt does not match source and codec identity",
            path.display()
        ));
    }
    Ok(Some(record))
}

fn existing(path: &Path, source_key: &str, width: usize, height: usize) -> Result<bool, String> {
    let record = receipt(path, source_key, width, height)?;
    if !ordinary_file(path)? {
        return Ok(false);
    }
    let bytes = read_bounded(path, image::MAX_IMAGE_BYTES)?;
    let dimensions = ktx2::inspect(&bytes).map_err(|error| {
        format!(
            "{}: invalid occupied KTX2 cache entry: {error}",
            path.display()
        )
    })?;
    if dimensions != (width, height) {
        return Err(format!(
            "{}: occupied KTX2 cache dimensions differ from source",
            path.display()
        ));
    }
    // A crash after publishing the payload can leave no receipt. Re-encoding
    // verifies those exact bytes before adopting them; existence is not proof.
    let Some(record) = record else {
        return Ok(false);
    };
    if record.bytes != bytes.len()
        || record.artifact_sha256 != format!("{:x}", Sha256::digest(&bytes))
    {
        return Err(format!(
            "{}: artifact SHA-256 differs from its cache receipt",
            path.display()
        ));
    }
    Ok(true)
}

fn publish_immutable(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = path.parent().ok_or("Output requires a parent directory")?;
    fs::create_dir_all(parent).map_err(|error| format!("{}: {error}", parent.display()))?;
    static SERIAL: AtomicUsize = AtomicUsize::new(0);
    let temporary = parent.join(format!(
        ".city-data-{}-{}.tmp",
        std::process::id(),
        SERIAL.fetch_add(1, Ordering::Relaxed)
    ));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(|error| format!("{}: {error}", temporary.display()))?;
    let result = (|| {
        file.write_all(bytes)
            .and_then(|()| file.sync_all())
            .map_err(|error| format!("{}: {error}", temporary.display()))?;
        drop(file);
        // An atomic hard link publishes the completed bytes without replacing any
        // occupied path. Concurrent workers/processes may publish the same key.
        match fs::hard_link(&temporary, path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                if ordinary_file(path)? && read_bounded(path, image::MAX_IMAGE_BYTES)? == bytes {
                    Ok(())
                } else {
                    Err(format!(
                        "{}: refusing to replace a different occupied cache artifact",
                        path.display()
                    ))
                }
            }
            Err(error) => Err(format!(
                "{}: atomic cache publication failed: {error}",
                path.display()
            )),
        }
    })();
    let _ = fs::remove_file(&temporary);
    result
}

fn publish(
    path: &Path,
    bytes: &[u8],
    source_key: &str,
    width: usize,
    height: usize,
) -> Result<(), String> {
    let expected = Receipt {
        version: 1,
        source_key: source_key.into(),
        artifact_sha256: format!("{:x}", Sha256::digest(bytes)),
        width,
        height,
        bytes: bytes.len(),
    };
    if let Some(record) = receipt(path, source_key, width, height)? {
        if record != expected {
            return Err(format!(
                "{}: refusing to replace an unrelated cache receipt",
                receipt_path(path).display()
            ));
        }
    }
    publish_immutable(path, bytes)?;
    publish_immutable(
        &receipt_path(path),
        &serde_json::to_vec(&expected).map_err(|error| error.to_string())?,
    )
}

fn encode(work: &Work) -> Result<(usize, usize, usize), String> {
    let mut source = Vec::new();
    // Admission still checks every input on cache hits, including aliases.
    for input in &work.inputs {
        let bytes = read_bounded(input, image::MAX_IMAGE_BYTES)?;
        if key(&bytes) != work.key {
            return Err(format!(
                "{}: source does not match data-texture key {}",
                input.display(),
                work.key
            ));
        }
        source = bytes;
    }
    let input = work.inputs.first().unwrap();
    let info = image::info(&source).map_err(|error| format!("{}: {error}", input.display()))?;
    if !matches!(info.mime, "image/png" | "image/jpeg") {
        return Err(format!(
            "{}: data-texture input must be PNG or JPEG",
            input.display()
        ));
    }
    let mut missing = Vec::new();
    for output in &work.outputs {
        if !existing(output, &work.key, info.width, info.height)? {
            missing.push(output);
        }
    }
    let reused = work.outputs.len() - missing.len();
    if missing.is_empty() {
        return Ok((0, reused, 0));
    }
    let bytes = superspec_export::encode_texture(&source, superspec_export::TextureKind::Data)
        .map_err(|error| format!("{}: {error}", input.display()))?;
    for output in &missing {
        publish(output, &bytes, &work.key, info.width, info.height)?;
    }
    Ok((1, reused, missing.len()))
}

pub fn run(path: &Path) -> Result<(), String> {
    let manifest: Manifest = serde_json::from_slice(&read_bounded(path, 4 * 1024 * 1024)?)
        .map_err(|error| format!("{}: {error}", path.display()))?;
    if manifest.version != 1 || manifest.jobs.len() > MAX_JOBS {
        return Err("Data-texture manifest requires version 1 and at most 4096 jobs".into());
    }
    let workers = manifest.workers.unwrap_or_else(|| {
        std::thread::available_parallelism()
            .map(usize::from)
            .unwrap_or(1)
            .min(6)
    });
    if !(1..=6).contains(&workers) {
        return Err("Data-texture workers must be between 1 and 6".into());
    }
    let base = path.parent().unwrap_or(Path::new("."));
    let mut work: BTreeMap<String, Work> = BTreeMap::new();
    for job in manifest.jobs {
        if job.key.len() != 64
            || !job
                .key
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(format!(
                "{}: expected a lowercase SHA-256 data-texture key",
                job.input.display()
            ));
        }
        let output = base.join(job.output);
        let filename = output
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("");
        if filename != format!("{}.ktx2", job.key) && filename != format!("data-{}.ktx2", job.key) {
            return Err(format!(
                "{}: output filename must contain its complete data-texture key",
                output.display()
            ));
        }
        let entry = work.entry(job.key.clone()).or_insert_with(|| Work {
            key: job.key,
            inputs: BTreeSet::new(),
            outputs: BTreeSet::new(),
        });
        entry.inputs.insert(base.join(job.input));
        entry.outputs.insert(output);
    }
    let work: Vec<_> = work.into_values().collect();
    let next = AtomicUsize::new(0);
    let results = std::thread::scope(|scope| {
        let mut handles = Vec::new();
        for _ in 0..workers.min(work.len()) {
            handles.push(scope.spawn(|| {
                let mut results = Vec::new();
                loop {
                    let index = next.fetch_add(1, Ordering::Relaxed);
                    let Some(job) = work.get(index) else { break };
                    results.push(encode(job));
                }
                results
            }));
        }
        handles
            .into_iter()
            .flat_map(|handle| {
                handle
                    .join()
                    .unwrap_or_else(|_| vec![Err("Data-texture worker panicked".into())])
            })
            .collect::<Vec<_>>()
    });
    let (mut encoded, mut reused, mut published) = (0, 0, 0);
    let mut errors = Vec::new();
    for result in results {
        match result {
            Ok((e, r, p)) => {
                encoded += e;
                reused += r;
                published += p;
            }
            Err(error) => errors.push(error),
        }
    }
    println!(
        "{}",
        serde_json::json!({"version":1,"encoded":encoded,"reused":reused,"published":published,"workers":workers.min(work.len()),"failed":errors.len()})
    );
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("\n"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn source(root: &Path, name: &str, value: u8) -> (PathBuf, String, Vec<u8>) {
        let bytes = image::encode(&image::Image {
            width: 8,
            height: 4,
            rgba: [value, 132, 201, 255].repeat(32),
            mime: "image/png",
        })
        .unwrap();
        let path = root.join(name);
        fs::write(&path, &bytes).unwrap();
        (path, key(&bytes), bytes)
    }

    #[test]
    fn batch_uses_shared_codec_deduplicates_and_reuses_immutable_outputs() {
        let root = tempfile::tempdir().unwrap();
        let (input, key, bytes) = source(root.path(), "source.png", 34);
        let alias = root.path().join("alias.png");
        fs::write(&alias, &bytes).unwrap();
        let a = root.path().join("cache").join(format!("{key}.ktx2"));
        let b = root
            .path()
            .join("published")
            .join(format!("data-{key}.ktx2"));
        let manifest = root.path().join("manifest.json");
        fs::write(
            &manifest,
            serde_json::to_vec(&json!({"version":1,"workers":6,"jobs":[
                {"input":input,"output":a,"key":key}, {"input":alias,"output":b,"key":key}
            ]}))
            .unwrap(),
        )
        .unwrap();
        run(&manifest).unwrap();
        let expected =
            superspec_export::encode_texture(&bytes, superspec_export::TextureKind::Data).unwrap();
        assert_eq!(fs::read(&a).unwrap(), expected);
        assert_eq!(fs::read(&b).unwrap(), expected);
        let modified = fs::metadata(&a).unwrap().modified().unwrap();
        run(&manifest).unwrap();
        assert_eq!(fs::metadata(&a).unwrap().modified().unwrap(), modified);
        fs::write(&alias, b"changed").unwrap();
        assert!(
            run(&manifest)
                .unwrap_err()
                .contains("source does not match")
        );
        assert_eq!(fs::read(&a).unwrap(), expected);
    }

    #[test]
    fn occupied_bad_outputs_and_invalid_source_retain_files_and_recover() {
        let root = tempfile::tempdir().unwrap();
        let (input, key, bytes) = source(root.path(), "source.png", 79);
        let output = root.path().join(format!("{key}.ktx2"));
        let manifest = root.path().join("manifest.json");
        fs::write(
            &manifest,
            serde_json::to_vec(&json!({"version":1,"workers":2,"jobs":[
                {"input":input,"output":output,"key":key}
            ]}))
            .unwrap(),
        )
        .unwrap();
        fs::write(&output, b"retained").unwrap();
        assert!(run(&manifest).unwrap_err().contains("invalid occupied"));
        assert_eq!(fs::read(&output).unwrap(), b"retained");
        fs::remove_file(&output).unwrap();
        run(&manifest).unwrap();
        assert_eq!(ktx2::inspect(&fs::read(&output).unwrap()).unwrap(), (8, 4));
        assert!(
            fs::read_dir(root.path()).unwrap().all(|e| !e
                .unwrap()
                .file_name()
                .to_string_lossy()
                .ends_with(".tmp"))
        );
        fs::remove_file(&input).unwrap();
        assert!(run(&manifest).unwrap_err().contains("source.png"));
        fs::write(&input, &bytes).unwrap();
        run(&manifest).unwrap();
    }

    #[test]
    fn receipts_reject_valid_same_size_substitutions_and_recover_interrupted_publication() {
        let root = tempfile::tempdir().unwrap();
        let (input, key, bytes) = source(root.path(), "source.png", 22);
        let (_, _, other_bytes) = source(root.path(), "different.png", 210);
        let output = root.path().join(format!("{key}.ktx2"));
        let manifest = root.path().join("manifest.json");
        fs::write(
            &manifest,
            serde_json::to_vec(&json!({"version":1,"jobs":[
                {"input":input,"output":output,"key":key}
            ]}))
            .unwrap(),
        )
        .unwrap();
        run(&manifest).unwrap();
        let expected = fs::read(&output).unwrap();
        let receipt_file = receipt_path(&output);
        let expected_receipt = fs::read(&receipt_file).unwrap();
        let substitute =
            superspec_export::encode_texture(&other_bytes, superspec_export::TextureKind::Data)
                .unwrap();
        assert_ne!(expected, substitute);
        assert_eq!(
            ktx2::inspect(&expected).unwrap(),
            ktx2::inspect(&substitute).unwrap()
        );
        fs::write(&output, &substitute).unwrap();
        assert!(
            run(&manifest)
                .unwrap_err()
                .contains("artifact SHA-256 differs")
        );
        assert_eq!(fs::read(&output).unwrap(), substitute);
        assert_eq!(fs::read(&receipt_file).unwrap(), expected_receipt);

        // A payload published just before interruption is adopted only after
        // comparison with a fresh deterministic encode of the admitted source.
        fs::remove_file(&receipt_file).unwrap();
        assert!(run(&manifest).unwrap_err().contains("refusing to replace"));
        assert!(!receipt_file.exists());
        fs::write(&output, &expected).unwrap();
        run(&manifest).unwrap();
        assert_eq!(fs::read(&receipt_file).unwrap(), expected_receipt);
        assert_eq!(fs::read(&output).unwrap(), expected);

        // An intact receipt also admits recovery of a missing payload.
        fs::remove_file(&output).unwrap();
        run(&manifest).unwrap();
        assert_eq!(
            fs::read(&output).unwrap(),
            superspec_export::encode_texture(&bytes, superspec_export::TextureKind::Data).unwrap()
        );
        let record: Receipt = serde_json::from_slice(&expected_receipt).unwrap();
        assert_ne!(record.source_key, record.artifact_sha256);
        assert_eq!(
            record.artifact_sha256,
            format!("{:x}", Sha256::digest(&expected))
        );
    }

    #[test]
    fn manifest_rejects_unsupported_workers_versions_and_non_keyed_outputs() {
        let root = tempfile::tempdir().unwrap();
        let manifest = root.path().join("manifest.json");
        for value in [
            json!({"version":2,"jobs":[]}),
            json!({"version":1,"workers":0,"jobs":[]}),
            json!({"version":1,"workers":7,"jobs":[]}),
            json!({"version":1,"jobs":[{"input":"absent.png","output":"arbitrary.ktx2","key":"a".repeat(64)}]}),
        ] {
            fs::write(&manifest, serde_json::to_vec(&value).unwrap()).unwrap();
            assert!(run(&manifest).is_err());
        }
        assert!(!root.path().join("arbitrary.ktx2").exists());
    }
}

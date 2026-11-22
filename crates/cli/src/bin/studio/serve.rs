//! One warm native compiler per open project. Each worker owns its engine and last good packet.
#[path = "artifact.rs"]
mod artifact;
#[path = "../../static_file.rs"]
mod files;

use super::input::Input;
use serde_json::json;
use std::{
    collections::HashMap,
    ffi::OsString,
    io::Read,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU32, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};
use superspec_engine::authoring::{Diagnostic, Session};
use superspec_types::limits::MAX_PROJECT_FILE_BYTES;
use tiny_http::{Header, Method, Request, Response, Server};

fn reply(request: Request, status: u16, mime: &str, bytes: impl AsRef<[u8]>) {
    let response = Response::new(
        status.into(),
        vec![],
        std::io::Cursor::new(bytes.as_ref()),
        Some(bytes.as_ref().len()),
        None,
    )
    .with_header(Header::from_bytes("Content-Type", mime).unwrap())
    .with_header(Header::from_bytes("Cache-Control", "no-store").unwrap())
    .with_header(Header::from_bytes("X-Content-Type-Options", "nosniff").unwrap());
    let response = match header(&request, "X-Revision") {
        Some(value) => response.with_header(Header::from_bytes("X-Revision", value).unwrap()),
        None => response,
    };
    let _ = request.respond(response);
}
fn message(request: Request, status: u16, text: &str) {
    reply(
        request,
        status,
        "application/json",
        json!({"error":text}).to_string().into_bytes(),
    );
}
fn header<'a>(request: &'a Request, name: &'static str) -> Option<&'a str> {
    request
        .headers()
        .iter()
        .find(|h| h.field.equiv(name))
        .map(|h| h.value.as_str())
}

struct Job {
    _lease: Arc<()>,
    request: Request,
    revision: u32,
    action: String,
}

struct Delivery {
    job: Job,
    bytes: Arc<Vec<u8>>,
    mime: &'static str,
}

impl Delivery {
    fn enqueue(self, queue: &mpsc::SyncSender<Self>) {
        if let Err(err) = queue.try_send(self) {
            let (mpsc::TrySendError::Full(undelivered)
            | mpsc::TrySendError::Disconnected(undelivered)) = err;
            message(
                undelivered.job.request,
                503,
                "Native delivery workers are busy.",
            );
        }
    }
}

// A slow client owns a bounded delivery slot, independent of its warm compiler.
fn delivery_workers() -> mpsc::SyncSender<Delivery> {
    let (queue, receiver) = mpsc::sync_channel::<Delivery>(2);
    let receiver = Arc::new(Mutex::new(receiver));
    for _ in 0..2 {
        let receiver = receiver.clone();
        thread::spawn(move || {
            loop {
                let next = receiver.lock().expect("delivery queue").recv();
                let Ok(delivery) = next else { break };
                reply(
                    delivery.job.request,
                    200,
                    delivery.mime,
                    delivery.bytes.as_slice(),
                );
            }
        });
    }
    queue
}

struct Project {
    lease: Arc<()>,
    last_used: Instant,
    queue: mpsc::SyncSender<Job>,
    latest: Arc<AtomicU32>,
    alive: Arc<AtomicBool>,
}

fn compile(
    session: &mut Session,
    store: &mut superspec_engine::authoring::occlusion_store::OcclusionStore,
    input: &Input,
    job: &Job,
    cancelled: &dyn Fn() -> bool,
) -> Result<Vec<u8>, Vec<Diagnostic>> {
    let (output, occlusion) =
        super::delivery::build(session, store, input, job.revision, cancelled)?;
    let textures = if job.action == "glb-png" {
        superspec_export::Textures::Png
    } else {
        superspec_export::Textures::Ktx2
    };
    let result = super::delivery::project(
        &output,
        input,
        job.revision,
        &job.action,
        occlusion.as_ref(),
        textures,
        cancelled,
    );
    result.map_err(|error| vec![Diagnostic::message(&input.project.entry, error)])
}

fn editor_build(
    session: &mut Session,
    (doc, assets): super::archive::Contents,
    revision: u32,
    cancelled: &dyn Fn() -> bool,
) -> Result<Vec<u8>, Vec<Diagnostic>> {
    let invalid = |e| vec![Diagnostic::message("request", e)];
    let request: superspec_engine::authoring::Request =
        serde_json::from_value(doc).map_err(|e| invalid(e.to_string()))?;
    let asset = |name: &str| super::shading::asset(&assets, name);
    let output = session.build(&request, u64::from(revision), &asset, cancelled)?;
    output
        .scene(revision)
        .and_then(|scene| {
            super::delivery::editor_packet(&json!({"metadata":output.metadata}), scene.packet())
        })
        .map_err(invalid)
}

impl Project {
    fn open(deliveries: mpsc::SyncSender<Delivery>) -> Self {
        let (queue, receiver) = mpsc::sync_channel::<Job>(2);
        let latest = Arc::new(AtomicU32::new(0));
        let alive = Arc::new(AtomicBool::new(true));
        let wanted = latest.clone();
        let active = alive.clone();
        thread::spawn(move || {
            let mut compiler = Session::default();
            let mut planes = Default::default();
            let mut charts = Default::default();
            let mut snapshot: Option<Arc<Vec<u8>>> = None;
            let mut world_snapshot = Default::default();
            // A revision identifies the last complete companion set admitted in this session.
            let mut companions = None;
            while let Ok(mut job) = receiver.recv_timeout(Duration::from_secs(1800)) {
                if job.action == "snapshot" {
                    match &snapshot {
                        Some(bytes) => Delivery {
                            job,
                            bytes: bytes.clone(),
                            mime: "application/octet-stream",
                        }
                        .enqueue(&deliveries),
                        None => message(job.request, 404, "No successful scene yet."),
                    }
                    continue;
                }
                let cancelled = || wanted.load(Ordering::Acquire) != job.revision;
                if cancelled() {
                    message(job.request, 409, "Superseded revision.");
                    continue;
                }
                let mut bytes = Vec::new();
                let read = job
                    .request
                    .as_reader()
                    .take(MAX_PROJECT_FILE_BYTES as u64 + 1)
                    .read_to_end(&mut bytes);
                if read.is_err() || bytes.len() > MAX_PROJECT_FILE_BYTES {
                    message(job.request, 413, "Incomplete or oversized project.");
                    continue;
                }
                let mode = header(&job.request, "X-Input").unwrap_or("project");
                let result = if mode == "archive" || mode.starts_with("reuse-") {
                    let decoded = if mode == "archive" {
                        super::archive::read(&bytes)
                    } else {
                        let token = mode
                            .strip_prefix("reuse-")
                            .and_then(|s| s.parse::<u32>().ok());
                        let Some((_, assets)) =
                            companions.as_ref().filter(|(id, _)| Some(*id) == token)
                        else {
                            message(job.request, 412, "Companions must be uploaded again.");
                            continue;
                        };
                        serde_json::from_slice(&bytes)
                            .map(|doc| (doc, std::collections::BTreeMap::clone(assets)))
                            .map_err(|e| e.to_string())
                    };
                    drop(bytes);
                    decoded
                        .map_err(|e| vec![Diagnostic::message("request", e)])
                        .and_then(|contents| {
                            if mode == "archive" {
                                companions = Some((job.revision, contents.1.clone()));
                            }
                            if job.action.starts_with("aml-") {
                                super::world::build(
                                    &compiler,
                                    contents,
                                    &job.action,
                                    &mut world_snapshot,
                                    &cancelled,
                                )
                            } else if matches!(
                                job.action.as_str(),
                                "shade" | "shade-layouts" | "project-fields" | "fields"
                            ) {
                                super::shading::build(
                                    &mut compiler,
                                    &mut planes,
                                    &mut charts,
                                    contents,
                                    job.revision,
                                    &job.action,
                                    &cancelled,
                                )
                            } else if job.action == "editor-build" {
                                editor_build(&mut compiler, contents, job.revision, &cancelled)
                            } else {
                                Input::with_companions(contents)
                                    .map_err(|e| vec![Diagnostic::message("project", e)])
                                    .and_then(|input| {
                                        compile(
                                            &mut compiler,
                                            &mut planes,
                                            &input,
                                            &job,
                                            &cancelled,
                                        )
                                    })
                            }
                        })
                } else if header(&job.request, "X-Input") == Some("tree") {
                    super::foliage::scene(
                        "source.tree.json",
                        &bytes,
                        None,
                        job.revision,
                        &cancelled,
                    )
                    .and_then(|scene| {
                        super::foliage::deliver(
                            scene,
                            &job.action,
                            if job.action == "glb-png" {
                                superspec_export::Textures::Png
                            } else {
                                superspec_export::Textures::Ktx2
                            },
                            &cancelled,
                        )
                    })
                    .map_err(|e| vec![Diagnostic::message("source.tree.json", e)])
                } else {
                    Input::uploaded(&bytes)
                        .map_err(|error| vec![Diagnostic::message("", error)])
                        .and_then(|input| {
                            drop(bytes);
                            compile(&mut compiler, &mut planes, &input, &job, &cancelled)
                        })
                };
                if cancelled() {
                    message(job.request, 409, "Superseded revision.");
                    continue;
                }
                match result {
                    Ok(bytes) => {
                        let mime = match job.action.as_str() {
                            "check" | "aml-validate" => "application/json",
                            "scene" | "aml-scene" | "aml-update" | "editor-project"
                            | "editor-build" | "shade" | "shade-layouts" | "project-fields"
                            | "fields" => "application/octet-stream",
                            _ => "model/gltf-binary",
                        };
                        let bytes = Arc::new(bytes);
                        if job.action == "scene" {
                            snapshot = Some(bytes.clone());
                        }
                        Delivery { job, bytes, mime }.enqueue(&deliveries);
                    }
                    Err(errors) => reply(
                        job.request,
                        422,
                        "application/json",
                        json!({"diagnostics":errors}).to_string().into_bytes(),
                    ),
                }
            }
            active.store(false, Ordering::Release);
        });
        Self {
            lease: Arc::new(()),
            last_used: Instant::now(),
            queue,
            latest,
            alive,
        }
    }
}

fn recipe_worker() -> mpsc::SyncSender<Request> {
    let (queue, receiver) = mpsc::sync_channel::<Request>(8);
    thread::spawn(move || {
        use superspec_engine::authoring::recipe;
        for mut request in receiver {
            let mut source = String::new();
            let limit = 16 * 1024 * 1024;
            if request
                .as_reader()
                .take(limit + 1)
                .read_to_string(&mut source)
                .is_err()
                || source.len() > limit as usize
            {
                message(request, 413, "Invalid or oversized recipe request.");
                continue;
            }
            let report = match request.url() {
                "/api/recipe/check" => recipe::check(&source),
                "/api/recipe/inspect" => recipe::inspect(&source),
                "/api/recipe/edit" => recipe::edit(&source),
                "/api/recipe/catalog" => recipe::catalog_json(),
                "/api/recipe/sdk" => recipe::sdk_json(),
                _ => {
                    message(request, 404, "Unknown recipe operation.");
                    continue;
                }
            };
            reply(request, 200, "application/json", report);
        }
    });
    queue
}

pub fn run(mut args: impl Iterator<Item = OsString>) -> Result<(), String> {
    let mut port = 0u16;
    let mut ui = None;
    let mut artifacts = None;
    while let Some(arg) = args.next() {
        let value = args.next().ok_or("Missing serve option value.")?;
        if arg == "--port" {
            port = value
                .to_str()
                .and_then(|s| s.parse().ok())
                .ok_or("Invalid port.")?;
        } else if arg == "--artifacts" {
            let root = PathBuf::from(value)
                .canonicalize()
                .map_err(|e| e.to_string())?;
            if !root.is_dir() {
                return Err("Expected an artifact directory.".into());
            }
            artifacts = Some(root);
        } else if arg == "--ui" {
            ui = Some(PathBuf::from(value));
        } else {
            return Err(format!("Unknown serve option {arg:?}."));
        }
    }
    let ui = ui
        .unwrap_or_else(|| {
            let packaged = std::env::current_exe()
                .ok()
                .and_then(|path| path.parent().map(|p| p.join("native-viewer")));
            packaged.filter(|p| p.is_dir()).unwrap_or_else(|| {
                Path::new(env!("CARGO_MANIFEST_DIR")).join("../../generated/native-viewer")
            })
        })
        .canonicalize()
        .map_err(|_| "Build the native viewer with npm run build:native-viewer.")?;
    if !ui.join("native.html").is_file() {
        return Err("Missing native.html in the viewer directory.".into());
    }
    let server = Server::http(("127.0.0.1", port)).map_err(|e| e.to_string())?;
    let authority = server.server_addr().to_string();
    let origin = format!("http://{authority}");
    eprintln!("Native Studio listening at {origin}/");
    let mut projects = HashMap::<String, Project>::new();
    let mut next = 0u64;
    let recipes = recipe_worker();
    let deliveries = delivery_workers();
    for request in server.incoming_requests() {
        if header(&request, "Host") != Some(&authority)
            || header(&request, "Origin").is_some_and(|h| h != origin)
        {
            message(request, 403, "Invalid local origin.");
            continue;
        }
        let path = request.url().split('?').next().unwrap_or("/").to_string();
        if let Some(relative) = path.strip_prefix("/artifacts") {
            if request.method() != &Method::Get {
                message(request, 405, "Artifacts support GET.");
            } else if let Some((file, mime)) = artifacts
                .as_ref()
                .filter(|_| relative.starts_with('/'))
                .and_then(|root| artifact::artifact(root, relative).ok())
            {
                let response = Response::from_file(file)
                    .with_header(Header::from_bytes("Content-Type", mime).unwrap())
                    .with_header(Header::from_bytes("X-Content-Type-Options", "nosniff").unwrap());
                let _ = request.respond(response);
            } else {
                message(request, 404, "Artifact unavailable.");
            }
            continue;
        }
        if !path.starts_with("/api/") && request.method() == &Method::Get {
            match files::static_file(&ui, if path == "/" { "/native.html" } else { &path }) {
                Ok((bytes, mime)) => reply(request, 200, mime, bytes),
                Err(_) => message(request, 404, "Viewer resource unavailable."),
            }
            continue;
        }
        // The custom header also prevents cross-origin simple requests from changing sessions.
        if header(&request, "X-SuperSpec") != Some("1") {
            message(request, 403, "Missing Studio header.");
            continue;
        }
        if path.starts_with("/api/recipe/") && request.method() == &Method::Post {
            if let Err(err) = recipes.try_send(request) {
                let request = match err {
                    mpsc::TrySendError::Full(r) | mpsc::TrySendError::Disconnected(r) => r,
                };
                message(request, 503, "Recipe worker busy.");
            }
            continue;
        }
        projects.retain(|_, p| p.alive.load(Ordering::Acquire));
        if path == "/api/session" && request.method() == &Method::Post {
            if projects.len() >= 4 {
                let idle = projects
                    .iter()
                    .filter(|(_, p)| Arc::strong_count(&p.lease) == 1)
                    .min_by_key(|(_, p)| p.last_used)
                    .map(|(id, _)| id.clone());
                let Some(id) = idle else {
                    message(request, 429, "All native compilers are busy.");
                    continue;
                };
                projects.remove(&id);
            }
            next += 1;
            let id = format!("{}-{next}", std::process::id());
            projects.insert(id.clone(), Project::open(deliveries.clone()));
            reply(
                request,
                201,
                "application/json",
                json!({"session":id}).to_string().into_bytes(),
            );
            continue;
        }
        let parts: Vec<_> = path.trim_start_matches('/').split('/').collect();
        let ["api", "session", id, action] = parts.as_slice() else {
            message(request, 404, "Unknown operation.");
            continue;
        };
        let Some(project) = projects.get_mut(*id) else {
            message(request, 410, "Session expired.");
            continue;
        };
        project.last_used = Instant::now();
        if *action == "close" && request.method() == &Method::Post {
            project.latest.store(u32::MAX, Ordering::Release);
            projects.remove(*id);
            reply(request, 204, "text/plain", Vec::new());
            continue;
        }
        let snapshot = *action == "snapshot" && request.method() == &Method::Get;
        if !snapshot
            && (request.method() != &Method::Post
                || !matches!(
                    *action,
                    "scene"
                        | "glb"
                        | "glb-png"
                        | "check"
                        | "cancel"
                        | "aml-scene"
                        | "aml-update"
                        | "aml-validate"
                        | "aml-glb"
                        | "aml-glb-png"
                        | "shade"
                        | "shade-layouts"
                        | "project-fields"
                        | "fields"
                        | "editor-project"
                        | "editor-build"
                ))
        {
            message(request, 404, "Unknown operation.");
            continue;
        }
        let revision = if snapshot {
            0
        } else {
            let Some(rev) =
                header(&request, "X-Revision").and_then(|value| value.parse::<u32>().ok())
            else {
                message(request, 400, "Missing revision.");
                continue;
            };
            if project.latest.fetch_max(rev, Ordering::AcqRel) >= rev {
                message(request, 409, "Superseded revision.");
                continue;
            }
            rev
        };
        if *action == "cancel" {
            reply(request, 204, "text/plain", Vec::new());
            continue;
        }
        if request
            .body_length()
            .is_some_and(|n| n > MAX_PROJECT_FILE_BYTES)
        {
            message(request, 413, "Oversized project.");
            continue;
        }
        if let Err(err) = project.queue.try_send(Job {
            _lease: project.lease.clone(),
            request,
            revision,
            action: (*action).to_string(),
        }) {
            let job = match err {
                mpsc::TrySendError::Full(job) | mpsc::TrySendError::Disconnected(job) => job,
            };
            message(job.request, 503, "Native session busy or expired.");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tiny_http::TestRequest;

    #[test]
    fn snapshot_and_new_revision_can_finish_while_a_scene_is_still_being_delivered() {
        let (egress, completed) = mpsc::sync_channel(2);
        let project = Project::open(egress);
        let tree = include_str!("../../../../engine/src/treespec/fixtures/street.tree.json");
        let send = |action: &str, revision| {
            let request = TestRequest::new()
                .with_body(tree)
                .with_header(Header::from_bytes("X-Input", "tree").unwrap());
            project.latest.store(revision, Ordering::Release);
            project
                .queue
                .send(Job {
                    _lease: project.lease.clone(),
                    request: request.into(),
                    revision,
                    action: action.into(),
                })
                .expect("running compiler");
            completed
                .recv_timeout(Duration::from_secs(10))
                .expect("independent response")
        };
        let held = send("scene", 1);
        let snapshot = send("snapshot", 1);
        assert!(Arc::ptr_eq(&held.bytes, &snapshot.bytes));
        let newer = send("scene", 2);
        assert_eq!(newer.job.revision, 2);
        assert_ne!(newer.bytes, held.bytes);
    }
}

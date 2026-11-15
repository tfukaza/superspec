use serde_json::Value;
use std::{
    fs,
    io::{BufRead, BufReader, Read, Write},
    net::{TcpListener, TcpStream},
    path::Path,
    process::{Child, Command, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

const SOURCE: &str =
    r#"<architecture version="1" id="preview"><box id="box" width="2"/></architecture>"#;
struct Server {
    child: Child,
    authority: String,
}
impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
impl Server {
    fn start(source: &Path, ui: Option<&Path>, cwd: &Path) -> Self {
        let mut command = Command::new(env!("CARGO_BIN_EXE_superspec"));
        command
            .arg("preview")
            .arg(source)
            .args(["--port", "0"])
            .current_dir(cwd)
            .env("PATH", "")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped());
        if let Some(ui) = ui {
            command.arg("--ui").arg(ui);
        }
        Self::launch(command)
    }
    fn launch(mut command: Command) -> Self {
        let mut server = Self {
            child: command.spawn().unwrap(),
            authority: String::new(),
        };
        let stderr = server.child.stderr.take().unwrap();
        let (sender, receiver) = mpsc::channel();
        thread::spawn(move || {
            for line in BufReader::new(stderr).lines() {
                if let Ok(line) = line {
                    let _ = sender.send(line);
                } else {
                    break;
                }
            }
        });
        let line = receiver
            .recv_timeout(Duration::from_secs(10))
            .expect("preview startup");
        server.authority = line
            .strip_prefix("Preview listening at http://")
            .and_then(|value| value.strip_suffix('/'))
            .unwrap_or_else(|| panic!("{line}"))
            .to_owned();
        assert!(server.authority.starts_with("127.0.0.1:"));
        server
    }
    fn raw(&self, request: &str) -> (u16, Vec<u8>) {
        let (status, _, body) = self.response(request);
        (status, body)
    }
    fn response(&self, request: &str) -> (u16, String, Vec<u8>) {
        let mut stream = TcpStream::connect(&self.authority).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        stream.write_all(request.as_bytes()).unwrap();
        let mut bytes = vec![];
        stream.read_to_end(&mut bytes).unwrap();
        let split = bytes
            .windows(4)
            .position(|part| part == b"\r\n\r\n")
            .expect("HTTP header");
        let header = std::str::from_utf8(&bytes[..split]).unwrap();
        let status = header.split_whitespace().nth(1).unwrap().parse().unwrap();
        let length: usize = header
            .lines()
            .find_map(|line| line.strip_prefix("Content-Length: "))
            .unwrap()
            .parse()
            .unwrap();
        let body = bytes[split + 4..].to_vec();
        assert_eq!(body.len(), length);
        (status, header.to_owned(), body)
    }
    fn get(&self, path: &str) -> (u16, Vec<u8>) {
        self.raw(&format!(
            "GET {path} HTTP/1.1\r\nHost: {}\r\n\r\n",
            self.authority
        ))
    }
    fn status(&self) -> Value {
        let (code, bytes) = self.get("/api/status");
        assert_eq!(code, 200);
        serde_json::from_slice(&bytes).unwrap()
    }
    fn scene(&self, session: &str, revision: u64) -> (u16, Vec<u8>) {
        self.get(&format!("/api/scene?session={session}&revision={revision}"))
    }
    fn wait(&self, after: u64, state: &str) -> Value {
        let start = Instant::now();
        loop {
            let status = self.status();
            if status["revision"].as_u64().unwrap() > after && status["state"] == state {
                return status;
            }
            assert!(
                start.elapsed() < Duration::from_secs(10),
                "Timed out: {status}"
            );
            thread::sleep(Duration::from_millis(20));
        }
    }
}
fn ui(root: &Path) -> std::path::PathBuf {
    let ui = root.join("ui");
    fs::create_dir(&ui).unwrap();
    fs::write(
        ui.join("preview.html"),
        "<!doctype html><title>native preview</title>",
    )
    .unwrap();
    ui
}

#[test]
fn native_service_watches_source_keeps_last_good_scene_and_needs_no_node() {
    let temp = tempfile::tempdir().unwrap();
    let ui = ui(temp.path());
    let source = temp.path().join("source.aml");
    let server = Server::start(&source, Some(&ui), temp.path());
    let missing = server.wait(0, "error");
    assert!(missing["sceneRevision"].is_null());
    fs::write(&source, SOURCE).unwrap();
    let first = server.wait(missing["revision"].as_u64().unwrap(), "ready");
    let revision = first["revision"].as_u64().unwrap();
    let session = first["session"].as_str().unwrap();
    let (code, packet) = server.scene(session, revision);
    assert_eq!(code, 200);
    assert_eq!(
        packet,
        superspec_engine::compile_packet(SOURCE, revision as u32)
    );
    fs::write(&source, "<architecture").unwrap();
    let failed = server.wait(revision, "error");
    assert_eq!(failed["sceneRevision"], revision);
    assert_eq!(failed["diagnostics"][0]["code"], "XML_SYNTAX");
    assert!(failed["diagnostics"][0]["source"]["line"].as_u64().unwrap() > 0);
    assert_eq!(server.scene(session, revision).1, packet);
    assert_eq!(
        server
            .scene(session, failed["revision"].as_u64().unwrap())
            .0,
        409
    );
    fs::write(&source, SOURCE.replace('2', "5")).unwrap();
    let next = server.wait(failed["revision"].as_u64().unwrap(), "ready");
    let revision = next["revision"].as_u64().unwrap();
    assert_eq!(
        server.scene(session, revision).1,
        superspec_engine::compile_packet(&SOURCE.replace('2', "5"), revision as u32)
    );
    assert_eq!(
        fs::read_to_string(&source).unwrap(),
        SOURCE.replace('2', "5")
    );
    assert_eq!(
        server.get("/").1,
        fs::read(ui.join("preview.html")).unwrap()
    );
}

#[test]
fn http_surface_rejects_escape_mutation_bad_authority_and_bounded_resource_abuse() {
    let temp = tempfile::tempdir().unwrap();
    let ui = ui(temp.path());
    let source = temp.path().join("source.aml");
    fs::write(&source, SOURCE).unwrap();
    fs::write(temp.path().join("secret.js"), "OUTSIDE_SENTINEL").unwrap();
    fs::File::create(ui.join("huge.js"))
        .unwrap()
        .set_len(32 * 1024 * 1024 + 1)
        .unwrap();
    let server = Server::start(&source, Some(&ui), temp.path());
    server.wait(0, "ready");
    for path in [
        "/../secret.js",
        "/%2e%2e/secret.js",
        "/..\\secret.js",
        "//secret.js",
        "/C:/secret.js",
        "http://evil.test/",
        "/huge.js",
        "/source.aml",
    ] {
        let (code, bytes) = server.get(path);
        assert!(code >= 400, "{path}: {code}");
        assert!(!String::from_utf8_lossy(&bytes).contains("OUTSIDE_SENTINEL"));
    }
    for headers in [
        "Host: evil.test".to_owned(),
        "Host: 127.0.0.1:1".to_owned(),
        format!("Host: {}\r\nHost: {}", server.authority, server.authority),
        format!("Host: {}\r\nOrigin: http://evil.test", server.authority),
        format!(
            "Host: {}\r\nOrigin: http://{}\r\nOrigin: http://{}",
            server.authority, server.authority, server.authority
        ),
        format!("Host: {}\r\nContent-Length: 2", server.authority),
    ] {
        assert!(
            server
                .raw(&format!("GET /api/status HTTP/1.1\r\n{headers}\r\n\r\n"))
                .0
                >= 400
        );
    }
    assert_eq!(
        server
            .raw(&format!(
                "POST /api/status HTTP/1.1\r\nHost: {}\r\n\r\n",
                server.authority
            ))
            .0,
        405
    );
    assert_eq!(
        server
            .raw(&format!(
                "GET / HTTP/1.1\r\nHost: {}\r\nX-Padding: {}\r\n\r\n",
                server.authority,
                "x".repeat(8300)
            ))
            .0,
        400
    );
    let mut stalled = TcpStream::connect(&server.authority).unwrap();
    stalled.write_all(b"GET / HTTP/1.1\r\nHost:").unwrap();
    assert_eq!(server.get("/api/status").0, 200);
    stalled
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    let mut rejected = vec![];
    stalled.read_to_end(&mut rejected).unwrap();
    assert!(rejected.starts_with(b"HTTP/1.1 400"));
    assert_eq!(server.get("/").0, 200);
    assert_eq!(server.scene("different-process", 1).0, 409);
    assert_eq!(fs::read_to_string(source).unwrap(), SOURCE);
}

#[test]
fn packaged_ui_discovery_serves_nested_static_bytes_with_correct_mime_and_exact_origin() {
    let temp = tempfile::tempdir().unwrap();
    let packaged = temp.path().join("package");
    fs::create_dir(&packaged).unwrap();
    let ui = packaged.join("reviewer");
    fs::create_dir(&ui).unwrap();
    fs::create_dir(ui.join("assets")).unwrap();
    fs::write(ui.join("preview.html"), b"PACKAGED_UI").unwrap();
    let executable = packaged.join(if cfg!(windows) {
        "superspec.exe"
    } else {
        "superspec"
    });
    fs::copy(env!("CARGO_BIN_EXE_superspec"), &executable).unwrap();
    let mut command = Command::new(executable);
    command
        .args(["preview", "missing.aml"])
        .current_dir(temp.path())
        .env("PATH", "")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    let server = Server::launch(command);
    assert_eq!(server.get("/").1, b"PACKAGED_UI");
    for (name, mime, bytes) in [
        (
            "app.js",
            "text/javascript",
            b"export const ready=true;".as_slice(),
        ),
        (
            "module.wasm",
            "application/wasm",
            b"\0asm\x01\0\0\0".as_slice(),
        ),
        ("style.css", "text/css", b"body{}".as_slice()),
    ] {
        fs::write(ui.join("assets").join(name), bytes).unwrap();
        let (code, header, body) = server.response(&format!(
            "GET /assets/{name} HTTP/1.1\r\nHost: {}\r\nOrigin: http://{}\r\n\r\n",
            server.authority, server.authority
        ));
        assert_eq!(code, 200);
        assert!(header.contains(&format!("Content-Type: {mime}\r\n")));
        assert_eq!(body, bytes);
    }
}

#[test]
fn preview_startup_validates_options_ui_and_occupied_port() {
    let temp = tempfile::tempdir().unwrap();
    let ui = ui(temp.path());
    for args in [
        vec!["preview", "missing.aml", "--port", "65536"],
        vec!["preview", "missing.aml", "--port"],
        vec!["preview", "missing.aml", "--json"],
        vec!["scene", "missing.aml", "--ui", "x"],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_superspec"))
            .args(args)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2));
    }
    let output = Command::new(env!("CARGO_BIN_EXE_superspec"))
        .args(["preview", "missing.aml", "--ui"])
        .arg(temp.path())
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    let occupied = TcpListener::bind("127.0.0.1:0").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_superspec"))
        .args(["preview", "missing.aml", "--ui"])
        .arg(ui)
        .arg("--port")
        .arg(occupied.local_addr().unwrap().port().to_string())
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
}

#[cfg(unix)]
#[test]
fn ui_symlink_cannot_expose_files_outside_its_root() {
    let temp = tempfile::tempdir().unwrap();
    let ui = ui(temp.path());
    let source = temp.path().join("source.aml");
    fs::write(&source, SOURCE).unwrap();
    fs::write(temp.path().join("secret.js"), "OUTSIDE_SENTINEL").unwrap();
    std::os::unix::fs::symlink(temp.path().join("secret.js"), ui.join("link.js")).unwrap();
    let server = Server::start(&source, Some(&ui), temp.path());
    assert_eq!(server.get("/link.js").0, 403);
    assert_eq!(server.get("/").0, 200);
}

#[cfg(windows)]
#[test]
fn windows_junctions_cannot_escape_the_ui_or_companion_root() {
    use std::os::windows::process::CommandExt;
    let temp = tempfile::tempdir().unwrap();
    let ui = ui(temp.path());
    let project = temp.path().join("project");
    fs::create_dir(&project).unwrap();
    let outside = temp.path().join("outside");
    fs::create_dir(&outside).unwrap();
    fs::write(outside.join("secret.js"), "OUTSIDE_SENTINEL").unwrap();
    let image = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/aml/asymmetric-heightmap.png");
    fs::copy(image, outside.join("texture.png")).unwrap();
    let powershell = Path::new(&std::env::var_os("SystemRoot").unwrap())
        .join("System32/WindowsPowerShell/v1.0/powershell.exe");
    for link in [ui.join("linked"), project.join("linked")] {
        let result = Command::new(&powershell)
            .args(["-NoProfile", "-NonInteractive", "-Command", "$ErrorActionPreference = 'Stop'; New-Item -ItemType Junction -Path $env:SUPERSPEC_LINK -Target $env:SUPERSPEC_TARGET | Out-Null"])
            .env("SUPERSPEC_LINK", link)
            .env("SUPERSPEC_TARGET", &outside)
            .creation_flags(0x08000000)
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
    }
    let source = project.join("level.aml");
    fs::write(&source, r#"<architecture version="1" id="escape"><material id="paint" texture="linked/texture.png"/><box id="box" material="paint"/></architecture>"#).unwrap();
    let server = Server::start(&source, Some(&ui), temp.path());
    let failed = server.wait(0, "error");
    assert!(
        failed["diagnostics"][0]["message"]
            .as_str()
            .unwrap()
            .contains("escapes the source directory")
    );
    let (code, bytes) = server.get("/linked/secret.js");
    assert_eq!(code, 403);
    assert!(!String::from_utf8_lossy(&bytes).contains("OUTSIDE_SENTINEL"));
    assert_eq!(server.get("/").0, 200);
    fs::write(source, SOURCE).unwrap();
    server.wait(failed["revision"].as_u64().unwrap(), "ready");
}

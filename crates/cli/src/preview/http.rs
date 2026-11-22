#[path = "../static_file.rs"]
mod files;
use files::static_file;
// Read-only preview HTTP.
use super::watch::Published;
use std::{
    io::{self, Read, Write},
    net::TcpStream,
    path::Path,
    sync::Mutex,
    time::{Duration, Instant},
};

fn send(stream: &mut TcpStream, code: u16, mime: &str, bytes: &[u8]) -> io::Result<()> {
    write!(
        stream,
        "HTTP/1.1 {code} Response\r\nContent-Type: {mime}\r\nContent-Length: {}\r\nConnection: close\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nCross-Origin-Resource-Policy: same-origin\r\nCross-Origin-Opener-Policy: same-origin\r\nCross-Origin-Embedder-Policy: require-corp\r\n\r\n",
        bytes.len()
    )?;
    stream.write_all(bytes)
}

fn header(stream: &mut TcpStream) -> io::Result<String> {
    let start = Instant::now();
    let mut bytes = Vec::new();
    let mut chunk = [0; 1024];
    while bytes.len() <= 8192 && start.elapsed() < Duration::from_secs(1) {
        match stream.read(&mut chunk) {
            Ok(0) => break,
            Ok(count) => {
                bytes.extend_from_slice(&chunk[..count]);
                if bytes.len() > 8192 {
                    break;
                }
                if let Some(end) = bytes.windows(4).position(|value| value == b"\r\n\r\n") {
                    if end + 4 != bytes.len() {
                        break;
                    }
                    return String::from_utf8(bytes)
                        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error));
                }
            }
            Err(error)
                if [io::ErrorKind::TimedOut, io::ErrorKind::WouldBlock].contains(&error.kind()) => {
            }
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::InvalidData,
        "Invalid or incomplete HTTP header",
    ))
}

pub fn respond(
    mut stream: TcpStream,
    authority: &str,
    ui: &Path,
    published: &Mutex<Published>,
) -> io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_millis(200)))?;
    stream.set_write_timeout(Some(Duration::from_secs(5)))?;
    let Ok(request) = header(&mut stream) else {
        return send(&mut stream, 400, "text/plain", b"Invalid request");
    };
    let mut lines = request.split("\r\n");
    let parts: Vec<_> = lines.next().unwrap_or("").split(' ').collect();
    if parts.len() != 3 || parts[2] != "HTTP/1.1" {
        return send(&mut stream, 400, "text/plain", b"Invalid request");
    }
    if parts[0] != "GET" {
        return send(&mut stream, 405, "text/plain", b"Read-only preview");
    }
    let mut host = None;
    let mut origin = None;
    for line in lines.take_while(|line| !line.is_empty()) {
        let Some((name, value)) = line.split_once(':') else {
            return send(&mut stream, 400, "text/plain", b"Invalid header");
        };
        let value = value.trim();
        if name.eq_ignore_ascii_case("host") && host.replace(value).is_some()
            || name.eq_ignore_ascii_case("origin") && origin.replace(value).is_some()
        {
            return send(&mut stream, 400, "text/plain", b"Duplicate authority");
        }
        if name.eq_ignore_ascii_case("transfer-encoding")
            || name.eq_ignore_ascii_case("content-length") && value != "0"
        {
            return send(
                &mut stream,
                400,
                "text/plain",
                b"Request bodies are not supported",
            );
        }
    }
    if host != Some(authority) || origin.is_some_and(|value| value != format!("http://{authority}"))
    {
        return send(&mut stream, 403, "text/plain", b"Local origin required");
    }
    let uri = parts[1];
    if uri == "/api/status" {
        let status = published.lock().expect("preview state").status.clone();
        return send(
            &mut stream,
            200,
            "application/json",
            &serde_json::to_vec(&status)?,
        );
    }
    if let Some((session, revision)) = uri
        .strip_prefix("/api/scene?session=")
        .and_then(|value| value.split_once("&revision="))
        .and_then(|(session, revision)| {
            revision
                .parse::<u32>()
                .ok()
                .map(|revision| (session, revision))
        })
    {
        let packet = {
            let state = published.lock().expect("preview state");
            if state.status.session == session && state.status.scene_revision == Some(revision) {
                state.packet.clone()
            } else {
                None
            }
        };
        return match packet {
            Some(bytes) => send(&mut stream, 200, "application/octet-stream", &bytes),
            None => send(
                &mut stream,
                409,
                "text/plain",
                b"Scene revision no longer available",
            ),
        };
    }
    match static_file(ui, uri) {
        Ok((bytes, mime)) => send(&mut stream, 200, mime, &bytes),
        Err(error) => send(
            &mut stream,
            if error.kind() == io::ErrorKind::NotFound {
                404
            } else {
                403
            },
            "text/plain",
            b"UI resource unavailable",
        ),
    }
}

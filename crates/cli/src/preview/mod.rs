//! Loopback-only read-only preview serving. Compilation never blocks the HTTP workers.
mod http;
#[cfg(test)]
mod tests;
mod watch;

use std::{
    io,
    net::TcpListener,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, mpsc},
    thread,
    time::Duration,
};

/// Packaged releases locate prebuilt assets next to the executable; repository runs use generated UI.
pub fn default_ui() -> PathBuf {
    let packaged = std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(|parent| parent.join("reviewer")));
    if let Some(path) = packaged.filter(|path| path.is_dir()) {
        return path;
    }
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../generated/rust-editor")
}

pub fn serve(source: &Path, ui: &Path, port: u16) -> io::Result<()> {
    let source = std::path::absolute(source)?;
    let ui = ui.canonicalize()?;
    if !ui.join("preview.html").is_file() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "Prebuilt preview.html is missing. Build the Rust reviewer UI first.",
        ));
    }
    let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port))?;
    listener.set_nonblocking(true)?;
    let authority = listener.local_addr()?.to_string();
    let published = Arc::new(Mutex::new(watch::Published::default()));
    published.lock().expect("preview state").status.session = format!(
        "{:x}-{:x}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock")
            .as_nanos()
    );
    let watcher_state = Arc::clone(&published);
    thread::Builder::new()
        .name("preview-compiler".into())
        .spawn(move || {
            let mut watcher = watch::Watcher::new(source);
            let compiler = superspec_engine::Session::default();
            while !crate::cancelled() {
                watcher.refresh(&watcher_state, |source, revision, assets| {
                    compiler.compile_cancellable(source, revision, assets, true, &crate::cancelled)
                });
                watcher_state
                    .lock()
                    .expect("preview state")
                    .status
                    .bake_cache = compiler.bake_stats();
                thread::sleep(Duration::from_millis(250));
            }
        })?;
    let (sender, receiver) = mpsc::sync_channel(8);
    let receiver = Arc::new(Mutex::new(receiver));
    for index in 0..4 {
        let receiver = Arc::clone(&receiver);
        let published = Arc::clone(&published);
        let ui = ui.clone();
        let authority = authority.clone();
        thread::Builder::new()
            .name(format!("preview-http-{index}"))
            .spawn(move || {
                loop {
                    let Ok(stream) = receiver.lock().expect("HTTP queue").recv() else {
                        return;
                    };
                    let _ = http::respond(stream, &authority, &ui, &published);
                }
            })?;
    }
    eprintln!("Preview listening at http://{authority}/");
    while !crate::cancelled() {
        match listener.accept() {
            Ok((stream, _)) => {
                let _ = sender.try_send(stream);
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(25))
            }
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

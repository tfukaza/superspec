//! Optional process boundary with bounded waiting and cancellation.
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::Path,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

pub struct QuakeOptions {
    pub palette: std::path::PathBuf,
    pub qbsp: std::path::PathBuf,
    pub vis: std::path::PathBuf,
    pub light: std::path::PathBuf,
}

/// Runs a bounded external Blender operation using an explicitly staged script and log.
pub fn blender_script(
    executable: &Path,
    directory: &Path,
    script: &str,
    args: &[std::ffi::OsString],
    cancelled: &dyn Fn() -> bool,
) -> Result<(), String> {
    let executable = if executable.components().count() > 1 {
        std::path::absolute(executable).map_err(|e| e.to_string())?
    } else {
        executable.to_owned()
    };
    let log_path = directory.join(format!("{script}.log"));
    let log = File::create(&log_path).map_err(|e| e.to_string())?;
    let mut command = Command::new(executable);
    command
        .args([
            "--background",
            "--factory-startup",
            "--python-exit-code",
            "1",
            "--python",
        ])
        .arg(directory.join(script))
        .arg("--")
        .args(args)
        .current_dir(directory)
        .stdin(Stdio::null())
        .stdout(log.try_clone().map_err(|e| e.to_string())?)
        .stderr(log);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let mut child = command
        .spawn()
        .map_err(|e| format!("Cannot start Blender: {e}"))?;
    let start = Instant::now();
    loop {
        if cancelled()
            || start.elapsed() > Duration::from_secs(3600)
            || std::fs::metadata(&log_path).is_ok_and(|m| m.len() > 64 * 1024 * 1024)
        {
            let _ = child.kill();
            let _ = child.wait();
            return Err(
                "Blender stopped after cancellation or exceeding its time/log allowance.".into(),
            );
        }
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return Ok(()),
            Ok(Some(status)) => {
                let mut file = File::open(&log_path).map_err(|e| e.to_string())?;
                let size = file.metadata().map_err(|e| e.to_string())?.len().min(16384);
                file.seek(SeekFrom::End(-(size as i64)))
                    .map_err(|e| e.to_string())?;
                let mut tail = String::new();
                let _ = file.read_to_string(&mut tail);
                return Err(format!("Blender failed ({status}). {tail}"));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(error.to_string());
            }
        }
    }
}

/// Runs the explicitly configured Quake tools in the unpublished staging directory.
pub fn quake(
    options: &QuakeOptions,
    directory: &Path,
    cancelled: &dyn Fn() -> bool,
) -> Result<(), String> {
    for (name, executable, args) in [
        (
            "qbsp",
            &options.qbsp,
            vec!["-bsp2", "-leaktest", "scene.map"],
        ),
        ("vis", &options.vis, vec!["-fast", "scene.bsp"]),
        ("light", &options.light, vec!["scene.bsp"]),
    ] {
        if cancelled() {
            return Err("BSP export cancelled.".into());
        }
        eprintln!("Running {name}…");
        let log_name = format!("scene.{name}.log");
        let log = File::create(directory.join(&log_name)).map_err(|e| e.to_string())?;
        let executable = if executable.components().count() > 1 {
            std::path::absolute(executable).map_err(|e| e.to_string())?
        } else {
            executable.clone()
        };
        let mut command = Command::new(executable);
        command
            .args(args)
            .current_dir(directory)
            .stdin(Stdio::null())
            .stdout(log.try_clone().map_err(|e| e.to_string())?)
            .stderr(log);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000);
        }
        let mut child = command
            .spawn()
            .map_err(|e| format!("Cannot start {name}: {e}"))?;
        let start = Instant::now();
        loop {
            let oversized = std::fs::metadata(directory.join(&log_name))
                .is_ok_and(|m| m.len() > 8 * 1024 * 1024);
            if cancelled() || oversized || start.elapsed() > Duration::from_secs(600) {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!(
                    "{name} stopped after cancellation or exceeding its time/log allowance."
                ));
            }
            match child.try_wait() {
                Ok(Some(status)) if status.success() => break,
                Ok(Some(status)) => {
                    let mut file =
                        File::open(directory.join(&log_name)).map_err(|e| e.to_string())?;
                    let count = file.metadata().map_err(|e| e.to_string())?.len().min(16384);
                    file.seek(SeekFrom::End(-(count as i64)))
                        .map_err(|e| e.to_string())?;
                    let mut tail = String::new();
                    let _ = file.read_to_string(&mut tail);
                    return Err(format!("{name} failed ({status}). {tail}"));
                }
                Ok(None) => std::thread::sleep(Duration::from_millis(50)),
                Err(e) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(e.to_string());
                }
            }
        }
    }
    let mut header = [0; 4];
    File::open(directory.join("scene.bsp"))
        .and_then(|mut f| f.read_exact(&mut header))
        .map_err(|e| e.to_string())?;
    if &header != b"BSP2" {
        return Err("Quake compiler did not produce BSP2.".into());
    }
    Ok(())
}

pub fn fbx(blender: &Path, directory: &Path, cancelled: &dyn Fn() -> bool) -> Result<(), String> {
    let log = File::create(directory.join("fbx.log")).map_err(|e| e.to_string())?;
    let mut command = Command::new(blender);
    command
        .args(["--background", "--factory-startup", "--python"])
        .arg(directory.join("fbx-convert.py"))
        .arg("--")
        .arg(directory.join("scene.glb"))
        .arg(directory.join("scene.fbx"))
        .stdin(Stdio::null())
        .stderr(log.try_clone().map_err(|e| e.to_string())?)
        .stdout(log);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let mut child = command
        .spawn()
        .map_err(|e| format!("Cannot start Blender: {e}"))?;
    let start = Instant::now();
    loop {
        if cancelled() || start.elapsed() > Duration::from_secs(600) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(if cancelled() {
                "FBX export cancelled."
            } else {
                "Blender exceeded ten minutes."
            }
            .into());
        }
        let status = match child.try_wait() {
            Ok(status) => status,
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(error.to_string());
            }
        };
        match status {
            Some(status) if status.success() && directory.join("scene.fbx").is_file() => {
                return Ok(());
            }
            Some(status) => {
                let detail = log_tail(directory).unwrap_or_default();
                return Err(format!("Blender FBX export failed ({status}). {detail}"));
            }
            None => std::thread::sleep(Duration::from_millis(50)),
        }
    }
}

/// Retains bounded failure evidence before the temporary export directory is removed.
fn log_tail(directory: &Path) -> std::io::Result<String> {
    let mut file = File::open(directory.join("fbx.log"))?;
    let count = file.metadata()?.len().min(16 * 1024);
    file.seek(SeekFrom::End(-(count as i64)))?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    Ok(String::from_utf8_lossy(&bytes).trim().to_owned())
}

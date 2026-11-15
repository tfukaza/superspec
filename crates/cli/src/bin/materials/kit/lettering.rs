//! Native font acquisition and cache publication for the theme's lettering recipe.
use reqwest::blocking::Client;
use skrifa::Font;
use std::{
    fs::{self, OpenOptions},
    io::{ErrorKind, Read, Write},
    path::Path,
    time::{Duration, SystemTime},
};
use superspec_engine::worldspec::themes::kowloon::authoring::kit::lettering::{self, FontSpec};

const FONT_SOURCE: &str = "https://raw.githubusercontent.com/google/fonts/main/ofl";
const FONT_BYTES_LIMIT: u64 = 32 * 1024 * 1024;

fn bounded_bytes(source: impl Read) -> Result<Vec<u8>, String> {
    let mut data = Vec::new();
    source
        .take(FONT_BYTES_LIMIT + 1)
        .read_to_end(&mut data)
        .map_err(|e| e.to_string())?;
    if data.len() as u64 > FONT_BYTES_LIMIT {
        return Err("Font exceeds 32 MiB.".into());
    }
    Ok(data)
}

fn cache_font(directory: &Path, file: &Path, data: &[u8]) -> Result<(), String> {
    fs::create_dir_all(directory).map_err(|e| e.to_string())?;
    let timestamp = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_nanos();
    let temporary = directory.join(format!(".font-{}-{timestamp}", std::process::id()));
    let mut writer = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(|e| e.to_string())?;
    let written = writer.write_all(data);
    drop(writer);
    let published = written.and_then(|()| fs::rename(&temporary, file));
    if let Err(error) = published {
        let _ = fs::remove_file(&temporary);
        return Err(format!("{}: {error}", file.display()));
    }
    Ok(())
}

fn load_font(font: FontSpec, directory: &Path, client: &Client) -> Result<Font, String> {
    let file = directory.join(font.filename);
    let font_source = match fs::File::open(&file) {
        Ok(reader) => Font::new(bounded_bytes(reader)?, 0)
            .ok_or_else(|| format!("Invalid font {}", file.display()))?,
        Err(error) if error.kind() == ErrorKind::NotFound => {
            let filename = font.filename.replace('[', "%5B").replace(']', "%5D");
            let url = format!("{FONT_SOURCE}/{}/{filename}", font.folder);
            let response = client
                .get(url)
                .send()
                .and_then(|r| r.error_for_status())
                .map_err(|e| {
                    format!(
                        "Could not download {}: {e}. Place the font in {}.",
                        font.filename,
                        directory.display()
                    )
                })?;
            let data = bounded_bytes(response)?;
            let downloaded = Font::new(data.clone(), 0)
                .ok_or_else(|| format!("Invalid font {}", file.display()))?;
            cache_font(directory, &file, &data)?;
            downloaded
        }
        Err(error) => return Err(format!("{}: {error}", file.display())),
    };
    Ok(match font.weight {
        Some(weight) => font_source
            .instance_builder()
            .variations([("wght", weight)])
            .build(),
        None => font_source,
    })
}

pub(super) fn bake(directory: &Path, extra: &str) -> Result<(Vec<u8>, Vec<u8>), String> {
    let mut client = None;
    lettering::bake(extra, &mut |spec| {
        if client.is_none() {
            client = Some(
                Client::builder()
                    .timeout(Duration::from_secs(45))
                    .build()
                    .map_err(|e| e.to_string())?,
            );
        }
        load_font(spec, directory, client.as_ref().unwrap())
    })
}

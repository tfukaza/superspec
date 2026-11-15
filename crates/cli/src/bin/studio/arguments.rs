use std::{ffi::OsString, path::PathBuf};

pub const HELP: &str = "SuperSpec native authoring
Usage:
  superspec-studio <scene|glb|check> <project.json|source.ms|source.material.ts|source.item.json> [--profile preview|production] [--textures ktx2|png] [--resolution 16-512|2048] [--seed 0-4294967295] [--parameters values.json] [--types] [--sdk <dir>]
  superspec-studio inspect <source.material.ts>
  superspec-studio edit <source.material.ts> --op <json|@file> [--write]
  superspec-studio sdk --out <directory>
  superspec-studio catalog
Scene and GLB commands publish binary stdout; check, inspect, edit and catalog publish a JSON report.
check --types also runs strict TypeScript (tsc) against the generated SDK in --sdk <dir> (default generated/material-sdk).
edit prints the patches and the edited source; --write saves the edited module only after the recipe still compiles.
--profile production admits resolutions up to 2048 and bakes rows on native threads; output bytes match preview.
glb stores textures as KTX2 (KHR_texture_basisu) by default; --textures png writes PNG for tools without KTX2 support.
sdk writes index.d.ts, ports.json, ai-reference.md and tsconfig.json for type-checking recipes.
";

/// Commands that read or edit recipe source without building a scene.
pub const RECIPE_COMMANDS: [&str; 4] = ["inspect", "edit", "sdk", "catalog"];

pub struct Arguments {
    pub command: String,
    pub source: PathBuf,
    pub resolution: Option<usize>,
    pub profile: Option<superspec_engine::authoring::Profile>,
    pub textures: superspec_export::Textures,
    pub seed: Option<u32>,
    pub parameters: Option<PathBuf>,
    pub types: bool,
    pub sdk: Option<PathBuf>,
    pub op: Option<String>,
    pub write: bool,
    pub out: Option<PathBuf>,
}

impl Arguments {
    pub fn parse(values: impl IntoIterator<Item = OsString>) -> Result<Option<Self>, String> {
        let mut values = values.into_iter();
        let Some(command) = values.next() else {
            return Err(HELP.into());
        };
        if command == "--help" || command == "-h" {
            return Ok(None);
        }
        let command = command.to_str().ok_or("Command must be UTF-8.")?.to_owned();
        let known = ["scene", "glb", "check"].contains(&command.as_str())
            || RECIPE_COMMANDS.contains(&command.as_str());
        if !known {
            return Err(HELP.into());
        }
        let needs_source = !matches!(command.as_str(), "sdk" | "catalog");
        let source = if needs_source {
            values
                .next()
                .filter(|value| !value.to_string_lossy().starts_with('-'))
                .ok_or("A source or project file is required.")?
        } else {
            OsString::new()
        };
        let mut parsed = Self {
            command,
            source: source.into(),
            resolution: None,
            profile: None,
            textures: superspec_export::Textures::Ktx2,
            seed: None,
            parameters: None,
            types: false,
            sdk: None,
            op: None,
            write: false,
            out: None,
        };
        while let Some(option) = values.next() {
            let mut value = |name: &str| -> Result<OsString, String> {
                values.next().ok_or(format!("{name} requires a value."))
            };
            match option.to_str() {
                Some("--resolution") if parsed.resolution.is_none() => {
                    let value = value("--resolution")?;
                    let value: usize = value
                        .to_str()
                        .ok_or("Invalid resolution.")?
                        .parse()
                        .map_err(|_| "Invalid resolution.")?;
                    parsed.resolution = Some(value);
                }
                Some("--textures") if parsed.command == "glb" => {
                    parsed.textures = match value("--textures")?.to_str() {
                        Some("ktx2") => superspec_export::Textures::Ktx2,
                        Some("png") => superspec_export::Textures::Png,
                        _ => return Err("Textures must be ktx2 or png.".into()),
                    };
                }
                Some("--profile") if parsed.profile.is_none() => {
                    use superspec_engine::authoring::Profile;
                    parsed.profile = Some(match value("--profile")?.to_str() {
                        Some("preview") => Profile::Preview,
                        Some("production") => Profile::Production,
                        _ => return Err("Profile must be preview or production.".into()),
                    });
                }
                Some("--seed") if parsed.seed.is_none() => {
                    parsed.seed = Some(
                        value("--seed")?
                            .to_str()
                            .ok_or("Invalid seed.")?
                            .parse()
                            .map_err(|_| "Seed must be an unsigned 32-bit integer.")?,
                    );
                }
                Some("--parameters") if parsed.parameters.is_none() => {
                    parsed.parameters = Some(value("--parameters")?.into())
                }
                Some("--types") if !parsed.types && parsed.command == "check" => {
                    parsed.types = true;
                }
                Some("--sdk") if parsed.sdk.is_none() && parsed.command == "check" => {
                    parsed.sdk = Some(value("--sdk")?.into());
                }
                Some("--op") if parsed.op.is_none() && parsed.command == "edit" => {
                    parsed.op = Some(
                        value("--op")?
                            .into_string()
                            .map_err(|_| "--op must be UTF-8.")?,
                    );
                }
                Some("--write") if !parsed.write && parsed.command == "edit" => {
                    parsed.write = true;
                }
                Some("--out") if parsed.out.is_none() && parsed.command == "sdk" => {
                    parsed.out = Some(value("--out")?.into());
                }
                _ => {
                    return Err(format!(
                        "Unknown or repeated option {}.",
                        option.to_string_lossy()
                    ));
                }
            }
        }
        if let Some(resolution) = parsed.resolution {
            // A project file may select production itself; Rust enforces the selected profile.
            let max = parsed
                .profile
                .map_or(superspec_engine::authoring::Profile::Production, |p| p)
                .max_resolution();
            if !(16..=max).contains(&resolution) {
                return Err(format!("Resolution must be between 16 and {max}."));
            }
        }
        if parsed.command == "edit" && parsed.op.is_none() {
            return Err("edit requires --op.".into());
        }
        if parsed.command == "sdk" && parsed.out.is_none() {
            return Err("sdk requires --out.".into());
        }
        Ok(Some(parsed))
    }
}

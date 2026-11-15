use super::*;
use superspec_engine::tracing::{Options, Raster, trace_encoded};

pub(super) fn run(arguments: &Arguments, result: &mut CommandResult) -> Result<(), String> {
    let input = path(&arguments.paths[0])?;
    result.source = Some(input.clone());
    let mut bytes = Vec::new();
    File::open(&input)
        .map_err(|e| e.to_string())?
        .take(40_000_001)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > 40_000_000 {
        return Err("Trace input exceeds 40 MB.".into());
    }
    let stem = input
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("traced");
    let text_option = |key: &str, default: &str| -> Result<String, String> {
        arguments
            .values
            .get(key)
            .map(|v| {
                v.to_str()
                    .map(str::to_owned)
                    .ok_or_else(|| format!("{key} must be UTF-8."))
            })
            .unwrap_or_else(|| Ok(default.to_owned()))
    };
    let raster: Raster = if arguments.flags.contains("--raster") {
        serde_json::from_slice(&bytes).map_err(|e| e.to_string())?
    } else {
        let options = Options {
            cells: arguments.integer("--cells", 46)?,
            bands: arguments.integer("--bands", 4)?,
            grid: arguments.number("--grid", 0.)?,
            span: arguments.number("--span", 68.)?,
            fill: arguments.number("--fill", 0.45)?,
            background_tolerance: arguments.number("--bg-tol", 26.)?,
            alpha_cut: arguments.number("--alpha-cut", 40.)?,
            smooth: arguments.integer("--smooth", 3)?,
            minimum_band: arguments.number("--min-band", 0.03)?,
            crop: !arguments.flags.contains("--no-crop"),
        };
        trace_encoded(
            &bytes,
            &text_option("--id", stem)?,
            &text_option("--name", stem)?,
            &input.to_string_lossy(),
            &options,
            &cancelled,
        )?
    };
    let aml = raster.aml(&cancelled)?;
    let scene = validate_proposal(&aml)?;
    result.diagnostics = scene.manifest.diagnostics.clone();
    if !scene.manifest.success {
        return Err("Traced proposal failed validation.".into());
    }
    let mut files = Vec::new();
    if let Some(output) = arguments.values.get("--out") {
        files.push((path(output)?, aml.as_bytes().to_vec()));
    }
    if let Some(output) = arguments.values.get("--evidence") {
        files.push((
            path(output)?,
            serde_json::to_vec_pretty(&raster).map_err(|e| e.to_string())?,
        ));
    }
    if files.iter().any(|(p, _)| p == &input) {
        return Err("Trace output must differ from its input.".into());
    }
    result.artifacts = publish_files(files)?;
    result.report =
        Some(serde_json::json!({"aml":aml,"validation":scene.manifest.validation,"raster":raster}));
    Ok(())
}

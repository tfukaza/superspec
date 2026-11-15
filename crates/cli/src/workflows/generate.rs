use super::*;
use superspec_engine::worldspec::layouts::{Options, propose};

pub(super) fn run(arguments: &Arguments, result: &mut CommandResult) -> Result<(), String> {
    // Keep the historical default seed so the product rename preserves generated layouts.
    let seed = arguments.text("--seed")?.unwrap_or("levelspec");
    let attempts = arguments.integer("--attempts", 14)?;
    if !(1..=128).contains(&attempts) {
        return Err("Generation attempts must be 1..128.".into());
    }
    let depth = arguments.integer("--depth", 1)?;
    let routes = arguments.integer("--min-routes", 2)?;
    let mut errors = vec![];
    for attempt in 0..attempts {
        if cancelled() {
            return Err("Generation cancelled.".into());
        }
        let candidate_seed = format!("{seed}:v2:{}", attempt + 1);
        let mut proposal = propose(
            &Options {
                seed: &candidate_seed,
                depth,
                minimum_routes: routes,
                archetype: arguments.text("--archetype")?,
                theme: arguments.text("--theme")?,
            },
            &cancelled,
        )?;
        eprintln!("Checking generated candidate {}…", attempt + 1);
        let scene = match validate_proposal(&proposal.aml) {
            Ok(scene) => scene,
            Err(error) => {
                errors.push(error);
                continue;
            }
        };
        let run = match superspec_engine::worldspec::layouts::populate(
            &mut proposal,
            &scene,
            depth,
            &cancelled,
        ) {
            Ok(run) => run,
            Err(error) => {
                errors.push(error);
                continue;
            }
        };
        let scene = validate_proposal(&proposal.aml)?;
        if let Some(output) = arguments.values.get("--out") {
            result.artifacts =
                publish_files(vec![(path(output)?, proposal.aml.as_bytes().to_vec())])?;
        }
        result.diagnostics = scene.manifest.diagnostics;
        result.report = Some(
            serde_json::json!({"aml":proposal.aml,"seed":seed,"candidateSeed":candidate_seed,"attempts":attempt+1,"archetype":proposal.archetype,"theme":proposal.theme,"run":run,"validation":scene.manifest.validation}),
        );
        return Ok(());
    }
    Err(format!(
        "No accepted candidate in {attempts} attempts. {}",
        errors.join("\n")
    ))
}

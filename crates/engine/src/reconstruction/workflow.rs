use super::{Evidence, Report, Thresholds, evaluate};
use crate::{Scene, Session, validation};
use serde::Serialize;

#[derive(Serialize)]
pub struct Attempt {
    pub iteration: usize,
    pub source: String,
    pub valid: bool,
    pub accepted: bool,
    pub report: Option<Report>,
    pub diagnostics: Vec<String>,
    pub score: f64,
}
pub struct RevisionRequest<'a> {
    pub iteration: usize,
    pub previous_source: &'a str,
    pub attempts: &'a [Attempt],
}
pub struct LoopOptions {
    pub maximum_revisions: usize,
    pub maximum_source_characters: usize,
    pub evaluation_dimension: usize,
    pub thresholds: Thresholds,
}
impl Default for LoopOptions {
    fn default() -> Self {
        Self {
            maximum_revisions: 4,
            maximum_source_characters: 2_000_000,
            evaluation_dimension: 512,
            thresholds: Thresholds::default(),
        }
    }
}
#[derive(Serialize)]
pub struct LoopResult {
    pub accepted: bool,
    pub status: &'static str,
    pub source: Option<String>,
    pub report: Option<Report>,
    pub attempts: Vec<Attempt>,
    pub error: Option<String>,
    #[serde(skip)]
    pub scene: Option<Scene>,
}
fn score(report: &Report) -> f64 {
    report.issues.len() as f64 * 100.
        + report
            .views
            .iter()
            .map(|v| {
                v.silhouette_io_u.map_or(0., |iou| (1. - iou) * 100.)
                    + v.stroke_error_pixels.unwrap_or(0.)
                    + v.landmark_rms_pixels.unwrap_or(0.) / 4.
            })
            .sum::<f64>()
}

/// Evaluates every candidate through the shared compiler and keeps the best valid result.
/// The caller owns model/network I/O through the reviser callback.
pub fn run(
    initial: &str,
    evidence: &[Evidence],
    options: &LoopOptions,
    assets: &dyn Fn(&str) -> Result<Vec<u8>, String>,
    cancelled: &(dyn Fn() -> bool + Sync),
    mut revise: impl FnMut(RevisionRequest<'_>) -> Result<Option<String>, String>,
) -> Result<LoopResult, String> {
    if options.maximum_revisions > 30
        || !(1000..=10_000_000).contains(&options.maximum_source_characters)
        || !(32..=4096).contains(&options.evaluation_dimension)
    {
        return Err("Invalid reconstruction iteration or source limits.".into());
    }
    let compiler = Session::default();
    let mut result = LoopResult {
        accepted: false,
        status: "exhausted",
        source: None,
        report: None,
        attempts: vec![],
        error: None,
        scene: None,
    };
    let mut source = initial.to_owned();
    let mut best = f64::INFINITY;
    for iteration in 0..=options.maximum_revisions {
        if cancelled() {
            result.status = "aborted";
            break;
        }
        if iteration > 0 || source.is_empty() {
            match revise(RevisionRequest {
                iteration,
                previous_source: &source,
                attempts: &result.attempts,
            }) {
                Ok(Some(next)) => source = next,
                Ok(None) => {
                    result.status = "stopped";
                    break;
                }
                Err(error) => {
                    result.status = "failed";
                    result.error = Some(error);
                    break;
                }
            }
        }
        let evaluated = (|| {
            if source.encode_utf16().count() > options.maximum_source_characters {
                return Err("Candidate exceeds its source-character allowance.".into());
            }
            let mut scene = compiler
                .compile_cancellable(&source, iteration as u32, assets, true, cancelled)
                .map_err(|e| e.to_string())?;
            validation::audit(&mut scene, cancelled)?;
            let report = evaluate(
                &scene,
                evidence,
                &options.thresholds,
                options.evaluation_dimension,
                cancelled,
            )?;
            Ok::<_, String>((scene, report))
        })();
        match evaluated {
            Ok((scene, report)) => {
                let valid = scene.manifest.success;
                let accepted = valid && report.passed;
                let score = score(&report);
                result.attempts.push(Attempt {
                    iteration,
                    source: source.clone(),
                    valid,
                    accepted,
                    diagnostics: scene
                        .manifest
                        .diagnostics
                        .iter()
                        .map(|d| format!("{}: {}", d.code, d.message))
                        .collect(),
                    report: Some(report.clone()),
                    score,
                });
                if valid && score < best {
                    best = score;
                    result.source = Some(source.clone());
                    result.report = Some(report);
                    result.scene = Some(scene);
                }
                if accepted {
                    result.accepted = true;
                    result.status = "accepted";
                    break;
                }
            }
            Err(error) => result.attempts.push(Attempt {
                iteration,
                source: source.clone(),
                valid: false,
                accepted: false,
                report: None,
                diagnostics: vec![error],
                score: 1e9,
            }),
        }
    }
    if cancelled() {
        result.status = "aborted";
    }
    Ok(result)
}

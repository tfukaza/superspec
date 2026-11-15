use super::*;
use serde::Deserialize;
use std::cell::RefCell;
use superspec_engine::reconstruction::{Evidence, Thresholds};
mod model;

struct FrozenAssets {
    parent: PathBuf,
    bytes: RefCell<BTreeMap<String, Result<Vec<u8>, String>>>,
}
impl FrozenAssets {
    fn new(source: &str, parent: &Path) -> Result<Self, String> {
        let mut names = BTreeSet::new();
        if let Ok(document) = superspec_profiles::parse(source) {
            let mut pending = vec![&document.root];
            while let Some(node) = pending.pop() {
                for key in [
                    "src",
                    "heightmap",
                    "texture",
                    "normal-texture",
                    "metallic-roughness-texture",
                    "height-texture",
                ] {
                    if let Some(name) = node.text(key) {
                        names.insert(name.to_owned());
                    }
                }
                pending.extend(&node.children);
            }
        }
        let mut bytes = BTreeMap::new();
        let mut total = 0usize;
        for name in names {
            let content = assets::read(parent, &name);
            total += content.as_ref().map_or(0, Vec::len);
            if total > 128 * 1024 * 1024 {
                return Err("Reconstruction companion snapshot exceeds 128 MiB.".into());
            }
            bytes.insert(name, content);
        }
        Ok(Self {
            parent: parent.to_owned(),
            bytes: RefCell::new(bytes),
        })
    }
    fn read(&self, name: &str) -> Result<Vec<u8>, String> {
        if let Some(bytes) = self.bytes.borrow().get(name) {
            return bytes.clone();
        }
        if !name.starts_with("superspec-material:") {
            return Err(format!(
                "Candidate references a companion outside the frozen input set: {name}."
            ));
        }
        let bytes = assets::read(&self.parent, name);
        self.bytes
            .borrow_mut()
            .insert(name.to_owned(), bytes.clone());
        bytes
    }
}

#[derive(Deserialize)]
struct EvidenceFile {
    #[serde(default)]
    views: Vec<serde_json::Value>,
    #[serde(default)]
    thresholds: Thresholds,
}

pub(super) fn run(arguments: &Arguments, result: &mut CommandResult) -> Result<(), String> {
    let input = path(
        arguments
            .values
            .get("--source")
            .or_else(|| arguments.paths.first())
            .ok_or("Reconstruction requires --source <file.aml>.")?,
    )?;
    result.source = Some(input.clone());
    let source = read_source(&input).map_err(|e| e.to_string())?;
    let mut evidence = vec![];
    let mut thresholds = Thresholds::default();
    if let Some(file) = arguments.values.get("--evidence") {
        let file = path(file)?;
        let raw = read_source(&file).map_err(|e| e.to_string())?;
        let parsed: EvidenceFile = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
        thresholds = parsed.thresholds;
        for view in parsed.views {
            let mut item: Evidence =
                serde_json::from_value(view.clone()).map_err(|e| e.to_string())?;
            if let Some(name) = view["silhouettePath"].as_str() {
                item.silhouette = Some(superspec_engine::reconstruction::silhouette(
                    &assets::read(file.parent().unwrap(), name)?,
                )?);
            }
            evidence.push(item);
        }
    }
    let parent = input.parent().unwrap();
    let frozen = FrozenAssets::new(&source, parent)?;
    let load = |name: &str| frozen.read(name);
    let model = arguments
        .text("--model")?
        .map(|_| model::Model::new(arguments, &source, parent))
        .transpose()?;
    if let Some(model) = &model {
        model.pin_photos(&mut evidence)?;
    }
    let limit = arguments.integer("--max-dimension", 512)?;
    let options = superspec_engine::reconstruction::LoopOptions {
        maximum_revisions: if model.is_some() {
            arguments.integer("--max-revisions", 4)?
        } else {
            0
        },
        maximum_source_characters: arguments.integer("--max-source-characters", 2_000_000)?,
        evaluation_dimension: limit,
        thresholds,
    };
    let run = superspec_engine::reconstruction::run(
        &source,
        &evidence,
        &options,
        &load,
        &cancelled,
        |request| {
            model
                .as_ref()
                .map_or(Ok(None), |model| model.revise(request, &load))
        },
    )?;
    let mut files = vec![];
    if let Some(out) = arguments.values.get("--out") {
        let out = path(out)?;
        if out == input {
            return Err("Reconstruction output must differ from the source.".into());
        }
        files.push((
            out.join("evaluation.json"),
            serde_json::to_vec_pretty(&run).map_err(|e| e.to_string())?,
        ));
        if model.is_some() {
            if let Some(source) = &run.source {
                files.push((out.join("candidate.aml"), source.as_bytes().to_vec()));
            }
        }
        if let Some(scene) = &run.scene {
            if let Some(context) = &scene.manifest.architecture {
                for (index, reference) in context
                    .metadata
                    .iter()
                    .filter(|n| n.kind == "reference")
                    .enumerate()
                {
                    files.push((
                        out.join(format!("view-{index}.png")),
                        superspec_engine::reconstruction::render_png(
                            scene, reference, limit, &cancelled,
                        )?,
                    ));
                }
            }
        }
    }
    result.artifacts = publish_files(files)?;
    if let Some(scene) = &run.scene {
        result.diagnostics = scene.manifest.diagnostics.clone();
    }
    let passed = run.accepted;
    result.report = Some(serde_json::to_value(run).map_err(|e| e.to_string())?);
    if !passed {
        return Err("Reconstruction has unverified views or failed evidence thresholds. Inspect the evaluation report.".into());
    }
    Ok(())
}

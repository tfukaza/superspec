use super::*;
use sha2::{Digest, Sha256};
use std::fmt::Write;

fn diagram(report: &serde_json::Value) -> Result<Vec<u8>, String> {
    let samples = report["samples"]
        .as_array()
        .ok_or("Missing visibility samples.")?;
    let points: Vec<[f64; 3]> = samples
        .iter()
        .map(|s| serde_json::from_value(s["feet"].clone()).map_err(|e| e.to_string()))
        .collect::<Result<_, _>>()?;
    let low = std::array::from_fn::<_, 2, _>(|i| {
        points.iter().map(|p| p[i]).fold(f64::INFINITY, f64::min) - 2.
    });
    let high = std::array::from_fn::<_, 2, _>(|i| {
        points
            .iter()
            .map(|p| p[i])
            .fold(f64::NEG_INFINITY, f64::max)
            + 2.
    });
    let scale = (640. / (high[0] - low[0])).min(680. / (high[1] - low[1]));
    let x = |v: f64| 40. + (v - low[0]) * scale;
    let y = |v: f64| 780. - (v - low[1]) * scale;
    let mut svg = String::from(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"1440\" height=\"880\"><rect width=\"100%\" height=\"100%\" fill=\"#f1f4f4\"/><g font-family=\"sans-serif\" fill=\"#203842\"><text x=\"40\" y=\"36\" font-size=\"25\">Sampled visibility review</text>",
    );
    for elevated in [false, true] {
        write!(
            svg,
            "<g transform=\"translate({} 0)\"><text x=\"40\" y=\"70\" font-size=\"17\">{}</text>",
            if elevated { 720 } else { 0 },
            if elevated {
                "Elevated feet Z ≥ 2 m"
            } else {
                "Lower feet Z &lt; 2 m"
            }
        )
        .unwrap();
        for (sample, p) in samples
            .iter()
            .zip(&points)
            .filter(|(_, p)| (p[2] >= 2.) == elevated)
        {
            let color = if sample["maxVisibleDistance"].as_f64().unwrap_or(0.)
                >= report["settings"]["longDistance"].as_f64().unwrap_or(35.)
            {
                "#d47d2d"
            } else {
                "#83aaa9"
            };
            write!(
                svg,
                "<circle cx=\"{}\" cy=\"{}\" r=\"2\" fill=\"{color}\"/>",
                x(p[0]),
                y(p[1])
            )
            .unwrap();
        }
        for line in report["longLines"]
            .as_array()
            .ok_or("Missing visibility lines.")?
            .iter()
            .take(20)
        {
            let a: [f64; 3] =
                serde_json::from_value(line["from"].clone()).map_err(|e| e.to_string())?;
            let b: [f64; 3] =
                serde_json::from_value(line["to"].clone()).map_err(|e| e.to_string())?;
            if (a[2] - line["eyeHeight"].as_f64().unwrap_or(0.) >= 2.) != elevated {
                continue;
            }
            write!(
                svg,
                "<path d=\"M {} {} L {} {}\" stroke=\"#b64e37\" opacity=\".45\"/>",
                x(a[0]),
                y(a[1]),
                x(b[0]),
                y(b[1])
            )
            .unwrap();
        }
        svg.push_str("</g>");
    }
    write!(svg,"<text x=\"40\" y=\"835\">{} supported samples. Complete within sampled scope = {}. Inspect report omissions.</text></g></svg>",points.len(),report["complete"]).unwrap();
    Ok(svg.into_bytes())
}

pub(super) fn run(arguments: &Arguments, result: &mut CommandResult) -> Result<(), String> {
    let input = path(&arguments.paths[0])?;
    let configuration = path(&arguments.paths[1])?;
    let output = path(
        arguments
            .values
            .get("--out")
            .ok_or("Visibility requires --out <new-directory>.")?,
    )?;
    if output.exists() {
        return Err("Visibility output directory already exists.".into());
    }
    let source = read_source(&input).map_err(|e| e.to_string())?;
    let config = read_source(&configuration).map_err(|e| e.to_string())?;
    let mut scene = superspec_engine::Session::default()
        .compile_cancellable(
            &source,
            0,
            &|name| assets::read(input.parent().unwrap(), name),
            true,
            &cancelled,
        )
        .map_err(|e| e.to_string())?;
    superspec_engine::validation::audit(&mut scene, &cancelled)?;
    if !scene.manifest.success {
        return Err("Visibility input has validation errors.".into());
    }
    let mut report = superspec_engine::visibility::audit(
        &scene,
        serde_json::from_str(&config).map_err(|e| e.to_string())?,
        &cancelled,
    )?;
    report["source"] = serde_json::json!(input);
    report["sourceSha256"] = format!("{:x}", Sha256::digest(source.as_bytes())).into();
    report["configurationSha256"] = format!("{:x}", Sha256::digest(config.as_bytes())).into();
    report["renderHash"] = format!("{:x}", Sha256::digest(scene.packet())).into();
    let svg = diagram(&report)?;
    result.source = Some(input);
    result.artifacts = publish_files(vec![
        (output.join("report.svg"), svg),
        (
            output.join("report.json"),
            serde_json::to_vec_pretty(&report).map_err(|e| e.to_string())?,
        ),
    ])?;
    let complete = report["complete"] == true;
    result.report = Some(report);
    if !complete {
        return Err(
            "Visibility scope is incomplete. Inspect retained omissions and query limits.".into(),
        );
    }
    Ok(())
}

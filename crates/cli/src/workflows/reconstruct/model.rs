use super::super::*;
use base64::{Engine, engine::general_purpose::STANDARD};
use serde_json::{Value, json};
use std::{sync::mpsc, time::Duration};
use superspec_engine::reconstruction::RevisionRequest;

pub(super) struct Model {
    client: reqwest::blocking::Client,
    endpoint: String,
    key: String,
    name: String,
    photos: Vec<Value>,
    identities: Vec<(String, String, [usize; 2])>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{BufRead, BufReader, Read, Write},
        net::TcpListener,
    };

    #[test]
    fn structured_response_transport_uses_the_selected_model_and_returns_aml() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut length = 0usize;
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                if line == "\r\n" {
                    break;
                }
                if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                    length = value.trim().parse().unwrap();
                }
            }
            let mut body = vec![0; length];
            reader.read_exact(&mut body).unwrap();
            let body: Value = serde_json::from_slice(&body).unwrap();
            assert_eq!(body["model"], "selected-model");
            assert_eq!(body["store"], false);
            assert_eq!(body["text"]["format"]["strict"], true);
            let candidate=json!({"aml":"<architecture version=\"1\" id=\"response\"/>","rationale":"fixture"}).to_string();
            let response=json!({"status":"completed","output":[{"type":"message","content":[{"type":"output_text","text":candidate}]}]}).to_string();
            write!(stream,"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{response}",response.len()).unwrap();
        });
        let model = Model {
            client: reqwest::blocking::Client::new(),
            endpoint: format!("http://{address}/v1/responses"),
            key: "test-key".into(),
            name: "selected-model".into(),
            photos: vec![],
            identities: vec![],
        };
        let source = "<architecture version=\"1\" id=\"initial\"/>";
        let result = model
            .revise(
                RevisionRequest {
                    iteration: 1,
                    previous_source: source,
                    attempts: &[],
                },
                &|_| Err("No assets in fixture.".into()),
            )
            .unwrap();
        assert_eq!(
            result.as_deref(),
            Some("<architecture version=\"1\" id=\"response\"/>")
        );
        server.join().unwrap();
    }
}
impl Model {
    pub fn new(arguments: &Arguments, source: &str, parent: &Path) -> Result<Self, String> {
        let name = arguments
            .text("--model")?
            .ok_or("Model name is required.")?
            .to_owned();
        if name.trim().is_empty() {
            return Err("Specify a model name.".into());
        }
        let key = std::env::var("OPENAI_API_KEY")
            .map_err(|_| "Set OPENAI_API_KEY to enable model-assisted reconstruction.")?;
        if key.trim().is_empty() {
            return Err("OPENAI_API_KEY is empty.".into());
        }
        let endpoint = arguments
            .text("--api-url")?
            .unwrap_or("https://api.openai.com/v1/responses")
            .to_owned();
        let document = superspec_profiles::parse(source).map_err(|e| e.to_string())?;
        let mut photos = vec![];
        let mut identities = vec![];
        let mut photo_bytes = 0usize;
        for reference in document.metadata.iter().filter(|n| n.kind == "reference") {
            let filename = reference
                .text("image")
                .ok_or("Reference image path is missing.")?;
            let bytes = assets::read(parent, filename)?;
            photo_bytes += bytes.len();
            if photo_bytes > 128 * 1024 * 1024 || photos.len() >= 128 {
                return Err("Reference photographs exceed their count or byte allowance.".into());
            }
            let decoded = superspec_engine::reconstruction::silhouette(&bytes)?;
            if decoded.width as f64 != reference.number("width", 0.)
                || decoded.height as f64 != reference.number("height", 0.)
            {
                return Err("Reference dimensions differ from their photograph.".into());
            }
            identities.push((
                reference.id.clone(),
                filename.to_owned(),
                [decoded.width, decoded.height],
            ));
            photos.push(json!({"type":"input_text","text":format!("Original photograph {}. Preserve image=\"{}\", width={}, height={} in the reference declaration.",reference.id,filename,reference.number("width",0.),reference.number("height",0.))}));
            let mime = if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
                "image/png"
            } else {
                "image/jpeg"
            };
            photos.push(json!({"type":"input_image","image_url":format!("data:{mime};base64,{}",STANDARD.encode(bytes)),"detail":"high"}));
        }
        if photos.is_empty() {
            return Err(
                "Model-assisted reconstruction requires source reference photographs.".into(),
            );
        }
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(300))
            .connect_timeout(Duration::from_secs(15))
            .build()
            .map_err(|e| e.to_string())?;
        Ok(Self {
            client,
            endpoint,
            key,
            name,
            photos,
            identities,
        })
    }

    pub fn pin_photos(
        &self,
        evidence: &mut Vec<superspec_engine::reconstruction::Evidence>,
    ) -> Result<(), String> {
        use superspec_engine::reconstruction::Evidence;
        for (id, filename, size) in &self.identities {
            if let Some(item) = evidence.iter_mut().find(|e| &e.reference_id == id) {
                if item
                    .image_filename
                    .as_ref()
                    .is_some_and(|name| name != filename)
                    || item
                        .image_dimensions
                        .is_some_and(|dimensions| dimensions != *size)
                {
                    return Err(
                        "Independent annotations disagree with the frozen photograph identity."
                            .into(),
                    );
                }
                item.image_filename = Some(filename.clone());
                item.image_dimensions = Some(*size);
            } else {
                evidence.push(Evidence {
                    reference_id: id.clone(),
                    landmarks: vec![],
                    masks: vec![],
                    image_filename: Some(filename.clone()),
                    image_dimensions: Some(*size),
                    silhouette: None,
                    strokes: vec![],
                });
            }
        }
        Ok(())
    }

    pub fn revise(
        &self,
        request: RevisionRequest<'_>,
        assets: &dyn Fn(&str) -> Result<Vec<u8>, String>,
    ) -> Result<Option<String>, String> {
        let feedback = request
            .attempts
            .last()
            .map(|attempt| json!({"diagnostics":attempt.diagnostics,"report":attempt.report}));
        let mut content = vec![
            json!({"type":"input_text","text":format!("Revision {}. Return a complete corrected AML document for the supplied photographs. Preserve the reference IDs. Prior source follows.\n{}\nMeasured feedback follows.\n{}",request.iteration,request.previous_source,feedback.unwrap_or(Value::Null))}),
        ];
        content.extend(self.photos.clone());
        if let Ok(scene) =
            superspec_engine::Session::default().compile(request.previous_source, 0, assets)
        {
            if let Some(context) = &scene.manifest.architecture {
                for reference in context.metadata.iter().filter(|n| n.kind == "reference") {
                    if let Ok(png) = superspec_engine::reconstruction::render_png(
                        &scene, reference, 512, &cancelled,
                    ) {
                        content.push(json!({"type":"input_text","text":format!("Current compiled candidate view {}. This is feedback from generated geometry.",reference.id)}));
                        content.push(json!({"type":"input_image","image_url":format!("data:image/png;base64,{}",STANDARD.encode(png)),"detail":"high"}));
                    }
                }
            }
        }
        let guide = include_str!("../../../../../docs/AML.md")
            .chars()
            .take(24000)
            .collect::<String>();
        let body = json!({"model":self.name,"store":false,"max_output_tokens":30000,"instructions":format!("Generate one coherent right-handed Z-up architectural AML scene. Use metres, preserve physical circulation and valid support, and use the supplied independent evidence to revise geometry.\n{guide}"),"input":[{"role":"user","content":content}],"text":{"format":{"type":"json_schema","name":"superspec_candidate","strict":true,"schema":{"type":"object","additionalProperties":false,"properties":{"aml":{"type":"string"},"rationale":{"type":"string"}},"required":["aml","rationale"]}}}});
        let mut last = String::new();
        for attempt in 0..3 {
            if cancelled() {
                return Err("Reconstruction request cancelled.".into());
            }
            if attempt > 0 {
                for _ in 0..(20 << attempt) {
                    if cancelled() {
                        return Err("Reconstruction retry cancelled.".into());
                    }
                    std::thread::sleep(Duration::from_millis(50));
                }
            }
            let client = self.client.clone();
            let endpoint = self.endpoint.clone();
            let key = self.key.clone();
            let body = body.clone();
            let (sender, receiver) = mpsc::channel();
            std::thread::spawn(move || {
                let outcome = (|| {
                    let response = client
                        .post(endpoint)
                        .bearer_auth(key)
                        .json(&body)
                        .send()
                        .map_err(|e| e.to_string())?;
                    let status = response.status().as_u16();
                    let mut bytes = vec![];
                    response
                        .take(32 * 1024 * 1024 + 1)
                        .read_to_end(&mut bytes)
                        .map_err(|e| e.to_string())?;
                    if bytes.len() > 32 * 1024 * 1024 {
                        return Err("Model response exceeds its byte allowance.".into());
                    }
                    let payload: Value = serde_json::from_slice(&bytes)
                        .map_err(|e| format!("Invalid model response JSON: {e}"))?;
                    Ok::<_, String>((status, payload))
                })();
                let _ = sender.send(outcome);
            });
            let response = loop {
                if cancelled() {
                    return Err("Reconstruction request cancelled.".into());
                }
                match receiver.recv_timeout(Duration::from_millis(50)) {
                    Ok(value) => break value,
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                    Err(error) => return Err(error.to_string()),
                }
            };
            let (status, payload) = match response {
                Ok(value) => value,
                Err(error) => {
                    last = error;
                    continue;
                }
            };
            if !(200..300).contains(&status) {
                last = format!(
                    "Model request failed ({status}): {}",
                    payload["error"]["message"]
                        .as_str()
                        .unwrap_or("unknown provider error")
                );
                if (status == 429 && payload["error"]["code"] != "insufficient_quota")
                    || status >= 500
                {
                    continue;
                }
                return Err(last);
            }
            if payload["status"] == "incomplete" {
                return Err(format!(
                    "Model response incomplete: {}",
                    payload["incomplete_details"]["reason"]
                ));
            }
            let parts = payload["output"]
                .as_array()
                .ok_or("Model response has no output array.")?
                .iter()
                .filter(|item| item["type"] == "message")
                .flat_map(|item| item["content"].as_array().into_iter().flatten())
                .collect::<Vec<_>>();
            if let Some(refusal) = parts.iter().find(|p| p["type"] == "refusal") {
                return Err(format!(
                    "Model refused reconstruction: {}",
                    refusal["refusal"]
                ));
            }
            let text = parts
                .iter()
                .filter(|p| p["type"] == "output_text")
                .filter_map(|p| p["text"].as_str())
                .collect::<String>();
            let candidate: Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
            return candidate["aml"]
                .as_str()
                .filter(|s| !s.trim().is_empty())
                .map(|s| Some(s.to_owned()))
                .ok_or_else(|| "Model response has no AML source.".into());
        }
        Err(last)
    }
}

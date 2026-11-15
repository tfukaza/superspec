//! Isolated edit benchmark: image admission reuse, not the detailed courtyard acceptance target.
use std::time::Instant;
use superspec_engine::Session;
fn main() {
    let path = std::env::args().nth(1).expect("PNG/JPEG path");
    let bytes = std::fs::read(&path).expect("image");
    let assets = |_: &str| Ok(bytes.clone());
    let cold = Session::new(0).unwrap();
    let reused = Session::default();
    let source = |width: f64| {
        format!(
            r##"<architecture version="1" id="image-admission"><material id="m" texture="image.png"/><box id="box" material="m" width="{width}" depth="2" height="3"/></architecture>"##
        )
    };
    let initial = source(2.);
    let start = Instant::now();
    let first = reused.compile(&initial, 0, &assets).unwrap().packet();
    let cold_ms = start.elapsed().as_secs_f64() * 1000.;
    assert_eq!(first, cold.compile(&initial, 0, &assets).unwrap().packet());
    let mut samples = Vec::new();
    // Interleave ABBA cohorts; every timed call uses a changed source and includes LSP assembly.
    for round in 0..4 {
        for (step, cached) in [false, true, true, false].into_iter().enumerate() {
            let source = source(2. + (round * 4 + step + 1) as f64 * 0.01);
            let session = if cached { &reused } else { &cold };
            let start = Instant::now();
            let packet = session.compile(&source, 0, &assets).unwrap().packet();
            let ms = start.elapsed().as_secs_f64() * 1000.;
            let reference = Session::new(0)
                .unwrap()
                .compile(&source, 0, &assets)
                .unwrap()
                .packet();
            assert_eq!(packet, reference);
            samples.push(
                serde_json::json!({"cached":cached,"milliseconds":ms,"packetBytes":packet.len()}),
            );
        }
    }
    println!(
        "{}",
        serde_json::json!({"scope":"two-meter box with one real texture; not courtyard", "asset":path,"encodedBytes":bytes.len(),"coldMilliseconds":cold_ms,"samples":samples,"cache":reused.stats(),"freshPacketParity":true})
    );
}

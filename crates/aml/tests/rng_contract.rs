use serde_json::Value;
use superspec_aml::rng::SeededRng;

#[test]
fn frozen_numeric_and_unicode_streams_match_the_previous_generator() {
    let fixture: Value = serde_json::from_str(include_str!("fixtures/rng.json")).unwrap();
    for case in fixture["cases"].as_array().unwrap() {
        let mut rng = if let Some(seed) = case["seed"].as_str() {
            SeededRng::from_text(seed)
        } else {
            SeededRng::from_u32(case["seed"].as_u64().unwrap() as u32)
        };
        let mut fractional = rng.clone();
        for expected in case["words"].as_array().unwrap() {
            let expected = expected.as_u64().unwrap() as u32;
            assert_eq!(rng.next_u32(), expected, "seed {}", case["seed"]);
            assert_eq!(fractional.next_f64(), f64::from(expected) / 4_294_967_296.0);
        }
        assert_eq!(
            fractional.next_f64(),
            f64::from(rng.next_u32()) / 4_294_967_296.0
        );
    }
}

#[test]
fn zero_uses_the_existing_nonzero_fallback() {
    let mut zero = SeededRng::from_u32(0);
    let mut fallback = SeededRng::from_u32(0x9e37_79b9);
    for _ in 0..128 {
        assert_eq!(zero.next_u32(), fallback.next_u32());
    }
}

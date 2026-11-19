//! Reads the portable lighting record a scene carries (`Manifest::lighting`) from JSON.
//! The record and its limits are in `superspec_types::lighting`.
pub use superspec_types::lighting::{Invalid, Lighting};

/// Admits a JSON lighting record: unknown fields, wrong types and a missing `version` are refused
/// with their line and column, out-of-range values by field path. Missing fields take the shipped
/// defaults.
pub fn from_json(text: &str) -> Result<Lighting, Vec<Invalid>> {
    let lighting: Lighting = serde_json::from_str(text).map_err(|error| {
        vec![Invalid {
            field: String::new(),
            message: error.to_string(),
        }]
    })?;
    let findings = lighting.validate();
    if findings.is_empty() {
        Ok(lighting)
    } else {
        Err(findings)
    }
}

/// The lighting record a project document carries in its optional `lighting` field, admitted as
/// `from_json` admits one; `None` when the project has no such field and so the shipped look.
/// The authoring engine's project ignores the field: a host that delivers the project reads it
/// here and hands it to the exporter (`Manifest::lighting`).
pub fn of_project(document: &serde_json::Value) -> Result<Option<Lighting>, Vec<Invalid>> {
    let Some(record) = document.get("lighting") else {
        return Ok(None);
    };
    let lighting: Lighting = serde_json::from_value(record.clone()).map_err(|error| {
        vec![Invalid {
            field: String::new(),
            message: error.to_string(),
        }]
    })?;
    let findings = lighting.validate();
    if findings.is_empty() {
        Ok(Some(lighting))
    } else {
        Err(findings)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Scene;
    use serde_json::{Value, json};

    /// The shipped look as JSON. The Studio viewer's `DEFAULT_LIGHTING` is checked against the
    /// same file (`tests/browser/lighting.test.ts`), so the two defaults cannot drift apart.
    const DEFAULT: &str = include_str!("lighting.default.json");
    /// Records both implementations must refuse, each with the field a range finding names
    /// (`structural` ones are refused while reading, before any field is checked).
    const INVALID: &str = include_str!("lighting.invalid.json");

    #[test]
    fn default_matches_the_shared_fixture() {
        let expected: Value = serde_json::from_str(DEFAULT).unwrap();
        let actual = serde_json::to_value(Lighting::default()).unwrap();
        assert_eq!(
            actual,
            expected,
            "lighting.default.json is stale; the default is\n{}",
            serde_json::to_string_pretty(&actual).unwrap()
        );
    }

    #[test]
    fn version_alone_is_the_shipped_look() {
        assert_eq!(from_json(r#"{"version":1}"#), Ok(Lighting::default()));
        assert_eq!(from_json(DEFAULT), Ok(Lighting::default()));
    }

    #[test]
    fn round_trips_through_json() {
        let mut lighting = Lighting::default();
        lighting.exposure = 1.25;
        lighting.environment.source = "none".into();
        lighting.ambient_occlusion.enabled = false;
        let sun = &mut lighting.lights[0];
        sun.direction = [0.6, 0.0, 0.8];
        sun.casts_shadow = false;
        sun.shadow.tint = [0.1, 0.2, 0.9];
        sun.shadow.noise.seed = u32::MAX;
        let text = serde_json::to_string(&lighting).unwrap();
        assert_eq!(from_json(&text), Ok(lighting.clone()));
        // Delivered names are camelCase, as other delivered records.
        let value: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(value["toneMapping"], "aces");
        assert_eq!(value["ambientOcclusion"]["enabled"], false);
        assert_eq!(value["lights"][0]["castsShadow"], false);
        assert_eq!(value["lights"][0]["shadow"]["maxSoftness"], 0.06);
        assert_eq!(value["lights"][0]["shadow"]["noise"]["seed"], 4294967295u32);
    }

    #[test]
    fn partial_records_keep_the_other_defaults() {
        let lighting = from_json(
            r#"{"version":1,"lights":[{"shadow":{"style":"plain","noise":{"seed":3}}}],"ambient":{"intensity":0.2}}"#,
        )
        .unwrap();
        let mut expected = Lighting::default();
        expected.ambient.intensity = 0.2;
        expected.lights[0].shadow.style = superspec_types::lighting::ShadowStyle::Plain;
        expected.lights[0].shadow.noise.seed = 3;
        assert_eq!(lighting, expected);
        assert_eq!(
            from_json(r#"{"version":1,"lights":[]}"#).unwrap().lights,
            []
        );
    }

    #[test]
    fn refuses_every_shared_invalid_case() {
        let cases: Vec<Value> = serde_json::from_str(INVALID).unwrap();
        assert!(cases.len() >= 12);
        for case in cases {
            let name = case["name"].as_str().unwrap();
            let findings = from_json(&case["record"].to_string())
                .expect_err(&format!("{name} must be refused"));
            if case["structural"] == json!(true) {
                assert_eq!(findings.len(), 1, "{name}");
                assert_eq!(findings[0].field, "", "{name}");
                assert!(findings[0].to_string().starts_with("lighting: "), "{name}");
            } else {
                let fields: Vec<&str> = findings.iter().map(|invalid| &*invalid.field).collect();
                assert_eq!(fields, [case["field"].as_str().unwrap()], "{name}");
            }
        }
    }

    #[test]
    fn a_project_document_carries_an_optional_record() {
        assert_eq!(of_project(&json!({"kind": "material"})), Ok(None));
        let project = json!({"kind": "material", "lighting": {"version": 1, "exposure": 2.0}});
        let mut expected = Lighting::default();
        expected.exposure = 2.0;
        assert_eq!(of_project(&project), Ok(Some(expected)));
        // A stated record is admitted like any other: no version, null and a range fault are refused.
        for record in [json!({"exposure": 2.0}), Value::Null] {
            let findings = of_project(&json!({"lighting": record})).unwrap_err();
            assert_eq!(findings[0].field, "");
        }
        let findings = of_project(&json!({"lighting": {"version": 1, "exposure": -1.0}}));
        assert_eq!(findings.unwrap_err()[0].field, "exposure");
    }

    #[test]
    fn painted_finishing_is_a_scene_setting_written_only_when_changed() {
        let json = serde_json::to_value(Lighting::default()).unwrap();
        assert!(json.get("painted").is_none(), "{json}");
        let read = from_json(r#"{"version":1,"painted":{"toneMapping":true}}"#).unwrap();
        assert!(read.painted.tone_mapping && read.painted.ambient_occlusion);
        assert_eq!(
            serde_json::to_value(&read).unwrap()["painted"],
            json!({"toneMapping": true, "ambientOcclusion": true})
        );
        assert!(from_json(r#"{"version":1,"painted":{"tint":1}}"#).is_err());
    }

    #[test]
    fn form_light_and_contours_are_written_only_when_set() {
        let json = serde_json::to_value(Lighting::default()).unwrap();
        assert!(json.get("contours").is_none(), "{json}");
        assert!(json["lights"][0]["shadow"].get("form").is_none(), "{json}");
        let read = from_json(
            r#"{"version":1,"contours":{"enabled":true,"width":2.5},"lights":[{"shadow":{"form":0.35}}]}"#,
        )
        .unwrap();
        assert!(
            read.contours.enabled && read.contours.width == 2.5 && read.contours.strength == 1.0
        );
        assert_eq!(read.lights[0].shadow.form, 0.35);
        let written = serde_json::to_value(&read).unwrap();
        assert_eq!(written["contours"]["darken"], 0.6);
        assert!(written["contours"].get("color").is_none());
        assert_eq!(written["lights"][0]["shadow"]["form"], 0.35);
        assert!(written["lights"][0]["shadow"].get("formFill").is_none());
        assert_eq!(
            from_json(r#"{"version":1,"contours":{"width":9}}"#).unwrap_err()[0].field,
            "contours.width"
        );
        assert!(from_json(r#"{"version":1,"contours":{"glow":1}}"#).is_err());
    }

    #[test]
    fn a_scene_without_a_record_delivers_no_lighting_key() {
        let mut scene = Scene::empty(1, "test");
        let manifest = serde_json::to_value(&scene.manifest).unwrap();
        assert!(manifest.get("lighting").is_none());
        scene.manifest.lighting = Some(Lighting::default());
        let manifest = serde_json::to_value(&scene.manifest).unwrap();
        assert_eq!(
            manifest["lighting"],
            serde_json::from_str::<Value>(DEFAULT).unwrap()
        );
    }
}

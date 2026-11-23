use superspec_aml::{architecture::parse_with_registry, extensions::Registry};
use superspec_profiles::{default_registry, parse};

#[test]
fn default_kowloon_profile_preserves_defaults_and_registration_is_explicit() {
    let text = "<architecture version=\"1\" id=\"scene\"><dressing seed=\"seed\"><floor host=\"walk\"/></dressing></architecture>";
    let default = parse(text).unwrap();
    let explicit = parse_with_registry(text, default_registry()).unwrap();
    assert_eq!(
        serde_json::to_value(&default.root).unwrap(),
        serde_json::to_value(&explicit.root).unwrap()
    );
    let settings = &explicit.metadata[0];
    assert_eq!(settings.text("theme"), Some("kowloon-street@1"));
    assert_eq!(settings.number("grime", -1.), 0.4);
    assert_eq!(
        settings.children[0].text("material"),
        Some("builtin:concrete")
    );
    let named = parse(&text.replace(
        "<dressing seed",
        "<dressing theme=\"kowloon-street@1\" seed",
    ))
    .unwrap();
    assert_eq!(
        serde_json::to_value(&settings.attributes).unwrap(),
        serde_json::to_value(&named.metadata[0].attributes).unwrap()
    );
    assert_eq!(
        serde_json::to_value(&settings.children[0].attributes).unwrap(),
        serde_json::to_value(&named.metadata[0].children[0].attributes).unwrap()
    );
    assert!(
        parse_with_registry(
            "<architecture version=\"1\" id=\"plain\"><box id=\"b\"/></architecture>",
            &Registry::new(&[]).unwrap()
        )
        .is_ok()
    );
    assert!(parse_with_registry(text, &Registry::new(&[]).unwrap()).is_err());
    assert!(superspec_aml::architecture::parse(text).is_err());
    assert!(superspec_aml::format::serialize(&default).is_err());
    let output = superspec_profiles::serialize(&default).unwrap();
    let reparsed = parse(&output).unwrap();
    assert_eq!(
        serde_json::to_value(&reparsed.metadata[0].attributes).unwrap(),
        serde_json::to_value(&settings.attributes).unwrap()
    );
    assert_eq!(
        serde_json::to_value(&reparsed.metadata[0].children[0].attributes).unwrap(),
        serde_json::to_value(&settings.children[0].attributes).unwrap()
    );
    let duplicate = text.replace(
        "</architecture>",
        "<dressing seed=\"other\"/></architecture>",
    );
    assert_eq!(parse(&duplicate).unwrap_err().code, "AML_DRESSING");
    assert_eq!(
        parse(&text.replace(
            "<dressing seed",
            "<dressing theme=\"kowloon-street@2\" seed"
        ))
        .unwrap_err()
        .code,
        "AML_DRESSING"
    );
    let unknown = text.replace("floor", "unknown-recipe");
    let failure = parse(&unknown).unwrap_err();
    assert_eq!(failure.code, "AML_CHILD");
    let start = unknown.find("<unknown-recipe").unwrap();
    assert_eq!(
        failure.source.offset,
        unknown[..start].encode_utf16().count()
    );
}

#[test]
fn retired_building_dressing_reports_the_city_replacement_at_the_source() {
    for recipe in ["<building host='house'/>", "<spans hosts='house lane'/>"] {
        let text = format!(
            "<architecture version='1' id='scene'>\n  <dressing seed='seed'>\n    {recipe}\n  </dressing>\n</architecture>"
        );
        let failure = parse(&text).unwrap_err();
        assert_eq!(failure.code, "AML_DRESSING_REMOVED");
        assert_eq!(failure.source.line, 3);
        assert!(failure.message.contains("<city><building"));
    }
}

#[test]
fn hosts_compose_kowloon_with_custom_checks_without_changing_the_shared_profile() {
    use superspec_aml::{
        architecture::{Document, Node},
        diagnostic::Diagnostic,
        extensions::{Check, Element, Extension, Tag},
        source::AmlSource,
    };
    fn decode(element: Element<'_, '_>, source: &AmlSource<'_>) -> Result<Node, Diagnostic> {
        Ok(Node {
            kind: "project-note".into(),
            id: String::new(),
            source: source.location(element),
            attributes: Default::default(),
            children: vec![],
        })
    }
    fn check(document: &Document) -> Result<(), Diagnostic> {
        let note = document
            .metadata
            .iter()
            .find(|node| node.kind == "project-note");
        if let Some(note) = note {
            if !document.declarations.contains_key("finish") {
                return Err(note.fail(
                    "PROJECT_FINISH",
                    "Project note requires an authored finish.",
                ));
            }
        }
        Ok(())
    }
    static PROJECT: Extension = Extension {
        id: "project",
        version: 1,
        aml_versions: &[1],
        tags: &[Tag {
            name: "project-note",
            parent: "architecture",
            selector: None,
        }],
        decode,
        checks: &[Check {
            id: "project-finish",
            run: check,
        }],
    };
    let source = "<architecture version=\"1\" id=\"scene\"><project-note/><material id=\"finish\"/><dressing seed=\"seed\"/></architecture>";
    let combined = default_registry().with_extensions(&[&PROJECT]).unwrap();
    let document = parse_with_registry(source, &combined).unwrap();
    assert_eq!(document.metadata.len(), 2);
    let output = superspec_aml::format::serialize_with_registry(&document, &combined).unwrap();
    assert_eq!(
        parse_with_registry(&output, &combined)
            .unwrap()
            .metadata
            .len(),
        2
    );
    assert_eq!(parse(source).unwrap_err().code, "AML_CHILD");
    let custom_only = Registry::new(&[&PROJECT]).unwrap();
    assert_eq!(
        parse_with_registry(source, &custom_only).unwrap_err().code,
        "AML_CHILD"
    );
    assert_eq!(
        parse_with_registry(&source.replace("<material id=\"finish\"/>", ""), &combined)
            .unwrap_err()
            .code,
        "PROJECT_FINISH"
    );
    assert!(parse_with_registry(source, &combined).is_ok());
}

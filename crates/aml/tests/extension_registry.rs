use std::collections::BTreeMap;
use superspec_aml::{
    architecture::{Document, Node, Value, parse, parse_with_registry},
    diagnostic::Diagnostic,
    extensions::{Check, Element, Extension, Registry, Selector, Tag},
    format::serialize_with_registry,
    source::AmlSource,
};

fn decode(element: Element<'_, '_>, source: &AmlSource<'_>) -> Result<Node, Diagnostic> {
    for attribute in element.attributes() {
        if !["id", "extension", "material"].contains(&attribute.name()) {
            return Err(source.failure(element, "FIXTURE_ATTRIBUTE", "Unknown fixture attribute."));
        }
    }
    if element.children().any(|child| child.is_element()) {
        return Err(source.failure(element, "FIXTURE_CHILD", "Fixture accepts no children."));
    }
    Ok(Node {
        kind: element.tag_name().name().into(),
        id: element.attribute("id").unwrap_or("").into(),
        source: source.location(element),
        attributes: element
            .attributes()
            .map(|a| (a.name().into(), Value::Text(a.value().into())))
            .collect::<BTreeMap<_, _>>(),
        children: vec![],
    })
}

fn check(document: &Document) -> Result<(), Diagnostic> {
    for node in document
        .metadata
        .iter()
        .filter(|n| n.kind == "fixture-check")
    {
        let material = node
            .text("material")
            .and_then(|id| document.declarations.get(id));
        if material.is_none_or(|material| {
            material.kind != "material" || material.number("roughness", 1.) < 0.5
        }) {
            return Err(node.fail(
                "FIXTURE_MATERIAL",
                "Fixture requires a declared rough material.",
            ));
        }
    }
    Ok(())
}

static FIXTURE: Extension = Extension {
    id: "fixture",
    version: 1,
    aml_versions: &[1],
    tags: &[Tag {
        name: "fixture-check",
        parent: "architecture",
        selector: Some(Selector {
            attribute: "extension",
            default: None,
        }),
    }],
    decode,
    checks: &[Check {
        id: "fixture-material",
        run: check,
    }],
};
const SOURCE: &str = "<architecture version=\"1\" id=\"scene\" name=\"😀\">\n  <fixture-check id=\"check\" extension=\"fixture@1\" material=\"later\"/>\n  <material id=\"later\" roughness=\"0.75\"/>\n</architecture>";

#[test]
fn generic_entry_points_admit_core_vocabulary_and_require_registration_for_extensions() {
    let plain = "<architecture version=\"1\" id=\"plain\"><box id=\"b\"/></architecture>";
    let document = parse(plain).unwrap();
    let output = superspec_aml::format::serialize(&document).unwrap();
    assert_eq!(parse(&output).unwrap().elements[0].id, "b");
    let extension = "<architecture version=\"1\" id=\"plain\" name=\"😀\">\n  <dressing seed=\"seed\"/>\n</architecture>";
    let error = parse(extension).unwrap_err();
    assert_eq!(error.code, "AML_CHILD");
    assert_eq!(error.source.line, 2);
    assert_eq!(error.source.column, 3);
    let start = extension.find("<dressing").unwrap();
    assert_eq!(
        error.source.offset,
        extension[..start].encode_utf16().count()
    );
    assert!(parse(plain).is_ok());
}

#[test]
fn registered_checks_see_later_declarations_and_round_trip_with_the_same_profile() {
    let registry = Registry::new(&[&FIXTURE]).unwrap();
    let document = parse_with_registry(SOURCE, &registry).unwrap();
    let output = serialize_with_registry(&document, &registry).unwrap();
    let reparsed = parse_with_registry(&output, &registry).unwrap();
    assert_eq!(reparsed.metadata[0].id, "check");
    assert_eq!(reparsed.metadata[0].text("material"), Some("later"));
    assert!(parse(&output).is_err());
    let combined = Registry::default().with_extensions(&[&FIXTURE]).unwrap();
    assert!(parse_with_registry(SOURCE, &combined).is_ok());
    assert!(parse(SOURCE).is_err());
    assert!(combined.with_extensions(&[&FIXTURE]).is_err());
}

#[test]
fn missing_registration_and_invalid_payload_keep_exact_source_identity_and_utf16_span() {
    let registry = Registry::new(&[&FIXTURE]).unwrap();
    let start = SOURCE.find("<fixture-check").unwrap();
    let end = start + SOURCE[start..].find("/>").unwrap() + 2;
    for error in [
        parse_with_registry(SOURCE, &Registry::new(&[]).unwrap()).unwrap_err(),
        parse_with_registry(&SOURCE.replace("0.75", "0.25"), &registry).unwrap_err(),
    ] {
        assert_eq!(error.objects, ["check"]);
        assert_eq!(error.source.line, 2);
        assert_eq!(error.source.column, 3);
        assert_eq!(error.source.offset, SOURCE[..start].encode_utf16().count());
        assert_eq!(error.source.end, Some(SOURCE[..end].encode_utf16().count()));
    }
    let invalid = parse_with_registry(&SOURCE.replace("0.75", "0.25"), &registry).unwrap_err();
    assert_eq!(invalid.code, "FIXTURE_MATERIAL");
    assert!(parse_with_registry(SOURCE, &registry).is_ok());
}

#[test]
fn registration_and_scope_errors_cannot_silently_admit_source() {
    let registry = Registry::new(&[&FIXTURE]).unwrap();
    for (source, code, offending) in [
        (
            SOURCE.replace("fixture@1", "fixture@2"),
            "AML_EXTENSION_VERSION",
            "<fixture-check",
        ),
        (
            SOURCE.replace(" extension=\"fixture@1\"", ""),
            "AML_EXTENSION_VERSION",
            "<fixture-check",
        ),
        (
            SOURCE.replace("fixture-check", "unknown-check"),
            "AML_CHILD",
            "<unknown-check",
        ),
        (
            SOURCE
                .replace("  <fixture-check", "  <group id=\"g\"><fixture-check")
                .replace("material=\"later\"/>", "material=\"later\"/></group>"),
            "AML_CHILD",
            "<fixture-check",
        ),
        (
            SOURCE.replace("id=\"check\"", "id=\"later\""),
            "AML_DUPLICATE_ID",
            "<material",
        ),
        (
            SOURCE.replace("id=\"check\"", "id=\"invalid id\""),
            "AML_ID",
            "<fixture-check",
        ),
    ] {
        let error = parse_with_registry(&source, &registry).unwrap_err();
        assert_eq!(error.code, code, "{source}");
        let start = source.find(offending).unwrap();
        let end = start + source[start..].find("/>").unwrap() + 2;
        assert_eq!(
            error.source.offset,
            source[..start].encode_utf16().count(),
            "{source}"
        );
        assert_eq!(
            error.source.end,
            Some(source[..end].encode_utf16().count()),
            "{source}"
        );
    }
    assert!(Registry::new(&[&FIXTURE, &FIXTURE]).is_err());
    static SHADOW: Extension = Extension {
        id: "shadow",
        tags: &[Tag {
            name: "box",
            parent: "architecture",
            selector: None,
        }],
        ..FIXTURE
    };
    static CONFLICT: Extension = Extension {
        id: "conflict",
        checks: &[],
        ..FIXTURE
    };
    static CHECK_CONFLICT: Extension = Extension {
        id: "check-conflict",
        tags: &[Tag {
            name: "other-check",
            parent: "architecture",
            selector: None,
        }],
        ..FIXTURE
    };
    assert!(Registry::new(&[&SHADOW]).is_err());
    assert!(Registry::new(&[&FIXTURE, &CONFLICT]).is_err());
    assert!(Registry::new(&[&FIXTURE, &CHECK_CONFLICT]).is_err());
}

#[test]
fn owned_subtrees_enforce_each_selector_and_scope_before_calling_the_decoder() {
    fn decode_tree(element: Element<'_, '_>, source: &AmlSource<'_>) -> Result<Node, Diagnostic> {
        Ok(Node {
            kind: element.tag_name().name().into(),
            id: element.attribute("id").unwrap_or("").into(),
            source: source.location(element),
            attributes: element
                .attributes()
                .map(|a| (a.name().into(), Value::Text(a.value().into())))
                .collect(),
            children: element
                .children()
                .filter(Element::is_element)
                .map(|child| decode_tree(child, source))
                .collect::<Result<_, _>>()?,
        })
    }
    static TREE: Extension = Extension {
        id: "tree",
        version: 1,
        aml_versions: &[1],
        tags: &[
            Tag {
                name: "fixture-tree",
                parent: "architecture",
                selector: None,
            },
            Tag {
                name: "fixture-leaf",
                parent: "fixture-tree",
                selector: Some(Selector {
                    attribute: "extension",
                    default: None,
                }),
            },
        ],
        decode: decode_tree,
        checks: &[],
    };
    let registry = Registry::new(&[&TREE]).unwrap();
    let source = "<architecture version=\"1\" id=\"s\"><fixture-tree id=\"tree\"><fixture-leaf id=\"leaf\" extension=\"tree@1\"/></fixture-tree></architecture>";
    assert!(parse_with_registry(source, &registry).is_ok());
    for (invalid, code, tag) in [
        (
            source.replace(" extension=\"tree@1\"", ""),
            "AML_EXTENSION_VERSION",
            "<fixture-leaf",
        ),
        (
            source.replace("tree@1", "tree@2"),
            "AML_EXTENSION_VERSION",
            "<fixture-leaf",
        ),
        (
            source.replace("fixture-leaf", "unowned-leaf"),
            "AML_CHILD",
            "<unowned-leaf",
        ),
    ] {
        let error = parse_with_registry(&invalid, &registry).unwrap_err();
        assert_eq!(error.code, code);
        assert_eq!(error.objects, ["leaf"]);
        let start = invalid.find(tag).unwrap();
        let end = start + invalid[start..].find("/>").unwrap() + 2;
        assert_eq!(error.source.offset, invalid[..start].encode_utf16().count());
        assert_eq!(
            error.source.end,
            Some(invalid[..end].encode_utf16().count())
        );
    }
    fn must_not_decode(_: Element<'_, '_>, _: &AmlSource<'_>) -> Result<Node, Diagnostic> {
        panic!("invalid ownership must fail before extension decoding")
    }
    static GUARDED: Extension = Extension {
        decode: must_not_decode,
        ..TREE
    };
    let guarded = Registry::new(&[&GUARDED]).unwrap();
    assert_eq!(
        parse_with_registry(&source.replace("fixture-leaf", "unowned-leaf"), &guarded)
            .unwrap_err()
            .code,
        "AML_CHILD"
    );
    static UNSUPPORTED_AML: Extension = Extension {
        aml_versions: &[2],
        ..TREE
    };
    let unsupported = Registry::new(&[&UNSUPPORTED_AML]).unwrap();
    assert_eq!(
        parse_with_registry(source, &unsupported).unwrap_err().code,
        "AML_EXTENSION_VERSION"
    );
}

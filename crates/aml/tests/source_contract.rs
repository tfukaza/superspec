use superspec_aml::source::{SourceLimits, parse_source};

#[test]
fn inspection_preserves_source_and_does_not_claim_semantic_validation() {
    let text = "<architecture version='1' id='a' name='A &amp; B'><unsupported id='same'/><box id='same' width='nonsense'/></architecture>";
    let source = parse_source(text, SourceLimits::default()).unwrap();
    let result = source.inspect();
    assert_eq!(result.name, "A & B");
    assert!(result.syntax_valid);
    assert_eq!(result.semantic_validation, "not-run");
    assert_eq!(result.geometry_validation, "not-run");
    assert_eq!(result.navigation_validation, "not-run");
    assert_eq!(result.element_count, 3);
    assert_eq!(result.tags["unsupported"], 1);
    assert_eq!(source.root().attribute("name"), Some("A & B"));
}

#[test]
fn maintained_courtyard_source_is_inspectable_without_compiling_or_reading_assets() {
    let source = parse_source(
        include_str!("fixtures/courtyard.aml"),
        SourceLimits::default(),
    )
    .unwrap();
    let inspection = source.inspect();
    assert_eq!(inspection.id, "dressing-demo");
    assert_eq!(inspection.tags["building"], 3);
    assert_eq!(inspection.tags["dressing"], 1);
    assert_eq!(inspection.semantic_validation, "not-run");
}

#[test]
fn xml_failures_never_produce_a_successful_inspection() {
    for text in [
        "",
        "<architecture",
        "<architecture version='1' id='a'></wrong>",
        "<architecture version='1' id='a'/><architecture version='1' id='b'/>",
        "<architecture version='1' id='a' id='b'/>",
        "<architecture version='1' id='a' name='&unknown;'/>",
        "<!DOCTYPE architecture [<!ENTITY stolen SYSTEM 'file:///not-readable'>]><architecture version='1' id='a'/>",
        "<!DOCTYPE architecture><architecture version='1' id='a'/>",
        "<architecture version='1' id='a'>text</architecture>",
        "<architecture version='1' id='a'><![CDATA[ignored?]]></architecture>",
        "<architecture version='1' id='a'><x xmlns='urn:other'/></architecture>",
    ] {
        let failure = parse_source(text, SourceLimits::default())
            .err()
            .expect(text);
        assert_eq!(failure.code, "XML_SYNTAX", "{text}");
        assert_eq!(failure.severity, "error");
        assert!(failure.source.line >= 1 && failure.source.column >= 1);
    }
}

#[test]
fn declarations_comments_and_xml_character_references_are_supported() {
    let text = "<?xml version='1.0' encoding='UTF-8'?><!-- <!DOCTYPE is just a comment> -->\n<architecture version='1' id='a' name='&#x1F600; &quot; &apos; &lt; &gt; &amp;'><![CDATA[ \n ]]></architecture>";
    let source = parse_source(text, SourceLimits::default()).unwrap();
    assert_eq!(source.inspect().name, "😀 \" ' < > &");
}

#[test]
fn root_identity_is_required_without_claiming_child_semantics() {
    for (text, code) in [
        ("<svg/>", "AML_ROOT"),
        (
            "<architecture xmlns='urn:other' version='1' id='a'/>",
            "AML_ROOT",
        ),
        ("<architecture version='2' id='a'/>", "AML_VERSION"),
        ("<architecture id='a'/>", "AML_VERSION"),
        ("<architecture version='1'/>", "AML_ATTRIBUTE"),
        ("<architecture version='1' id='2bad'/>", "AML_ATTRIBUTE"),
        (
            "<architecture version='1' id='bad space'/>",
            "AML_ATTRIBUTE",
        ),
    ] {
        assert_eq!(
            parse_source(text, SourceLimits::default())
                .err()
                .unwrap()
                .code,
            code
        );
    }
}

#[test]
fn utf16_depth_and_node_limits_have_explicit_boundaries() {
    let text = "<architecture version='1' id='a' name='😀窗'/>";
    let units = text.encode_utf16().count();
    for budget in [units, units + 1] {
        assert!(
            parse_source(
                text,
                SourceLimits {
                    utf16_units: budget,
                    ..SourceLimits::default()
                }
            )
            .is_ok()
        );
    }
    assert!(
        parse_source(
            text,
            SourceLimits {
                utf16_units: units - 1,
                ..SourceLimits::default()
            }
        )
        .is_err()
    );
    let nested = "<architecture version='1' id='a'><group><box/></group></architecture>";
    for depth in [3, 4] {
        assert!(
            parse_source(
                nested,
                SourceLimits {
                    depth,
                    ..SourceLimits::default()
                }
            )
            .is_ok()
        );
    }
    assert!(
        parse_source(
            nested,
            SourceLimits {
                depth: 2,
                ..SourceLimits::default()
            }
        )
        .is_err()
    );
    for nodes in [2, 3] {
        assert!(
            parse_source(
                text,
                SourceLimits {
                    nodes,
                    ..SourceLimits::default()
                }
            )
            .is_ok()
        );
    }
    assert!(
        parse_source(
            text,
            SourceLimits {
                nodes: 1,
                ..SourceLimits::default()
            }
        )
        .is_err()
    );
    assert!(
        parse_source(
            text,
            SourceLimits {
                nodes: 0,
                ..SourceLimits::default()
            }
        )
        .is_err()
    );
}

#[test]
fn source_ranges_use_utf16_lf_lines_and_exclusive_ends() {
    for newline in ["\n", "\r\n", "\r"] {
        let text = format!(
            "<!-- 😀e\u{301} -->{newline}<architecture version='1' id='a' name='窗😀&amp;'>{newline}  <box/>{newline}</architecture>"
        );
        let source = parse_source(&text, SourceLimits::default()).unwrap();
        let child = source
            .root()
            .children()
            .find(|node| node.is_element())
            .unwrap();
        let start = text.find("<box/>").unwrap();
        let end = start + "<box/>".len();
        let prefix = &text[..start];
        let location = source.location(child);
        assert_eq!(location.offset, prefix.encode_utf16().count());
        assert_eq!(location.end, Some(text[..end].encode_utf16().count()));
        assert_eq!(
            location.line,
            prefix.bytes().filter(|byte| *byte == b'\n').count() + 1
        );
        assert_eq!(
            location.column,
            prefix.rsplit('\n').next().unwrap().encode_utf16().count() + 1
        );
        assert_eq!(
            source.inspect().source.end,
            Some(text.encode_utf16().count())
        );
    }
}

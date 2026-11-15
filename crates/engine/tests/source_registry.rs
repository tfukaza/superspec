use std::{
    cell::Cell,
    sync::atomic::{AtomicUsize, Ordering},
};
use superspec_aml::{
    architecture::{Document, Node, Value},
    diagnostic::Diagnostic,
    extensions::{Check, Element, Extension, Registry, Selector, Tag},
    source::AmlSource,
};
use superspec_engine::{compile_with_assets, compile_with_registry, source::prepare};

static CHECKS: AtomicUsize = AtomicUsize::new(0);
fn decode(element: Element<'_, '_>, source: &AmlSource<'_>) -> Result<Node, Diagnostic> {
    if element
        .attributes()
        .any(|a| !["id", "extension", "material"].contains(&a.name()))
        || element.children().any(|c| c.is_element())
    {
        return Err(source.failure(
            element,
            "FIXTURE_INPUT",
            "Fixture accepts only id, extension and material.",
        ));
    }
    Ok(Node {
        kind: element.tag_name().name().into(),
        id: element.attribute("id").unwrap_or("").into(),
        source: source.location(element),
        attributes: element
            .attributes()
            .map(|a| (a.name().into(), Value::Text(a.value().into())))
            .collect(),
        children: vec![],
    })
}
fn check(document: &Document) -> Result<(), Diagnostic> {
    CHECKS.fetch_add(1, Ordering::SeqCst);
    for node in document
        .metadata
        .iter()
        .filter(|n| n.kind == "fixture-check")
    {
        if node
            .text("material")
            .and_then(|id| document.declarations.get(id))
            .is_none_or(|material| {
                material.kind != "material" || material.number("roughness", 1.) < 0.5
            })
        {
            return Err(node.fail("FIXTURE_MATERIAL", "A rough authored material is required."));
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

#[test]
fn source_checks_precede_geometry_and_assets_and_run_once_without_poisoning_other_profiles() {
    let source = "<architecture version=\"1\" id=\"s\" name=\"😀\"><fixture-check id=\"check\" extension=\"fixture@1\" material=\"later\"/><material id=\"later\" roughness=\"0.75\" texture=\"unread.png\"/><box id=\"box\" width=\"-1\" material=\"later\"/></architecture>";
    let registry = Registry::new(&[&FIXTURE]).unwrap();
    CHECKS.store(0, Ordering::SeqCst);
    let prepared = prepare(source, &registry).unwrap();
    assert_eq!(
        prepared.document().declarations["later"].text("texture"),
        Some("unread.png")
    );
    assert_eq!(CHECKS.load(Ordering::SeqCst), 1);
    let reads = Cell::new(0);
    let assets = |_: &str| {
        reads.set(reads.get() + 1);
        Err("Unexpected image read".into())
    };
    let invalid = source.replace("0.75", "0.25");
    let error = compile_with_registry(&invalid, 0, &assets, &registry).unwrap_err();
    assert_eq!(error.code, "FIXTURE_MATERIAL");
    assert_eq!(error.objects, ["check"]);
    let start = source.find("<fixture-check").unwrap();
    let end = start + source[start..].find("/>").unwrap() + 2;
    assert_eq!(error.source.offset, source[..start].encode_utf16().count());
    assert_eq!(error.source.end, Some(source[..end].encode_utf16().count()));
    assert_eq!(reads.get(), 0);
    assert_eq!(CHECKS.load(Ordering::SeqCst), 2);
    assert!(compile_with_registry(source, 0, &assets, &registry).is_err());
    assert_eq!(reads.get(), 0);
    assert_eq!(CHECKS.load(Ordering::SeqCst), 3);
    let valid = source
        .replace(" width=\"-1\"", "")
        .replace(" texture=\"unread.png\"", "");
    let scene = compile_with_registry(&valid, 0, &assets, &registry).unwrap();
    assert!(scene.manifest.success);
    assert_eq!(CHECKS.load(Ordering::SeqCst), 4);
    assert!(compile_with_assets(&valid, 0, &assets).is_err());
    assert_eq!(CHECKS.load(Ordering::SeqCst), 4);
    assert!(compile_with_registry(&valid, 0, &assets, &registry).is_ok());
}

#[test]
fn plain_and_kowloon_scenes_preserve_packets_with_explicit_registry() {
    for source in [
        "<architecture version=\"1\" id=\"s\"><box id=\"box\"/></architecture>",
        "<architecture version=\"1\" id=\"s\"><material id=\"paint\"/><profile id=\"p\" points=\"0 0;2 0;2 2;0 2\"/><slab id=\"floor\" profile=\"p\"/><dressing seed=\"seed\"><floor host=\"floor\" material=\"paint\"/></dressing></architecture>",
    ] {
        let assets = |_: &str| Err("No images required".into());
        let default = compile_with_assets(source, 7, &assets).unwrap();
        let explicit =
            compile_with_registry(source, 7, &assets, superspec_profiles::default_registry())
                .unwrap();
        assert_eq!(default.packet(), explicit.packet());
        let session = superspec_engine::Session::default();
        assert_eq!(
            default.packet(),
            session.compile(source, 7, &assets).unwrap().packet()
        );
        let discovered: Vec<String> =
            serde_json::from_str(&superspec_engine::library_assets(source)).unwrap();
        if source.contains("<dressing") {
            assert!(discovered.contains(&"superspec-material:concrete-shared.png".into()));
            let error =
                compile_with_registry(source, 7, &assets, &Registry::default()).unwrap_err();
            assert_eq!(error.code, "AML_CHILD");
            assert!(session.compile(source, 7, &assets).is_ok());
        } else {
            assert!(discovered.is_empty());
            assert_eq!(
                default.packet(),
                compile_with_registry(source, 7, &assets, &Registry::default())
                    .unwrap()
                    .packet()
            );
        }
        let themed = source.replace(
            "<dressing seed",
            "<dressing theme=\"kowloon-street@1\" seed",
        );
        let explicit_theme =
            compile_with_registry(&themed, 7, &assets, superspec_profiles::default_registry())
                .unwrap();
        // Source spans differ when the selector is written explicitly, but scene buffers must not.
        assert_eq!(default.positions, explicit_theme.positions);
        assert_eq!(default.indices, explicit_theme.indices);
        assert_eq!(default.colors, explicit_theme.colors);
        assert_eq!(default.asset_bytes, explicit_theme.asset_bytes);
    }
}

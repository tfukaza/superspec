use superspec_catalog::{image_name, images, material};

#[test]
fn catalog_definitions_are_independent_and_only_shipped_images_resolve() {
    let concrete = material("builtin:concrete").unwrap();
    let mut authored = material("builtin:concrete").unwrap();
    authored.id = "my-concrete".into();
    authored.texture = Some("local.png".into());
    assert_eq!(material("builtin:concrete").unwrap(), concrete);
    assert!(material("my-concrete").is_none());
    assert!(material("concrete").is_none());
    assert_eq!(
        image_name("superspec-material:concrete-shared.png"),
        Some("concrete-shared.png")
    );
    assert!(image_name("superspec-material:../concrete-shared.png").is_none());
    assert!(image_name("superspec-material:unlisted.png").is_none());

    let shipped = images();
    assert!(shipped.contains("superspec-material:concrete-shared.png"));
    assert!(shipped.contains("superspec-material:concrete-shared-normal.png"));
    assert!(shipped.iter().all(|id| image_name(id).is_some()));
    assert!(!shipped.contains("local.png"));
}

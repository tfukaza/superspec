use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};

struct Fixture(PathBuf);
impl Fixture {
    fn new(files: &[(&str, &str)]) -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let directory = std::env::temp_dir().join(format!(
            "superspec-boundaries-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&directory).unwrap();
        for (path, source) in files {
            let target = directory.join(path);
            fs::create_dir_all(target.parent().unwrap()).unwrap();
            fs::write(target, source).unwrap();
        }
        Self(directory)
    }
    fn read(&self) -> Sources {
        let mut sources = Sources::default();
        sources
            .load_crate("superspec-engine", &self.0.join("lib.rs"), &[])
            .unwrap();
        sources
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

#[test]
fn comments_strings_and_test_only_modules_do_not_create_runtime_edges() {
    let fixture = Fixture::new(&[(
        "lib.rs",
        r##"
        pub mod materialspec {
            // crate::worldspec::themes::Forbidden
            pub const LABEL: &str = r#"crate::worldspec::themes::Forbidden"#;
            #[cfg(test)] mod tests { use crate::worldspec::themes::Forbidden; }
            pub fn ordinary() { let _ = format!("crate::worldspec::themes::Forbidden"); }
        }
        pub mod worldspec { pub mod themes { pub struct Forbidden; } }
    "##,
    )]);
    assert!(fixture.read().violations().is_empty());
}

#[test]
fn aliases_reexports_nested_use_trees_and_globs_resolve_actual_owners() {
    let fixture = Fixture::new(&[(
        "lib.rs",
        r#"
        pub mod worldspec { pub mod themes { pub struct Forbidden; } }
        pub mod neutral { pub struct Allowed; }
        pub mod facade { pub use crate::worldspec::themes::*; pub use crate::neutral::*; }
        pub use worldspec::{themes::{Forbidden as Hidden}};
        pub type HiddenAlias = Hidden;
        pub mod materialspec {
            use crate as engine;
            use engine::{facade as bridge};
            pub fn allowed(_: bridge::Allowed) {}
            pub fn bad(_: bridge::Forbidden) {}
            pub fn also_bad(_: engine::HiddenAlias) {}
        }
    "#,
    )]);
    let failures = fixture.read().violations();
    assert_eq!(failures.len(), 2, "{failures:#?}");
    assert!(
        failures
            .iter()
            .all(|message| message.contains("worldspec::themes::Forbidden"))
    );
}

#[test]
fn explicit_module_paths_use_physical_owner_and_macro_paths_are_checked() {
    let fixture = Fixture::new(&[
        (
            "lib.rs",
            r#"
            pub mod worldspec { pub mod themes { pub struct Forbidden; } }
            #[path="materialspec/borrowed.rs"] pub mod disguised;
        "#,
        ),
        (
            "materialspec/borrowed.rs",
            r#"
            pub fn bad() { wrapper!(crate::worldspec::themes::Forbidden); }
        "#,
        ),
    ]);
    let failures = fixture.read().violations();
    assert_eq!(failures.len(), 1, "{failures:#?}");
    assert!(
        failures
            .first()
            .unwrap()
            .contains("superspec_engine::materialspec::borrowed ->")
    );
}

#[test]
fn renamed_crates_and_neutral_type_reexports_resolve_without_parser_false_positive() {
    let fixture = Fixture::new(&[
        (
            "lib.rs",
            "pub mod materialspec { pub fn allowed(_: parser::Diagnostic) {} pub fn bad() { parser::parse(); } }",
        ),
        ("aml/lib.rs", "pub use data::Diagnostic; pub fn parse() {}"),
        ("types/lib.rs", "pub struct Diagnostic;"),
    ]);
    let mut sources = Sources::default();
    sources
        .load_crate(
            "superspec-engine",
            &fixture.0.join("lib.rs"),
            &[Dependency {
                alias: "parser".into(),
                package: "superspec-aml".into(),
            }],
        )
        .unwrap();
    sources
        .load_crate(
            "superspec-aml",
            &fixture.0.join("aml/lib.rs"),
            &[Dependency {
                alias: "data".into(),
                package: "superspec-types".into(),
            }],
        )
        .unwrap();
    sources
        .load_crate("superspec-types", &fixture.0.join("types/lib.rs"), &[])
        .unwrap();
    let failures = sources.violations();
    assert_eq!(failures.len(), 1, "{failures:#?}");
    assert!(failures.first().unwrap().contains("superspec_aml::parse"));
}

#[test]
fn unsupported_source_generation_is_reported_instead_of_silently_ignored() {
    let fixture = Fixture::new(&[(
        "lib.rs",
        "pub mod runtime { include!(concat!(env!(\"OUT_DIR\"), \"/generated.rs\")); }",
    )]);
    let failures = fixture.read().violations();
    assert_eq!(failures.len(), 1);
    assert!(
        failures
            .first()
            .unwrap()
            .contains("include! requires a literal source path")
    );
}

#[test]
fn crate_policy_uses_dependency_identity_and_permits_only_the_portable_graph() {
    assert!(rules::crate_edge("superspec-export", "superspec-model").is_none());
    assert!(rules::crate_edge("superspec-export", "superspec-engine").is_some());
    assert!(rules::crate_edge("superspec-model", "superspec-aml").is_some());
    assert!(rules::crate_edge("superspec-types", "superspec-profiles").is_some());
}

#[test]
fn block_local_alias_chains_preserve_lexical_scope() {
    let fixture = Fixture::new(&[(
        "lib.rs",
        r#"
        pub mod worldspec { pub mod themes { pub struct Forbidden; } }
        pub mod facade { pub use crate::worldspec::themes::Forbidden; }
        pub mod materialspec {
            pub fn bad() {
                use crate::facade as a;
                use a as b;
                let _: Option<b::Forbidden> = None;
            }
        }
    "#,
    )]);
    let failures = fixture.read().violations();
    assert_eq!(failures.len(), 1, "{failures:#?}");
    assert!(
        failures
            .first()
            .unwrap()
            .contains("worldspec::themes::Forbidden")
    );
}

#[test]
fn expression_include_stack_accepts_reuse_and_rejects_recursion() {
    let fixture = Fixture::new(&[
        (
            "lib.rs",
            r#"pub mod materialspec { const A: u8 = include!("value.rs"); const B: u8 = include!("value.rs"); }"#,
        ),
        ("value.rs", "7"),
    ]);
    assert!(fixture.read().violations().is_empty());
    fs::write(fixture.0.join("value.rs"), "include!(\"value.rs\")").unwrap();
    let failures = fixture.read().violations();
    assert!(
        failures
            .iter()
            .any(|message| message.contains("recursive include!"))
    );
}

#[test]
fn explicit_composition_is_allowed_but_neutral_owners_cannot_select_it() {
    let fixture = Fixture::new(&[(
        "lib.rs",
        r#"
        pub mod architecture {
            pub mod build { pub struct PhysicalBuild; }
            pub mod kowloon { pub fn adapter() { crate::worldspec::themes::recipe(); } }
            pub fn bad() { crate::worldspec::shipped::compile(); }
        }
        pub mod worldspec {
            pub mod themes { pub fn recipe() {} }
            pub mod shipped { pub fn compile() { crate::architecture::kowloon::adapter(); } }
            pub mod build {
                pub fn allowed(_: crate::architecture::build::PhysicalBuild) {}
                pub fn bad() { super::shipped::compile(); }
            }
        }
        pub mod session { pub fn bad() { crate::worldspec::shipped::compile(); } }
    "#,
    )]);
    let failures = fixture.read().violations();
    assert_eq!(failures.len(), 3, "{failures:#?}");
    assert!(
        failures
            .iter()
            .all(|message| message.contains("worldspec::shipped::compile"))
    );
}

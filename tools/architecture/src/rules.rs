//! Architectural policy, separate from Rust name and module resolution.

pub fn checked_module(owner: &[String]) -> bool {
    owner.first().map(String::as_str) == Some("superspec_engine")
        && matches!(
            owner.get(1).map(String::as_str),
            Some(
                "materialspec"
                    | "itemspec"
                    | "treespec"
                    | "assets"
                    | "runtime"
                    | "session"
                    | "architecture"
                    | "visibility"
            )
        )
        || (owner.first().map(String::as_str) == Some("superspec_engine")
            && owner.get(1).map(String::as_str) == Some("worldspec")
            && owner.get(2).map(String::as_str) == Some("build"))
}

/// Normal workspace dependencies are intentionally narrower than test dependencies.
pub fn crate_edge(from: &str, to: &str) -> Option<&'static str> {
    let allowed: Option<&[&str]> = match from {
        "superspec-types" | "superspec-geometry" => Some(&[]),
        "superspec-model" => Some(&["superspec-types", "superspec-geometry"]),
        "superspec-catalog" => Some(&["superspec-types"]),
        "superspec-aml" => Some(&["superspec-types", "superspec-catalog"]),
        "superspec-profiles" => Some(&["superspec-aml"]),
        "superspec-export" => Some(&["superspec-types", "superspec-model", "superspec-geometry"]),
        _ => None,
    };
    allowed.filter(|allowed| !allowed.contains(&to)).map(|_| {
        "normal dependencies must follow the portable types/model/geometry and explicit AML-profile layers"
    })
}

/// Paths are canonical module identities after aliases and reexports are resolved.
pub fn module_edge(from: &[String], to: &[String]) -> Option<&'static str> {
    if from.first().map(String::as_str) != Some("superspec_engine") {
        return None;
    }
    let owner = from.get(1).map(String::as_str).unwrap_or("");
    let target_crate = to.first().map(String::as_str).unwrap_or("");
    let target = to.get(1).map(String::as_str).unwrap_or("");
    if matches!(
        owner,
        "materialspec" | "itemspec" | "treespec" | "assets" | "runtime"
    ) {
        if matches!(target_crate, "superspec_aml" | "superspec_profiles")
            || (target_crate == "superspec_engine"
                && matches!(
                    target,
                    "worldspec" | "architecture" | "resolve" | "source" | "review" | "validation"
                ))
        {
            return Some(
                "generic material/item/asset/runtime operations cannot depend on AML, themes or physical assembly",
            );
        }
    }
    if owner == "session"
        && (matches!(target_crate, "superspec_aml" | "superspec_profiles")
            || (target_crate == "superspec_engine"
                && matches!(target, "worldspec" | "architecture" | "resolve" | "source")))
    {
        return Some(
            "Session owns neutral resources; explicit profile composition supplies theme and physical adapters",
        );
    }
    if owner == "session"
        && target_crate == "superspec_engine"
        && target == "authoring"
        && to.get(2).map(String::as_str) == Some("project")
    {
        return Some("Session may retain kernel artifacts but cannot select project composition");
    }
    if owner == "visibility"
        && target_crate == "superspec_engine"
        && ((target == "worldspec"
            && matches!(to.get(2).map(String::as_str), Some("themes" | "shipped")))
            || (target == "architecture" && to.get(2).map(String::as_str) == Some("kowloon")))
    {
        return Some("generic visibility checks cannot depend on concrete themes");
    }
    if owner == "worldspec" && from.get(2).map(String::as_str) == Some("build") {
        if target_crate == "superspec_profiles"
            || (target_crate == "superspec_engine"
                && ((target == "worldspec"
                    && matches!(to.get(2).map(String::as_str), Some("themes" | "shipped")))
                    || (target == "architecture"
                        && to.get(2).map(String::as_str) != Some("build"))))
        {
            return Some(
                "World build order uses supplied theme interfaces and the typed physical-build boundary",
            );
        }
    }
    if owner == "architecture"
        && from.get(2).map(String::as_str) != Some("kowloon")
        && (target_crate == "superspec_profiles"
            || (target_crate == "superspec_engine"
                && ((target == "worldspec"
                    && matches!(to.get(2).map(String::as_str), Some("themes" | "shipped")))
                    || (target == "architecture"
                        && to.get(2).map(String::as_str) == Some("kowloon")))))
    {
        return Some(
            "physical lowering does not choose a concrete theme; the explicit receiving-surface adapter owns that dependency",
        );
    }
    None
}

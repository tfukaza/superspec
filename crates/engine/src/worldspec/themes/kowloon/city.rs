//! Maps AML roles to individual, local ItemSpec sources in the shipped theme.
use std::sync::Arc;

macro_rules! items {
    ($($id:literal),*) => {
        pub(super) fn definition(entry:&str)->Option<&'static str> {
            match entry {
                $(concat!($id,".item.json") => Some(include_str!(concat!("items/",$id,".item.json"))),)*
                _ => None,
            }
        }
    }
}
items!(
    "kowloon-aircon",
    "kowloon-avenue",
    "kowloon-building",
    "kowloon-corner",
    "kowloon-duct-bracket",
    "kowloon-duct-riser",
    "kowloon-duct-run",
    "kowloon-facade-side",
    "kowloon-floor",
    "kowloon-ground-floor",
    "kowloon-lamp",
    "kowloon-parapet",
    "kowloon-road",
    "kowloon-roof",
    "kowloon-shopfront",
    "kowloon-sidewalk",
    "kowloon-sign",
    "kowloon-tree",
    "kowloon-window"
);

pub(crate) fn request(
    node: &superspec_types::architecture::Node,
) -> Result<crate::authoring::Request, String> {
    let entry = format!("kowloon-{}.item.json", node.kind);
    let source = definition(&entry).ok_or_else(|| format!("Unknown city role {}.", node.kind))?;
    let mut parameters = std::collections::BTreeMap::new();
    for key in ["length", "width"] {
        if let Some(value) = node.optional_number(key) {
            parameters.insert(key.into(), value.into());
        }
    }
    if let Some(floors) = node.optional_number("floors") {
        parameters.insert("floors".into(), (floors as u32).into());
    }
    Ok(crate::authoring::Request {
        kind: "item".into(),
        entry,
        source: source.into(),
        parameters,
        ..Default::default()
    })
}

pub(crate) fn asset(entry: &str) -> Result<Arc<[u8]>, String> {
    definition(entry)
        .map(|s| Arc::from(s.as_bytes()))
        .ok_or_else(|| format!("Unknown city item dependency {entry}."))
}

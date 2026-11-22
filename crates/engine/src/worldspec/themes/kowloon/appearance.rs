use crate::itemspec::definition::Values;
use std::sync::OnceLock;

pub(super) fn reference_building() -> &'static Values {
    static PRESET: OnceLock<Values> = OnceLock::new();
    PRESET.get_or_init(|| {
        serde_json::from_str(include_str!("materials/reference-building.json"))
            .expect("reference shop-house preset")
    })
}

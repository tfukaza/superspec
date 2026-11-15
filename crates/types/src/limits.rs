//! Input and output allowances shared by native and browser authoring.
//!
//! They bound untrusted input, and they are sized for real scenes: one production bake writes a
//! few 2048 px maps per material, and imported scene GLBs are commonly 50-500 MB. The browser
//! mirrors these values in `browser/model/limits.ts`; a test keeps the two identical.

const MIB: usize = 1024 * 1024;

/// One model file (GLB) or other companion file.
pub const MAX_FILE_BYTES: usize = 512 * MIB;
/// One encoded image (PNG, JPEG or KTX2), standalone or inside a model.
pub const MAX_IMAGE_BYTES: usize = 64 * MIB;
/// All companion files of one source, project or scene, together.
pub const MAX_COMPANION_BYTES: usize = 1024 * MIB;
/// Number of companion files of one source, project or scene.
pub const MAX_COMPANIONS: usize = 1024;
/// Encoded images delivered in one scene packet.
pub const MAX_SCENE_IMAGE_BYTES: usize = 1024 * MIB;
/// Base-level pixels of all images in one scene (512 maps of 1024 px, or 128 of 2048 px).
pub const MAX_SCENE_IMAGE_PIXELS: usize = 512 * MIB;
/// A saved project file, whose companions are embedded as base64 (4 bytes per 3).
pub const MAX_PROJECT_FILE_BYTES: usize = MAX_COMPANION_BYTES / 3 * 4 + 64 * MIB;
/// One exported GLB, which glTF limits to 4 GiB.
pub const MAX_EXPORT_BYTES: usize = 2048 * MIB;

#[cfg(test)]
mod tests {
    use super::*;

    /// The browser's copy must agree with this table exactly.
    #[test]
    fn browser_limits_match() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../browser/model/limits.ts");
        let source = std::fs::read_to_string(path).expect("browser limits module");
        let value = |name: &str| -> usize {
            let line = source
                .lines()
                .find(|line| line.contains(&format!("export const {name} =")))
                .unwrap_or_else(|| panic!("{name} is missing from limits.ts"));
            let expression = line.split('=').nth(1).unwrap().trim().trim_end_matches(';');
            expression
                .split('*')
                .map(|factor| {
                    let factor = factor.trim().replace('_', "");
                    match factor.as_str() {
                        "MIB" => MIB,
                        number => number.parse::<usize>().unwrap_or_else(|_| {
                            panic!("{name}: unsupported factor {number} in limits.ts")
                        }),
                    }
                })
                .product()
        };
        for (name, expected) in [
            ("MAX_FILE_BYTES", MAX_FILE_BYTES),
            ("MAX_IMAGE_BYTES", MAX_IMAGE_BYTES),
            ("MAX_COMPANION_BYTES", MAX_COMPANION_BYTES),
            ("MAX_COMPANIONS", MAX_COMPANIONS),
            ("MAX_SCENE_IMAGE_BYTES", MAX_SCENE_IMAGE_BYTES),
            ("MAX_PROJECT_FILE_BYTES", MAX_PROJECT_FILE_BYTES),
        ] {
            assert_eq!(value(name), expected, "{name}");
        }
    }
}

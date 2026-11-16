//! A scene's lighting record in glTF: its lights as `KHR_lights_punctual`
//! (https://github.com/KhronosGroup/glTF/tree/main/extensions/2.0/Khronos/KHR_lights_punctual),
//! which any glTF consumer reads, and the whole record under `extras.superspec.lighting`, which
//! the SuperSpec game runtime reads (`browser/renderer/game-lighting.ts`): shadow style, ambient light,
//! ambient occlusion, exposure and tone mapping have no glTF form.
use serde_json::{Value, json};
use superspec_types::lighting::{Light, LightKind, Lighting};

pub(crate) const EXTENSION: &str = "KHR_lights_punctual";

/// The rotation (x, y, z, w) of a light's node. A punctual light shines along its node's local
/// -Z; the record's `direction` points from the scene toward the light, so local +Z is turned
/// onto it. The node is a child of the scene's root node, whose rotation takes the domain's Z-up
/// frame to glTF's Y-up, so this rotation stays in domain coordinates.
fn orientation(direction: [f64; 3]) -> [f64; 4] {
    let length = direction.iter().map(|axis| axis * axis).sum::<f64>().sqrt();
    let [x, y, z] = direction.map(|axis| axis / length);
    // The shortest turn from +Z to the direction: axis (0, 0, 1) x d, cosine d.z.
    let w = 1.0 + z;
    let norm = (x * x + y * y + w * w).sqrt();
    if norm < 1e-9 {
        // Straight down: any half turn about a horizontal axis.
        return [1.0, 0.0, 0.0, 0.0];
    }
    [-y / norm, x / norm, 0.0, w / norm]
}

fn definition(light: &Light) -> Value {
    match light.kind {
        // The record's intensity is a renderer scalar (see docs/plans/lighting-settings.md); it is
        // written as it is, which is also how three.js reads a directional light's intensity.
        LightKind::Directional => json!({
            "name": light.id, "type": "directional", "color": light.color, "intensity": light.intensity,
        }),
    }
}

/// The extension's light definitions and one node per light, in the record's order. Each node
/// refers to its definition by index and is to be a child of the scene's root node.
pub(crate) fn punctual(lighting: &Lighting) -> (Vec<Value>, Vec<Value>) {
    let definitions = lighting.lights.iter().map(definition).collect();
    let nodes = lighting
        .lights
        .iter()
        .enumerate()
        .map(|(index, light)| {
            json!({
                "name": light.id, "rotation": orientation(light.direction),
                "extensions": { (EXTENSION): { "light": index } },
            })
        })
        .collect();
    (definitions, nodes)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Rotates `v` by the unit quaternion `q` (x, y, z, w).
    fn rotate(q: [f64; 4], v: [f64; 3]) -> [f64; 3] {
        let [x, y, z, w] = q;
        let cross = |a: [f64; 3], b: [f64; 3]| {
            [
                a[1] * b[2] - a[2] * b[1],
                a[2] * b[0] - a[0] * b[2],
                a[0] * b[1] - a[1] * b[0],
            ]
        };
        let t = cross([x, y, z], v).map(|axis| 2.0 * axis);
        let u = cross([x, y, z], t);
        [0, 1, 2].map(|axis| v[axis] + w * t[axis] + u[axis])
    }

    #[test]
    fn a_light_node_shines_away_from_the_recorded_direction() {
        let third = 1.0 / 3f64.sqrt();
        for direction in [
            [0.0, 0.0, 1.0],
            [0.0, 0.0, -1.0],
            [1.0, 0.0, 0.0],
            [0.0, -1.0, 0.0],
            [0.6, 0.0, 0.8],
            [-third, third, -third],
            Light::default().direction,
        ] {
            let q = orientation(direction);
            let length = q.iter().map(|part| part * part).sum::<f64>().sqrt();
            assert!((length - 1.0).abs() < 1e-12, "{direction:?}");
            // The light travels along the node's local -Z, away from where the light stands.
            let travel = rotate(q, [0.0, 0.0, -1.0]);
            for axis in 0..3 {
                assert!(
                    (travel[axis] + direction[axis]).abs() < 1e-12,
                    "{direction:?}"
                );
            }
        }
    }
}

//! Portable values of an instanced batch: one local mesh drawn many times, each time with its own
//! placement and, optionally, a small fixed set of per-instance attributes. The delivered record
//! (ranges into a scene's batch buffers, bounds) is `superspec_model::batches::Batch`.
//!
//! Meters, right-handed Z-up. A placement is translation, rotation and scale (applied scale first,
//! then rotation, then translation), not a matrix: it is what `EXT_mesh_gpu_instancing` stores, so
//! an export writes the authored numbers and never decomposes anything. Shear and mirroring are
//! not representable; a producer that needs them delivers ordinary meshes.

use serde::{Deserialize, Serialize};

/// Batches in one scene.
pub const MAX_BATCHES: usize = 4096;
/// Instances of all batches of one scene together.
pub const MAX_INSTANCES: usize = 1_000_000;
/// Vertices of all batch meshes of one scene together (each mesh is stored once).
pub const MAX_BATCH_VERTICES: usize = 1_500_000;
/// Indices of all batch meshes of one scene together.
pub const MAX_BATCH_INDICES: usize = 9_000_000;
/// Numbers per delivered placement: translation (3), rotation quaternion `x y z w` (4), scale (3).
pub const PLACEMENT_FLOATS: usize = 10;

/// Where one instance of a batch's mesh stands.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Placement {
    pub translation: [f64; 3],
    /// Unit quaternion `x y z w`.
    #[serde(default = "no_rotation")]
    pub rotation: [f64; 4],
    /// Positive factors along the mesh's own axes.
    #[serde(default = "unit_scale")]
    pub scale: [f64; 3],
}

fn no_rotation() -> [f64; 4] {
    [0., 0., 0., 1.]
}
fn unit_scale() -> [f64; 3] {
    [1.; 3]
}

impl Default for Placement {
    fn default() -> Self {
        Self {
            translation: [0.; 3],
            rotation: no_rotation(),
            scale: unit_scale(),
        }
    }
}

impl Placement {
    /// Finite, within the supported coordinate range, a unit rotation and positive scale.
    pub fn validate(&self) -> Result<(), String> {
        if self
            .translation
            .iter()
            .any(|v| !v.is_finite() || v.abs() > 1e9)
        {
            return Err("Instance translation leaves the supported coordinate range.".into());
        }
        let length = self.rotation.iter().map(|v| v * v).sum::<f64>().sqrt();
        if !length.is_finite() || (length - 1.).abs() > 1e-4 {
            return Err("Instance rotation must be a unit quaternion (x, y, z, w).".into());
        }
        if self
            .scale
            .iter()
            .any(|v| !v.is_finite() || *v < 1e-9 || *v > 1e9)
        {
            return Err("Instance scale must be positive and finite on every axis.".into());
        }
        Ok(())
    }

    /// The delivered form: `PLACEMENT_FLOATS` single-precision numbers, the rotation normalized.
    pub fn delivered(&self) -> [f32; PLACEMENT_FLOATS] {
        let length = self.rotation.iter().map(|v| v * v).sum::<f64>().sqrt();
        let r = self.rotation.map(|v| (v / length) as f32);
        let (t, s) = (
            self.translation.map(|v| v as f32),
            self.scale.map(|v| v as f32),
        );
        [t[0], t[1], t[2], r[0], r[1], r[2], r[3], s[0], s[1], s[2]]
    }
}

/// The rotation columns of a delivered placement, each scaled by its axis: the upper 3x3 of the
/// instance matrix, column-major. Only products and sums, so native and Wasm agree bit for bit.
pub fn basis(placement: &[f32]) -> [[f64; 3]; 3] {
    let [x, y, z, w] = [3, 4, 5, 6].map(|i| f64::from(placement[i]));
    let s = [7, 8, 9].map(|i| f64::from(placement[i]));
    [
        [
            (1. - 2. * (y * y + z * z)) * s[0],
            (2. * (x * y + z * w)) * s[0],
            (2. * (x * z - y * w)) * s[0],
        ],
        [
            (2. * (x * y - z * w)) * s[1],
            (1. - 2. * (x * x + z * z)) * s[1],
            (2. * (y * z + x * w)) * s[1],
        ],
        [
            (2. * (x * z + y * w)) * s[2],
            (2. * (y * z - x * w)) * s[2],
            (1. - 2. * (x * x + y * y)) * s[2],
        ],
    ]
}

/// A point of the batch's mesh as one delivered placement puts it in the scene.
pub fn place(placement: &[f32], point: [f64; 3]) -> [f64; 3] {
    let m = basis(placement);
    std::array::from_fn(|axis| {
        m[0][axis] * point[0]
            + m[1][axis] * point[1]
            + m[2][axis] * point[2]
            + f64::from(placement[axis])
    })
}

/// What a batch is to collision. Render visibility is the batch's `visible`; the two are
/// independent, as for meshes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Collision {
    /// Render only: nothing collides with the instances (the default).
    #[default]
    None,
    /// Every instance collides as its mesh's triangles.
    Instances,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placements_validate_and_place_points() {
        assert!(Placement::default().validate().is_ok());
        let half = std::f64::consts::FRAC_1_SQRT_2;
        // A quarter turn about +Z, doubled along the mesh's X, moved to (10, 0, 1).
        let placement = Placement {
            translation: [10., 0., 1.],
            rotation: [0., 0., half, half],
            scale: [2., 1., 1.],
        };
        placement.validate().unwrap();
        let point = place(&placement.delivered(), [1., 0., 0.]);
        for (found, expected) in point.iter().zip([10., 2., 1.]) {
            assert!((found - expected).abs() < 1e-6, "{point:?}");
        }
        for invalid in [
            Placement {
                rotation: [0., 0., 0., 2.],
                ..Default::default()
            },
            Placement {
                scale: [1., -1., 1.],
                ..Default::default()
            },
            Placement {
                scale: [1., 0., 1.],
                ..Default::default()
            },
            Placement {
                translation: [f64::NAN, 0., 0.],
                ..Default::default()
            },
        ] {
            assert!(invalid.validate().is_err(), "{invalid:?}");
        }
        // The delivered rotation is unit length whatever rounding the authored one carried.
        let delivered = Placement {
            rotation: [0., 0., 0.70712, 0.70712],
            ..Default::default()
        }
        .delivered();
        let length = delivered[3..7].iter().map(|v| v * v).sum::<f32>().sqrt();
        assert!((length - 1.).abs() < 1e-6);
    }

    #[test]
    fn collision_defaults_to_none() {
        assert_eq!(Collision::default(), Collision::None);
    }
}

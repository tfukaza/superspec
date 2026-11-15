//! Owned, offline-baked stock geometry; compiling never invokes an asset provider.
use super::super::{fixture::Fixture, pen::Tint};
use std::sync::OnceLock;
use superspec_geometry::{
    V3,
    math::{add, unit},
};
type Faces = Vec<Vec<[f64; 8]>>;
fn apples() -> &'static Faces {
    static ASSET: OnceLock<Faces> = OnceLock::new();
    ASSET.get_or_init(|| {
        serde_json::from_str(include_str!("assets/apple-pile.json")).expect("shipped apple pile")
    })
}
pub(super) fn packet() -> &'static Faces {
    static ASSET: OnceLock<Faces> = OnceLock::new();
    ASSET.get_or_init(|| {
        serde_json::from_str(include_str!("assets/snack-packet.json"))
            .expect("shipped snack packet")
    })
}
struct Stock<'a> {
    faces: &'a Faces,
    slot: &'static str,
    textured: bool,
    variant: usize,
}
fn draw(
    d: &mut Fixture<'_, '_>,
    stock: Stock<'_>,
    at: V3,
    size: V3,
    tint: V3,
) -> Result<(), String> {
    if at.iter().any(|v| !v.is_finite() || v.abs() > 1000.)
        || size
            .iter()
            .any(|v| !v.is_finite() || !(0.001..=100.).contains(v))
        || tint
            .iter()
            .any(|v| !v.is_finite() || !(0. ..=4.).contains(v))
        || stock.variant > 3
    {
        return Err("Baked stock requires bounded dimensions, color and atlas variant.".into());
    }
    d.mesh.check_work(stock.faces.len() * 2)?;
    let snack = stock.slot == "snack-packet";
    let sign = if snack && d.pen.mirrored { -1. } else { 1. };
    for face in stock.faces {
        let printed = stock.textured && (!snack || face.iter().all(|v| v[1] < 0.));
        let plain = if stock.textured && snack {
            [0.55, 0.43, 0.25]
        } else {
            tint
        };
        let points: Vec<_> = face
            .iter()
            .map(|v| {
                [
                    at[0] + sign * v[0] * size[0],
                    at[1] + v[1] * size[1],
                    at[2] + v[2] * size[2],
                ]
            })
            .collect();
        let normals: Vec<_> = face
            .iter()
            .map(|v| unit([sign * v[3] / size[0], v[4] / size[1], v[5] / size[2]]))
            .collect::<Result<_, _>>()?;
        let hint = normals.iter().fold([0.; 3], |sum, n| add(sum, *n));
        let uv: Vec<_> = face
            .iter()
            .map(|v| {
                if snack {
                    [
                        (stock.variant % 2) as f64 * 0.5 + (4. + v[6] * 504.) / 1024.,
                        ((1 - stock.variant / 2) as f64 * 512. + 4. + v[7] * 504.) / 1024.,
                    ]
                } else {
                    [v[6], v[7]]
                }
            })
            .collect();
        let slot = if printed { stock.slot } else { "paint" };
        d.pen.face(
            d.mesh,
            slot,
            &points,
            hint,
            &Tint::Color(if printed { [1.; 3] } else { plain }),
            d.repeat(slot),
            printed.then_some(uv.as_slice()),
            Some(&normals),
        )?;
    }
    Ok(())
}
pub fn apple_pile(
    d: &mut Fixture<'_, '_>,
    at: V3,
    size: V3,
    tint: V3,
    textured: bool,
) -> Result<(), String> {
    draw(
        d,
        Stock {
            faces: apples(),
            slot: "market-apple",
            textured,
            variant: 0,
        },
        at,
        size,
        tint,
    )
}
pub fn snack_packet(
    d: &mut Fixture<'_, '_>,
    at: V3,
    size: [f64; 2],
    tint: V3,
    textured: bool,
    variant: usize,
) -> Result<(), String> {
    draw(
        d,
        Stock {
            faces: packet(),
            slot: "snack-packet",
            textured,
            variant,
        },
        at,
        [size[0], 0.068_f64.min(size[0] * 0.5), size[1]],
        tint,
    )
}

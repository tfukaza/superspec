//! Kitchen atlas fields and tiled walls with shared PBR channel coordinates.
use super::super::{
    fixture::Fixture,
    palette::{shade, srgb},
    pen::Tint,
    wall::finish_seed,
};
use super::forms;
use superspec_geometry::V3;
#[derive(Clone, Copy)]
pub struct Settings {
    pub fields: bool,
    pub tiles: bool,
    pub metal_repeat: f64,
}
pub fn uv(tile: usize, u: f64, v: f64) -> Result<[f64; 2], String> {
    if tile > 3 || !u.is_finite() || !v.is_finite() {
        return Err("Kitchen chart requires finite coordinates and tile 0–3.".into());
    }
    Ok([
        ((tile % 2) as f64 * 256. + 4.5 + u * 247.) / 512.,
        1. - ((tile / 2) as f64 * 256. + 4.5 + (1. - v) * 247.) / 512.,
    ])
}
pub struct Field<'a> {
    pub tile: usize,
    pub coordinates: &'a [[f64; 2]],
    pub brightness: f64,
    pub normals: Option<&'a [V3]>,
    pub textured: bool,
}
pub fn face(
    d: &mut Fixture<'_, '_>,
    points: &[V3],
    hint: V3,
    field: &Field<'_>,
) -> Result<(), String> {
    if field.tile > 3 || !field.brightness.is_finite() || !(0. ..=4.).contains(&field.brightness) {
        return Err("Kitchen field requires bounded brightness and tile 0–3.".into());
    }
    let fallback = ["#aaa69d", "#dfd8be", "#a9763d", "#b8c9b0"][field.tile];
    let tint = if field.textured {
        [field.brightness; 3]
    } else {
        shade(srgb(fallback), field.brightness)
    };
    let coordinates: Vec<_> = field
        .coordinates
        .iter()
        .map(|p| uv(field.tile, p[0], p[1]))
        .collect::<Result<_, _>>()?;
    let slot = if field.textured {
        "restaurant-surfaces"
    } else {
        "paint"
    };
    d.pen.face(
        d.mesh,
        slot,
        points,
        hint,
        &Tint::Color(tint),
        d.repeat(slot),
        Some(&coordinates),
        field.normals,
    )
}
pub fn tiles(
    d: &mut Fixture<'_, '_>,
    size: [f64; 2],
    y: f64,
    seed: u32,
    backsplash: bool,
    settings: Settings,
) -> Result<(), String> {
    let [width, height] = size;
    if size
        .iter()
        .any(|v| !v.is_finite() || !(0.01..=100.).contains(v))
        || !y.is_finite()
        || y.abs() > 1000.
    {
        return Err("Kitchen tile walls require bounded positive dimensions.".into());
    }
    d.mesh.check_work(0)?;
    if settings.tiles {
        let offset = (seed % 7) as f64 * 0.18;
        let points = [
            [0., y, 0.],
            [width, y, 0.],
            [width, y, height],
            [0., y, height],
        ];
        let coordinates = points.map(|p| [(p[0] + offset) / 3.6, p[2] / 1.8]);
        return d.pen.face(
            d.mesh,
            "restaurant-tiles",
            &points,
            [0., -1., 0.],
            &Tint::Color([1.; 3]),
            d.repeat("restaurant-tiles"),
            Some(&coordinates),
            None,
        );
    }
    forms::front(
        d,
        "paint",
        [0., width, 0., height],
        y + 0.001,
        srgb("#77786b"),
    )?;
    let columns = (width / 0.15).round().max(1.) as usize;
    let rows = (height / 0.15).round().max(1.) as usize;
    d.mesh.check_work(columns * rows * 2)?;
    let dx = width / columns as f64;
    let dz = height / rows as f64;
    for row in 0..rows {
        for column in 0..columns {
            let hash = finish_seed(&format!("{seed}/{row}/{column}"));
            let x = column as f64 * dx;
            let z = row as f64 * dz;
            let tile = if row < 2 || backsplash && row % 7 == 0 {
                3
            } else {
                1
            };
            let brightness = 0.91 + (hash % 13) as f64 / 100. - if row == 0 { 0.12 } else { 0. };
            let shift = (hash % 7) as f64 / 50.;
            let coordinates = [
                [shift, shift],
                [0.85 + shift, shift],
                [0.85 + shift, 0.85 + shift],
                [shift, 0.85 + shift],
            ];
            face(
                d,
                &[
                    [x + 0.002, y, z + 0.002],
                    [x + dx - 0.002, y, z + 0.002],
                    [x + dx - 0.002, y, z + dz - 0.002],
                    [x + 0.002, y, z + dz - 0.002],
                ],
                [0., -1., 0.],
                &Field {
                    tile,
                    coordinates: &coordinates,
                    brightness,
                    normals: None,
                    textured: settings.fields,
                },
            )?;
        }
    }
    Ok(())
}

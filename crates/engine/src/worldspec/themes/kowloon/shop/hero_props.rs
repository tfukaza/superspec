//! Approved close-range pieces with baked albedo, brush normals and surface properties.
use super::super::{fixture::Fixture, pen::Tint};
use superspec_geometry::{
    V3,
    math::{Matrix, add, unit},
};

const SLOT: &str = "market-hero-props";
const WOOD: &[u8] = include_bytes!("assets/wooden-stool.triangles.bin");
const JADE: &[u8] = include_bytes!("assets/jade-stool.triangles.bin");
const BUCKET: &[u8] = include_bytes!("assets/bucket.triangles.bin");

fn draw(
    d: &mut Fixture<'_, '_>,
    triangles: &[u8],
    at: V3,
    size: V3,
    source: V3,
    yaw: f64,
) -> Result<(), String> {
    let scale: V3 = std::array::from_fn(|i| size[i] / source[i]);
    let placement = Matrix::placement(at, [0., 0., yaw.to_degrees()], [1.; 3])?;
    let pen = d
        .pen
        .shape_frame(d.pen.matrix.then(placement), &d.pen.part)?;
    let (faces, remainder) = triangles.as_chunks::<96>();
    debug_assert!(remainder.is_empty());
    d.mesh.check_work(faces.len())?;
    for face in faces {
        let vertices = std::array::from_fn::<_, 3, _>(|index| {
            let vertex = &face.as_chunks::<32>().0[index];
            let values = std::array::from_fn(|i| {
                f64::from(f32::from_le_bytes(vertex.as_chunks::<4>().0[i]))
            });
            let [x, y, z, nx, ny, nz, u, v] = values;
            ([x, y, z], [nx, ny, nz], [u, v])
        });
        let points = vertices.map(|(point, _, _)| std::array::from_fn(|i| point[i] * scale[i]));
        let [a, b, c] =
            vertices.map(|(_, normal, _)| unit(std::array::from_fn(|i| normal[i] / scale[i])));
        let normals = [a?, b?, c?];
        let uv = [vertices[0].2, vertices[1].2, vertices[2].2];
        pen.face_raw(
            d.mesh,
            SLOT,
            &points,
            add(add(normals[0], normals[1]), normals[2]),
            &Tint::Color([1.; 3]),
            1.,
            Some(&uv),
            Some(&normals),
        )?;
    }
    Ok(())
}

/// A reduced level's stand-in for a baked piece: its turned outline in a paint of its own,
/// shaded per instance from where it stands so that neighbours differ as the baked ones do.
fn turned(
    d: &mut Fixture<'_, '_>,
    at: V3,
    profile: &[[f64; 2]],
    paint: &str,
    values: &[f64],
) -> Result<(), String> {
    use super::super::palette::{shade, srgb};
    let seed = super::super::wall::finish_seed(&format!("{}/{at:?}", d.pen.part));
    let own = shade(srgb(paint), 0.9 + (seed % 1000) as f64 / 1000. * 0.18);
    let tints: Vec<_> = values
        .iter()
        .map(|value| Tint::Color(shade(own, *value)))
        .collect();
    super::forms::lathe(
        d,
        "paint",
        at,
        profile,
        &super::forms::Tints::Bands(&tints),
        12,
        None,
    )
}

pub(super) fn wooden_stool(
    d: &mut Fixture<'_, '_>,
    at: V3,
    requested: V3,
    yaw: f64,
) -> Result<(), String> {
    if d.mesh.graphics_detail().reduced() {
        // A round seat on a splayed base.
        let (r, h) = (requested[0].min(requested[1]) / 2., requested[2]);
        return turned(
            d,
            at,
            &[
                [r * 0.95, 0.],
                [r * 0.7, h * 0.86],
                [r, h * 0.86],
                [r, h],
                [0., h],
            ],
            "#7a5a3a",
            &[0.8, 0.7, 1., 1.1],
        );
    }
    draw(d, WOOD, at, requested, [0.39212, 0.39473, 0.39705], yaw)
}

pub(super) fn jade_stool(
    d: &mut Fixture<'_, '_>,
    at: V3,
    requested: V3,
    yaw: f64,
) -> Result<(), String> {
    if d.mesh.graphics_detail().reduced() {
        // A waisted plastic stool.
        let (r, h) = (requested[0].min(requested[1]) / 2., requested[2]);
        return turned(
            d,
            at,
            &[
                [r, 0.],
                [r * 0.72, h * 0.5],
                [r * 0.9, h * 0.94],
                [r * 0.9, h],
                [0., h],
            ],
            "#5f9a84",
            &[0.85, 1., 0.9, 1.1],
        );
    }
    draw(d, JADE, at, requested, [0.388, 0.388, 0.475], yaw)
}

pub(super) fn bucket(
    d: &mut Fixture<'_, '_>,
    at: V3,
    radius: f64,
    height: f64,
    yaw: f64,
) -> Result<(), String> {
    if d.mesh.graphics_detail().reduced() {
        // The market bucket: its taper, rolled rim and shadowed inside, in a red that
        // varies bucket by bucket and band by band as the baked finish does.
        let (r, h) = (radius, height);
        return turned(
            d,
            at,
            &[
                [r * 0.72, 0.],
                [r * 0.96, h * 0.96],
                [r * 1.05, h],
                [r * 0.94, h * 0.99],
                [r * 0.7, 0.03],
                [0., 0.03],
            ],
            "#a8554a",
            &[1., 1.15, 0.75, 0.6, 0.5],
        );
    }
    draw(
        d,
        BUCKET,
        at,
        [2. * radius, 2. * radius, height],
        [0.2744, 0.273, 0.23],
        yaw,
    )
}

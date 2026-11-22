//! Static one-bounce diffuse irradiance. All inputs and probe centers are in world Z-up meters.
use crate::authoring::{
    occlusion::Bvh,
    receiver::{add, cross, dot, mul, sub, unit},
};
use superspec_types::bounced_light::{ProbeBake, SpotEmitter};

fn spot_irradiance(spot: &SpotEmitter, hit: [f64; 3], normal: [f64; 3], tree: &Bvh) -> f64 {
    let delta = sub(spot.position, hit);
    let squared = dot(delta, delta);
    if squared >= spot.range * spot.range || squared < 1e-8 {
        return 0.;
    }
    let distance = squared.sqrt();
    let to_light = mul(delta, 1. / distance);
    let cosine = dot(normal, to_light);
    let angle = -dot(to_light, spot.direction);
    if cosine <= 0. || angle <= spot.outer_cosine {
        return 0.;
    }
    if tree.blocked(hit, to_light, distance) {
        return 0.;
    }
    // Match the live spotlight's smooth cone, inverse-square law and finite cutoff.
    let blend =
        ((angle - spot.outer_cosine) / (spot.inner_cosine - spot.outer_cosine).max(1e-8)).min(1.);
    let cutoff = (1. - (squared / spot.range.powi(2)).powi(2)).powi(2);
    cosine * blend * blend * (3. - 2. * blend) * cutoff / squared.max(0.01)
}

/// Six axis-facing cosine lobes (+X, -X, +Y, -Y, +Z, -Z), each a complete
/// x-fastest volume of RGB8 irradiance. Empty space emits no indirect light.
pub fn bake(mut input: ProbeBake) -> Result<Vec<u8>, String> {
    let [nx, ny, nz] = input.dimensions;
    if input.dimensions.iter().any(|&n| n == 0 || n > 256)
        || nz > 42
        || nx * ny * nz > 262144
        || !(0.05..=20.).contains(&input.spacing)
        || input.surfaces.len() > 1_000_000
        || input.surfaces.is_empty()
        || !input
            .origin
            .iter()
            .chain(&input.sun_direction)
            .all(|v| v.is_finite())
        || dot(input.sun_direction, input.sun_direction) < 1e-12
        || !input.sun_radiance.iter().all(|v| (0.0..=100.).contains(v))
    {
        return Err("Invalid probe grid, reflector count or sun".into());
    }
    let mut faces = Vec::with_capacity(input.surfaces.len());
    if input.spots.len() > 256 {
        return Err("Too many probe spot emitters".into());
    }
    for (i, spot) in input.spots.iter_mut().enumerate() {
        if !spot
            .position
            .iter()
            .chain(&spot.direction)
            .all(|v| v.is_finite())
            || !spot.intensity.iter().all(|v| (0.0..=10000.).contains(v))
            || !(0.01..=1000.).contains(&spot.range)
            || !(0.0..=1.).contains(&spot.outer_cosine)
            || !(spot.outer_cosine..=1.).contains(&spot.inner_cosine)
        {
            return Err(format!("Invalid probe spot emitter {i}"));
        }
        spot.direction = unit(spot.direction)
            .map_err(|_| format!("Invalid direction on probe spot emitter {i}"))?;
    }
    let mut colors = Vec::with_capacity(input.surfaces.len());
    for (i, surface) in input.surfaces.into_iter().enumerate() {
        if !surface.vertices.iter().flatten().all(|v| v.is_finite())
            || !surface.color.iter().all(|v| (0.0..=1.).contains(v))
        {
            return Err(format!("Invalid reflector {i}"));
        }
        let [a, b, c] = surface.vertices;
        colors.push((surface.color, unit(cross(sub(b, a), sub(c, a)))?));
        faces.push(surface.vertices);
    }
    let (tree, order) = Bvh::ordered(faces);
    let sun = unit(input.sun_direction)?;
    let count = nx * ny * nz;
    let mut bytes = vec![0; count * 18];
    let mut weights = [0.; 6];
    let rays: Vec<_> = (0..64)
        .map(|i| {
            let z = 1. - 2. * (i as f64 + 0.5) / 64.;
            let angle = i as f64 * 2.399963229728653;
            let radial = (1. - z * z).sqrt();
            let ray = [radial * angle.cos(), radial * angle.sin(), z];
            for (k, &v) in ray.iter().enumerate() {
                weights[2 * k + usize::from(v < 0.)] += v.abs();
            }
            ray
        })
        .collect();
    for probe in 0..count {
        let cell = [probe % nx, probe / nx % ny, probe / (nx * ny)];
        let p = std::array::from_fn(|k| input.origin[k] + cell[k] as f64 * input.spacing);
        let mut light = [[0.; 3]; 6];
        for &ray in &rays {
            let Some((face, t)) = tree.trace::<false>(p, ray, 32.) else {
                continue;
            };
            let (color, mut normal) = colors[order[face] as usize];
            if dot(normal, ray) > 0. {
                normal = mul(normal, -1.);
            }
            let cosine = dot(normal, sun);
            let hit = add(add(p, mul(ray, t)), mul(normal, 0.002));
            let mut irradiance = [0.; 3];
            if cosine > 0. && !tree.blocked(hit, sun, 1000.) {
                irradiance = mul(input.sun_radiance, cosine);
            }
            for spot in &input.spots {
                let strength = spot_irradiance(spot, hit, normal, &tree);
                irradiance = add(irradiance, mul(spot.intensity, strength));
            }
            for (k, &v) in ray.iter().enumerate() {
                let lobe = &mut light[2 * k + usize::from(v < 0.)];
                for c in 0..3 {
                    lobe[c] += v.abs() * color[c] * irradiance[c];
                }
            }
        }
        for (side, rgb) in light.iter().enumerate() {
            for c in 0..3 {
                bytes[3 * (side * count + probe) + c] = (rgb[c] / weights[side] * (255. / 2.))
                    .round()
                    .clamp(0., 255.) as u8;
            }
        }
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use superspec_types::bounced_light::Reflector;
    #[test]
    fn red_ground_lights_only_receivers_facing_it_and_needs_sun() {
        let sample = |sun| {
            bake(ProbeBake {
                origin: [0., 0., 1.],
                dimensions: [1; 3],
                spacing: 1.,
                sun_direction: sun,
                sun_radiance: [1.; 3],
                spots: vec![],
                surfaces: vec![Reflector {
                    vertices: [[-80., -80., 0.], [80., -80., 0.], [0., 80., 0.]],
                    color: [1., 0., 0.],
                }],
            })
            .unwrap()
        };
        let lit = sample([0., 0., 1.]);
        assert_eq!(&lit[12..15], &[0, 0, 0]);
        assert!(lit[15] >= 125 && lit[15] <= 128);
        assert_eq!(&lit[16..], &[0, 0]);
        assert_eq!(sample([0., 0., -1.]), vec![0; 18]);
    }

    #[test]
    fn ceiling_between_sun_and_reflector_removes_its_bounce() {
        let surface = |height, color| Reflector {
            vertices: [
                [-200., -200., height],
                [200., -200., height],
                [0., 200., height],
            ],
            color,
        };
        let bytes = bake(ProbeBake {
            origin: [0., 0., 1.],
            dimensions: [1; 3],
            spacing: 1.,
            sun_direction: [0., 0., 1.],
            sun_radiance: [1.; 3],
            spots: vec![],
            surfaces: vec![surface(0., [1., 0., 0.]), surface(3., [0.; 3])],
        })
        .unwrap();
        assert_eq!(bytes, vec![0; 18]);
    }

    #[test]
    fn warm_spot_bounces_from_the_floor_only_when_it_can_reach_it() {
        let sample = |direction, blocked| {
            let plane = |z, color| Reflector {
                vertices: [[-60., -60., z], [60., -60., z], [0., 60., z]],
                color,
            };
            let mut surfaces = vec![plane(0., [0.8, 0.8, 0.8])];
            if blocked {
                surfaces.push(plane(1.5, [0.; 3]));
            }
            bake(ProbeBake {
                origin: [0., 0., 0.5],
                dimensions: [1; 3],
                spacing: 1.,
                sun_direction: [0., 0., 1.],
                sun_radiance: [0.; 3],
                spots: vec![SpotEmitter {
                    position: [0., 0., 2.],
                    direction,
                    intensity: [6., 2., 0.],
                    range: 5.,
                    outer_cosine: 0.6,
                    inner_cosine: 0.9,
                }],
                surfaces,
            })
            .unwrap()
        };
        let lit = sample([0., 0., -1.], false);
        assert!(lit[15] > lit[16] && lit[16] > 0 && lit[17] == 0);
        assert_eq!(&lit[12..15], &[0; 3]);
        assert_eq!(sample([0., 0., 1.], false), vec![0; 18]);
        assert_eq!(sample([0., 0., -1.], true), vec![0; 18]);
    }
}

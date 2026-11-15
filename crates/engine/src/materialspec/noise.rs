//! Deterministic lattice fields shared by intrinsic surface recipes. Coordinates are f64 meters.
fn uint32(value: f64) -> u32 {
    if !value.is_finite() {
        return 0;
    }
    let reduced = value.trunc() % 4294967296.;
    (if reduced < 0. {
        reduced + 4294967296.
    } else {
        reduced
    }) as u32
}
pub fn lattice_hash(x: f64, y: f64, seed: f64) -> f64 {
    let mut h =
        uint32(x).wrapping_mul(374761393) ^ uint32(y).wrapping_mul(668265263) ^ uint32(seed);
    h = (h ^ (h >> 13)).wrapping_mul(1274126177);
    f64::from(h ^ (h >> 16)) / 4294967296.
}
pub fn value_noise(x: f64, y: f64, seed: f64) -> f64 {
    let ix = x.floor();
    let iy = y.floor();
    let u = x - ix;
    let v = y - iy;
    let u = u * u * (3. - 2. * u);
    let v = v * v * (3. - 2. * v);
    let low = lattice_hash(ix, iy, seed) * (1. - u) + lattice_hash(ix + 1., iy, seed) * u;
    let high =
        lattice_hash(ix, iy + 1., seed) * (1. - u) + lattice_hash(ix + 1., iy + 1., seed) * u;
    low * (1. - v) + high * v
}
pub fn value_noise3(x: f64, y: f64, z: f64, seed: f64) -> f64 {
    let iz = z.floor();
    let w = z - iz;
    let w = w * w * (3. - 2. * w);
    let low = value_noise(x, y, seed + iz * 7919.);
    let high = value_noise(x, y, seed + (iz + 1.) * 7919.);
    low * (1. - w) + high * w
}
/// Positive octave count; each octave doubles frequency and halves its normalized weight.
pub fn fractal_noise(x: f64, y: f64, seed: f64, octaves: u8) -> f64 {
    let mut sum = 0.;
    let mut weight = 0.;
    let mut amplitude = 1.;
    let mut frequency = 1.;
    for octave in 0..octaves {
        let octave = f64::from(octave);
        sum += amplitude
            * value_noise(
                x * frequency + octave * 17.31,
                y * frequency + octave * 9.73,
                seed + octave * 131.,
            );
        weight += amplitude;
        amplitude /= 2.;
        frequency *= 2.;
    }
    sum / weight
}
pub fn smooth_step(low: f64, high: f64, value: f64) -> f64 {
    let t = ((value - low) / (high - low)).clamp(0., 1.);
    t * t * (3. - 2. * t)
}
/// Conservative interval including clipped lattice corners; large rectangles use the global bound.
pub fn value_noise_range(bounds: [f64; 4], seed: f64) -> Result<[f64; 2], String> {
    let [x0, y0, x1, y1] = bounds;
    if bounds.iter().any(|v| !v.is_finite()) || x0 > x1 || y0 > y1 || !seed.is_finite() {
        return Err("Noise bounds require a finite ordered rectangle and seed.".into());
    }
    if bounds.iter().any(|v| v.abs() >= 9007199254740990.)
        || (x1.floor() - x0.floor() + 2.) * (y1.floor() - y0.floor() + 2.) > 4096.
    {
        return Ok([0., 1.]);
    }
    let coordinates = |low: f64, high: f64| {
        let mut values = vec![low];
        let mut value = low.floor() + 1.;
        while value < high {
            values.push(value);
            value += 1.;
        }
        if high != low {
            values.push(high);
        }
        values
    };
    let xs = coordinates(x0, x1);
    let ys = coordinates(y0, y1);
    let mut low = f64::INFINITY;
    let mut high = f64::NEG_INFINITY;
    for x in xs {
        for y in &ys {
            let v = value_noise(x, *y, seed);
            low = low.min(v);
            high = high.max(v);
        }
    }
    Ok([
        (low - f64::EPSILON * 16.).max(0.),
        (high + f64::EPSILON * 16.).min(1.),
    ])
}
pub fn fractal_noise_range(bounds: [f64; 4], seed: f64, octaves: u8) -> Result<[f64; 2], String> {
    if !(1..=16).contains(&octaves) {
        return Err("Noise interval requires 1–16 octaves.".into());
    }
    let mut low = 0.;
    let mut high = 0.;
    let mut weight = 0.;
    let mut amplitude = 1.;
    let mut frequency = 1.;
    for octave in 0..octaves {
        let octave = f64::from(octave);
        let [a, b] = value_noise_range(
            [
                bounds[0] * frequency + octave * 17.31,
                bounds[1] * frequency + octave * 9.73,
                bounds[2] * frequency + octave * 17.31,
                bounds[3] * frequency + octave * 9.73,
            ],
            seed + octave * 131.,
        )?;
        low += amplitude * a;
        high += amplitude * b;
        weight += amplitude;
        amplitude /= 2.;
        frequency *= 2.;
    }
    Ok([
        (low / weight - f64::EPSILON * 16.).max(0.),
        (high / weight + f64::EPSILON * 16.).min(1.),
    ])
}

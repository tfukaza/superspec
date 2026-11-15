material 1;
fn coverage(field: scalar, threshold: scalar, softness: scalar) -> scalar =
    smoothstep(threshold - softness, threshold + softness, field);
fn tinted(base: vec3, detail: scalar) -> vec3 = base * (0.85 + detail * 0.3);

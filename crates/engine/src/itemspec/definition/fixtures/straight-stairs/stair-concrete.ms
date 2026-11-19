material 1;
param tint: vec3 = vec3(0.39, 0.36, 0.31) range(0.0, 1.0);
let aggregate = fbm(surface / 0.014, seed("aggregate"), 3);
let stain = fbm(surface / 0.36, seed("stain"), 3);
output base_color = tint * (0.76 + 0.16 * aggregate + 0.18 * stain);
output roughness = 0.72 + 0.16 * aggregate;
output metalness = 0.0;
output height = aggregate * 0.0006;

material 1;
param tint: vec3 = vec3(0.47, 0.43, 0.34) range(0.0, 1.0);
let grain = noise(surface / 0.016, seed("sand"));
let discoloration = fbm(surface / vec2(0.22, 0.85), seed("discoloration"), 3);
let worn = smoothstep(0.54, 0.67, discoloration + (grain - 0.5) * 0.07);
output base_color = mix(tint, tint * vec3(0.69, 0.72, 0.68), worn) * (0.90 + 0.16 * grain);
output roughness = 0.78 + worn * 0.13;
output metalness = 0.0;
output height = grain * 0.00065 - worn * 0.0014;

export const worktopLampShader = /* glsl */ `// Static near-field reflectors fitted to the three condiment jars on the Stoa counter.
// All coordinates are in game-space metres. Resolve the silhouette analytically
// so contact and parallax remain stable at close range.
vec4 msJarStroke(vec3 p, vec3 ray, vec3 base, float radius, float height, vec3 glaze) {
  vec2 offset = base.xz - p.xz;
  float slope = max(ray.y, .001);
  float entry = max(0., (base.y - p.y) / slope);
  float exitAt = (base.y + height - p.y) / slope;
  if (ray.y <= 0. || exitAt <= entry) return vec4(0.);
  float lateral = max(dot(ray.xz, ray.xz), .00001);
  float closest = dot(offset, ray.xz) / lateral;
  float across = length(offset - ray.xz * closest);
  float hit = closest - sqrt(max(0., radius * radius - across * across) / lateral);
  float t = clamp(closest, entry, exitAt);
  float distanceToJar = length(offset - ray.xz * t);
  float spread = .0028 + .041 * t;
  float coverage = 1. - smoothstep(radius - spread, radius + spread, distanceToJar);
  float level = clamp((p.y + clamp(hit, entry, exitAt) * slope - base.y) / height, 0., 1.);
  float label = smoothstep(.11, .27, level) * (1. - smoothstep(.67, .82, level));
  vec3 pigment = mix(glaze, vec3(.95,.87,.69), label);
  pigment = mix(pigment, vec3(.76,.64,.43), smoothstep(.89,.99,level));
  return vec4(pigment, coverage * .89);
}

vec3 msWorktopLamp(vec3 paint, vec3 position, vec3 reflection) {
  // Bring the overhead lamp and dark service rail down into the same shallow
  // proxy plane. The stroke follows the reflected ray when the viewer moves.
  vec2 ceiling = position.xz + reflection.xz * max(1.70 - position.y, 0.) / max(reflection.y, .03);
  float drift = .042 * sin(ceiling.y * 4.1) + .017 * sin(ceiling.y * 13.0);
  float lampLength = smoothstep(-6.15, -5.70, ceiling.y) * (1.-smoothstep(-3.05, -2.70, ceiling.y));
  float lamp = lampLength * (1.-smoothstep(.075, .23, abs(ceiling.x - 17.12 - drift)));
  float railLength = smoothstep(-5.94, -5.45, ceiling.y) * (1.-smoothstep(-3.16, -2.82, ceiling.y));
  float rail = 1.-smoothstep(.13, .27, abs(ceiling.x - 16.76 - drift));
  float jamb = 1.-smoothstep(.07, .24, abs(ceiling.x - 18.72 - drift));
  paint = mix(paint, vec3(.050, .11, .12), .84 * railLength * max(rail,jamb));
  paint = mix(paint, vec3(.89, .65, .35), .92 * lamp);
  if (distance(position.xz, vec2(18.31,-3.44)) < .85 && abs(position.y-.95) < .025) {
    const vec2 centers[3] = vec2[3](vec2(18.295,-3.356), vec2(18.326,-3.440), vec2(18.296,-3.522));
    const vec3 glazes[3] = vec3[3](vec3(.54,.13,.075), vec3(.40,.21,.095), vec3(.15,.35,.23));
    const float radii[3] = float[3](.033, .037, .033);
    const float heights[3] = float[3](.106, .069, .126);
    vec4 jars[3];
    float depth[3];
    for (int i=0; i<3; ++i) {
      vec3 base = vec3(centers[i].x, .954, centers[i].y);
      jars[i] = msJarStroke(position, reflection, base, radii[i], heights[i], glazes[i]);
      depth[i] = dot(centers[i]-position.xz, reflection.xz);
    }
    paint *= (1.-jars[0].a)*(1.-jars[1].a)*(1.-jars[2].a);
    for (int i=0; i<3; ++i) {
      float visible = jars[i].a;
      for (int j=0; j<3; ++j)
        if (depth[j]<depth[i] || (depth[j]==depth[i] && j<i)) visible *= 1.-jars[j].a;
      paint += visible*jars[i].rgb;
    }
  }
  return paint;
}`;

import { paintBrushField } from "./paintedBrush.ts";

// All distances outside of shadow projection coordinates are in metres.
export const paintedCastShadow = /* glsl */ `
#if defined(USE_SHADOWMAP) && defined(SHADOWMAP_TYPE_PCF) && NUM_DIR_LIGHT_SHADOWS > 0
${paintBrushField}
uniform vec3 cityShadowSpan;
float cityCompare(sampler2DShadow depth, vec3 p, vec2 offset, vec2 slope) {
  return texture(depth, vec3(p.xy + offset, p.z + dot(offset, slope)));
}
void citySector(vec4 samples, inout float weights, inout float sum) {
  float mean = dot(samples, vec4(.25));
  float variance = dot(samples * samples, vec4(.25)) - mean * mean;
  // Soft minimum: equal-variance sectors on opposite sides of a silhouette
  // must meet smoothly as the shadow travels across a paving stone.
  float weight = 1. / (.035 + max(0., variance));
  weights += weight;
  sum += weight * mean;
}
float cityPaintCast(sampler2DShadow depth, vec4 projected, float bias, float visibility, vec3 gamePosition) {
  vec3 p = projected.xyz / projected.w;
  p.z += bias;
  vec3 dx = dFdx(p), dy = dFdy(p);
  vec3 world = vec3(gamePosition.x, -gamePosition.z, gamePosition.y);
  vec3 worldDx = dFdx(world), worldDy = dFdy(world);
  vec3 area = cross(worldDx, worldDy);
  vec3 face = dot(area, area) > 1e-20 ? normalize(area) : vec3(0.,0.,1.);
  vec3 dominant = abs(face);
  vec3 axisHint = dominant.z >= max(dominant.x, dominant.y) ? vec3(1.,0.,0.) : vec3(0.,0.,1.);
  vec3 v = normalize(cross(face, axisHint)), u = cross(v, face);
  vec2 meters = vec2(dot(world, u), dot(world, v));
  vec2 footprintUV = (abs(vec2(dot(worldDx,u), dot(worldDx,v)))
                    + abs(vec2(dot(worldDy,u), dot(worldDy,v)))) / (.36 * .82 * vec2(1., .34));
  float footprint = max(footprintUV.x, footprintUV.y);
  float det = dx.x * dy.y - dx.y * dy.x;
  vec2 slope = abs(det) > 1e-12
    ? vec2(dx.z * dy.y - dy.z * dx.y, dx.x * dy.z - dy.x * dx.z) / det : vec2(0.);
  if (any(lessThan(p, vec3(0.))) || any(greaterThan(p, vec3(1.)))) return visibility;
  vec2 metre = 1. / cityShadowSpan.xy;
  vec2 reach = .36 * metre;
  vec2 east = vec2(reach.x, 0.), north = vec2(0., reach.y);
  vec4 border = vec4(cityCompare(depth, p, east, slope), cityCompare(depth, p, -east, slope),
                     cityCompare(depth, p, north, slope), cityCompare(depth, p, -north, slope));
  float low = min(min(border.x, border.y), min(border.z, border.w));
  float high = max(max(border.x, border.y), max(border.z, border.w));
  float edge = smoothstep(.01, .65, max(visibility, high) - min(visibility, low));
  if (edge == 0.) return visibility;

  // Comparison samplers expose visibility rather than raw depth. Query a
  // covered neighbour at successively closer distances to bracket its caster.
  vec2 gradient = vec2(border.x - border.y, border.z - border.w);
  vec2 offset = -reach * gradient / max(1., length(gradient)) * visibility;
  vec3 blocker = vec3(p.xy + offset, p.z + dot(offset, slope));
  float towardsLight = -1. / cityShadowSpan.z;
  #ifdef USE_REVERSED_DEPTH_BUFFER
    towardsLight = -towardsLight;
  #endif
  vec3 blocked;
  blocked.x = 1. - texture(depth, blocker + vec3(0., 0., .15 * towardsLight));
  blocked.y = 1. - texture(depth, blocker + vec3(0., 0., .90 * towardsLight));
  blocked.z = 1. - texture(depth, blocker + vec3(0., 0., 2.6 * towardsLight));
  float radius = .024 + dot(blocked, vec3(.028, .058, .096));

  float stroke = cityBrushPigment(meters, .36, footprint);
  vec2 axis = normalize(vec2(1., .38 + .35 * stroke));
  vec2 a = axis * metre * radius * (1.10 + .55 * stroke);
  vec2 b = vec2(-axis.y, axis.x) * metre * radius * .72;
  float c = cityCompare(depth, p, vec2(0.), slope);
  vec4 side = vec4(cityCompare(depth, p, a, slope), cityCompare(depth, p, b, slope),
                   cityCompare(depth, p, -a, slope), cityCompare(depth, p, -b, slope));
  vec4 corners = vec4(cityCompare(depth, p, a+b, slope), cityCompare(depth, p, b-a, slope),
                      cityCompare(depth, p, -a-b, slope), cityCompare(depth, p, a-b, slope));
  float broad = .20 * c + dot(side, vec4(.14)) + dot(corners, vec4(.06));
  float weights = 0., sum = 0.;
  citySector(vec4(c, side.x, side.y, corners.x), weights, sum);
  citySector(vec4(c, side.y, side.z, corners.y), weights, sum);
  citySector(vec4(c, side.z, side.w, corners.z), weights, sum);
  citySector(vec4(c, side.w, side.x, corners.w), weights, sum);
  float ink = mix(broad, sum / weights, .21);
  return mix(visibility, ink, edge * smoothstep(.015, .11, radius) * .55);
}
#endif
`;

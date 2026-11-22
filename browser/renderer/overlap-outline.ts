export const overlapOutlineShader = `
// Project one raster pixel onto the foreground tangent plane along the camera
// ray. The largest singular value gives metres per pixel in its longest direction.
float outlineSizeFade(vec2 pixelMeters, vec3 normal, vec3 eye) {
  float facing = dot(normal, eye);
  if (abs(facing) < 1.e-5) return 0.;
  vec3 ray = eye / facing;
  vec3 dx = pixelMeters.x * (vec3(1.,0.,0.) - ray * normal.x);
  vec3 dy = pixelMeters.y * (vec3(0.,1.,0.) - ray * normal.y);
  float a = dot(dx,dx), b = dot(dx,dy), c = dot(dy,dy);
  float span = sqrt(.5 * (a+c+length(vec2(a-c,2.*b))));
  return 1. - smoothstep(.04,.05,span);
}
// Window depth is affine across a projected plane. A shared corner has a
// cross-edge slope between those of its two faces, even at a sharp fold.
float outlineSide(sampler2D depths, vec2 uv, vec2 offset, float here, float nearSlope, float there, float tolerance) {
  float jump = there - here;
  if (there >= 1.0) return 0.0;
  float next = texture2D(depths, uv + 2.0 * offset).r;
  float farSlope = next - there;
  float gap = jump - max(0.0, max(nearSlope, farSlope));
  return smoothstep(tolerance, 2.0 * tolerance, gap);
}
float outlineAxis(sampler2D depths, vec2 uv, vec2 offset, float here, float tolerance) {
  float a = texture2D(depths, uv - offset).r, b = texture2D(depths, uv + offset).r;
  return max(outlineSide(depths, uv, offset, here, here-a, b, tolerance),
             outlineSide(depths, uv,-offset, here, here-b, a, tolerance));
}
float overlapOutline(sampler2D depths, vec2 uv, vec2 texel, float depthPerMeter, float footprint) {
  float here = texture2D(depths, uv).r;
  float tolerance = max(4.8e-7, depthPerMeter * max(.012, 1.5 * footprint));
  float x = outlineAxis(depths, uv, vec2(texel.x,0.), here, tolerance);
  float y = outlineAxis(depths, uv, vec2(0.,texel.y), here, tolerance);
  return max(x,y);
}`;

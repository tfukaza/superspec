import type * as THREE from "three";

/** Carry the visible specular lobe to the outline resolve in the existing HDR target. */
export function writeOutlineSpecular(material: THREE.MeshStandardMaterial): void {
  if (material.transparent || material.userData.grassField) return;
  const previous = material.onBeforeCompile, key = material.customProgramCacheKey.bind(material);
  material.onBeforeCompile = function(shader, renderer) {
    previous.call(this, shader, renderer);
    shader.fragmentShader = shader.fragmentShader.replace("#include <dithering_fragment>", `#include <dithering_fragment>
      vec3 luma = vec3(.2126,.7152,.0722);
      float spec = dot(totalSpecular, luma);
      float reflected = dot(totalDiffuse + totalSpecular, luma);
      float visibleSpec = spec * clamp(dot(outgoingLight - totalEmissiveRadiance, luma) / max(reflected, .0001), 0., 1.);
      float highlight = smoothstep(.24, .72, visibleSpec);
      // Expand the existing masonry tag (1 + 1/16) to 5. After the four-sample
      // resolve, each covered masonry sample adds one, leaving room for the mask.
      gl_FragColor.a = 1. + 64. * (gl_FragColor.a - 1.) + .5 * highlight;
    `);
  };
  material.customProgramCacheKey = () => key() + "|outline-specular";
}

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
float overlapOutline(sampler2D depths, vec2 uv, vec2 texel, float depthPerMeter, float footprint, bool fineEdges) {
  float here = texture2D(depths, uv).r;
  float tolerance = max(4.8e-7, depthPerMeter * max(fineEdges ? .006 : .012, 1.5 * footprint));
  float x = outlineAxis(depths, uv, vec2(texel.x,0.), here, tolerance);
  float y = outlineAxis(depths, uv, vec2(0.,texel.y), here, tolerance);
  return max(x,y);
}
float backgroundOutline(sampler2D depths, vec2 uv, vec2 texel) {
  if (texture2D(depths,uv).r >= 1.) return 0.;
  vec4 neighbors = vec4(
    texture2D(depths,uv+vec2(texel.x,0.)).r, texture2D(depths,uv-vec2(texel.x,0.)).r,
    texture2D(depths,uv+vec2(0.,texel.y)).r, texture2D(depths,uv-vec2(0.,texel.y)).r);
  return step(1., max(max(neighbors.x,neighbors.y), max(neighbors.z,neighbors.w)));
}`;

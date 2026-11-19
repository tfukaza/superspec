import * as THREE from "three";
import { FullScreenQuad, Pass } from "three/examples/jsm/postprocessing/Pass.js";
import type { Contours } from "./lighting.ts";

/** What the camera sees, as the ambient occlusion draws it: depth, face normals and where foliage is in front. */
export interface ContourSource {
  depth: THREE.Texture;
  /** View-space normals, packed to [0, 1]. */
  normal: THREE.Texture;
  /** Foliage in front of that depth, in its alpha; read only while `hasFoliage`. */
  foliage: THREE.Texture;
  hasFoliage: boolean;
}

/**
 * A crease is drawn this much thinner than a silhouette, so the outline of a form weighs a little more than the edges
 * inside it, as a drawn line does.
 */
const CREASE_WIDTH = 0.7;
/** How foliage is outlined: not at all (0), or along the outer edge of its mask (1, tried and left off, see `ContourPass`). */
const FOLIAGE_OUTLINE = { value: 0 };
(globalThis as { __contourFoliage?: unknown }).__contourFoliage = FOLIAGE_OUTLINE; // TEMPORARY, for the comparison

const VERTEX = /* glsl */`varying vec2 vUv; void main() { vUv = uv; gl_Position = projectionMatrix * modelViewMatrix * vec4( position, 1.0 ); }`;

/**
 * Where a line is, 0 to 1, in red. Lengths named `Px` are device pixels.
 *
 * Silhouettes come from depth. Across a flat surface the reciprocal of the view distance (the view distance itself in
 * an orthographic view) is linear on screen however steeply the surface is seen, so its second difference is zero there
 * and positive on the nearer side of a step: a surface seen at a grazing angle draws nothing, and the line lies on the
 * nearer surface, one line width wide (the two taps are a line width away). The step is in meters, never less than the
 * depth buffer can tell apart at that distance nor than a line width and a half as the pixel covers it.
 *
 * Creases come from the face normals: the angle between a pixel's and its neighbor's, owned by the nearer of the two so
 * the line is on one side only.
 *
 * Too small to draw: where the surface is farther on both sides within three line widths (a cage bar, a cable, a
 * lattice), the pixel is part of something too thin to hold a line on each side and gets none.
 */
const EDGE = /* glsl */`
uniform sampler2D tDepth;
uniform sampler2D tNormal;
uniform sampler2D tFoliage;
uniform float hasFoliage;
uniform float foliageOutline;
uniform vec2 texel;
uniform float cameraNear;
uniform float cameraFar;
uniform float perspective;
uniform float pixelWorld;
uniform float pixelRatio;
uniform float silhouettePx;
uniform float creasePx;
uniform float depthStep;
uniform float creaseLow;
uniform float creaseHigh;
uniform float fade;
varying vec2 vUv;
#include <packing>
// Larger is nearer; linear on screen across a flat surface.
float nearness( vec2 uv ) {
	float depth = texture2D( tDepth, uv ).x;
	return perspective > 0.5 ? - 1.0 / perspectiveDepthToViewZ( depth, cameraNear, cameraFar ) : orthographicDepthToViewZ( depth, cameraNear, cameraFar );
}
vec3 normalAt( vec2 uv ) { return normalize( texture2D( tNormal, uv ).xyz * 2.0 - 1.0 ); }
float leafAt( vec2 uv ) { return hasFoliage * texture2D( tFoliage, uv ).a; }
// The crease between this pixel and a neighbor, if this pixel is the nearer of the two (ties go to forward pairs).
float crease( vec3 normal, float here, vec2 uv, float forward ) {
	float there = nearness( uv );
	float owned = here > there ? 1.0 : ( here == there ? forward : 0.0 );
	return owned * smoothstep( creaseLow, creaseHigh, 1.0 - dot( normal, normalAt( uv ) ) );
}
void main() {
	float raw = texture2D( tDepth, vUv ).x;
	if ( raw >= 1.0 ) { gl_FragColor = vec4( 0.0, 0.0, 0.0, 1.0 ); return; }
	float here = nearness( vUv );
	float away = perspective > 0.5 ? 1.0 / here : - here;
	// Meters per unit of nearness, and per CSS pixel, at this pixel.
	float meters = perspective > 0.5 ? away * away : 1.0;
	float footprint = ( perspective > 0.5 ? away : 1.0 ) * pixelWorld;
	// What a 24-bit depth buffer can tell apart here, with room for the three values of a second difference.
	float resolved = perspective > 0.5 ? away * away / cameraNear * 4.8e-7 : ( cameraFar - cameraNear ) * 4.8e-7;
	float jump = max( depthStep, max( resolved, 1.5 * silhouettePx * footprint / pixelRatio ) );
	vec2 across = vec2( texel.x * silhouettePx, 0.0 ), up = vec2( 0.0, texel.y * silhouettePx );
	float left = nearness( vUv - across ), right = nearness( vUv + across ), below = nearness( vUv - up ), above = nearness( vUv + up );
	float silhouette = smoothstep( jump, 2.0 * jump, max( 2.0 * here - left - right, 2.0 * here - below - above ) * meters );
	// Too thin for a line on each side with paper between: farther on both sides, three line widths out.
	float thin = max(
		smoothstep( jump, 2.0 * jump, ( here - nearness( vUv - 3.0 * across ) ) * meters ) * smoothstep( jump, 2.0 * jump, ( here - nearness( vUv + 3.0 * across ) ) * meters ),
		smoothstep( jump, 2.0 * jump, ( here - nearness( vUv - 3.0 * up ) ) * meters ) * smoothstep( jump, 2.0 * jump, ( here - nearness( vUv + 3.0 * up ) ) * meters ) );
	vec3 normal = normalAt( vUv );
	vec2 acrossCrease = vec2( texel.x * creasePx, 0.0 ), upCrease = vec2( 0.0, texel.y * creasePx );
	float creased = max( max( crease( normal, here, vUv + acrossCrease, 1.0 ), crease( normal, here, vUv - acrossCrease, 0.0 ) ),
		max( crease( normal, here, vUv + upCrease, 1.0 ), crease( normal, here, vUv - upCrease, 0.0 ) ) );
	float line = max( silhouette, creased ) * ( 1.0 - thin );
	// Far detail is not drawn: lines fade as a pixel covers more of the scene.
	line *= 1.0 - smoothstep( 0.25 * fade, fade, footprint );
	// Foliage is not in the depth: nothing is drawn through a leaf.
	float leaf = leafAt( vUv );
	line *= 1.0 - leaf;
	if ( foliageOutline > 0.5 && leaf > 0.5 ) {
		float outside = min( min( leafAt( vUv - across ), leafAt( vUv + across ) ), min( leafAt( vUv - up ), leafAt( vUv + up ) ) );
		line = max( line, 1.0 - smoothstep( 0.25, 0.75, outside ) );
	}
	gl_FragColor = vec4( line, 0.0, 0.0, 1.0 );
}`;

/**
 * Multiplies the image by the line's color where a line is. The line is read through a small tent filter, which gives
 * it a soft, antialiased edge (the depth it comes from has one sample a pixel), and is left out where lines crowd: where
 * more than about a third of the surrounding block (eight line widths across) is line, the detail is too dense to draw
 * and would only blacken.
 */
const COMPOSITE = /* glsl */`
uniform sampler2D tDiffuse;
uniform sampler2D tEdge;
uniform vec2 texel;
uniform float softPx;
uniform float blockPx;
uniform float strength;
uniform vec3 color;
varying vec2 vUv;
void main() {
	vec4 image = texture2D( tDiffuse, vUv );
	vec2 soft = texel * softPx;
	float line = 0.25 * ( texture2D( tEdge, vUv + soft ).r + texture2D( tEdge, vUv - soft ).r
		+ texture2D( tEdge, vUv + vec2( soft.x, - soft.y ) ).r + texture2D( tEdge, vUv + vec2( - soft.x, soft.y ) ).r );
	float level = log2( max( blockPx, 1.0 ) );
	vec2 block = texel * blockPx * 0.5;
	float crowd = 0.25 * ( textureLod( tEdge, vUv + block, level ).r + textureLod( tEdge, vUv - block, level ).r
		+ textureLod( tEdge, vUv + vec2( block.x, - block.y ), level ).r + textureLod( tEdge, vUv + vec2( - block.x, block.y ), level ).r );
	line *= 1.0 - smoothstep( 0.3, 0.55, crowd );
	gl_FragColor = vec4( image.rgb * mix( vec3( 1.0 ), color, strength * line ), image.a );
}`;

/**
 * Contour lines over the composited frame (`contours` in the lighting record): a global setting of the scene, never
 * part of a material. It draws nothing of the scene itself: it reads the depth, the face normals and the foliage mask
 * the ambient occlusion draws (`source` hands them over, drawing them first where the occlusion is off), finds the
 * lines (`EDGE`) and multiplies them over the image before the output pass, so they are tone mapped with it.
 *
 * What gets a line: silhouettes and creases of everything in that depth. Glass is in it as a surface, so a pane gets a
 * line round it and none across it, and nothing seen through it is outlined. Foliage is not in it and gets no line
 * (painted crowns are soft masses; a line round every leaf card of the mask was tried and read as scribble), and lines
 * of what stands behind a leaf are not drawn over the leaf. Inspection overlays, points and lines get none.
 */
export class ContourPass extends Pass {
  private readonly edge = new THREE.ShaderMaterial({
    uniforms: {
      tDepth: { value: null }, tNormal: { value: null }, tFoliage: { value: null }, hasFoliage: { value: 0 }, foliageOutline: FOLIAGE_OUTLINE,
      texel: { value: new THREE.Vector2(1, 1) }, cameraNear: { value: 0.1 }, cameraFar: { value: 1000 }, perspective: { value: 1 },
      pixelWorld: { value: 0.001 }, pixelRatio: { value: 1 }, silhouettePx: { value: 2 }, creasePx: { value: 1 }, depthStep: { value: 0.04 },
      creaseLow: { value: 0.06 }, creaseHigh: { value: 0.23 }, fade: { value: 0.08 },
    },
    vertexShader: VERTEX, fragmentShader: EDGE, depthTest: false, depthWrite: false,
  });
  private readonly composite = new THREE.ShaderMaterial({
    uniforms: {
      tDiffuse: { value: null }, tEdge: { value: null }, texel: { value: new THREE.Vector2(1, 1) }, softPx: { value: 0.5 }, blockPx: { value: 16 },
      strength: { value: 0 }, color: { value: new THREE.Color() },
    },
    vertexShader: VERTEX, fragmentShader: COMPOSITE, depthTest: false, depthWrite: false,
  });
  /** The lines, with mip levels: the composite reads how crowded they are from a coarse one. */
  private readonly lines = new THREE.WebGLRenderTarget(1, 1, { type: THREE.UnsignedByteType, depthBuffer: false, generateMipmaps: true,
    minFilter: THREE.LinearMipmapLinearFilter, magFilter: THREE.LinearFilter });
  private readonly quad = new FullScreenQuad(this.edge);
  private pixelRatio = 1;
  private width = 2;

  constructor(private readonly camera: THREE.Camera, private readonly source: () => ContourSource, contours: Contours) {
    super();
    this.set(contours);
  }

  /** Changes the look in place. */
  set(contours: Contours): void {
    const edge = this.edge.uniforms, composite = this.composite.uniforms;
    this.width = contours.width;
    edge.depthStep.value = contours.depth;
    // One minus the cosine of the angle between two normals: the line starts at half the crease angle.
    edge.creaseLow.value = 1 - Math.cos(contours.normal / 2);
    edge.creaseHigh.value = 1 - Math.cos(contours.normal);
    edge.fade.value = contours.fade;
    composite.strength.value = contours.strength;
    (composite.color.value as THREE.Color).setRGB(...contours.color, THREE.LinearSRGBColorSpace);
    this.fit();
  }

  /** Line weights are CSS pixels: a picture at twice the device pixels has lines of the same apparent weight. */
  setPixelRatio(pixelRatio: number): void {
    this.pixelRatio = pixelRatio;
    this.fit();
  }

  private fit(): void {
    const edge = this.edge.uniforms, composite = this.composite.uniforms;
    const silhouette = Math.max(1, this.width * this.pixelRatio);
    edge.pixelRatio.value = this.pixelRatio;
    edge.silhouettePx.value = silhouette;
    edge.creasePx.value = Math.max(1, silhouette * CREASE_WIDTH);
    composite.softPx.value = 0.5 * Math.max(1, this.pixelRatio);
    composite.blockPx.value = 8 * silhouette;
  }

  override setSize(width: number, height: number): void {
    this.lines.setSize(Math.max(1, width), Math.max(1, height));
    (this.edge.uniforms.texel.value as THREE.Vector2).set(1 / Math.max(1, width), 1 / Math.max(1, height));
    (this.composite.uniforms.texel.value as THREE.Vector2).set(1 / Math.max(1, width), 1 / Math.max(1, height));
  }

  override render(renderer: THREE.WebGLRenderer, writeBuffer: THREE.WebGLRenderTarget, readBuffer: THREE.WebGLRenderTarget): void {
    const source = this.source(), edge = this.edge.uniforms, camera = this.camera as THREE.PerspectiveCamera | THREE.OrthographicCamera;
    edge.tDepth.value = source.depth;
    edge.tNormal.value = source.normal;
    edge.tFoliage.value = source.foliage;
    edge.hasFoliage.value = source.hasFoliage ? 1 : 0;
    edge.cameraNear.value = camera.near;
    edge.cameraFar.value = camera.far;
    edge.perspective.value = (camera as THREE.PerspectiveCamera).isPerspectiveCamera ? 1 : 0;
    // Meters a CSS pixel covers one meter away (perspective) or anywhere (orthographic): the view's height over the picture's.
    edge.pixelWorld.value = 2 / (camera.projectionMatrix.elements[5] * Math.max(1, this.lines.height / this.pixelRatio));
    const autoClear = renderer.autoClear;
    renderer.autoClear = false;
    try {
      this.quad.material = this.edge;
      renderer.setRenderTarget(this.lines);
      this.quad.render(renderer);
      this.composite.uniforms.tDiffuse.value = readBuffer.texture;
      this.composite.uniforms.tEdge.value = this.lines.texture;
      this.quad.material = this.composite;
      renderer.setRenderTarget(this.renderToScreen ? null : writeBuffer);
      this.quad.render(renderer);
    } finally { renderer.autoClear = autoClear; }
  }

  override dispose(): void {
    this.edge.dispose();
    this.composite.dispose();
    this.lines.dispose();
    this.quad.dispose();
  }
}

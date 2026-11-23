import * as THREE from "three";
import { overlapOutlineShader } from "#render-engine";
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
const CREASE_WIDTH = 0.6;
/** How much more saturated a line is than the surface it is drawn on, so a darker line does not read as dirtier. */
const LINE_SATURATION = 1.15;

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
${overlapOutlineShader}
uniform bool overlapsOnly;
uniform mat4 inverseProjection;
uniform sampler2D tNormal;
uniform sampler2D tFoliage;
uniform float hasFoliage;
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
// The crease between this pixel and a neighbor, if this pixel is the nearer of the two (ties go to forward pairs).
float crease( vec3 normal, float here, vec2 uv, float forward ) {
	float there = nearness( uv );
	float owned = here > there ? 1.0 : ( here == there ? forward : 0.0 );
	return owned * smoothstep( creaseLow, creaseHigh, 1.0 - dot( normal, normalAt( uv ) ) );
}
void main() {
	float raw = texture2D( tDepth, vUv ).x;
	if ( raw >= 1.0 || texture2D(tNormal, vUv).a < .5 ) { gl_FragColor = vec4( 0.0, 0.0, 0.0, 1.0 ); return; }
	float here = nearness( vUv );
	float away = perspective > 0.5 ? 1.0 / here : - here;
	// Meters per unit of nearness, and per CSS pixel, at this pixel.
	float meters = perspective > 0.5 ? away * away : 1.0;
	float footprint = ( perspective > 0.5 ? away : 1.0 ) * pixelWorld;
	if (overlapsOnly) {
		float depthPerMeter = perspective > .5 ? cameraNear * cameraFar / ((cameraFar-cameraNear)*meters) : 1./(cameraFar-cameraNear);
		float ink = overlapOutline(tDepth,vUv,texel,depthPerMeter,footprint/pixelRatio,false);
		vec4 ray = inverseProjection * vec4(vUv*2.-1.,0.,1.);
		vec3 eye = perspective > .5 ? normalize(-ray.xyz) : vec3(0.,0.,1.);
		vec3 n = normalAt(vUv);
		float faceCoverage = smoothstep(.06,.25,abs(dot(n,eye)));
		vec2 pixelMeters = 2. * texel * vec2(inverseProjection[0][0], inverseProjection[1][1])
			* (perspective > .5 ? away : 1.);
		float sizeFade = outlineSizeFade(pixelMeters,n,eye);
		gl_FragColor = vec4(ink * faceCoverage * sizeFade * (1.-hasFoliage*texture2D(tFoliage,vUv).a),0.,0.,1.);
		return;
	}
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
	line *= 1.0 - hasFoliage * texture2D( tFoliage, vUv ).a;
	gl_FragColor = vec4( line, 0.0, 0.0, 1.0 );
}`;

/**
 * Draws the line in the color of the surface it belongs to, darker: where a line is, the image becomes that surface's
 * own color (linear, before tone mapping) made `darken` as bright to the eye, a little more saturated and times the optional `tint`. A cream
 * wall gets a darker cream line, a green frame a darker green one. The surface is the one that owns the line (the nearer
 * side of a silhouette, the nearer face of a crease, as `EDGE` places it): its color is read from the same taps as the
 * line, weighted by how much line each holds, so the soft edge of a line spilling onto the far surface still takes the
 * near surface's color.
 *
 * The line is read through a small tent filter, which gives it a soft, antialiased edge (the depth it comes from has one
 * sample a pixel), and is left out where lines crowd: where more than about a third of the surrounding block (eight
 * line widths across, at least twelve pixels) is line, the detail is too dense to draw and would only blacken.
 */
const COMPOSITE = /* glsl */`
uniform sampler2D tDiffuse;
uniform sampler2D tEdge;
uniform bool overlapsOnly;
uniform vec2 texel;
uniform float softPx;
uniform float blockPx;
uniform float strength;
uniform float darken;
uniform float saturation;
uniform vec3 tint;
varying vec2 vUv;
void main() {
	vec4 image = texture2D( tDiffuse, vUv );
	if (overlapsOnly) {
		float line = texelFetch(tEdge, ivec2(gl_FragCoord.xy), 0).r;
		gl_FragColor = vec4(image.rgb * mix(vec3(1.), darken * tint, strength * line), image.a);
		return;
	}
	vec2 soft = texel * softPx;
	vec2 taps[ 4 ];
	taps[ 0 ] = vUv + soft; taps[ 1 ] = vUv - soft; taps[ 2 ] = vUv + vec2( soft.x, - soft.y ); taps[ 3 ] = vUv + vec2( - soft.x, soft.y );
	float line = 0.0;
	vec3 owner = vec3( 0.0 );
	for ( int i = 0; i < 4; i ++ ) {
		float held = texture2D( tEdge, taps[ i ] ).r;
		line += held;
		owner += held * texture2D( tDiffuse, taps[ i ] ).rgb;
	}
	owner = line > 0.0 ? owner / line : image.rgb;
	line *= 0.25;
	float level = log2( max( blockPx, 1.0 ) );
	vec2 block = texel * blockPx * 0.5;
	float crowd = 0.25 * ( textureLod( tEdge, vUv + block, level ).r + textureLod( tEdge, vUv - block, level ).r
		+ textureLod( tEdge, vUv + vec2( block.x, - block.y ), level ).r + textureLod( tEdge, vUv + vec2( - block.x, block.y ), level ).r );
	line *= 1.0 - smoothstep( 0.3, 0.55, crowd );
	float grey = dot( owner, vec3( 0.2126, 0.7152, 0.0722 ) );
	vec3 ink = max( mix( vec3( grey ), owner, saturation ), 0.0 ) * darken * tint;
	gl_FragColor = vec4( mix( image.rgb, ink, strength * line ), image.a );
}`;
/**
 * Contour lines over the composited frame (`contours` in the lighting record): a global setting of the scene, never
 * part of a material. It draws nothing of the scene itself: it reads the depth, the face normals and the foliage mask
 * the ambient occlusion draws (`source` hands them over, drawing them first where the occlusion is off), finds the
 * lines (`EDGE`) and draws them in the darkened color of the surface they belong to (`COMPOSITE`) before the output
 * pass, so they are tone mapped with the image.
 *
 * What gets a line: silhouettes and creases of everything in that depth. Glass is in it as a surface, so a pane gets a
 * line round it and none across it, and nothing seen through it is outlined. Foliage is not in it and gets no line, and
 * lines of what stands behind a leaf are not drawn over the leaf: painted crowns are soft masses that the viewer draws
 * over the frame with passes of their own, so a line along the edge of the leaf mask (tried on the avenue) was covered by
 * them and changed nothing that could be seen. Inspection overlays, points and lines get none.
 */
export class ContourPass extends Pass {
  private readonly edge = new THREE.ShaderMaterial({
    uniforms: {
      tDepth: { value: null }, tNormal: { value: null }, tFoliage: { value: null }, hasFoliage: { value: 0 },
      overlapsOnly: { value: false },
      inverseProjection: { value: new THREE.Matrix4() },
      texel: { value: new THREE.Vector2(1, 1) }, cameraNear: { value: 0.1 }, cameraFar: { value: 1000 }, perspective: { value: 1 },
      pixelWorld: { value: 0.001 }, pixelRatio: { value: 1 }, silhouettePx: { value: 2 }, creasePx: { value: 1 }, depthStep: { value: 0.04 },
      creaseLow: { value: 0.06 }, creaseHigh: { value: 0.23 }, fade: { value: 0.08 },
    },
    vertexShader: VERTEX, fragmentShader: EDGE, depthTest: false, depthWrite: false,
  });
  private readonly composite = new THREE.ShaderMaterial({
    uniforms: {
      overlapsOnly: { value: false },
      tDiffuse: { value: null }, tEdge: { value: null }, texel: { value: new THREE.Vector2(1, 1) }, softPx: { value: 0.5 }, blockPx: { value: 16 },
      strength: { value: 0 }, darken: { value: 0.6 }, saturation: { value: LINE_SATURATION }, tint: { value: new THREE.Color(1, 1, 1) },
    },
    vertexShader: VERTEX, fragmentShader: COMPOSITE, depthTest: false, depthWrite: false,
  });
  /** The lines, with mip levels: the composite reads how crowded they are from a coarse one. */
  private readonly lines = new THREE.WebGLRenderTarget(1, 1, { type: THREE.UnsignedByteType, depthBuffer: false, generateMipmaps: true,
    minFilter: THREE.LinearMipmapLinearFilter, magFilter: THREE.LinearFilter });
  private readonly quad = new FullScreenQuad(this.edge);
  private pixelRatio = 1;
  private width = 1.25;

  constructor(private readonly camera: THREE.Camera, private readonly source: () => ContourSource, contours: Contours) {
    super();
    this.set(contours);
  }

  setOverlapsOnly(enabled: boolean): void {
    this.edge.uniforms.overlapsOnly.value = this.composite.uniforms.overlapsOnly.value = enabled;
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
    // `darken` is how much darker the line looks on screen; the image here is linear, so it is put through the display's
    // gamma (about 2.2): 0.6 is a line that looks six tenths as bright as its surface.
    composite.darken.value = Math.pow(contours.darken, 2.2);
    (composite.tint.value as THREE.Color).setRGB(...(contours.color ?? [1, 1, 1] as const), THREE.LinearSRGBColorSpace);
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
    // A tent narrower than a pixel: a one-pixel line keeps most of its strength and still has a soft edge.
    composite.softPx.value = 0.35 * Math.max(1, this.pixelRatio);
    composite.blockPx.value = Math.max(8 * silhouette, 12 * this.pixelRatio);
  }

  override setSize(width: number, height: number): void {
    this.lines.setSize(Math.max(1, width), Math.max(1, height));
    (this.edge.uniforms.texel.value as THREE.Vector2).set(1 / Math.max(1, width), 1 / Math.max(1, height));
    (this.composite.uniforms.texel.value as THREE.Vector2).set(1 / Math.max(1, width), 1 / Math.max(1, height));
  }

  override render(renderer: THREE.WebGLRenderer, writeBuffer: THREE.WebGLRenderTarget, readBuffer: THREE.WebGLRenderTarget): void {
    const source = this.source(), edge = this.edge.uniforms, camera = this.camera as THREE.PerspectiveCamera | THREE.OrthographicCamera;
    edge.tDepth.value = source.depth;
    edge.inverseProjection.value.copy(camera.projectionMatrixInverse);
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

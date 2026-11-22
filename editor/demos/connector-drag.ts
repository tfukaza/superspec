import * as THREE from "three";

const gravity = 9.81;
const step = 1 / 120;
const floorRadius = 0.083;
const plugDragSpeed = 2.4;
const latchClearance = 0.24;
const openingAngle = THREE.MathUtils.degToRad(72);

/** Simulate only the free span. The two straps hold the rest of each run in place. */
class CableSpan {
  private readonly position;
  private readonly normal;
  private readonly vertices: THREE.Vector3[] = [];
  private readonly normals: THREE.Vector3[] = [];
  private readonly stations: number[] = [];
  private readonly rest: THREE.Vector3[];
  private readonly points: THREE.Vector3[];
  private readonly guide: THREE.Vector3[];
  private readonly gripWeight: number[] = [];
  private readonly bowWeight: number[] = [];
  private readonly spanLength: number;
  private readonly bowSide: number;
  private readonly restAxes: THREE.Vector3[];
  private readonly rotations: THREE.Quaternion[];
  private readonly fixedFrom: number;
  readonly start: THREE.Vector3;
  private readonly floor: number;
  private readonly delta = new THREE.Vector3();

  constructor(readonly mesh: THREE.Mesh) {
    const geometry = mesh.geometry;
    this.position = geometry.getAttribute("position");
    this.normal = geometry.getAttribute("normal");
    const station = geometry.getAttribute("_cable_station");
    const samples: Map<string, THREE.Vector3>[] = [];
    for (let i = 0; i < this.position.count; i++) {
      const index = station.getX(i);
      const center = new THREE.Vector3().fromBufferAttribute(this.position, i);
      this.stations.push(index);
      this.vertices.push(center);
      this.normals.push(new THREE.Vector3().fromBufferAttribute(this.normal, i));
      (samples[index] ??= new Map()).set(center.toArray().join(","), center);
    }
    this.rest = samples.map(ring => {
      const center = new THREE.Vector3();
      for (const point of ring.values()) center.add(point);
      return center.divideScalar(ring.size);
    });
    this.start = mesh.localToWorld(this.rest[0].clone());
    this.floor = mesh.worldToLocal(new THREE.Vector3()).y + floorRadius;
    this.fixedFrom = this.rest.findIndex(point => point.x > 2.83);
    this.points = this.rest.map(point => point.clone());
    this.guide = this.rest.map(point => point.clone());
    this.spanLength = this.rest.slice(1, this.fixedFrom).reduce(
      (length, point, i) => length + point.distanceTo(this.rest[i + 2]), 0);
    this.bowSide = this.rest[1].z >= this.rest[this.fixedFrom].z ? 1 : -1;
    let distance = 0;
    for (let i = 1; i <= this.fixedFrom; i++) {
      const t = distance / this.spanLength;
      this.gripWeight[i] = 1 - t * t * (3 - 2 * t);
      this.bowWeight[i] = Math.sin(Math.PI * t) ** 2;
      if (i < this.fixedFrom) distance += this.rest[i].distanceTo(this.rest[i + 1]);
    }
    this.restAxes = this.rest.map((_, i) => this.axis(this.rest, i, new THREE.Vector3()));
    this.rotations = this.rest.map(() => new THREE.Quaternion());
    // The edited span can move beyond its authored bounds.
    mesh.frustumCulled = false;
  }

  private axis(points: THREE.Vector3[], i: number, result: THREE.Vector3): THREE.Vector3 {
    return result.subVectors(points[Math.min(i + 1, points.length - 1)], points[Math.max(0, i - 1)]).normalize();
  }

  private routedLength(bow: number): number {
    let length = 0;
    for (let i = 2; i <= this.fixedFrom; i++) {
      const a = this.guide[i - 1], b = this.guide[i];
      length += Math.hypot(b.x - a.x, b.y - a.y,
        b.z - a.z + bow * this.bowSide * (this.bowWeight[i] - this.bowWeight[i - 1]));
    }
    return length;
  }

  integrate(anchor: THREE.Vector3, returning: boolean): void {
    const { points, rest, fixedFrom, delta } = this;
    if (returning) {
      for (let i = 0; i < fixedFrom; i++) points[i].lerp(rest[i], 0.15);
      return;
    }
    this.mesh.worldToLocal(delta.copy(anchor)).sub(rest[0]);
    points[0].copy(rest[0]).add(delta);
    const sag = Math.min(.28, delta.length() * .6);
    for (let i = 1; i <= fixedFrom; i++) {
      const point = this.guide[i].copy(rest[i]).addScaledVector(delta, this.gripWeight[i]);
      point.y = Math.max(this.floor, point.y - sag * this.bowWeight[i]);
    }
    // Route slack into a continuous lateral bend. A fixed side keeps a
    // compressed jacket from repeatedly switching between equivalent folds.
    const slack = this.spanLength - this.routedLength(0);
    let bow = 0;
    if (slack > 0) {
      let low = 0, high = this.spanLength;
      for (let pass = 0; pass < 13; pass++) {
        const mid = (low + high) / 2;
        if (this.routedLength(mid) < this.spanLength) low = mid;
        else high = mid;
      }
      bow = (low + high) / 2 * THREE.MathUtils.smoothstep(slack, 0, .015);
    }
    for (let i = 1; i < fixedFrom; i++) {
      points[i].copy(this.guide[i]).z += this.bowSide * bow * this.bowWeight[i];
    }
  }

  draw(reset = false): void {
    const { points, rest, restAxes, rotations, delta } = this;
    if (reset) for (let i = 0; i < this.fixedFrom; i++) points[i].copy(rest[i]);
    for (let i = 0; i < this.fixedFrom; i++) {
      rotations[i].setFromUnitVectors(restAxes[i], this.axis(points, i, delta));
    }
    for (let i = 0; i < this.vertices.length; i++) {
      const station = this.stations[i];
      if (station >= this.fixedFrom) continue;
      const rotation = rotations[station];
      delta.copy(this.vertices[i]).sub(rest[station]).applyQuaternion(rotation).add(points[station]);
      this.position.setXYZ(i, delta.x, delta.y, delta.z);
      delta.copy(this.normals[i]).applyQuaternion(rotation);
      this.normal.setXYZ(i, delta.x, delta.y, delta.z);
    }
    this.position.needsUpdate = true;
    this.normal.needsUpdate = true;
  }
}

type LatchJoint = { node: THREE.Object3D; position: THREE.Vector3; rotation: THREE.Quaternion };

class Plug {
  readonly home: THREE.Vector3;
  readonly center: THREE.Vector3;
  readonly position: THREE.Vector3;
  readonly goal: THREE.Vector3;
  readonly minimumY: number;
  readonly joints: LatchJoint[] = [];
  private readonly dragDelta = new THREE.Vector3();
  attached = true;
  open = 0;
  velocity = 0;
  // No animation loop or dynamic shadow updates while the bench is idle.
  remaining = 0;

  constructor(readonly node: THREE.Object3D, readonly socket: THREE.Object3D, readonly cable: CableSpan) {
    this.home = node.getWorldPosition(new THREE.Vector3());
    const bounds = new THREE.Box3().setFromObject(node);
    this.center = bounds.getCenter(new THREE.Vector3());
    this.minimumY = this.home.y - bounds.min.y + .018;
    this.position = this.home.clone();
    this.goal = this.home.clone();
    socket.traverse(child => {
      if (child.userData.connectorLatch) this.joints.push({
        node: child, position: child.position.clone(), rotation: child.quaternion.clone(),
      });
    });
  }

  integrate(held: boolean): void {
    this.open = THREE.MathUtils.lerp(this.open, this.attached ? 0 : 1, .16);
    if (this.attached) this.position.lerp(this.home, .25);
    else if (held) {
      this.dragDelta.subVectors(this.goal, this.position).multiplyScalar(.22).clampLength(0, plugDragSpeed * step);
      this.position.add(this.dragDelta);
    }
    else {
      // Fast releases still clear the socket before the plug settles.
      if (this.position.x < this.home.x + latchClearance) {
        this.position.x = Math.min(this.home.x + latchClearance, this.position.x + plugDragSpeed * step);
      }
      this.velocity -= gravity * step;
      this.position.y = Math.max(this.minimumY, this.position.y + this.velocity * step);
      if (this.position.y === this.minimumY) this.velocity = 0;
    }
    const anchor = this.position.clone().sub(this.home).add(this.cable.start);
    this.cable.integrate(anchor, this.attached);
  }

  draw(): void {
    const reset = this.attached && this.remaining <= 0;
    if (reset) { this.position.copy(this.home); this.open = 0; }
    this.node.position.copy(this.position);
    this.node.parent?.worldToLocal(this.node.position);
    this.node.updateMatrix();
    for (const joint of this.joints) {
      if (joint.node.userData.connectorLatch === "cam") {
        joint.node.quaternion.copy(joint.rotation).multiply(new THREE.Quaternion().setFromAxisAngle(
          new THREE.Vector3(0, 0, 1), openingAngle * this.open));
      } else joint.node.position.copy(joint.position).z += .02 * this.open;
      joint.node.updateMatrix();
    }
    this.cable.draw(reset);
  }
}

/** Install only for a published bench carrying the drag record and station data. */
export function installConnectorDrag(root: THREE.Object3D, camera: THREE.Camera, canvas: HTMLCanvasElement,
  controls: { enabled: boolean }, redraw: () => void): (() => void) | undefined {
  if (root.userData.modernIndustrialCableDrag !== 1) return;
  root.updateMatrixWorld(true);
  const nodes = new Map<string, THREE.Object3D>();
  const jackets: THREE.Mesh[] = [];
  root.traverse(node => {
    if (node.userData.objectId) nodes.set(node.userData.objectId, node);
    if (node instanceof THREE.Mesh && node.geometry.hasAttribute("_cable_station")) jackets.push(node);
  });
  if (jackets.length !== 3) return;
  const sockets = [1, 2, 3].map(i => nodes.get(`high-voltage-socket-${i}`));
  const bodies = [1, 2, 3].map(i => nodes.get(`high-voltage-plug-${i}`));
  if (sockets.some(node => !node) || bodies.some(node => !node)) return;
  // The authored mount row runs along -Z after the glTF axis conversion.
  const cables = jackets.map(mesh => new CableSpan(mesh)).sort((a, b) => b.start.z - a.start.z);
  const plugs = cables.map((cable, i) => new Plug(bodies[i]!, sockets[i]!, cable));
  const owners = new Map<THREE.Object3D, Plug>();
  for (const plug of plugs) {
    plug.node.traverse(node => owners.set(node, plug));
    plug.socket.traverse(node => owners.set(node, plug));
  }
  const targets = plugs.flatMap(plug => [plug.node, plug.socket]);
  const ray = new THREE.Raycaster(), cursor = new THREE.Vector2();
  const plane = new THREE.Plane(), hit = new THREE.Vector3(), offset = new THREE.Vector3();
  const normal = new THREE.Vector3(), desired = new THREE.Vector3();
  const snapPoint = new THREE.Vector3(), seatedGrip = new THREE.Vector3();
  let dragging: { plug: Plug; pointer: number; startX: number; startY: number; controlsEnabled: boolean; near: boolean } | undefined;
  let frame = 0, last = 0, accumulator = 0;

  function tick(now: number) {
    frame = 0;
    accumulator += Math.min(.04, (now - last) / 1000);
    last = now;
    const active = plugs.filter(plug => plug.remaining > 0 || dragging?.plug === plug);
    if (!active.length) return;
    while (accumulator >= step) {
      for (const plug of active) {
        const held = dragging?.plug === plug;
        if (!held) plug.remaining -= step;
        plug.integrate(held);
      }
      accumulator -= step;
    }
    for (const plug of active) plug.draw();
    redraw();
    if (dragging || active.some(plug => plug.remaining > 0)) frame = requestAnimationFrame(tick);
  }

  function animate() {
    if (frame) return;
    last = performance.now();
    accumulator = 0;
    frame = requestAnimationFrame(tick);
  }

  function cast(event: PointerEvent) {
    const rect = canvas.getBoundingClientRect();
    cursor.set(2 * (event.clientX - rect.left) / rect.width - 1, 1 - 2 * (event.clientY - rect.top) / rect.height);
    ray.setFromCamera(cursor, camera);
  }

  function down(event: PointerEvent) {
    if (event.button !== 0 || dragging) return;
    cast(event);
    const intersection = ray.intersectObjects(targets, true)[0];
    const plug = intersection && owners.get(intersection.object);
    if (!plug) return;
    camera.getWorldDirection(normal);
    plane.setFromNormalAndCoplanarPoint(normal, intersection.point);
    offset.copy(plug.position).sub(intersection.point);
    seatedGrip.copy(plug.home).sub(offset);
    dragging = { plug, pointer: event.pointerId, startX: event.clientX, startY: event.clientY,
      controlsEnabled: controls.enabled, near: plug.attached };
    plug.velocity = 0;
    controls.enabled = false;
    event.preventDefault();
    event.stopImmediatePropagation();
    canvas.setPointerCapture(event.pointerId);
    canvas.style.cursor = "grabbing";
  }

  function move(event: PointerEvent) {
    if (!dragging || dragging.pointer !== event.pointerId) return;
    cast(event);
    if (!ray.ray.intersectPlane(plane, hit)) return;
    const { plug, startX, startY } = dragging;
    const movement = Math.hypot(event.clientX - startX, event.clientY - startY);
    if (plug.attached && movement < 7) return;
    desired.copy(hit).add(offset);
    // A detached plug can be at a different camera depth from its seat.
    // Compare the projected grip and socket as well as the 3D drag point.
    const { width, height } = canvas.getBoundingClientRect();
    const closeOnScreen = (point: THREE.Vector3) => {
      snapPoint.copy(point).project(camera);
      return Math.hypot((snapPoint.x - cursor.x) * width / 2, (snapPoint.y - cursor.y) * height / 2) < 0.052 * height;
    };
    dragging.near = desired.distanceTo(plug.home) < .19 || closeOnScreen(seatedGrip) || closeOnScreen(plug.center);
    plug.attached = false;
    canvas.style.cursor = dragging.near ? "alias" : "grabbing";
    plug.goal.set(THREE.MathUtils.clamp(desired.x, plug.home.x + latchClearance, plug.home.x + .78),
      THREE.MathUtils.clamp(desired.y, plug.minimumY, plug.home.y + .70),
      THREE.MathUtils.clamp(desired.z, plug.home.z - .50, plug.home.z + .50));
    animate();
  }

  function up(event: PointerEvent) {
    if (!dragging || dragging.pointer !== event.pointerId) return;
    const { plug, near, controlsEnabled } = dragging;
    if (!plug.attached) {
      plug.attached = near && event.type === "pointerup";
      plug.remaining = plug.attached ? .45 : 2.4;
      animate();
    }
    dragging = undefined;
    controls.enabled = controlsEnabled;
    canvas.style.cursor = "";
    if (canvas.hasPointerCapture(event.pointerId)) canvas.releasePointerCapture(event.pointerId);
  }

  canvas.addEventListener("pointerdown", down, true);
  canvas.addEventListener("pointermove", move);
  canvas.addEventListener("pointerup", up);
  canvas.addEventListener("pointercancel", up);
  canvas.addEventListener("lostpointercapture", up);
  return () => {
    cancelAnimationFrame(frame);
    canvas.removeEventListener("pointerdown", down, true);
    canvas.removeEventListener("pointermove", move);
    canvas.removeEventListener("pointerup", up);
    canvas.removeEventListener("pointercancel", up);
    canvas.removeEventListener("lostpointercapture", up);
  };
}

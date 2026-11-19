import { Box3, type Frustum, type Vector3 } from "three";

/** Camera-dependent tile levels for one patch of static foliage. */
export class FoliageVisibility {
  readonly chosen = new Map<number, number>();
  private bounds?: Box3;
  private allHidden = false;
  private readonly tiles: ReadonlyMap<number, Box3>;
  private readonly starts: ReadonlyArray<readonly [number, number]>;
  private readonly far: number;

  constructor(tiles: ReadonlyMap<number, Box3>, starts: ReadonlyArray<readonly [number, number]>, far: number) {
    this.tiles = tiles;
    this.starts = starts;
    this.far = far;
    // A single-tile plant already has the smallest useful bound.
    if (tiles.size > 1) {
      this.bounds = new Box3();
      for (const tile of tiles.values()) this.bounds.union(tile);
    }
  }

  /** Returns true only when the renderer needs to repack instance buffers. */
  update(frustum: Frustum, eye: Vector3): boolean {
    if (this.bounds && (!frustum.intersectsBox(this.bounds) || this.bounds.distanceToPoint(eye) >= this.far)) {
      if (this.allHidden) return false;
      for (const tile of this.tiles.keys()) this.chosen.set(tile, -1);
      this.allHidden = true;
      return true;
    }
    let changed = false;
    this.allHidden = true;
    for (const [tile, box] of this.tiles) {
      let level = -1;
      if (frustum.intersectsBox(box)) {
        const distance = box.distanceToPoint(eye);
        if (distance < this.far) for (const [index, start] of this.starts) if (start <= distance) level = index;
      }
      if (level >= 0) this.allHidden = false;
      if (this.chosen.get(tile) !== level) {
        this.chosen.set(tile, level);
        changed = true;
      }
    }
    return changed;
  }
}

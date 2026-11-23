// A small Z-up lawn in meters. The slab occupies the west edge of the grid.
export const slab = { min: [-2, -1.5], max: [-0.95, 0.9], top: 0 };

function lawnHeight(x, y) {
  const dx = Math.max(slab.min[0] - x, x - slab.max[0], 0);
  const dy = Math.max(slab.min[1] - y, y - slab.max[1], 0);
  const edge = Math.min(1, Math.hypot(dx, dy) / 0.9);
  const roll = 0.031 * Math.sin(2.1 * x - 0.4) * Math.cos(1.7 * y)
    + 0.018 * Math.sin(1.1 * x + 1.8 * y);
  return -0.045 + roll * edge ** 2 * (3 - 2 * edge);
}

export const shrubBase = [0.1, 0.15, lawnHeight(0.1, 0.15)];

function makeGround() {
  const columns = 28, rows = 24, stride = columns + 1;
  const xAt = column => (-200 + 15 * column) / 100;
  const yAt = row => (-180 + 15 * row) / 100;
  const positions = [], indices = [];
  for (let row = 0; row <= rows; row++) {
    for (let column = 0; column <= columns; column++) {
      const x = xAt(column), y = yAt(row);
      positions.push([x, y, lawnHeight(x, y)]);
    }
  }
  for (let row = 0; row < rows; row++) {
    for (let column = 0; column < columns; column++) {
      const x = xAt(column + 0.5), y = yAt(row + 0.5);
      if (x > slab.min[0] && x < slab.max[0] && y > slab.min[1] && y < slab.max[1]) continue;
      const corner = row * stride + column;
      indices.push(corner, corner + 1, corner + stride + 1, corner, corner + stride + 1, corner + stride);
    }
  }
  return { positions, indices };
}

export const ground = makeGround();

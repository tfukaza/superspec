"""Convert the arcade's orthogonal window solids to Klali's Y-up box brushes."""

import math
import struct


def collision_accessor(meta, binary, index):
    a = meta["accessors"][index]
    view = meta["bufferViews"][a["bufferView"]]
    kind = {"VEC3": 3, "SCALAR": 1}[a["type"]]
    fmt = struct.Struct("<" + {5126: "f", 5125: "I", 5123: "H", 5121: "B"}[a["componentType"]] * kind)
    origin = view.get("byteOffset", 0) + a.get("byteOffset", 0)
    stride = view.get("byteStride", fmt.size)
    return [fmt.unpack_from(binary, origin + i * stride) for i in range(a["count"])]


def window_brushes(meta, binary, mesh):
    # Native collision positions already include the entire AML host transform.
    # An AABB of the complete host or window would seal its open aperture.
    ends_y, ends_z, faces = set(), set(), []
    for primitive in mesh["primitives"]:
        points = [tuple(round(v, 5) for v in p) for p in collision_accessor(meta, binary, primitive["attributes"]["POSITION"])]
        indices = ([i for i, in collision_accessor(meta, binary, primitive["indices"])]
                   if "indices" in primitive else list(range(len(points))))
        for i in range(0, len(indices), 3):
            a, b, c = (points[k] for k in indices[i:i+3])
            if not any(a[j] == b[j] == c[j] for j in range(3)):
                raise ValueError("Arcade window collision contains an oblique face")
            if a[0] != b[0] or a[0] != c[0]:
                continue
            tri = (a[1:], b[1:], c[1:])
            ends_y.update(p[0] for p in tri)
            ends_z.update(p[1] for p in tri)
            faces.append((a[0], tri))

    def cross(a, b, p):
        return (b[0]-a[0])*(p[1]-a[1]) - (b[1]-a[1])*(p[0]-a[0])

    def intervals(values):
        values = sorted(values)
        return zip(values, values[1:])

    # Slice only at actual vertex coordinates. Every resulting cell is either
    # occupied or empty. Use an off-diagonal interior point to test triangle fills.
    result = []
    for z0, z1 in intervals(ends_z):
        row = {}
        for y0, y1 in intervals(ends_y):
            p = (y0 + (y1-y0)*0.381966, z0 + (z1-z0)*0.533687)
            crossings = {}
            for x, (a, b, c) in faces:
                signs = (cross(a,b,p), cross(b,c,p), cross(c,a,p))
                if min(signs) > 0 or max(signs) < 0:
                    crossings[x] = crossings.get(x, 0) + (1 if signs[0] > 0 else -1)
            winding = 0
            active = {}
            for x, change in sorted(crossings.items()):
                if not change:
                    continue
                if not winding:
                    start = x
                winding += change
                if not winding:
                    key = (start, x)
                    if key in row:
                        brush = row[key]
                        brush["minZ"] = -y1
                    else:
                        brush = dict(minX=start,maxX=x,minY=z0,maxY=z1,minZ=-y1,maxZ=-y0)
                        result.append(brush)
                    active[key] = brush
            if winding:
                raise ValueError("Arcade window collision is not a closed solid")
            row = active
    if not result or not all(math.isfinite(v) for brush in result for v in brush.values()):
        raise ValueError("Missing or invalid arcade window collision")
    return result

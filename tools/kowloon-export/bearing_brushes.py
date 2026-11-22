"""Conservative Klali box strips taken from the native bearing triangle mesh."""

import math

from window_brushes import collision_accessor


def bearing_brushes(meta, binary, mesh):
    points = []
    edges = []
    for part in mesh["primitives"]:
        p = collision_accessor(meta, binary, part["attributes"]["POSITION"])
        indices = (
            [i for (i,) in collision_accessor(meta, binary, part["indices"])]
            if "indices" in part
            else list(range(len(p)))
        )
        points.extend(p)
        for i in range(0, len(indices), 3):
            face = [p[j] for j in indices[i : i + 3]]
            edges.extend(zip(face, face[1:] + face[:1]))
    if not points:
        raise ValueError("Empty bearing collision mesh")
    axis = max(
        (0, 1), key=lambda k: max(v[k] for v in points) - min(v[k] for v in points)
    )
    stations = sorted({round(v[axis], 6) for v in points})
    # Include actual profile corners, then subdivide so a player cannot collide
    # with the flat low bounding box of the whole incline.
    bounds = []
    for start, end in zip(stations, stations[1:]):
        pieces = math.ceil((end - start) / 0.030)
        bounds.extend(start + (end - start) * i / pieces for i in range(pieces))
    bounds.append(stations[-1])
    result = []
    for start, end in zip(bounds, bounds[1:]):
        samples = [p for p in points if start - 0.000001 <= p[axis] <= end + 0.000001]
        for a, b in edges:
            delta = b[axis] - a[axis]
            if abs(delta) < 1e-9:
                continue
            for cut in (start, end):
                t = (cut - a[axis]) / delta
                if 0 <= t <= 1:
                    samples.append([u + (v - u) * t for u, v in zip(a, b)])
        if not samples:
            continue
        low = [round(min(v[k] for v in samples), 4) for k in range(3)]
        high = [round(max(v[k] for v in samples), 4) for k in range(3)]
        low[axis], high[axis] = round(start, 4), round(end, 4)
        if min(high[k] - low[k] for k in range(3)) <= 0:
            continue
        result.append(
            dict(
                minX=low[0],
                maxX=high[0],
                minY=low[2],
                maxY=high[2],
                minZ=-high[1],
                maxZ=-low[1],
            )
        )
    return result

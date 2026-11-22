"""Fit the lower-arcade passage stencil against the authored wall and gameplay masonry."""

import json
import subprocess
import sys
import tempfile
from pathlib import Path
from xml.etree import ElementTree as ET

LAYOUT = json.loads(Path(__file__).with_name("passage-layout.json").read_text())
WIDTH = LAYOUT["stencilWidth"]
HEIGHT = WIDTH * 384 / 1024
RADIUS = 0.42
STANDING_HEIGHT = 1.82


def intersect(a, b):
    return all(a[i] < b[i + 3] and b[i] < a[i + 3] for i in range(3))


def box_bounds(box):
    p = [float(t) for t in box.get("position").split()]
    return [
        *p,
        *(p[i] + float(box.get(k)) for i, k in enumerate(("width", "depth", "height"))),
    ]


def passage_wall(root):
    host = root.find(f"box[@id='{LAYOUT['host']}']")
    if (
        host is None
        or host.get("visible") == "false"
        or host.get("rotation")
        or host.get("material") != "m-yellow"
    ):
        return None
    return host


def fit(root, masonry, city=None):
    host = passage_wall(root)
    if host is None:
        return None
    x0, y0, z0, x1, face, top = box_bounds(host)
    x, z = (x0 + x1) / 2, z0 + LAYOUT["stencilCenterHeight"]
    if x1 - x0 < WIDTH + 0.2 or z + HEIGHT / 2 > top - 0.1:
        return None
    surface = [
        x - WIDTH / 2,
        face + 0.001,
        z - HEIGHT / 2,
        x + WIDTH / 2,
        face + 0.2,
        z + HEIGHT / 2,
    ]
    if any(
        intersect(surface, box_bounds(b))
        for b in root.findall("box")
        if b is not host and b.get("visible") != "false" and not b.get("rotation")
    ):
        return None

    # Existing maintenance labels ride the planter service runs upstairs. Keep
    # an equipment-free patch if cabinets are moved onto the lower wall.
    if city is not None:
        for cabinet in city.findall("utility-cabinet"):
            cx, cy, cz = map(float, cabinet.get("position").split())
            if intersect(
                surface,
                [cx - 0.48, cy - 0.48, cz - 0.5, cx + 0.48, cy + 0.48, cz + 1.1],
            ):
                return None

    # Travel parallel to the face, around the open east jamb and into the south aisle.
    # The perpendicular second leg actually crosses the wall's plane.
    near_y = face + 0.92
    posts = [root.find(f"box[@id='brush-{i}']") for i in (686, 687)]
    if any(
        b is None or b.get("rotation") or b.get("collidable") != "true" for b in posts
    ):
        return None
    left, right_post = (box_bounds(b) for b in posts)
    if abs(left[4] - right_post[4]) > 0.01 or right_post[0] - left[3] <= 2 * RADIUS:
        return None
    solids = [
        [b["minX"], -b["maxZ"], b["minY"], b["maxX"], -b["minZ"], b["maxY"]]
        for b in masonry
    ]
    # Klali keeps its imported arena brushes alongside the supplemental masonry
    # JSON. The original floor and jambs belong to the first group.
    solids.extend(
        box_bounds(b)
        for b in root.findall("box")
        if b.get("id", "").startswith("brush-")
        and b.get("collidable") == "true"
        and not b.get("rotation")
    )

    def walkable(route):
        for (ax, ay), (bx, by) in zip(route, route[1:]):
            sweep = [
                min(ax, bx) - RADIUS,
                min(ay, by) - RADIUS,
                z0 + 0.002,
                max(ax, bx) + RADIUS,
                max(ay, by) + RADIUS,
                z0 + STANDING_HEIGHT,
            ]
            if any(intersect(sweep, b) for b in solids):
                return False
            # The entire swept footprint must have level support under it.
            if not any(
                abs(b[5] - z0) < 0.002
                and all(b[i] <= sweep[i] and sweep[i + 3] <= b[i + 3] for i in (0, 1))
                for b in solids
            ):
                return False
        return True

    # Prefer the free east end of the original wall. If that path is closed,
    # try the west end and cross the back aisle to the same real doorway.
    for jamb in (x1 + 0.85, x0 - 0.85):
        door_x = min(max(jamb, left[3] + RADIUS + 0.04), right_post[0] - RADIUS - 0.04)
        back_y = y0 - RADIUS - 0.08
        route = [
            (x, near_y),
            (jamb, near_y),
            (jamb, back_y),
            (door_x, back_y),
            (door_x, left[4] - 0.03),
        ]
        if walkable(route):
            break
    else:
        return None
    normal = [0, 1, 0]
    right = [-normal[1], normal[0], 0]
    towards = sum((route[1][i] - route[0][i]) * right[i] for i in (0, 1))
    return dict(
        host=host.get("id"),
        center=[x, face + 0.003, z],
        width=WIDTH,
        height=HEIGHT,
        right=right,
        normal=normal,
        art="passage-left.png" if towards < 0 else "passage.png",
    )


def passage_lamp_fits(root, masonry, city, lamp):
    """Retain the authored sign fitting only while it has a stencil and a clear seat."""
    station = fit(root, masonry, city)
    if not station:
        return False
    x, y, z = map(float, lamp.get("position").split())
    cx, cy, cz = station["center"]
    expected_z = cz - LAYOUT["stencilCenterHeight"] + LAYOUT["lampCenterHeight"]
    if (
        abs(x - cx) > 0.002
        or abs(y - (cy - 0.004)) > 0.002
        or abs(z - expected_z) > 0.002
        or abs(float(lamp.get("heading", 0)) % 360 - 180) > 0.001
    ):
        return False
    envelope = [x - 0.27, y - 0.001, z - 0.155, x + 0.27, y + 0.23, z + 0.155]
    for b in masonry:
        bounds = [b["minX"], -b["maxZ"], b["minY"], b["maxX"], -b["minZ"], b["maxY"]]
        if intersect(envelope, bounds):
            return False
    for cabinet in city.findall("utility-cabinet"):
        px, py, pz = map(float, cabinet.get("position").split())
        if intersect(
            envelope, [px - 0.48, py - 0.48, pz - 0.5, px + 0.48, py + 0.48, pz + 1.1]
        ):
            return False
    return True


def build_wayfinding(root, masonry, output, city=None):
    station = fit(root, masonry, city if city is not None else root.find("city"))
    if not station:
        return False
    return export_station(station, "build.mjs", output)


def export_station(station, script, output):
    with tempfile.NamedTemporaryFile(
        dir=output.parent, suffix=".glb", delete=False
    ) as f:
        tmp = Path(f.name)
    try:
        subprocess.run(
            ["node", str(Path(__file__).with_name(script)), str(tmp)],
            input=json.dumps(station),
            text=True,
            check=True,
        )
        tmp.replace(output)
    finally:
        tmp.unlink(missing_ok=True)
    return True


if __name__ == "__main__":
    scene, masonry, output = map(Path, sys.argv[1:])
    if not build_wayfinding(
        ET.parse(scene).getroot(), json.loads(masonry.read_text()), output
    ):
        raise SystemExit("No supported lower-arcade passage stencil")

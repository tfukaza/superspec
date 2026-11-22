"""Survey Stoa's authored routes, opaque wall panels and equipment for maintenance marks."""
import json
import math
import subprocess
import sys
import tempfile
from pathlib import Path
from xml.etree import ElementTree as ET


def vector(text):
    return [float(v) for v in text.split()]


def maintenance_inputs(root, city):
    # Dressed panels replace the visible brush while retaining its collision. Reject
    # other hidden brushes as decal hosts, since their faces may open into empty air.
    replaced = set()
    for panel in city.findall("procedural-wall"):
        x, y, z = vector(panel.get("position"))
        width, depth = float(panel.get("width")), float(panel.get("depth"))
        angle = math.radians(float(panel.get("heading", 0)))
        c, s = math.cos(angle), math.sin(angle)
        corners = [(x + u*c - v*s, y + u*s + v*c) for u in (0, width) for v in (0, depth)]
        low = [min(p[0] for p in corners), min(p[1] for p in corners), z]
        high = [max(p[0] for p in corners), max(p[1] for p in corners), z + float(panel.get("height"))]
        replaced.add(tuple(round(v, 3) for v in low + high))
    walls = []
    for box in root.findall("box"):
        if not box.get("id", "").startswith("brush-") or box.get("rotation"):
            continue
        p = vector(box.get("position"))
        size = [float(box.get(k, 0)) for k in ("width", "depth", "height")]
        if size[2] >= 1.2 and min(size[:2]) <= 1:
            high = [v + size[i] for i, v in enumerate(p)]
            if box.get("visible") != "false" or tuple(round(v, 3) for v in p + high) in replaced:
                walls.append(dict(id=box.get("id"), low=p, high=high))
    pipes, cabinets = [], []
    for obj in city:
        if obj.tag not in ("pipe-run", "utility-cabinet"):
            continue
        origin = vector(obj.get("position"))
        angle = math.radians(float(obj.get("heading", 0)))
        c, s = math.cos(angle), math.sin(angle)

        def turn(p):
            return [p[0]*c-p[1]*s, p[0]*s+p[1]*c, p[2]]

        if obj.tag == "pipe-run" and obj.get("supports") and obj.get("id", "").startswith("planter-service-"):
            points = [[v + origin[i] for i, v in enumerate(turn(vector(p)))] for p in obj.get("points").split(";")]
            supports = [turn(vector(p)) for p in obj.get("supports").split(";")]
            pipes.append(dict(id=obj.get("id"), points=points, supports=supports, radius=float(obj.get("radius", .055))))
        elif obj.tag == "utility-cabinet":
            cabinets.append(dict(position=origin, normal=turn([0, -1, 0])))
    return dict(walls=walls, pipes=pipes, cabinets=cabinets)


def build_maintenance(root, city, output):
    layout = maintenance_inputs(root, city)
    if not layout["pipes"]:
        return False
    program = Path(__file__).with_name("maintenance") / "build.mjs"
    with tempfile.NamedTemporaryFile(dir=output.parent, suffix=".glb", delete=False) as stream:
        temporary = Path(stream.name)
    try:
        result = subprocess.run(
            ["node", str(program), str(temporary)], input=json.dumps(layout),
            text=True, check=True, stdout=subprocess.PIPE,
        )
        if not json.loads(result.stdout)["stations"]:
            return False
        temporary.replace(output)
    finally:
        temporary.unlink(missing_ok=True)
    return True


if __name__ == "__main__":
    scene = ET.parse(sys.argv[1]).getroot()
    build_maintenance(scene, scene.find("city"), Path(sys.argv[2]))

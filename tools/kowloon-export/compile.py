"""Compile the editable scene in bounded pieces. Repeated exterior props need just one native build."""
import argparse
import json
import os
from concurrent.futures import ThreadPoolExecutor
import struct
import subprocess
import tempfile
from shutil import copyfile
from pathlib import Path
from xml.etree import ElementTree as ET
from window_brushes import window_brushes
from bearing_brushes import bearing_brushes
from upper_lighting import planter_light_mount
from maintenance_layout import build_maintenance
from vines.source import build_vines
from wayfinding.source import build_wayfinding, passage_lamp_fits
from wayfinding.drain import build_drain

parser = argparse.ArgumentParser()
parser.add_argument("scene", type=Path)
parser.add_argument("cli")
parser.add_argument("output", type=Path)
parser.add_argument("--jobs", type=int, default=min(4, os.cpu_count() or 1),
                    help="maximum independent native prototype exports (default: up to four)")
args = parser.parse_args()
if args.jobs < 1:
    parser.error("--jobs must be positive")
out = args.output.resolve()
out.mkdir(parents=True, exist_ok=True)
project = args.scene.resolve().parent
root = ET.parse(args.scene).getroot()
palette_path = Path(__file__).resolve().parents[2] / "crates/engine/src/worldspec/themes/kowloon/materials/arena-materials.json"
palette = json.loads(palette_path.read_text())
for material in root.findall("material"):
    finish = palette.get(material.get("id", "").removeprefix("m-"))
    if finish and set(material.attrib) == {"id"}:
        material.set("color", "#" + finish["color"])
        if "texture" in finish:
            material.set("texture", "superspec-material:" + finish["texture"])
            material.set("repeat", str(finish.get("repeat", 0.45)))
        if "roughness" in finish:
            material.set("roughness", str(finish["roughness"]))
city = root.find("city")
root.remove(city)


def export(target, command, aml, *options):
    # Preserve the last usable asset if the compiler rejects the edited scene.
    with tempfile.NamedTemporaryFile(dir=out, delete=False) as stream:
        temporary = Path(stream.name)
    try:
        with temporary.open("wb") as stream:
            subprocess.run([args.cli, command, str(aml), *options], stdout=stream, check=True)
        temporary.replace(target)
    finally:
        temporary.unlink(missing_ok=True)


def compile_part(name, source, tree=False):
    aml = out / (name + ".aml")
    ET.ElementTree(source).write(aml, encoding="utf-8")
    if tree:
        # The scene packet carries the illustrated crown and exact bark meshes.
        export(out / (name + ".lsp"), "scene", aml)
    else:
        export(out / (name + ".glb"), "glb", aml, "--textures", "png", "--no-uv")


for material in root.findall("material"):
    texture = material.get("texture", "")
    if texture and not texture.startswith("superspec-material:"):
        copyfile(project / texture, out / texture)
arena = out / "arena.aml"
ET.ElementTree(root).write(arena, encoding="utf-8")
with ThreadPoolExecutor(max_workers=min(2, args.jobs)) as pool:
    visual = pool.submit(export, out / "arena.glb", "glb", arena, "--textures", "png", "--no-uv")
    collision = pool.submit(export, out / "collision.glb", "glb", arena, "--collision", "--no-uv")
    visual.result()
    collision.result()
glb = (out / "collision.glb").read_bytes()
meta = json.loads(glb[20:20+struct.unpack_from("<I",glb,12)[0]])
brushes = []
for node in meta["nodes"]:
    owner = node.get("extras", {}).get("objectId", "")
    if owner.startswith(("masonry-arcade-window-", "masonry-bearing-")):
        binary = memoryview(glb)[28 + struct.unpack_from("<I", glb, 12)[0]:]
        convert = bearing_brushes if owner.startswith("masonry-bearing-") else window_brushes
        brushes.extend(convert(meta, binary, meta["meshes"][node["mesh"]]))
        continue
    if not node.get("extras",{}).get("objectId","").startswith("masonry-"):
        continue
    # Native mesh accessors are Z-up under a single axis-conversion root.
    prim, = meta["meshes"][node["mesh"]]["primitives"]
    acc = meta["accessors"][prim["attributes"]["POSITION"]]
    x0,y0,z0 = acc["min"]
    x1,y1,z1 = acc["max"]
    brushes.append(dict(minX=round(x0,3),maxX=round(x1,3),minY=round(z0,3),maxY=round(z1,3),
                        minZ=round(-y1,3),maxZ=round(-y0,3)))
if not brushes:
    raise ValueError("No native masonry colliders")
(out / "stoacity-masonry.json").write_text(json.dumps(brushes,indent=2)+"\n")
lawn_sources = {}
prototypes = {}
placements = []
jobs = []
for prop in city:
    if (prop.tag == "passage-sign-lamp" and prop.get("id") == "arcade-passage-sign-lamp"
            and not passage_lamp_fits(root, brushes, city, prop)):
        continue
    intrinsic = {k: v for k, v in prop.attrib.items() if k not in ("id", "position", "heading", "count", "step", "scale")}
    key = (prop.tag, tuple(sorted(intrinsic.items())))
    if key not in prototypes:
        name = f"{prop.tag}-{len(prototypes)}"
        if prop.tag == "grass-field":
            companion = (project / prop.attrib["src"]).resolve()
            relative = companion.relative_to(project)
            target = out / relative
            target.parent.mkdir(parents=True, exist_ok=True)
            copyfile(companion, target)
            lawn_sources[name] = relative.as_posix()
        source = ET.Element("architecture", version="1", id=name)
        ET.SubElement(ET.SubElement(source, "city"), prop.tag, id="prototype", position="0 0 0", **intrinsic)
        jobs.append((name, source, prop.tag == "tree"))
        prototypes[key] = name
    origin = list(map(float, prop.attrib["position"].split()))
    step = list(map(float, prop.get("step", "0 0 0").split()))
    for i in range(int(prop.get("count", "1"))):
        placement = dict(prototype=prototypes[key], heading=float(prop.get("heading", 0)),
                         position=[v + i*d for v, d in zip(origin, step, strict=True)])
        if "scale" in prop.attrib:
            placement["scale"] = float(prop.attrib["scale"])
        if (
            prop.tag == "ceiling-twin-strip"
            and prop.get("enclosed") == "true"
            and prop.get("id", "").startswith("arcade-lower-light-")
        ):
            placement["lowerArcadeBatten"] = True
        if prop.tag in ("ceiling-twin-strip", "ceiling-bulkhead") and prop.get("id", "").startswith("arcade-light-"):
            mount = planter_light_mount(root, prop.tag, placement["position"])
            if mount:
                placement["planterLight"] = mount
        placements.append(placement)


# IDs and companion files are settled before any concurrent compiler reads.
with ThreadPoolExecutor(max_workers=args.jobs) as pool:
    exports = [pool.submit(compile_part, name, source, tree) for name, source, tree in jobs]
    dressing = [
        ("maintenance", pool.submit(build_maintenance, root, city, out / "maintenance.glb")),
        ("vines", pool.submit(build_vines, root, city, out / "vines.glb", project)),
        ("wayfinding", pool.submit(build_wayfinding, root, brushes, out / "wayfinding.glb", city)),
        ("passage-drain", pool.submit(build_drain, root, city, brushes, out / "passage-drain.glb")),
    ]
    for result in exports:
        result.result()
    for name, result in dressing:
        if result.result():
            placements.append(dict(prototype=name, position=[0, 0, 0], heading=0))

(out / "placements.json").write_text(json.dumps(placements, indent=2))

(out / "lawn-sources.json").write_text(json.dumps(lawn_sources, indent=2))

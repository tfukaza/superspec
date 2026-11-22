"""Collect native supports and soil extents for render-only vine geometry."""
import json
from pathlib import Path
from functools import lru_cache
import struct
import subprocess
import tempfile

def vector(s):
    return list(map(float, s.split()))

@lru_cache(maxsize=32)
def lawn_size(asset):
    # Read the native authored footprint without decoding meshes or PNGs.
    with asset.open("rb") as stream:
        magic, version, _, size, chunk = struct.unpack("<4s4I", stream.read(20))
        if magic != b"glTF" or version != 2 or chunk != 0x4E4F534A:
            raise ValueError(f"{asset}: expected GLB 2 JSON metadata")
        meta = json.loads(stream.read(size))
    lawn = meta["scenes"][meta.get("scene", 0)].get("extras", {}).get("treeSpecPaintedLawn")
    return (lawn["width"], lawn["depth"]) if lawn else None


def survey(root, city, project):
    boxes, beds, beams = [], [], []
    for box in root.iter("box"):
        if box.get("rotation") or float(box.get("heading", "0")) % 360:
            continue
        p = vector(box.get("position"))
        boxes.append(dict(id=box.get("id"), lo=p, hi=[p[i]+float(box.get(k)) for i,k in enumerate(("width","depth","height"))]))
    for bed in (root if city is None else city).iter("grass-field"):
        size = lawn_size(project / bed.get("src"))
        if size and not float(bed.get("heading", "0")) % 360:
            p = vector(bed.get("position"))
            beds.append(dict(lo=p, hi=[p[0]+size[0], p[1]+size[1], p[2]]))
    elements = {e.get("id"): e for e in root.iter() if e.get("id")}
    for sweep in root.iter("sweep"):
        if not sweep.get("id","").startswith("masonry-downstand-"):
            continue
        path, profile = elements[sweep.get("path")], elements[sweep.get("profile")]
        a,b = map(vector,path.get("points").split(";"))
        section = list(map(vector,profile.get("points").split(";")))
        if abs(a[2]-b[2]) > .001:
            continue
        long_axis = 1 if abs(a[0]-b[0]) < .001 else 0
        across = 1-long_axis
        if abs(a[across]-b[across]) > .001:
            continue
        lo = [min(x,y) for x,y in zip(a,b)]
        hi = [max(x,y) for x,y in zip(a,b)]
        lo[across] += min(v[0] for v in section)
        hi[across] += max(v[0] for v in section)
        lo[2] += min(v[1] for v in section)
        hi[2] += max(v[1] for v in section)
        beams.append(dict(id=sweep.get("id"), lo=lo, hi=hi))
    return dict(boxes=boxes, beds=beds, beams=beams)

def build_vines(root, city, output, project):
    source = survey(root, city, project)
    with tempfile.NamedTemporaryFile(dir=output.parent, suffix=".glb", delete=False) as stream:
        temporary = Path(stream.name)
    try:
        result = subprocess.run(["node",str(Path(__file__).with_name("build.mjs")),str(temporary)],input=json.dumps(source),text=True,check=True,stdout=subprocess.PIPE)
        if not json.loads(result.stdout)["sites"]:
            return False
        temporary.replace(output)
        return True
    finally:
        temporary.unlink(missing_ok=True)

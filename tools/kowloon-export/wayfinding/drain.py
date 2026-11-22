"""Survey a flush modular drain on the passage wall's actual paving."""

import json
import sys
from pathlib import Path
from xml.etree import ElementTree as ET
from .source import box_bounds, export_station, passage_wall, intersect

MODULE = 0.8
COUNT = 3
BED_WIDTH = 0.224


def drain_station(root, city, masonry):
    host = passage_wall(root)
    if host is None:
        return None
    x0, _, z, x1, front, _ = box_bounds(host)
    y = front + 0.166
    half_width = BED_WIDTH / 2
    length = MODULE * COUNT
    floors = [
        (box.get("id"), box_bounds(box))
        for box in root.findall("box")
        if box.get("id", "").startswith("brush-")
        and box.get("collidable") == "true"
        and box.get("visible") != "false"
        and not box.get("rotation")
    ]
    weeds = [
        (
            list(map(float, weed.get("position").split())),
            0.28 * float(weed.get("scale", 1)),
        )
        for weed in city.findall("weed")
    ]
    boxes = [
        box_bounds(b)
        for b in root.findall("box")
        if not b.get("rotation")
        and (b.get("visible") != "false" or b.get("collidable") == "true")
    ]
    boxes.extend(
        [b["minX"], -b["maxZ"], b["minY"], b["maxX"], -b["minZ"], b["maxY"]]
        for b in masonry
    )
    # Search away from the junction to keep a continuous grate beside the wall.
    # Leave a trim gap around the iron, and exclude the entire spread of each tuft.
    for i in range(int((x1 - x0) / 0.1)):
        end = x1 - 0.8 - i * 0.1
        start = end - length
        if start < x0 + 0.40:
            break
        low, high = y - half_width, y + half_width
        floor = next(
            (
                id
                for id, b in floors
                if abs(b[5] - z) < 0.001
                and b[0] <= start - 0.015
                and b[3] >= end + 0.015
                and b[1] <= low
                and b[4] >= high
            ),
            None,
        )
        if floor is None or any(
            intersect([start - 0.015, low, z + 0.001, end + 0.015, high, z + 0.15], b)
            for b in boxes
        ):
            continue
        if any(
            abs(p[2] - z) < 0.18
            and max(start - 0.015 - p[0], 0, p[0] - end - 0.015) ** 2
            + max(low - p[1], 0, p[1] - high) ** 2
            < (radius + 0.06) ** 2
            for p, radius in weeds
        ):
            continue
        return dict(
            origin=[start, y, z],
            module=MODULE,
            count=COUNT,
            width=BED_WIDTH,
            wall=host.get("id"),
            floor=floor,
        )
    return None


def build_drain(root, city, masonry, output):
    station = drain_station(root, city, masonry)
    return bool(station) and export_station(station, "drain-build.mjs", output)


if __name__ == "__main__":
    scene, masonry, output = map(Path, sys.argv[1:])
    root = ET.parse(scene).getroot()
    if not build_drain(
        root, root.find("city"), json.loads(masonry.read_text()), output
    ):
        raise SystemExit("No floor receiver for the lower arcade drain")

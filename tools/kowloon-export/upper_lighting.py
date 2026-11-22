"""Find the suspended floor below each upper arcade luminaire in native metres."""


def planter_light_mount(root, kind, position):
    x, y, elevation = position
    supports = []
    for box in root.findall("box"):
        if box.get("rotation") or box.get("collidable") != "true":
            continue
        width, depth, thickness = (float(box.get(k)) for k in ("width", "depth", "height"))
        if min(width, depth) < 2 or not 0.08 <= thickness <= 0.5:
            continue
        left, near, underside = map(float, box.get("position").split())
        floor = underside + thickness
        if left <= x <= left + width and near <= y <= near + depth and 1.8 < elevation - floor < 4.5:
            supports.append((floor, underside))
    if not supports:
        return None
    floor, underside = max(supports)
    return dict(fixture=kind, floorY=floor, undersideY=underside)

"""Build the Modern Industrial bench's three routed cables in the center mount frame."""

from math import cos, pi, sin

from mathutils import Vector

import connector_mesh as m
from power_plug import circular_section


MOUNT_SPACING = 1.17
LANE_SPACING = .19
JACKET_RADIUS = .083
FLOOR_CENTER = .09 - .735
EDGE_START = 3.31
DROP_RADIUS = .23


def run(lane):
    """Leave the gland straight, converge at the floor, then turn over the slab."""
    mount_y = lane * MOUNT_SPACING
    lane_y = lane * LANE_SPACING
    controls = tuple(map(Vector, (
        (1.366, mount_y, 0), (1.82, mount_y, 0),
        (2.29, lane_y, FLOOR_CENTER), (2.84, lane_y, FLOOR_CENTER),
    )))
    a, b, c, d = controls
    yield Vector((1.164, mount_y, 0)), Vector((1, 0, 0))
    for i in range(33):
        t = i / 32
        u = 1 - t
        center = u**3*a + 3*u*u*t*b + 3*u*t*t*c + t**3*d
        tangent = u*u*(b-a) + 2*u*t*(c-b) + t*t*(d-c)
        yield center, tangent.normalized()
    yield Vector((EDGE_START, lane_y, FLOOR_CENTER)), Vector((1, 0, 0))
    for i in range(1, 17):
        angle = .5 * pi * i / 16
        yield Vector((EDGE_START + DROP_RADIUS*sin(angle), lane_y,
                      FLOOR_CENTER - DROP_RADIUS*(1-cos(angle)))), Vector((cos(angle), 0, -sin(angle)))
    # Leave the hanging end straight with a horizontal cut.
    yield Vector((EDGE_START + DROP_RADIUS, lane_y, -1.27)), Vector((0, 0, -1))


def jackets(materials):
    # Keep one mesh for the cable finish binding and assign one material slot per run.
    section = circular_section(48)
    sides = len(section)
    vertices, faces, caps, lane_ends, stations = [], [], [], [], []
    for lane in (-1, 0, 1):
        frames = list(run(lane))
        start = len(vertices)
        for station, (center, axis) in enumerate(frames):
            across = (Vector((0, 1, 0)) - axis * axis.y).normalized()
            up = axis.cross(across)
            vertices.extend(tuple(center + JACKET_RADIUS * (across*y + up*z)) for y, z in section)
            stations.extend([station] * sides)
        caps.append(len(faces))
        faces.append(tuple(reversed(range(start, start+sides))))
        for ring in range(len(frames)-1):
            for j in range(sides):
                a = start + ring*sides + j
                b = start + ring*sides + (j+1)%sides
                faces.append((a, b, b+sides, a+sides))
        caps.append(len(faces))
        faces.append(tuple(range(len(vertices)-sides, len(vertices))))
        lane_ends.append(len(faces))
    jacket = m.mesh("Power cable | orange jacket", vertices, faces, materials[0], smooth=True)
    # Preserve the sweep's station identity through GLB vertex splitting.
    station_attribute = jacket.data.attributes.new("_CABLE_STATION", "FLOAT", "POINT")
    station_attribute.data.foreach_set("value", stations)
    for material in materials[1:]:
        jacket.data.materials.append(material)
    for slot, (start, end) in enumerate(zip([0] + lane_ends, lane_ends)):
        for polygon in jacket.data.polygons[start:end]:
            polygon.material_index = slot
    for cap in caps:
        jacket.data.polygons[cap].use_smooth = False


def band(x, index, strap, buckle):
    """Two nested stadium sections form a continuous wrap around the bundle."""
    def outline(radius):
        return [(side * LANE_SPACING + radius*cos(angle), FLOOR_CENTER + radius*sin(angle))
                for side, start in [(1, -.5*pi), (-1, .5*pi)]
                for angle in (start + step*pi/16 for step in range(17))]

    width = .030
    profile = [(x-width/2, .083), (x+width/2, .083),
               (x+width/2, .089), (x-width/2, .089)]
    vertices = [(station, y, z) for station, radius in profile for y, z in outline(radius)]
    count = len(vertices) // len(profile)
    faces = []
    for ring in range(len(profile)):
        for j in range(count):
            k = (j+1) % count
            base, next_base = ring*count, ((ring+1)%len(profile))*count
            faces.append((base+j, next_base+j, next_base+k, base+k))
    wrap = m.mesh(f"Bundle band {index} | graphite wrap", vertices, faces, strap)
    for polygon in wrap.data.polygons[2*count:3*count]:
        polygon.use_smooth = True
    m.box(f"Bundle band {index} | locking buckle", (x, 0, FLOOR_CENTER+.093), (.042, .061, .014), buckle)


def build():
    jackets((
        m.material("Plug | red jacket", (.76, .20, .23)),
        m.material("Plug | safety orange jacket", (.89, .355, .112)),
        m.material("Plug | gold yellow jacket", (.89, .69, .22)),
    ))
    strap = m.material("Bundle | graphite strap", (.19, .195, .21))
    buckle = m.material("Bundle | satin buckle", (.35, .36, .39))
    for index, x in enumerate((2.97, 3.20), 1):
        band(x, index, strap, buckle)

"""Editable plug, strain relief and attached power cable (metres, Z up)."""

from math import cos, pi, sin, sqrt

import connector_mesh as m


def circular_section(count=48, exponent=1.0):
    def coordinate(value):
        return (1 if value >= 0 else -1) * abs(value) ** exponent

    return [(coordinate(cos(i * 2 * pi / count)), coordinate(sin(i * 2 * pi / count)))
            for i in range(count)]


def housing_section(flat):
    return [(1, flat), (flat, 1), (-flat, 1), (-1, flat),
            (-1, -flat), (-flat, -1), (flat, -1), (1, -flat)]


def loft(name, section, stations, material, smooth=False, sharp_rings=()):
    """Solid along X with independently scaled Y and Z sections."""
    count = len(section)
    vertices = [(x, y * ry, z * rz) for x, ry, rz in stations for y, z in section]
    faces = [tuple(reversed(range(count)))]
    for i in range(len(stations) - 1):
        for j in range(count):
            a, b = i * count + j, i * count + (j + 1) % count
            faces.append((a, b, b + count, a + count))
    faces.append(tuple(range(len(vertices) - count, len(vertices))))
    obj = m.mesh(name, vertices, faces, material, smooth=smooth)
    obj.data.polygons[0].use_smooth = obj.data.polygons[-1].use_smooth = False
    if sharp_rings:
        for edge in obj.data.edges:
            first, last = (index // count for index in edge.vertices)
            if first == last and first in sharp_rings:
                edge.use_edge_sharp = True
    return obj


def side_fastener(name, x, y, z, side, radius, material, normal_z=0):
    """Turned screw head with a recessed six-sided drive in its crown."""
    depth = radius * .28
    normal_y = side * sqrt(1 - normal_z**2)
    vertices = []
    for profile_radius, offset, count in [(1, -.001, 12), (.84, depth, 12),
                                          (.41, depth, 6), (.34, .0001, 6)]:
        for i in range(count):
            a = i * 2 * pi / count
            r = profile_radius * radius
            across = r * sin(a)
            vertices.append((x + r * cos(a),
                             y + normal_y * offset + normal_z * across,
                             z + normal_z * offset - normal_y * across))
    faces = [tuple(reversed(range(12)))]
    for j in range(12):
        k = (j + 1) % 12
        faces.append((j, k, k + 12, j + 12))
    for j in range(6):
        k = (j + 1) % 6
        # One planar crown sector bridges two outside edges to each drive flat.
        faces.append((12 + 2*j, 13 + 2*j, 12 + 2*k, 24+k, 24+j))
        faces.append((24+j, 24+k, 30+k, 30+j))
    faces.append(tuple(range(30, 36)))
    head = m.mesh(name, vertices, faces, material)
    # Only the turned perimeter is circular. The crown and the six drive flats are planar.
    for face in range(1, 13):
        head.data.polygons[face].use_smooth = True


def access_cover(side, finish, dark, screws):
    # This grid leaves two through-slots. The housing beneath it forms the back of the vents.
    name = 'Port' if side < 0 else 'Starboard'
    x_min, x_max = .348, .634
    levels = [-.147, -.134, -.092, -.082, -.067, -.057, .128, .141]
    vertices = []
    for depth in [.2755, .285]:
        for row, z in enumerate(levels):
            clip = .012 if row in [0, len(levels) - 1] else 0
            vertices.extend((x, side * depth, z) for x in
                            [x_min + clip, .394, .555, x_max - clip])
    count = len(levels) * 4
    faces = []
    edges = {}
    for row in range(len(levels) - 1):
        for col in range(3):
            if col == 1 and row in (2, 4):
                continue
            i = row * 4 + col
            loop = (i, i + 1, i + 5, i + 4)
            if side > 0:
                loop = tuple(reversed(loop))
            faces.append(tuple(v + count for v in loop))
            faces.append(tuple(reversed(loop)))
            for a, b in zip(loop, loop[1:] + loop[:1]):
                if (b, a) in edges:
                    del edges[b, a]
                else:
                    edges[a, b] = True
    faces.extend((a, b, b + count, a + count) for a, b in edges)
    m.box(f'{name} | vent cavity', (.475, side * .2741, -.075),
          (.182, .0025, .065), dark)
    m.mesh(f'{name} | twin-slot access cover', vertices, faces, finish)
    for x in [.362, .619]:
        for z in [-.128, .123]:
            side_fastener(f'{name} | cover screw', x, side * .285, z, side, .0073, screws)


def clamp_bracket(name, angle, housing, machined, fasteners):
    """A stepped saddle with a reinforcing web, bearing shoes, and recessed hardware."""
    first = set(m.bpy.data.objects)
    # An axial section follows the larger plate's rim and steps onto the smaller plate.
    profile = [(1.041, .169), (1.086, .169), (1.103, .136), (1.142, .136),
               (1.146, .148), (1.121, .155), (1.105, .183), (1.047, .186)]
    half_width = .026
    vertices = [(x, y, z) for z in [-half_width, half_width] for x, y in profile]
    count = len(profile)
    faces = [tuple(reversed(range(count))), tuple(range(count, 2 * count))]
    for i in range(count):
        j = (i + 1) % count
        faces.append((i, j, j + count, i + count))
    m.mesh(f'{name} | stepped clamp web', vertices, faces, housing)
    for x, y, label in [(1.060, .183, 'base'), (1.130, .147, 'cap')]:
        # Small bridge shoes span the web and end in beveled hex-socket heads.
        m.box(f'{name} | {label} bridge shoe', (x, y, 0), (.025, .012, .079), machined)
        for z in (-.027, .027):
            side_fastener(f'{name} | {label} socket-head screw', x, y + .006, z,
                          1, .0084, fasteners)
    m.box(f'{name} | raised clamp spine', (1.078, .188, 0), (.026, .009, .018), machined)
    # Repeat the saddle on four flats without introducing reflected winding.
    from mathutils import Matrix
    turn = Matrix.Rotation(angle, 4, 'X')
    for object_ in set(m.bpy.data.objects) - first:
        object_.matrix_basis = turn @ object_.matrix_basis


def power_cable(orange):
    # The straight throat and bend share a horizontal tangent at x=1.366.
    spans = [((1.366, 0), (1.491, 0), (1.593, .018), (1.709, .178)),
             ((1.709, .178), (1.829, .343), (1.755, .540), (1.635, .680)),
             ((1.635, .680), (1.537, .795), (1.417, .875), (1.328, .969))]
    frames = [(1.164, 0, 0, 1)]
    for k, ((p0, p1, p2, p3), steps) in enumerate(zip(spans, (9, 12, 6))):
        for i in range(0 if k == 0 else 1, steps + 1):
            t, u = i / steps, 1 - i / steps
            cx, cz = [u**3 * p0[c] + 3*u*u*t * p1[c] + 3*u*t*t * p2[c] + t**3 * p3[c]
                      for c in range(2)]
            dx, dz = [3 * (u*u * (p1[c]-p0[c]) + 2*u*t * (p2[c]-p1[c])
                           + t*t * (p3[c]-p2[c])) for c in range(2)]
            length = sqrt(dx*dx + dz*dz)
            frames.append((cx, cz, -dz/length, dx/length))
    sides = 48
    radius = .083
    section = circular_section(sides)
    vertices = [(cx + nx*radius*z, radius*y, cz + nz*radius*z)
                for cx, cz, nx, nz in frames for y, z in section]
    faces = [tuple(reversed(range(sides)))]
    for start in range(0, len(vertices) - sides, sides):
        for j in range(sides):
            a, b = start + j, start + (j + 1) % sides
            faces.append((a, b, b + sides, a + sides))
    faces.append(tuple(range(len(vertices) - sides, len(vertices))))
    jacket = m.mesh('Power cable | orange jacket', vertices, faces, orange, smooth=True)
    jacket.data.polygons[0].use_smooth = jacket.data.polygons[-1].use_smooth = False


def build(*, attached_cable=True):
    shell = m.material('Plug | warm slate enamel', (.54, .50, .49))
    panel = m.material('Plug | service covers', (.64, .60, .575))
    band = m.material('Plug | cast aluminum', (.66, .64, .625))
    gunmetal = m.material('Plug | gunmetal', (.305, .305, .32))
    screws = m.material('Plug | fasteners', (.40, .395, .385))
    graphite = m.material('Plug | graphite elastomer', (.202, .19, .226))
    groove = m.material('Plug | rubber fold recess', (.143, .135, .155))
    gasket = m.material('Plug | compressed gasket', (.113, .112, .133))
    contact = m.material('Plug | contact alloy', (.68, .57, .41))
    orange = m.material('Plug | safety orange jacket', (.89, .355, .112))

    circle = circular_section()
    loft('Mating spigot | graphite carrier', circle,
         [(-.123, .288, .288), (-.112, .300, .300), (-.100, .303, .303),
          (.212, .303, .303), (.229, .299, .299)], gunmetal, smooth=True, sharp_rings=(2, 3))
    m.cylinder('Mating spigot | insulating insert', (-.1235, 0, 0), (-.120, 0, 0),
               .246, gasket)
    for idx, (y, z) in enumerate([(-.105, .08), (.105, .08), (0, -.116)], 1):
        m.cylinder(f'Contact {idx} | shoulder', (-.126, y, z), (-.122, y, z), .043,
                   graphite, vertices=20)
        pin = loft(f'Contact {idx} | pin', circular_section(20),
                   [(-.160, .031, .031), (-.155, .035, .035), (-.122, .035, .035)],
                   contact, smooth=True, sharp_rings=(1,))
        pin.location.y, pin.location.z = y, z

    loft('Coupling | receiver-side retaining band', circle,
         [(.212, .302, .302), (.223, .316, .316), (.320, .316, .316),
          (.322, .314, .314)], band, smooth=True, sharp_rings=(1, 2))
    loft('Coupling | housing seam', circle, [(.321, .311, .311), (.325, .311, .311)],
         gasket, smooth=True)
    octagon = housing_section(.565)
    loft('Housing | armored casting', octagon,
         [(.324, .271, .286), (.326, .273, .288), (.629, .273, .288),
          (.666, .239, .256)], shell)
    for side in [-1, 1]:
        access_cover(side, panel, gasket, screws)
        # Midpoint and outward normal of the cast chamfers at the full-width station.
        reach_y, reach_z = .273, .288
        mid = (1 + .565) * .5
        normal_z = reach_y / sqrt(reach_y**2 + reach_z**2)
        for vertical, name in [(-1, 'Lower'), (1, 'Upper')]:
            side_fastener(f'Housing | {name.lower()} chamfer screw', .47, side * mid * reach_y,
                          vertical * mid * reach_z, side, .0145, gunmetal, vertical * normal_z)

    loft('Strain relief | housing seal', housing_section(.58),
         [(.648, .244, .26), (.664, .253, .267), (.694, .247, .258)], gasket)
    rubber_section = circular_section(40, .79)
    stations = []
    # The profile rolls only the lip corners; most of each crown and wall stays straight.
    fold = [(.095, 0), (.13, .002), (.166, .029), (.224, .033),
            (.713, .033), (.771, .029), (.813, .002), (.85, 0)]
    for i in range(5):
        for phase, rise in fold:
            t = (i + phase)/5
            base = .221 - .055*t
            stations.append((.677 + .332*t, base + rise, base + rise + .013))
    stations.extend([(1.009, .166, .179), (1.031, .158, .173), (1.050, .155, .173)])
    lip_edges = {i * len(fold) + edge for i in range(5) for edge in (1, 3, 4, 6)}
    boot = loft('Strain relief | five convolutions', rubber_section, stations, graphite,
                smooth=True, sharp_rings=lip_edges)
    boot.data.materials.append(groove)
    sides = len(rubber_section)
    for polygon in boot.data.polygons:
        if len(polygon.vertices) == 4:
            station = min(polygon.vertices) // sides
            if station < 5 * len(fold) and station % len(fold) in (0, 1, 5, 6, 7):
                polygon.material_index = 1

    clamp = housing_section(sqrt(2) - 1)
    loft('Cable clamp | gasket', clamp,
         [(1.025, .157, .157), (1.037, .168, .168), (1.046, .168, .168)], gasket)
    loft('Cable clamp | broad octagonal base', clamp,
         [(1.039, .174, .174), (1.041, .176, .176), (1.089, .176, .176),
          (1.091, .174, .174)], band)
    loft('Cable clamp | stepped octagonal cap', clamp,
         [(1.089, .140, .140), (1.091, .142, .142), (1.142, .142, .142),
          (1.144, .140, .140)], band)
    for index, name in enumerate(('Starboard', 'Upper', 'Port', 'Lower')):
        clamp_bracket(name, index * pi / 2, shell, band, screws)
    loft('Cable gland | compression ring', circular_section(),
         [(1.140, .111, .111), (1.146, .115, .115), (1.161, .115, .115),
          (1.166, .108, .108)], gunmetal, smooth=True, sharp_rings=(1, 2))
    loft('Cable gland | sealing lip', circular_section(),
         [(1.164, .099, .099), (1.172, .099, .099), (1.183, .091, .091)],
         gasket, smooth=True, sharp_rings=(1,))
    if attached_cable:
        power_cable(orange)

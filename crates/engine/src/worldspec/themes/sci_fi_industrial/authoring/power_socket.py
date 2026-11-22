"""Wall receptacle for the industrial power connector. +X is out of the wall."""

from math import cos, pi, sin, tau

from mathutils import Vector
from mathutils.geometry import delaunay_2d_cdt

import connector_mesh as m


RECEIVER_BASE = (.046, .438)
RECEIVER_LIP = (.180, .361)


def receiver_radius(x):
    base_x, base_r = RECEIVER_BASE
    lip_x, lip_r = RECEIVER_LIP
    return base_r + (lip_r - base_r) * (x - base_x) / (lip_x - base_x)


def circle(radius, center=(0, 0), count=64):
    y, z = center
    return [(y + radius * cos(tau * i / count), z + radius * sin(tau * i / count))
            for i in range(count)]


def rectangle(width, height, radius):
    loop = []
    for corner, (sy, sz) in enumerate(((1, 1), (-1, 1), (-1, -1), (1, -1))):
        y = sy * (width / 2 - radius)
        z = sz * (height / 2 - radius)
        for i in range(7):
            angle = (corner + i / 6) * pi / 2
            loop.append((y + radius * cos(angle), z + radius * sin(angle)))
    return loop


def cap(loops):
    """Triangulate one profile, retaining the mounting and contact apertures."""
    coords, outlines = [], []
    for loop in loops:
        outlines.append(tuple(range(len(coords), len(coords) + len(loop))))
        coords.extend(Vector(pt) for pt in loop)
    _, _, tris, sources, _, domains = delaunay_2d_cdt(coords, [], outlines, 1, 1e-8)
    return [tuple(sources[v][0] for v in tri) for tri, domain in zip(tris, domains)
            if list(domain) == [0]]


def casting(name, sections, holes, material, flat_holes=()):
    """Loft an outer profile with through bores of constant cross-section."""
    verts, faces, strips = [], [], []
    for idx, (x, outer) in enumerate(sections):
        start = len(verts)
        loops = [outer, *holes]
        for loop in loops:
            verts.extend((x, y, z) for y, z in loop)
        stride = len(verts) - start
        if idx in (0, len(sections) - 1):
            for tri in cap(loops):
                faces.append(tuple(start + v for v in (tri[::-1] if idx == 0 else tri)))
        if idx:
            base = start - stride
            for k, loop in enumerate(loops):
                strip_start = len(faces)
                flats = []
                for a in range(len(loop)):
                    b = (a + 1) % len(loop)
                    side = (base+a, base+b, base+b+stride, base+a+stride)
                    faces.append(side[::-1] if k else side)
                    dy = loop[b][0] - loop[a][0]
                    dz = loop[b][1] - loop[a][1]
                    flats.append(k-1 in flat_holes or min(abs(dy), abs(dz)) < 1e-9)
                strips.append((strip_start, flats))
                base += len(loop)
    part = m.mesh(name, verts, faces, material)
    mesh = part.data
    for edge in mesh.edges:
        a, b = edge.vertices
        if verts[a][0] == verts[b][0]:
            edge.use_edge_sharp = True

    # A rounded rectangle meets its straight rail tangentially. Give the
    # corner's endpoint the rail normal, keeping the broad stamped face flat.
    normals = [(0, 0, 0)] * len(mesh.loops)
    for start, flats in strips:
        for i, flat in enumerate(flats):
            if flat:
                continue
            face = mesh.polygons[start + i]
            face.use_smooth = True
            for j in ((i-1) % len(flats), (i+1) % len(flats)):
                if flats[j]:
                    rail = mesh.polygons[start + j]
                    for idx in face.loop_indices:
                        if mesh.loops[idx].vertex_index in rail.vertices[:]:
                            normals[idx] = tuple(rail.normal)
    if any(n != (0, 0, 0) for n in normals):
        mesh.normals_split_custom_set(normals)
    return part


def ring(name, section, material, center=(0, 0), count=64):
    """Turn a closed radial section about the insertion axis."""
    verts = [(x, y, z) for x, r in section for y, z in circle(r, center, count)]
    faces = []
    for i in range(len(section)):
        k = (i + 1) % len(section)
        for j in range(count):
            n = (j + 1) % count
            faces.append((i*count+j, i*count+n, k*count+n, k*count+j))
    part = m.mesh(name, verts, faces, material, smooth=True)
    # Smooth around each band while preserving its machined cross-section.
    for edge in part.data.edges:
        a, b = edge.vertices
        if a // count == b // count:
            edge.use_edge_sharp = True
    for i, (x, _) in enumerate(section):
        if x == section[(i + 1) % len(section)][0]:
            for j in range(count):
                part.data.polygons[i * count + j].use_smooth = False
    return part


def mounting_screw(index, y, z, metal, dark):
    center = (y, z)
    ring(f"Mount {index} | countersink",
         [(.021, .032), (.0465, .032), (.0465, .0285),
          (.033, .020), (.021, .020)], dark, center, 24)
    # Recessed cross in the crown with a solid web behind the drive.
    profile = [(1, 3.3), (-1, 3.3), (-1, 1), (-3.3, 1), (-3.3, -1), (-1, -1),
               (-1, -3.3), (1, -3.3), (1, -1), (3.3, -1), (3.3, 1), (1, 1)]
    angle = (.21, -.53, .67, -.19)[index - 1]
    scale = .0049
    drive = [(y + scale * (u*cos(angle)-v*sin(angle)),
              z + scale * (u*sin(angle)+v*cos(angle))) for u, v in profile]
    casting(f"Mount {index} | cross-head",
            [(.036, circle(.021, center, 24)), (.043, circle(.028, center, 24)),
             (.047, circle(.022, center, 24))],
            [drive], metal, flat_holes=(0,))
    m.cylinder(f"Mount {index} | drive floor", (.031, y, z), (.036, y, z),
               .021, dark, vertices=24)


def build():
    ivory = m.material("Socket | warm porcelain enamel", (.82, .80, .74))
    bezel = m.material("Socket | ivory casting", (.77, .75, .69))
    gasket = m.material("Socket | charcoal seal", (.17, .175, .18))
    throat = m.material("Socket | insulator", (.285, .29, .29))
    steel = m.material("Socket | satin hardware", (.54, .54, .52))
    copper = m.material("Socket | contact lining", (.49, .395, .28))

    mounts = [(y, z) for z in (.503, -.503) for y in (-.467, .467)]
    casting("Socket | wall gasket",
            [(0, rectangle(1.087, 1.166, .079)), (.012, rectangle(1.086, 1.166, .079))],
            [circle(.337)], gasket)
    casting("Socket | radiused mounting plate",
            [(.009, rectangle(1.10, 1.18, .082)), (.029, rectangle(1.10, 1.18, .082)),
             (.046, rectangle(1.072, 1.152, .073))],
            [circle(.337), *(circle(.032, c, 24) for c in mounts)], ivory)
    for i, (y, z) in enumerate(mounts, 1):
        mounting_screw(i, y, z, steel, gasket)

    ring("Socket | bezel base seal",
         [(.041, .441), (.050, .441), (.050, .335), (.041, .335)], gasket)
    ring("Socket | flared receiver",
         [RECEIVER_BASE, RECEIVER_LIP, (.180, .335), (.170, .329), (.046, .333)], bezel)
    ring("Socket | throat sleeve",
         [(.072, .332), (.1765, .330), (.1765, .325),
          (.172, .322), (.072, .322)], gasket)
    ring("Socket | recessed barrel",
         [(-.201, .339), (-.031, .339), (-.031, .343), (-.003, .343),
          (.002, .336), (.074, .336), (.074, .324), (-.201, .324)], throat)
    # The blind rear cap closes the pin wells behind the recessed contact bed.
    m.cylinder("Socket | rear closure", (-.211, 0, 0), (-.191, 0, 0),
               .338, gasket, vertices=64)
    contacts = [(-.105, .080), (.105, .080), (0, -.116)]
    casting("Socket | insulated pin carrier",
            [(-.192, circle(.330)), (-.130, circle(.330))],
            [circle(.054, c, 32) for c in contacts], throat)
    for i, (y, z) in enumerate(contacts, 1):
        ring(f"Contact {i} | entrance ferrule",
             [(-.190, .0544), (-.129, .0544), (-.129, .047),
              (-.135, .042), (-.190, .042)], gasket, (y, z), 32)
        ring(f"Contact {i} | spring sleeve",
             [(-.190, .0424), (-.142, .0424), (-.142, .041), (-.144, .038),
              (-.190, .038)], copper, (y, z), 32)
    # Rectangular index inlay lies on the front annulus at nine o'clock.
    loop = rectangle(.013, .086, .004)
    casting("Socket | polarizing index",
            [(x, [(y-.347, z) for y, z in loop]) for x in (.179, .181)],
            [], gasket)

"""Four linked cam-dog locks. X is the insertion axis; the master is on top."""

from math import atan2, ceil, cos, pi, radians, sin, tan

import bpy
from mathutils import Matrix, Vector
from mathutils.geometry import delaunay_2d_cdt

import connector_mesh as m
from power_socket import receiver_radius


HINGE = Vector((.138, 0, .423))
PIN = Vector((.169, 0, HINGE.z))
PIN_RETRACTION = .020
OPEN_ANGLE = radians(72)


def group(name, parent=None, offset=(0, 0, 0)):
    obj = bpy.data.objects.new("Top latch | " + name, None)
    obj.empty_display_size = .014
    bpy.context.collection.objects.link(obj)
    obj.parent = parent
    obj.location = offset
    return obj


def attach(obj, parent, pivot=None):
    bpy.context.view_layer.update()
    local = obj.matrix_basis.copy()
    obj.parent = parent
    obj.matrix_basis = Matrix.Translation(-pivot) @ local if pivot is not None else local
    return obj


def quadrants(master):
    """Copy the transform hierarchy, retaining shared mesh data in all four stations."""
    def copy_branch(original, parent, label):
        copy = original.copy()
        copy.name = original.name.replace("Top latch", label + " latch", 1)
        bpy.context.collection.objects.link(copy)
        copy.parent = parent
        for child in original.children:
            copy_branch(child, copy, label)
        return copy

    for label, angle in (("Port", pi/2), ("Bottom", pi), ("Starboard", -pi/2)):
        instance = copy_branch(master, None, label)
        instance.rotation_euler.x = angle


def rounded_outline(profile, setbacks):
    """Trim just the nominated casting corners with tangent circular arcs."""
    points = [Vector(p) for p in profile]
    result, normals = [], []
    for i, p in enumerate(points):
        setback = setbacks.get(i, 0)
        if not setback:
            result.append(tuple(p))
            normals.append(None)
            continue
        before, after = p-points[i-1], points[(i+1)%len(points)]-p
        trim = min(setback, .45*before.length, .45*after.length)
        a, b = before.normalized(), after.normalized()
        turn = atan2(a.x*b.y-a.y*b.x, a.dot(b))
        radius = trim/tan(turn/2)
        center = p-trim*a + radius*Vector((-a.y,a.x))
        radial = p-trim*a-center
        steps = max(2, ceil(abs(turn)/radians(12)))
        for step in range(steps+1):
            c, s = cos(turn*step/steps), sin(turn*step/steps)
            offset = Vector((c*radial.x-s*radial.y,s*radial.x+c*radial.y))
            result.append(tuple(center+offset))
            normals.append(offset.normalized()*(1 if turn > 0 else -1))
    return result, normals


def plate(name, profile, ymin, ymax, material, holes=(), rounds=None, edge_land=0):
    """Extrude the plate with separate normals for cheeks, cast rim and bearing bores."""
    profile = list(profile)
    rounds = rounds or {}
    area = sum(x1*z2-x2*z1 for (x1,z1),(x2,z2) in zip(profile,profile[1:]+profile[:1]))
    if area < 0:
        profile.reverse()
        rounds = {len(profile)-1-i:v for i,v in rounds.items()}
    outer, arc_normals = rounded_outline(profile,rounds)
    contour = [Vector(p) for p in outer]
    tangents = [(q-p).normalized() for p,q in zip(contour,contour[1:]+contour[:1])]
    flank = [Vector((t.y,-t.x)) for t in tangents]
    # A single inset loop carries the narrow machined land at the junction of
    # the jaw's flank and cheek. The two wide cheek planes remain perfectly flat.
    coords = []
    for i,p in enumerate(contour):
        bisector = (flank[i-1]+flank[i]).normalized()
        coords.append(tuple(p-edge_land/flank[i].dot(bisector)*bisector))
    count = len(coords)
    loops = [tuple(range(count))]
    for cx,cz,r in holes:
        start = len(coords)
        coords.extend((cx+r*cos(i*pi/12),cz+r*sin(i*pi/12)) for i in range(24))
        loops.append(tuple(range(start,len(coords))))
    n = len(coords)
    verts = [(x,y,z) for y in (ymin,ymax) for x,z in coords]
    _, _, triangles, mapping, _, domains = delaunay_2d_cdt(
        [Vector(p) for p in coords], [], loops, 1, 1e-9)
    faces, corner_normals = [], {}
    for triangle,region in zip(triangles,domains):
        if list(region) == [0]:
            a,b,c = (mapping[i][0] for i in triangle)
            faces.extend([(a,b,c),(c+n,b+n,a+n)])
    lower,upper = 0,n
    if edge_land:
        lower,upper = len(verts),len(verts)+count
        verts.extend((x,y,z) for y in (ymin+edge_land,ymax-edge_land) for x,z in outer)
    for a in range(count):
        b = (a+1)%count
        bands = [(lower,upper,0)]
        if edge_land:
            bands += [(0,lower,-1),(upper,n,1)]
        for start,end,axial in bands:
            if arc_normals[a] is not None or arc_normals[b] is not None:
                n1 = arc_normals[a] if arc_normals[a] is not None else flank[a]
                n2 = arc_normals[b] if arc_normals[b] is not None else flank[a]
                corner_normals[len(faces)] = [Vector((v.x,axial,v.y)).normalized()
                                              for v in (n2,n1,n1,n2)]
            faces.append((start+b,start+a,end+a,end+b))
    for (cx,cz,_),loop in zip(holes,loops[1:]):
        for a,b in zip(loop,loop[1:]+loop[:1]):
            face = (a,b,b+n,a+n)
            corner_normals[len(faces)] = [Vector((cx-verts[v][0],0,cz-verts[v][2])).normalized()
                                          for v in face]
            faces.append(face)
    obj = m.mesh("Top latch | " + name, verts, faces, material)
    if corner_normals:
        normals = [(0,0,0)]*len(obj.data.loops)
        for idx,values in corner_normals.items():
            face = obj.data.polygons[idx]
            face.use_smooth = True
            for corner,value in zip(face.loop_indices,values):
                normals[corner] = value
        obj.data.normals_split_custom_set(normals)
    return obj


def pin_guide(material):
    # The visible fixed annulus and its through bore use the pin's own datum.
    section = [(-.0628,.0134),(-.065,.0134),(-.068,.0128),(-.068,.0083),
               (-.067,.0078),(-.0628,.0078)]
    sides = 32
    verts = [(PIN.x+r*cos(j*2*pi/sides),y,PIN.z+r*sin(j*2*pi/sides))
             for y,r in section for j in range(sides)]
    faces = []
    for i in range(len(section)):
        k = (i+1)%len(section)
        for j in range(sides):
            b = (j+1)%sides
            faces.append((i*sides+j,i*sides+b,k*sides+b,k*sides+j))
    obj = m.mesh("Top latch | plunger guide bezel",verts,faces,material)
    normals = [(0,0,0)]*len(obj.data.loops)
    for i,(y,r) in enumerate(section):
        yy,rr = section[(i+1)%len(section)]
        if y == yy:
            continue
        for face in obj.data.polygons[i*sides:(i+1)*sides]:
            face.use_smooth = True
            for corner in face.loop_indices:
                j = obj.data.loops[corner].vertex_index%sides
                a = j*2*pi/sides
                normals[corner] = Vector(((y-yy)*cos(a),rr-r,(y-yy)*sin(a))).normalized()
    obj.data.normals_split_custom_set(normals)
    return obj


def saddle(material):
    # This wide, cast-in mounting plinth follows the receiver's taper in both axes.
    stations = [(.092, receiver_radius(.092)), (.170, receiver_radius(.170))]
    angles = [-.178,-.089,0,.089,.178]
    verts = [(x,(r+t)*sin(a),(r+t)*cos(a))
             for t in (-.002,.006) for x,r in stations for a in angles]
    nx, na = len(stations), len(angles)
    layer = nx*na
    faces = []
    for i in range(nx-1):
        for j in range(na-1):
            a = i*na+j
            outer = (a,a+na,a+na+1,a+1)
            faces.extend([outer[::-1],tuple(v+layer for v in outer)])
    skins = len(faces)
    perimeter = ([i*na for i in range(nx)] + list(range((nx-1)*na+1,layer)) +
                 [i*na+na-1 for i in reversed(range(nx-1))] + list(reversed(range(1,na-1))))
    for a,b in zip(perimeter, perimeter[1:]+perimeter[:1]):
        faces.append((a,b,b+layer,a+layer))
    obj = m.mesh("Top latch | conformal trunnion bed", verts, faces, material)
    for poly in obj.data.polygons[:skins]:
        poly.use_smooth = True
    return obj


def axle_end(side, material):
    # The flanged axle carries a recessed hex drive. Smooth the circular barrel;
    # leave the flange face, six tool flats, and bottom of the socket square.
    count = 24
    verts = []
    for y,r,hexagon in [(.053,.016,False),(.059,.016,False),(.060,.014,False),
                        (.060,.007,True),(.055,.007,True)]:
        for j in range(count):
            a = 2*pi*j/count
            radius = r*cos(pi/6)/cos(a%(pi/3)-pi/6) if hexagon else r
            verts.append((HINGE.x+radius*cos(a),side*y,HINGE.z-side*radius*sin(a)))
    faces = [tuple(reversed(range(count)))]
    for i in range(4):
        for j in range(count):
            a = i*count+j
            b = i*count+(j+1)%count
            faces.append((a,b,b+count,a+count))
    faces.append(tuple(range(4*count,5*count)))
    obj = m.mesh("Top latch | recessed axle end", verts, faces, material)
    for poly in obj.data.polygons[1:count+1]:
        poly.use_smooth = True
    return obj


# Lower toe flank follows the keeper's outboard load shoulder. The eccentric
# sweep takes up axial clearance during the final few degrees of closing.
DOG = [(.115,.424),(.118,.414),(.130,.404),(.147,.399),(.182,.390),
       (.228,.372),(.246,.365),(.261,.364),(.2712,.3621),(.2772,.3541),
       (.2832,.3381),(.288,.331),(.301,.331),(.309,.341),(.308,.356),
       (.297,.378),(.294,.389),(.302,.393),(.299,.403),(.289,.408),
       (.272,.407),(.255,.401),(.236,.401),(.209,.414),(.175,.437),
       (.155,.446),(.134,.446),(.123,.441),(.116,.434)]
KEEPER = [(.227,.308),(.301,.308),(.301,.327),(.287,.328),(.283,.338),
          (.277,.354),(.271,.362),(.258,.362),(.247,.351),(.245,.334),(.227,.329)]


def build_socket():
    frame_metal = m.material("Lock | cool cast guards", (.55,.56,.55))
    jaw_metal = m.material("Lock | graphite cam dog", (.29,.315,.34))
    axle = m.material("Lock | dark axle", (.245,.25,.26))
    orange = m.material("Lock | safety plunger", (.79,.32,.12))
    frame = group("trunnion mount")
    dog = group("cam dog", frame, HINGE)
    pin = group("safety plunger", frame)
    dog["joint"] = "cam dog"
    pin["joint"] = "safety plunger"
    attach(saddle(frame_metal),frame)
    axle_bore = (HINGE.x,HINGE.z,.0166)
    pin_bore = (PIN.x,PIN.z,.0078)
    cheek = [(.092,receiver_radius(.092)-.006),(.170,receiver_radius(.170)-.006),
             (.188,.378),(.190,.404),(.188,.423),(.178,.439),(.152,.448),
             (.141,.454),(.129,.454),(.116,.447),(.103,.438),(.094,.429)]
    guard_rounds = {i:.004 for i in range(3,len(cheek))}
    for side in (-1,1):
        a,b = sorted((side*.050,side*.063))
        holes = (axle_bore,pin_bore) if side < 0 else (axle_bore,)
        attach(plate("trunnion guard",cheek,a,b,frame_metal,holes,rounds=guard_rounds),frame)
        attach(axle_end(side,axle),frame)
    attach(m.cylinder("Top latch | cross axle",(HINGE.x,-.054,HINGE.z),
                      (HINGE.x,.054,HINGE.z),.016,axle,vertices=24),frame)
    # Roll the unloaded back, heel and crown. The working shoulder and
    # lifting ledge keep the angular faces that define engagement and grip.
    jaw_rounds = {i:.005 for i in (*range(7),13,14,15,*range(19,len(DOG)))}
    attach(plate("forged jaw and lifting ledge",DOG,-.045,.045,jaw_metal,
                 (axle_bore,pin_bore),rounds=jaw_rounds,edge_land=.00135),dog,HINGE)
    # The captive pin physically crosses the guard-to-jaw joint. Its low
    # mushroom head leaves finger room underneath to pull it out before lifting.
    attach(pin_guide(axle),frame)
    attach(m.cylinder("Top latch | lock pin",(PIN.x,-.074,PIN.z),(PIN.x,-.029,PIN.z),
                      .0073,axle,vertices=32),pin)
    attach(m.cylinder("Top latch | safety pull",(PIN.x,-.076,PIN.z),(PIN.x,-.0715,PIN.z),
                      .0112,orange,vertices=32),pin)
    quadrants(frame)


def build_keepers():
    metal = m.material("Lock | keeper steel", (.52,.50,.47))
    frame = group("plug keeper")
    # Integral load block enters the coupling band at its root, including the
    # transverse extremities where the cylinder recedes below its crown.
    attach(plate("radial keeper shoulder",KEEPER,-.056,.056,metal),frame)
    quadrants(frame)


def set_locked(collection):
    for obj in collection.all_objects:
        if obj.get("joint") == "cam dog":
            obj.rotation_euler.y = 0
        elif obj.get("joint") == "safety plunger":
            obj.location.y = 0


def set_unpinned(collection):
    for obj in collection.all_objects:
        if obj.get("joint") == "cam dog":
            obj.rotation_euler.y = 0
        elif obj.get("joint") == "safety plunger":
            obj.location.y = -PIN_RETRACTION


def set_released(collection):
    for obj in collection.all_objects:
        if obj.get("joint") == "cam dog":
            obj.rotation_euler.y = -OPEN_ANGLE
        elif obj.get("joint") == "safety plunger":
            obj.location.y = -PIN_RETRACTION

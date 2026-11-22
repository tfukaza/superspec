"""Small Blender construction helpers for the power connector, in meters."""

import bpy
from mathutils import Vector


def material(name, rgb):
    existing = bpy.data.materials.get(name)
    if existing:
        return existing
    mat = bpy.data.materials.new(name)
    linear = [v / 12.92 if v <= .04045 else ((v + .055) / 1.055) ** 2.4 for v in rgb]
    mat.diffuse_color = (*linear, 1)
    mat.use_nodes = True
    shader = mat.node_tree.nodes.get("Principled BSDF")
    shader.inputs["Base Color"].default_value = mat.diffuse_color
    shader.inputs["Roughness"].default_value = .88
    return mat


def _finish(obj, name, mat):
    obj.name = name
    obj.data.name = name
    obj.data.materials.append(mat)
    return obj


def mesh(name, vertices, faces, material, smooth=False):
    geometry = bpy.data.meshes.new(name)
    geometry.from_pydata(vertices, [], faces)
    geometry.update()
    if smooth:
        for poly in geometry.polygons:
            poly.use_smooth = True
    obj = bpy.data.objects.new(name, geometry)
    bpy.context.collection.objects.link(obj)
    return _finish(obj, name, material)


def box(name, center, dimensions, material):
    bpy.ops.mesh.primitive_cube_add(size=1, location=center)
    obj = bpy.context.object
    for vertex in obj.data.vertices:
        for axis, length in enumerate(dimensions):
            vertex.co[axis] *= length
    return _finish(obj, name, material)


def cylinder(name, start, end, radius, material, vertices=48):
    a, b = Vector(start), Vector(end)
    axis = b - a
    bpy.ops.mesh.primitive_cylinder_add(vertices=vertices, radius=radius,
                                       depth=axis.length, location=(a + b) * .5)
    obj = bpy.context.object
    obj.rotation_euler = axis.to_track_quat('Z', 'Y').to_euler()
    for poly in obj.data.polygons:
        poly.use_smooth = len(poly.vertices) == 4
    return _finish(obj, name, material)

"""Build with blender -b -P power_connector.py [-- --render | --bench-assets]."""

import sys
from math import cos, sin
from pathlib import Path

import bpy
from mathutils import Matrix, Vector

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
sys.dont_write_bytecode = True

import connector_mesh as mesh
import power_cable_bundle
import power_latch
import power_plug
import power_socket


def build_piece(label, *builders):
    collection = bpy.data.collections.new(label)
    bpy.context.scene.collection.children.link(collection)
    layer = bpy.context.view_layer.layer_collection.children[collection.name]
    bpy.context.view_layer.active_layer_collection = layer
    for build in builders:
        build()
    root = bpy.data.objects.new(label, None)
    root.empty_display_size = .08
    collection.objects.link(root)
    for obj in collection.objects:
        if obj != root and obj.parent is None:
            obj.parent = root
    return collection


def export_glb(path, collections, *, attributes=False):
    bpy.ops.object.select_all(action="DESELECT")
    for collection in collections:
        for obj in collection.all_objects:
            obj.select_set(True)
    bpy.ops.export_scene.gltf(filepath=str(path), use_selection=True,
                              export_format="GLB", export_apply=True,
                              export_cameras=False, export_lights=False, export_attributes=attributes)


def look_at(obj, target):
    obj.rotation_euler = (Vector(target) - obj.location).to_track_quat('-Z', 'Y').to_euler()


def reference_camera(camera):
    """Perspective fitted to landmarks on the original industrial connector."""
    scene = bpy.context.scene
    width, height = 1270, 920
    scene.render.resolution_x, scene.render.resolution_y = width, height
    yaw, pitch, roll = 1.886064, -.280485, .145220
    forward = Vector((cos(yaw)*cos(pitch), sin(yaw)*cos(pitch), sin(pitch)))
    right = Vector((sin(yaw), -cos(yaw), 0))
    up = right.cross(forward)
    right, up = right*cos(roll) + up*sin(roll), up*cos(roll) - right*sin(roll)
    camera.rotation_euler = Matrix((right, up, -forward)).transposed().to_euler()
    camera.location = (3.124484, -3.557956, -.282646)
    camera.data.type = "PERSP"
    camera.data.sensor_fit = 'HORIZONTAL'
    camera.data.lens = 1965.383 * camera.data.sensor_width / width
    camera.data.shift_x = (width/2 - 1094.095) / width
    camera.data.shift_y = (1319.786 - height/2) / width


def area(name, position, power, size, color, target):
    bpy.ops.object.light_add(type='AREA', location=position)
    lamp = bpy.context.object
    lamp.name = name
    lamp.data.energy = power
    lamp.data.shape = 'DISK'
    lamp.data.size = size
    lamp.data.color = color
    look_at(lamp, target)


def save_latch_poses(socket):
    """Withdraw the locking pins before swinging the cam jaws open in Blender."""
    scene = bpy.context.scene
    scene.frame_start, scene.frame_end = 1, 24
    scene.timeline_markers.new("Locked", frame=1)
    scene.timeline_markers.new("Pins withdrawn", frame=8)
    scene.timeline_markers.new("Released", frame=24)
    closed = {obj: obj.matrix_basis.copy() for obj in socket.objects}
    power_latch.set_unpinned(socket)
    unpinned = {obj: obj.matrix_basis.copy() for obj in socket.objects}
    power_latch.set_released(socket)
    for obj, locked in closed.items():
        released = obj.matrix_basis.copy()
        if released == locked:
            continue
        rotation = {"QUATERNION": "rotation_quaternion", "AXIS_ANGLE": "rotation_axis_angle"}.get(
            obj.rotation_mode, "rotation_euler")
        for frame, transform in [(1, locked), (8, unpinned[obj]), (24, released)]:
            obj.matrix_basis = transform
            obj.keyframe_insert(data_path="location", frame=frame, group="Latch pose")
            obj.keyframe_insert(data_path=rotation, frame=frame, group="Latch pose")
    scene.frame_set(1)


def render_previews(output, plug, socket):
    scene = bpy.context.scene
    scene.render.engine = "CYCLES"
    scene.cycles.samples = 24
    scene.cycles.use_denoising = True
    scene.render.resolution_percentage = 100
    scene.render.resolution_x = 1360
    scene.render.resolution_y = 1000
    scene.world.use_nodes = True
    ambient = scene.world.node_tree.nodes.get("Background")
    ambient.inputs["Color"].default_value = (.56, .62, .69, 1)
    ambient.inputs["Strength"].default_value = .45
    backdrop = mesh.material("Studio backdrop", (.40, .45, .49))
    wall = mesh.box("Studio wall", (-.14, 0, 0), (.28, 100, 100), backdrop)
    aperture = mesh.cylinder("Socket installation cutout", (-.35, 0, 0), (.05, 0, 0),
                             .36, backdrop, vertices=96)
    cut = wall.modifiers.new("Wall opening", "BOOLEAN")
    cut.operation = "DIFFERENCE"
    cut.object = aperture
    bpy.context.view_layer.objects.active = wall
    bpy.ops.object.modifier_apply(modifier=cut.name)
    bpy.data.objects.remove(aperture, do_unlink=True)
    area("Large softbox", (2, -2.3, 3.7), 320, 2.8, (1, .93, .84), (.55, 0, 0))
    area("Edge light", (1.0, 2.0, 2.1), 200, 2.2, (.79, .88, 1), (.7, 0, 0))
    area("Fill", (3, -1, -.5), 55, 1.5, (1, .95, .92), (.6, 0, 0))
    bpy.ops.object.camera_add()
    camera = bpy.context.object
    camera.data.type = "ORTHO"
    scene.camera = camera

    def shot(filename, target, direction, width):
        camera.location = Vector(target) + Vector(direction)
        look_at(camera, target)
        camera.data.ortho_scale = width
        scene.render.filepath = str(output / filename)
        bpy.ops.render.render(write_still=True)

    reference_camera(camera)
    scene.render.filepath = str(output / "reference-angle.png")
    bpy.ops.render.render(write_still=True)
    camera.data.type = "ORTHO"
    camera.data.shift_x = camera.data.shift_y = 0
    scene.render.resolution_x, scene.render.resolution_y = 1360, 1000
    shot("assembled.png", (.79, 0, .20), (3.6, -6.2, .95), 2.49)
    # Look at the top lock from its plunger side so the guide and cam are both visible.
    latch_view = ((.203, -.014, .436), (.62, -3.65, 1.85), .43)
    shot("latch-locked.png", *latch_view)
    shot("pin-alignment.png", (.196, 0, .408), (0, -3, 0), .27)
    scene.frame_set(scene.frame_end)
    shot("latch-released.png", *latch_view)
    plug.hide_render = True
    shot("socket.png", (.06, 0, .0), (5.8, -1.4, 1.0), 1.90)
    plug.hide_render = False
    socket.hide_render = True
    wall.hide_render = True
    shot("plug.png", (.81, 0, .17), (-3.5, -5.1, 1.4), 2.65)


def main():
    bpy.ops.wm.read_factory_settings(use_empty=True)
    scene = bpy.context.scene
    scene.world = bpy.data.worlds.new("Studio")
    scene.unit_settings.system = "METRIC"
    scene.unit_settings.length_unit = "METERS"
    items = HERE.parent / "items"
    if "--bench-assets" in sys.argv:
        plug = build_piece("Cable plug", lambda: power_plug.build(attached_cable=False), power_latch.build_keepers)
        bundle = build_piece("Three cable runs", power_cable_bundle.build)
        export_glb(items / "power-plug-body.glb", [plug])
        export_glb(items / "power-cable-bundle.glb", [bundle], attributes=True)
        return
    socket = build_piece("Wall socket", power_socket.build, power_latch.build_socket)
    plug = build_piece("Cable plug", power_plug.build, power_latch.build_keepers)
    bpy.context.view_layer.active_layer_collection = bpy.context.view_layer.layer_collection
    root = next(p for p in HERE.parents if (p / "Cargo.toml").exists() and (p / "editor").is_dir())
    output = root / "generated" / "sci-fi-industrial" / "power-connector"
    output.mkdir(parents=True, exist_ok=True)
    items.mkdir(exist_ok=True)
    for filename, collections in [("power-plug", [plug]), ("power-socket", [socket])]:
        export_glb(items / (filename + ".glb"), collections)
    export_glb(output / "power-connector.glb", [socket, plug])

    save_latch_poses(socket)
    bpy.ops.object.select_all(action="DESELECT")
    for screen in bpy.data.screens:
        for editor in screen.areas:
            if editor.type == "VIEW_3D":
                view = editor.spaces.active
                view.region_3d.view_rotation = Vector((-3.35, 5.4, -1.12)).to_track_quat('-Z', 'Y')
                view.region_3d.view_distance = 3.4
                view.region_3d.view_location = (.88, 0, .21)
                view.shading.color_type = "MATERIAL"
                view.shading.show_cavity = True
                view.shading.cavity_type = "WORLD"
                view.overlay.show_overlays = False
    bpy.context.preferences.filepaths.save_version = 0
    bpy.ops.wm.save_as_mainfile(filepath=str(output / "power-connector.blend"), compress=True)
    if "--render" in sys.argv:
        render_previews(output, plug, socket)


if __name__ == "__main__":
    main()

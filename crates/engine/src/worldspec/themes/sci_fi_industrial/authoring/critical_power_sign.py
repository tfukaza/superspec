"""Export bench sign GLBs with Blender: blender -b -P critical_power_sign.py."""

from pathlib import Path

import bpy


HERE = Path(__file__).resolve().parent
ROOT = next(path for path in HERE.parents
            if (path / "Cargo.toml").is_file() and (path / "editor").is_dir())
ART = ROOT / "generated" / "demos" / "critical-power-art"
ITEMS = HERE.parent / "items"
WIDTH = 1.02
HEIGHT = WIDTH / 4


def sign_backing():
    bpy.ops.mesh.primitive_cube_add(size=1, location=(.015, 0., 0.))
    backing = bpy.context.object
    backing.name = "Critical power sign | graphite housing"
    backing.dimensions = (.030, WIDTH + .024, HEIGHT + .024)
    bpy.ops.object.transform_apply(location=False, rotation=False, scale=True)
    bevel = backing.modifiers.new("Rolled housing edge", "BEVEL")
    bevel.width = .005
    bevel.segments = 2
    material = bpy.data.materials.new("Sign | graphite powder coat")
    material.use_nodes = True
    paint = material.node_tree.nodes.get("Principled BSDF")
    paint.inputs["Base Color"].default_value = (.025, .032, .038, 1.)
    paint.inputs["Metallic"].default_value = .28
    paint.inputs["Roughness"].default_value = .52
    backing.data.materials.append(material)


def sign_face(art):
    half_width, half_height = WIDTH / 2, HEIGHT / 2
    mesh = bpy.data.meshes.new("Critical power sign | label face")
    mesh.from_pydata(
        [(.031, -half_width, -half_height), (.031, half_width, -half_height),
         (.031, half_width, half_height), (.031, -half_width, half_height)],
        [],
        [(0, 1, 2, 3)],
    )
    mesh.update()
    face = bpy.data.objects.new(mesh.name, mesh)
    bpy.context.collection.objects.link(face)
    uv = mesh.uv_layers.new()
    for corner, coordinate in zip(uv.data, [(0., 0.), (1., 0.), (1., 1.), (0., 1.)]):
        corner.uv = coordinate

    material = bpy.data.materials.new("Sign | printed legend")
    material.use_nodes = True
    nodes = material.node_tree.nodes
    legend = nodes.new("ShaderNodeTexImage")
    legend.image = bpy.data.images.load(str(art))
    legend.extension = "EXTEND"
    paint = nodes.get("Principled BSDF")
    paint.inputs["Roughness"].default_value = .61
    paint.inputs["Emission Strength"].default_value = .19
    material.node_tree.links.new(legend.outputs["Color"], paint.inputs["Base Color"])
    material.node_tree.links.new(legend.outputs["Color"], paint.inputs["Emission Color"])
    mesh.materials.append(material)


def main():
    slugs = [f"critical-power-{number:02}" for number in range(1, 4)]
    artwork = [ART / (slug + ".png") for slug in slugs]
    for path in artwork:
        if not path.is_file():
            raise FileNotFoundError(path)

    for slug, art in zip(slugs, artwork):
        bpy.ops.wm.read_factory_settings(use_empty=True)
        sign_backing()
        sign_face(art)
        bpy.ops.export_scene.gltf(
            filepath=str(ITEMS / (slug + ".glb")),
            export_format="GLB",
            export_apply=True,
            export_cameras=False,
            export_lights=False,
        )


if __name__ == "__main__":
    main()

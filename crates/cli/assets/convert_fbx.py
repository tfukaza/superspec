"""Convert the Rust-owned glTF payload through Blender's binary FBX exporter."""
import sys
import bpy

source, output = sys.argv[sys.argv.index('--') + 1:]
bpy.ops.wm.read_factory_settings(use_empty=True)
bpy.ops.import_scene.gltf(filepath=source)
bpy.context.scene.unit_settings.system = 'METRIC'
bpy.context.scene.unit_settings.scale_length = 1.0
bpy.ops.export_scene.fbx(
    filepath=output, check_existing=False, axis_forward='-Z', axis_up='Y',
    global_scale=1.0, apply_unit_scale=True, use_mesh_modifiers=False,
    use_custom_props=True, use_tspace=True, bake_anim=False,
    path_mode='COPY', embed_textures=True,
)

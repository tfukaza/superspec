import struct
import unittest

from window_brushes import window_brushes


def collision_mesh(boxes):
    """Encode independent Z-up solids like the native collision GLB."""
    points = []
    indices = []
    for x0, x1, y0, y1, z0, z1 in boxes:
        start = len(points)
        points.extend((x, y, z) for x in (x0, x1) for y in (y0, y1) for z in (z0, z1))
        for a, b, c, d in ((0, 1, 3, 2), (4, 6, 7, 5), (0, 4, 5, 1),
                           (2, 3, 7, 6), (0, 2, 6, 4), (1, 5, 7, 3)):
            indices.extend(start + i for i in (a, b, c, a, c, d))
    xyz = b"".join(struct.pack("<3f", *p) for p in points)
    binary = xyz + struct.pack(f"<{len(indices)}H", *indices)
    meta = {
        "accessors": [
            dict(bufferView=0, count=len(points), type="VEC3", componentType=5126),
            dict(bufferView=1, count=len(indices), type="SCALAR", componentType=5123),
        ],
        "bufferViews": [{}, {"byteOffset": len(xyz)}],
    }
    return meta, binary, {"primitives": [{"attributes": {"POSITION": 0}, "indices": 1}]}


class WindowBrushTest(unittest.TestCase):
    def test_open_leaf_and_frame_preserve_air_between_them(self):
        # Two jambs and two rails frame a 2 m opening. The leaf projects 1 m inward.
        boxes = [(0, 0.2, y, y + 0.1, 0, 2) for y in (3, 5.1)]
        boxes.extend((0, 0.2, 3.1, 5.1, z, z + 0.1) for z in (0, 1.9))
        boxes.append((-1, 0, 3.1, 3.13, 0.1, 1.9))
        brushes = window_brushes(*collision_mesh(boxes))

        def solid(x, y, z):
            return any(b["minX"] < x < b["maxX"] and b["minY"] < y < b["maxY"]
                       and b["minZ"] < z < b["maxZ"] for b in brushes)

        self.assertTrue(solid(-0.9, 1, -3.11))
        self.assertTrue(solid(0.1, 1, -5.15))
        self.assertTrue(solid(0.1, 1.95, -4))
        self.assertFalse(solid(0.1, 1, -4))
        self.assertFalse(solid(-0.9, 1, -4))

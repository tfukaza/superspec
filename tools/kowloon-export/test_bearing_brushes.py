import struct
import unittest

from bearing_brushes import bearing_brushes


class BearingCollisionTest(unittest.TestCase):
    def test_sloped_native_underside_tracks_head_clearance_along_run(self):
        # Independent triangular prism in AML coordinates. Its flat roof is
        # at z=3.0; its lower face rises from z=2.4 to 2.7 over 0.9 metres.
        points = [
            (x, y, z)
            for x in (-0.16, 0.16)
            for y, z in ((0, 2.4), (0.9, 2.7), (0.9, 3.0), (0, 3.0))
        ]
        faces = [
            (0, 2, 1),
            (0, 3, 2),
            (4, 5, 6),
            (4, 6, 7),
            (0, 1, 5),
            (0, 5, 4),
            (1, 2, 6),
            (1, 6, 5),
            (2, 3, 7),
            (2, 7, 6),
            (3, 0, 4),
            (3, 4, 7),
        ]
        data = b"".join(struct.pack("<3f", *points[i]) for face in faces for i in face)
        meta = {
            "accessors": [
                {"bufferView": 0, "componentType": 5126, "count": 36, "type": "VEC3"}
            ],
            "bufferViews": [{"byteOffset": 0}],
        }
        brushes = bearing_brushes(
            meta, data, {"primitives": [{"attributes": {"POSITION": 0}}]}
        )
        self.assertGreaterEqual(len(brushes), 30)
        for i in range(901):
            y = i / 1000
            slices = [b for b in brushes if b["minZ"] <= -y <= b["maxZ"]]
            self.assertTrue(slices)
            low = min(b["minY"] for b in slices)
            self.assertLessEqual(low, 2.4 + y / 3 + 0.0001)
            self.assertGreater(low, 2.4 + y / 3 - 0.011)
            self.assertEqual(slices[0]["maxY"], 3.0)

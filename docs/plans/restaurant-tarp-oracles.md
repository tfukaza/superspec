# Restaurant and canvas reference corrections

> Superseded on 2026-11-22: the frozen JSON captures, their maintenance drivers and the tests that compared generator output against them were removed. Tests now check behaviour rather than captured outcomes; this record is historical.

The restaurant and canvas fixtures predate intentional changes already present in
the supported Rust implementation. This correction changes expected evidence; it
does not change those producers or relax their tests.

The historical TypeScript tree at `14ae2704d66f1eaacf0cf24d17ad4e5db8b45839`, with
the fixture-era compact-member adapter whose SHA-256 is
`d37617a867183b868bfd305d31df22c222db53f701fa09303e1dc227871b6825`, reproduces all
108 original cases. Every expanded position, normal, UV, color and material slot
matches, along with support records and random-stream continuations. The adapter
was recovered from a preserved source archive; using only the Git tree would miss
the compact metal finish policy. Original sharing indices remain the frozen
representation and are checked by the existing native and Wasm assertions.

Only these independently authored changes from
`3c64de86ebc4390ccfb9ff31a29d0f840716b69a` are applied:

- Kitchen equipment uses the separate `kitchen-metal` role. All positions, normals,
  UVs, colors and sharing remain unchanged. This changes 1,584 detail triangle
  slots and 3,436 fitting triangle slots.
- The revised stock-appearance asset supplies the condiment labels' fallback
  colors. Its complete parsed JSON equals the shipped Rust asset. The 54 fitting
  cases change 16,432 color scalars; their geometry, UVs and random state stay fixed.
- Food normals use normalized inverse-transpose vectors when adapting the shared
  bird mesh to duck proportions. Normalizing the rounded source vectors also
  changes 18 scalar values in the existing chicken/sliced-meat cases. Their
  geometry, atlas coordinates, vertex colors and anchors remain fixed.
- Settled canvas uses 18% of the former sag displacement and the reviewed fading,
  dust and grime formulas. These affect normals and attachment geometry as well
  as the visible cloth. Stable rest-space UV coordinates identify cloth corners
  when the new sag reverses a triangle's winding. Every original vertex identity
  remains represented, and metal receivers retain their original evidence.

The same complete original canvas is uniquely matched inside facade-cell case 17.
Its 7,848 triangles start at triangle 2,134 after the separately reviewed window
interior correction. Only that canvas's values and winding change. All other
facade geometry, material slots, receiver records and the interior revision remain
unchanged.

The optional drivers (since removed) captured the actual
historical producers and apply guarded deltas; they never import Rust output.
Each updated fixture records original bytes and source hashes. The staged source,
complete before/after observations and comparison reports are retained under
`generated/verification/restaurant-tarp-reference/`.

The four existing restaurant/canvas suites pass all eight tests after correction,
including exact indices, complete geometry, materials, support, randomness,
cancellation and recovery. No assertion or tolerance was changed.
The facade-cell suite also passes both existing tests with its narrow canvas delta.

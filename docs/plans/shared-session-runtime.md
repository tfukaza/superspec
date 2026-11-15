# Shared generation session and completed artifacts

The AML/Kowloon path and authored material/item path now need one real lifecycle
owner. `session::Session` owns worker settings, source admission metadata, retained
programs and one dependency runtime. `authoring::Session` remains an exact reexport
for callers; it does not contain another session. Project and world AML lowering
use this same session rather than constructing a stateless compilation.

The dependency runtime owns completed graph, raster and mesh-binding artifacts.
Named retention partitions preserve the existing independent byte and entry
allowances: authored graph artifacts 128 MiB/4096, raster artifacts 256 MiB/16384,
and bound meshes 384 MiB/64. All partitions use the same lookup, admission, LRU and
clear implementation. Kowloon retains recipe identity and compatibility counters,
but delegates actual artifact ownership to the session runtime. Legacy producer
keys use the dependency runtime's versioned content-key contract over the complete
existing recipe/input or binding identity. Native and Wasm dispatch, ordered probes,
cold-request grouping and successful-batch publication stay unchanged.

Image validation and exact-byte source identity remain typed admission metadata,
with their existing limits and counters. They share the session lifetime; this
change does not add a decoded-image cache or alter image processing.

Public packet revisions are authored labels and may repeat or decrease in legacy
callers. Internal build epochs are monotonic and separate from those labels.
Authored requests retain their existing rejection of obsolete request revisions;
an AML packet label does not alter that authored-request admission history.
Clearing a session invalidates its current epoch and all completed artifacts,
retained programs and admission metadata. Failed requests retain only previously
completed valid artifacts and cannot publish a partial result.

Verification covers mixed AML/material/item use, fresh-output equality, clear and
failure recovery, independent retention limits, legacy cache counters and native
parallel/cancellation behavior. This is ownership and lifecycle consolidation,
without a new executor, profiling or an optimization pass.

# Existing asset and execution ownership

This record describes the initial extraction. The later
[authoring milestone](authoring-implementation.md) adds the dependency graph;
[consolidation](superspec-consolidation.md) unifies authored and theme artifact
retention under one session. The [theme runtime boundary](theme-runtime-boundary.md)
then makes producer bookkeeping generic and removes the temporary root `generation`
API. The decisions below retain their original scope.

This extraction completes the ownership of existing mechanisms. It adds no
dependency graph, persistent cache, new reuse policy, profiling or optimization.

Before moving code, the dependency inventory found per-revision source loading in
the physical compiler, image admission at its root, and exact-byte source identity,
bounded artifact storage and indexed job dispatch inside the Kowloon theme.
Those mechanisms do not require knowledge of a recipe or receiving geometry.

| Owner | Existing implementation moved | Preserved behavior |
| --- | --- | --- |
| `assets` | Host-supplied source bytes per revision, exact-byte content identities and image validation metadata | Authored names, fresh resolution on the next revision, byte limits, eviction order and failed-read recovery |
| `runtime` | Completed artifact storage over caller-owned payloads, duplicate-key batch preparation and indexed worker dispatch | Existing limits, probe accounting, worker policies, source order, cancellation and join-before-publication behavior |
| `worldspec::layouts` | Seeded layout producers, placement and their supplied theme JSON | Existing generator output; the temporary root `generation` API was later retired |
| Kowloon | Recipe keys, raster evaluation, memory cost calculation, receiver binding and publication | Theme semantics remain outside the generic storage/execution implementation |

Filesystem and browser resource readers remain host adapters. The runtime does not
know AML, material recipes or theme mesh types. The theme cache retains its public
API as an adapter over generic storage, so existing session statistics and consumers
continue to work. The previous source image admission cache and procedural source
identity cache remain separate because they have distinct lifetimes and semantics.

Existing generation, cache, selective-source, cancellation, worker and scene-output
tests remain the integration checks. Focused runtime tests exercise non-theme payloads
and per-revision source loading directly before the production move. A shared graph,
new cache granularity and in-flight consumer lifetime tracking remain product work.

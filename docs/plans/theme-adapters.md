# Supplied theme assembly

The WorldSpec coordinator owns structural build order and accepts a supplied,
compiled-in theme adapter. It does not select Kowloon or own its floor finishes,
ground settings, planning state or cache. The physical stage owns resolution and
geometry maps privately; themes connect through typed build stages rather than
publishing those maps as a generic API.

The explicit shipped composition selects the source profile and Kowloon adapter.
Session owns common lifecycle and artifact storage; the composition requests its
theme's auxiliary state there. A no-theme adapter performs ordinary structural
rendering, and a separate test adapter uses the same actual compiler to publish
an independent visual contribution.

Kowloon's physical receiving-surface bridge lives in `architecture/kowloon`.
Its theme state and execution settings belong to
`worldspec/themes/kowloon/assembly`. Existing source checks, planning order,
phase markers, collision, validation, visual output and resource identities are
preserved. This internal boundary supports the existing compiled-in producers;
it does not add a dynamic plugin or authoring language.

Verification includes exact default scene packets, native/Wasm consumers,
no-theme and alternate-adapter collision/source invariants, phase ordering,
source failures, cancellation and recovery. No optimization or profiling is
part of this change.

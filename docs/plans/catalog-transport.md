# Catalog and scene transport ownership

These decisions precede the ownership moves. Existing source, scene and asset
identities stay unchanged.

`superspec-model::updates` owns the existing LSU1 delta encoder beside the LSP3
scene packet. It consumes completed scenes only. The compiler's `updates` path
remains a compatibility reexport; the browser host uses the model directly.
Preserve exact float-bit comparison, encoded image bytes and MIME matching,
revision handling, manifest field order and payload layout.

`superspec-catalog` owns the shipped material JSON and its pure material/image
lookup functions. It depends on shared material values and JSON decoding, with
no AML parser, compiler, filesystem or cache policy. Native hosts and themes use
that owner directly. Core retains the AML declaration and registration adapter,
including reserved-identity checks, source diagnostics and existing lookup
reexports. The external image bundle and all names remain unchanged.

Retain existing delta and material-catalog integration tests. Exercise a direct
catalog consumer and model transport in the compiler-free model consumer; check
that the moved JSON bytes and the normal dependency graph preserve these seams.

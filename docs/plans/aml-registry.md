# Compiled AML source registry

The source registry separates generic AML admission from shipped source profiles.
The [profile ownership decision](source-profiles.md) moves Kowloon decoding and
the default profile into a separate crate without changing theme semantics.

## Decisions and dependencies

| Decision | Coupling removed | Remaining boundary |
| --- | --- | --- |
| The core reader accepts an explicit immutable `extensions::Registry`. `architecture::parse` uses the empty profile. | Core no longer imports or registers the Kowloon reader or its single-section check. | Core XML admission, source limits, namespaces, typed core vocabulary and ID scopes remain authoritative. |
| Each compiled extension declares an ID, version, AML versions, `(parent, tag)` ownership, optional identity selectors, decoder and named source checks. | Adding a source-only extension no longer requires editing the core tag list or compiler assembly. | This is compiled Rust registration, not dynamic code loading or a geometry-generation plugin interface. |
| Decode to the existing typed `Node` tree; use `format::serialize_with_registry` for a custom profile. | Extension payloads do not bypass source locations, ID scope checks or AML round trips through opaque objects. | An extension owns attribute validation and its subtree; the registry checks every owned tag and selector before decoding. |
| `superspec-profiles::kowloon` owns dressing admission and its single-section check; compiler and host entry points select `default_registry()`. | Stable core grammar and its generic entry points do not depend on Kowloon recipe grammar. | `<dressing theme="kowloon-street@1">`, omitted theme defaults and `AML_DRESSING` theme diagnostics retain their decoder. The default XSD entry remains `schema/aml.xsd`. |
| `default_registry().with_extensions(...)` validates a new isolated profile. | Hosts need not copy shipped declarations or mutate process-wide registration. | Duplicate extension IDs, scoped tag ownership, named checks and core overrides fail during registry construction. |
| Compiler `source::prepare` owns source admission/checking and common catalog registration; assembly consumes its `PreparedDocument`. | Callers no longer repeat the implicit parse → catalog-registration sequence. | Source checks see authored declarations, before the common catalog adds reserved materials. Plain AML keeps common `builtin:` materials independently of Kowloon. |
| Default and explicit-registry compilation share one assembly body. | Native/Wasm defaults and a custom host profile cannot develop separate compiler implementations. | Existing resolution, material planning, physical checks, navigation and asset publication retain their current order. |

## Guarantees

`parse_with_registry` completes XML/typed-source admission, authored ID uniqueness
and the installed source checks. Checks see the whole immutable authored document,
including later declarations. Their errors retain stable diagnostic codes, object
IDs and the source reader's UTF-16 ranges. The checks run once per parse. They have
no asset resolver or geometry context. `PreparedDocument` additionally includes
common catalog declarations; its name deliberately does not certify every
architectural reference or physical/navigation validity.

`Resolution::new` still resolves frames, paths, profiles, instances and enclosure
attachments. Other reference and relationship checks remain in lowering, material
planning and context validation. Moving these checks requires a separate parity
slice; renaming this resolution as a fully checked document would be inaccurate.
The existing `inspect` command remains XML inspection and does not claim to have
run source-extension checks.

The shipped XSD describes the default core/Kowloon profile. A custom source profile
must supply matching schema vocabulary if it exposes XSD validation. This slice
does not weaken the shipped schema with an unrestricted wildcard. Test extension
tags exist only in Rust test fixtures, never in the production registry.

One diagnostic refinement is intentional: an unowned child inside a Kowloon
subtree now fails registry admission with `AML_CHILD` at that child before its
decoder runs. The old decoder sometimes used `AML_UNSUPPORTED` for that case.
The rejected syntax is unchanged. Theme and recipe-parameter errors retain their
existing `AML_DRESSING` contract.

## Verification

Focused tests cover plain AML with an empty registry, default/explicit Kowloon
defaults and scene packets, composed profiles, a real fixture decoder and check
against a later declaration, serialization with the same profile, missing
registration, invalid content, scope/version conflicts and exact Unicode source
ranges. Nested tag selectors and ownership are checked before extension decoding.
Compiler tests prove source checks precede geometry and asset reads, execute once,
and recover after failures without contaminating another profile.

```sh
cargo test -p superspec-aml --locked
cargo test -p superspec-profiles --locked
cargo test -p superspec-engine --test source_registry --test architecture --test material_catalog --locked
```

Native/Wasm and the pinned LevelSpec consumer remain final cutover checks. Source
distribution pins must change only after independent review and verification.

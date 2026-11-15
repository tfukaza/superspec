# Browser kernel failure diagnostics

The acceptance-only Wasm crate previously aborted on a shared assertion without
reporting its Rust panic message. The browser runner therefore exposed only
`unreachable`, hiding the fixture, source location and actual/expected values.

Before changing any assertion or production code, compare the first failing
shared fixture with the frozen `model-assets-baseline` implementation and verify
that its fixture bytes match the saved source manifest.

Install a console panic hook only in `tests/rust-kernels`, and forward browser
console errors from `tests/rust-kernels-browser.mjs`. Name the failing check group
when rethrowing its browser exception. Preserve the complete assertion sequence,
failure exit status and success report. The production Wasm/compiler panic
behavior and all golden expectations remain unchanged.

The improved Chromium run identifies the first procedural failure as
`window_fixtures.rs:386`, `identity/casement color[3504]: 0.011 != 0.005`.
The unchanged native test fails with exactly that message against both the frozen
implementation and the reorganized implementation. Its source and input fixture
bytes match the frozen manifest. Evidence is recorded in
`generated/verification/kernel-window-comparison.json` and the associated native
and `final-browser-kernels-diagnostic.log` logs. The assertion remains enabled;
checks later in this fail-fast browser group have not been established by this
run.

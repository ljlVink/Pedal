# HiNote instruction recovery fixtures

`HiNoteRecovery.ts` covers private data-field initialization and branding, two
independent private slots, a namespace-local class with accessors, a decorated
class with static/instance methods, inherited enumerable keys, spread construction,
an await-free async closure, and super getters/calls in captured-this callbacks.
The debug/release API 12 ABCs are checked in;
normal Cargo tests require neither the SDK nor Node. The TS compiler lowers the
decorator to a regular call; actual Sendable bytecode is covered by the optional
HiNote corpus test and synthetic import-slot tests.

Regenerate using `./regenerate.ps1 -SdkRoot C:/harmony_sdk/default/openharmony`.
From the workspace root:

```powershell
cargo run -p pedal-abc-bridge --example export_record -- crates/pedal-abc-bridge/tests/fixtures/hinote_recovery/debug.abc HiNoteRecovery target/hinote-debug.ets
cargo run -p pedal-abc-bridge --example export_record -- crates/pedal-abc-bridge/tests/fixtures/hinote_recovery/release.abc HiNoteRecovery target/hinote-release.ets
node crates/pedal-abc-bridge/tests/fixtures/hinote_recovery/compare.cjs target/hinote-debug.ets target/hinote-release.ets
```

The behavior comparison checks isolated private state, wrong-receiver errors,
field invisibility, dynamic initializer receivers, nested getters/setters, inherited
enumeration, calls within each iteration, construction, super receiver behavior,
and async completion.
This is a Node source-behavior comparison, not HarmonyOS device execution.

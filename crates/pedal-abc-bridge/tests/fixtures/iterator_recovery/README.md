# Iterator and exception recovery fixtures

`IteratorRecovery.ts` contains original lookup, early-break and continue/return
loops with outer exception handlers. The checked-in `debug.abc` and `release.abc`
were compiled with the OpenHarmony ETS SDK 26.0.0.105, targeting API 12. Ordinary
Cargo tests need neither the SDK nor the application corpus.

Regenerate both fixtures with `./regenerate.ps1 -SdkRoot C:/harmony_sdk/default/openharmony`.
Debug includes variable scopes; release uses optimization level 2.

From the repository root, export each record using the `export_record` example:

```powershell
cargo run -p pedal-abc-bridge --example export_record -- crates/pedal-abc-bridge/tests/fixtures/iterator_recovery/debug.abc IteratorRecovery target/iterator-debug.ets
cargo run -p pedal-abc-bridge --example export_record -- crates/pedal-abc-bridge/tests/fixtures/iterator_recovery/release.abc IteratorRecovery target/iterator-release.ets
node crates/pedal-abc-bridge/tests/fixtures/iterator_recovery/compare.cjs target/iterator-debug.ets target/iterator-release.ets
```

The harness compares return values, exceptions and event traces against the
original source in isolated VM contexts with a one-second execution timeout.
There are 38 scenarios per function, or 114 per compiler mode. Cases include
accessor and method failures, receiver identity, callable result objects, cleanup
exactly once, missing or invalid `return`, exception precedence and thrown
`undefined`, and poisoned `call` properties on iterator methods. The optional
additional argument can be an exported production
`networkwrapper2/queue/QueueManager` record: its original lookup method is extracted
without rewriting its body and checked against the same 38 cases with logger stubs.

These comparisons test the listed inputs, not every malformed iterator. Panda's
`getiterator` opcode returns a non-callable iterator property unchanged and leaves
object validation to later explicit instructions, unlike ECMAScript's abstract
GetIterator operation. The dedicated `runtime_instructions` Rust tests verify
this opcode boundary; exception tests separately compare bytecode and lifted AST
execution, including hole identity and register/accumulator aliasing.

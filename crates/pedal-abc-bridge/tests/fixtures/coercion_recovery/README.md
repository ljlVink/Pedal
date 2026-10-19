# Coercion recovery fixtures

The source's discarded addition, loose equality and unary negation must still
invoke user conversion hooks and preserve their exceptions. `protectedLoop`
also checks evaluation inside the original exception CFG and IteratorClose
precedence when either conversion or closing throws. The debug/release ABCs are
compiled by OpenHarmony ETS SDK 26.0.0.105, targeting API 12; ordinary Rust tests
need neither the SDK nor Node.

```powershell
./crates/pedal-abc-bridge/tests/fixtures/coercion_recovery/regenerate.ps1 -SdkRoot C:/harmony_sdk/default/openharmony
cargo run -p pedal-abc-bridge --example export_record -- crates/pedal-abc-bridge/tests/fixtures/coercion_recovery/debug.abc CoercionRecovery target/coercion-debug.ets
cargo run -p pedal-abc-bridge --example export_record -- crates/pedal-abc-bridge/tests/fixtures/coercion_recovery/release.abc CoercionRecovery target/coercion-release.ets
node crates/pedal-abc-bridge/tests/fixtures/coercion_recovery/compare.cjs target/coercion-debug.ets target/coercion-release.ets
```

The harness executes the complete source and recovered exports without repairing
the recovered bodies. Explicit oracles cover `valueOf`/`toString` invocation
counts, side effects, thrown strings and `undefined`, mixed BigInt/Number errors,
and competing conversion/close errors. It runs 35 comparisons per recovered file
and returns a nonzero exit status on any mismatch. `OHOS_HOME` overrides the SDK
path used for TypeScript transpilation.

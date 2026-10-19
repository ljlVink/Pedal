# Namespace recovery fixtures

`NamespaceRecovery.ts` is original test code with a `shaderHandler` namespace,
two private captured constants, an exported async method, a closure factory and
two exported independent counter instances. `debug.abc` and `release.abc` are
checked-in outputs of OpenHarmony ETS SDK 26.0.0.105 targeting API 12. Normal
Rust tests need neither the SDK nor Node.

Regenerate both variants:

```powershell
./crates/pedal-abc-bridge/tests/fixtures/namespace_recovery/regenerate.ps1 -SdkRoot C:/harmony_sdk/default/openharmony
```

The SDK uses hash names for anonymous namespace initializers. The additional
Rust metadata regression clones the parsed record and supplies the production
compiler naming families `#&#shaderHandler` and `#&@0*#resolve`/`makeCounter`.
It verifies the legacy class heuristic's guard independently of SDK name changes;
it does not alter bytecode or the original file's `method_at` data. Both compiler
variants also have separate tests using all original metadata unchanged.

Run full-record behavior comparisons against the source:

```powershell
cargo run -p pedal-abc-bridge --example export_record -- crates/pedal-abc-bridge/tests/fixtures/namespace_recovery/debug.abc NamespaceRecovery target/namespace-debug.ets
cargo run -p pedal-abc-bridge --example export_record -- crates/pedal-abc-bridge/tests/fixtures/namespace_recovery/release.abc NamespaceRecovery target/namespace-release.ets
node crates/pedal-abc-bridge/tests/fixtures/namespace_recovery/compare.cjs target/namespace-debug.ets target/namespace-release.ets
```

The harness transpiles and executes the complete source and recovered modules
without repairing their bodies. Explicit oracles cover the namespace object's
exported properties, privacy of captured constants, interleaved counter state,
a fresh third counter, fulfilled promises, immediate await values, rejected
promises, synchronous callback throws and callback arguments. Any mismatch or
timeout yields a nonzero exit code. `OHOS_HOME` overrides the default SDK path.

# Property and async rest recovery fixture

`PropertyRecovery.ts` exercises computed/numeric own fields, computed callbacks,
object copies, nested object methods, sixteen methods in one literal, and async
rest arguments with successful, rejected and await-free completion paths.
The API 12 debug/release ABC fixtures are checked in; ordinary Cargo tests require
neither the SDK nor Node.

Regenerate with `./regenerate.ps1 -SdkRoot C:/harmony_sdk/default/openharmony`.
From the workspace root, compare executable outputs with:

```powershell
cargo run -p pedal-abc-bridge --example export_record -- crates/pedal-abc-bridge/tests/fixtures/property_recovery/debug.abc PropertyRecovery target/property-debug.ets
cargo run -p pedal-abc-bridge --example export_record -- crates/pedal-abc-bridge/tests/fixtures/property_recovery/release.abc PropertyRecovery target/property-release.ets
node crates/pedal-abc-bridge/tests/fixtures/property_recovery/compare.cjs target/property-debug.ets target/property-release.ets
```

The Node checks compare ordinary data-property copies and function behavior with
the original source. They additionally verify Symbol/`__proto__` keys, key coercion,
function-name descriptors, non-enumerable/inherited exclusions and nullish sources.
Accessor copies are checked against the
[HarmonyOS 5 runtime descriptor-copy implementation](https://github.com/openharmony/arkcompiler_ets_runtime/blob/OpenHarmony-5.0.0-Release/ecmascript/stubs/runtime_stubs-inl.h),
which retains getter identities without evaluating them during the copy; these
cases deliberately use the runtime protocol rather than Node's spread semantics.
These are source/runtime-model checks, not execution on a HarmonyOS device.

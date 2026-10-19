# Advanced recovery fixtures

`AdvancedRecovery.ets` is original test code covering fulfilled/rejected awaits,
catch/finally, overridden returns, nested finalizers, catch rethrows, loop
continue, await inside finally, inheritance/accessors, duplicate member names,
and local/captured names. `debug.abc` and `release.abc` are its checked-in compiler
outputs, so the Rust regression tests run without an SDK or local corpus.
`PropertyDefinitions.ts` and `properties-{debug,release}.abc` cover own-property
definitions and static constants. `compare_properties.cjs <recovered.ets>` checks
five values and property descriptors against the source, including an inherited
setter and an injected non-writable inherited property. Dynamic initializers
remain explicit `Object.defineProperty` calls; literal static initializers can
become class fields without invoking those inherited setters.
`ArkUIProbe.ts` is the minimal `Circle`/`MathUtil`/`ProbePage` subset of the real
project's hvigor-lowered `Index.ts`; its debug/release ABCs cover the component's
twelve members and the order of constructor and superclass initialization.
`extract_arkui.cjs <hvigor Index.ts> <ArkUIProbe.ts>` recreates that source subset.

Regenerate with the OpenHarmony API 12 target using es2abc from the installed
OpenHarmony ETS SDK 26.0.0.105:

```powershell
./crates/pedal-abc-bridge/tests/fixtures/advanced_recovery/regenerate.ps1 -SdkRoot C:/harmony_sdk/default/openharmony
```

The debug variant preserves variable scopes; release uses optimization level 2
without debug variable names. No obfuscation is enabled. The relative source
path recorded in both fixtures is `AdvancedRecovery.ets`.

To compare actual behavior against the source, first export the record and run
the Node harness. The optional second harness argument supplies a TypeScript
compiler implementation; by default it uses the SDK under `OHOS_HOME`.

```powershell
cargo run -p pedal-abc-bridge --example export_record -- crates/pedal-abc-bridge/tests/fixtures/advanced_recovery/debug.abc AdvancedRecovery target/advanced-debug.ets
node crates/pedal-abc-bridge/tests/fixtures/advanced_recovery/compare.cjs target/advanced-debug.ets
```

The harness transpiles the original ETS source, checks it against explicit
return/exception/event-trace oracles, and then compares the decompiled functions
against that source. Each case runs in an isolated worker with a three-second
timeout. The first sixteen cases load only the relevant declarations, excluding
module startup and ArkUI host services. A seventeenth case executes the entire
record, accesses its actual exports, instantiates inherited classes, and calls
two separate closures to detect missing initialization and shared captures.
Seven additional cases inject rejected promises or a throwing cleanup to check
that finally overrides pending returns/throws and executes exactly once.
A failed comparison produces a nonzero exit code; the harness does not repair
or rewrite the recovered function bodies.

For full application compilation, copy `AdvancedRecovery.ets` into the local
`test_examples/arkts_probe/entry/src/main/ets/probes/` directory and import it from
`EntryAbility.ets`. From the project directory, invoke the DevEco Node runtime
with `tools/hvigor/bin/hvigorw.js --mode module -p product=default -p buildMode=debug assembleHap`
(repeat with `buildMode=release`). Copy each generated
`entry/build/default/intermediates/loader_out/default/ets/modules.abc` before the
next build to preserve both versions. This optional project is gitignored.

`compare_arkui.cjs <compiler-generated Index.ts> <recovered Index.ets>` additionally
compares the original project's lowered `ProbePage` member list and exercises
its constructor, getters/setters, state updates, nested `ForEach` creation,
reset-button callback and lifecycle methods with explicit ArkUI host stubs.
It verifies recovered class ownership and those
methods' behavior; it is not a HarmonyOS device or a UI rendering test.

`ImportedInheritance.ts` and `inheritance-{debug,release}.abc` cover imported
superclasses and implicit/explicit spread constructors. See [INHERITANCE.md](INHERITANCE.md)
for regeneration and the 63-case behavior comparison, including argument identity,
replacement receivers, exceptions and overridden array methods.

`ConcurrentRecovery.ts` and `concurrent-{debug,release}.abc` cover fulfilled/rejected
async completions, ordinary returns, implicit returns and multiple synchronous
returns with the SDK's lowered `"use concurrent"` directive. A verified complete
notification protocol restores this directive before all generated local
statements. Recompiling the exported record with the same API 12 es2abc compiler
recreates every `notifyconcurrentresult` completion site (2, 1, 1, 1, 2 and 1
notifications across the six functions). The normal Cargo regressions use these
checked-in ABCs and synthetic negative cases; SDK tools remain optional.

`compare_concurrent.cjs <recovered.ets>` compares 15 ordinary JS behavior cases
against the source, including injected rejections, thenable side effects,
receiver identity, object identity and throwing branch calls. It does not emulate
the native taskpool callback; that protocol is checked by the compiler roundtrip.

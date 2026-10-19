# Imported inheritance fixture

`ImportedInheritance.ts` covers a named import with an alias, a namespace import,
two derived classes with implicit constructors, explicit `super(...args)`, and
an explicit constructor that mutates a parameter before calling `super`.
All four classes initialize `isSheetShow = false`; the implicit constructors
also carry getters/setters. `inheritance-debug.abc` and
`inheritance-release.abc` are independent API 12 compiler outputs.

Regenerate with the installed OpenHarmony SDK:

```powershell
./crates/pedal-abc-bridge/tests/fixtures/advanced_recovery/regenerate_inheritance.ps1 -SdkRoot C:/harmony_sdk/default/openharmony
```

Export either variant and compare its complete module against the source:

```powershell
cargo run -p pedal-abc-bridge --example export_record -- crates/pedal-abc-bridge/tests/fixtures/advanced_recovery/inheritance-debug.abc ImportedInheritance target/inheritance-debug.ets
node crates/pedal-abc-bridge/tests/fixtures/advanced_recovery/compare_inheritance.cjs target/inheritance-debug.ets
```

The harness provides `./Base` through a VM `require` stub. Its 63 cases check
the actual module exports, both prototype chains, argument values and identity
at zero through one thousand arguments, accessors, field initialization, parent
constructors that return replacement objects, original exception identity,
and the exact order of effects before, inside and after `super`. Constructors
without source `push` calls also run with `Array.prototype.push` replaced by a
throwing function, proving that array spread does not invent a `push` call.
It transpiles the source and recovered ETS without rewriting function bodies.

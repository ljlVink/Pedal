# Object literal method recovery fixture

`ObjectMethods.ts` constructs three object literals with three methods each.
The last group captures the outer `seed` parameter. The checked-in API 12
`debug.abc` and `release.abc` files exercise object-method embedding and capture
recovery without requiring the SDK during ordinary Cargo tests. A separate
exported function returns a five-level arrow chain whose leaf captures `seed`.

Regenerate with `./regenerate.ps1 -SdkRoot C:/harmony_sdk/default/openharmony`.
To compare runtime behavior, first export both records from the repository root:

```powershell
cargo run -p pedal-abc-bridge --example export_record -- crates/pedal-abc-bridge/tests/fixtures/object_methods/debug.abc ObjectMethods target/object-methods-debug.ets
cargo run -p pedal-abc-bridge --example export_record -- crates/pedal-abc-bridge/tests/fixtures/object_methods/release.abc ObjectMethods target/object-methods-release.ets
node crates/pedal-abc-bridge/tests/fixtures/object_methods/compare.cjs target/object-methods-debug.ets target/object-methods-release.ets
```

# Small compiler fixtures

These fixtures are version-controlled test inputs, separate from the optional
large `test_examples/` corpus. All 22 ABC files (about 80 KiB in total) ship with
their TS/ETS sources and regeneration scripts. Missing or unreadable fixtures
fail the Rust tests; they are never silently skipped.

| Directory | Coverage | ABC files |
| --- | --- | --- |
| `advanced_recovery/` | Async/finally, classes, ArkUI runtime stubs, properties, imported inheritance, concurrent functions | 10 |
| `coercion_recovery/` | Discarded coercion and exception side effects | 2 |
| `hinote_recovery/` | Private fields, classes, inheritance and instruction recovery | 2 |
| `iterator_recovery/` | Iterator protocol, early exits and exception cleanup | 2 |
| `namespace_recovery/` | Namespace closures and lexical captures | 2 |
| `object_methods/` | Object literal methods and deep captures | 2 |
| `property_recovery/` | Own properties, descriptors, nested methods and async rest | 2 |

Normal `cargo test -p pedal-abc-bridge --locked` reads these existing ABCs and
requires neither an SDK nor Node. The sources are small regression probes, not
copies of the full production packages used by the corpus tests.

For regeneration, use OpenHarmony ETS SDK 26.0.0.105 targeting API 12 and the
`regenerate.ps1` in the relevant directory. These scripts use the Windows SDK
compiler; pass `-SdkRoot` or set `OHOS_HOME` to your SDK root. Imported inheritance
has its own `advanced_recovery/regenerate_inheritance.ps1`. Debug and optimized
release ABCs must both be updated when a source changes. See each directory's
README for exact commands, source provenance and optional Node comparisons.

Node comparisons use the SDK TypeScript compiler (`OHOS_HOME`) and exported ETS
files under `target/`; they are optional developer checks, not prerequisites for
Cargo tests. Some ArkUI comparisons additionally use a local hvigor project, as
documented in `advanced_recovery/README.md`.

Keep generated inspection output out of this directory. Commit small input
sources, ABCs and tools together, and run the regression suite before submitting.
See the [project testing guide](../../../../tests/README.md) for suite selection.

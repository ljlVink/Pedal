# pedal-abc-bridge

Pure-Rust Ark bytecode (`.abc`) bridge for Pedal: container parsing, instruction
decoding/disassembly, and pseudo-ArkTS decompilation for OpenHarmony/HarmonyOS
artifacts. No UI dependencies; every public operation returns
`Result<T, AbcError>`.

## Modules

| Module | Role |
|---|---|
| `reader` | Little-endian cursor: `u8/u16/u32/u64`, uleb128/sleb128, MUTF-8 strings (CESU-8 surrogate pairs) |
| `file` | PANDA container: header (both the modern ≥9.x 64-byte layout and the legacy `0.1.0.7` 72-byte layout), class/foreign/index regions, records (ESM source files), fields, methods, protos, code items, try blocks, literal arrays, ESM module records |
| `isa` + `isa_gen` | Panda ISA metadata. `isa_gen` is generated from OpenHarmony `arkcompiler_runtime_core/isa/isa.yaml` by `scripts/gen_isa.py`; `isa` adds the legacy opcode override table learned from real corpus files by `scripts/gen_isa_legacy.py` |
| `disasm` | Instruction decoder (prefix byte `0xfb..0xfe`, bit-packed operands) and pandasm-style text renderer with resolved strings and labels |
| `decompile` | Pseudo-ArkTS pipeline: `cfg` (basic blocks, try coverage) → `lift` (accumulator/register expression lifting, side-effect flush, this-call fusion) → `structure` (single-use register folding, compare-chain switch recovery inspired by *A Jump-Table-Agnostic Switch Recovery on ASTs*) → `printer` (functions, class/object literals, import/export headers) |
| `hap` | HAP/HSP/HAR package reader (zip central directory, per-entry lazy reads, abc/an/so/descriptor classification) |

## Supported inputs

- `.hap` / `.hsp` / `.har` / `.app` packages: every embedded `.abc` is parsed;
  other entries (`.so`, `.an`, `module.json`, resources) are exposed as raw bytes.
- Standalone `.abc` files, both bytecode layouts:
  - **Modern** (`9.0.0.0` … `13.x`, e.g. every app `ets/modules.abc`): full
    container + 100% instruction decode on the test corpus.
  - **Legacy** (`0.1.0.7`, e.g. HarmonyOS system framework files): container fully
    parsed; instruction decode uses an empirically learned override table
    (≈90% exact tiling on the corpus). Unknown opcodes fail that method only -
    the record still decompiles with a comment.

## Regenerating the ISA tables

```bash
# modern table (from a checkout of arkcompiler_runtime_core)
python scripts/gen_isa.py target/research/isa.yaml crates/pedal-abc-bridge/src/isa_gen.rs

# legacy overrides (learned + validated against real 0.1.0.7 files)
python scripts/gen_isa_legacy.py test_examples/system_framework/*.abc
```

## Debug CLI

```bash
cargo run -p pedal-abc-bridge --example dump -- file.hap [sample_count] [disasm]
```

Prints header/record/decode-coverage statistics, sample decompiled records and
pandasm disassembly for real corpus files.

```bash
cargo run -p pedal-abc-bridge --example audit_package -- input.hsp target/package-audit
```

Exports each record plus `records.tsv`, `methods.tsv`, `comments.tsv`,
`placeholders.tsv`, and `failures.txt`. The placeholder list also catches unresolved
closures represented by throwing function bodies. The lists are inspection aids (comment-like string contents
can also match), not a semantic error count. This allows before/after comparison
without opening every archive tab manually.

## Tests

`tests/corpus.rs` runs end-to-end against `test_examples/` when present (skipped
otherwise): modern abc parses + decodes 100% of methods, records decompile to
pseudo ArkTS, legacy abc parses with ≥80% decode coverage, HAP listings are
classified.

`tests/fixtures/*_recovery/` contains gitignored local SDK fixtures. Integration
and unit tests load them at runtime through `tests/common/mod.rs`; a missing file
skips only its dependent checks (visible with `-- --nocapture`). Other read errors
and invalid bytecode still fail. Synthetic bytecode tests run without the fixtures.

## Source readability and correctness

Modern function signatures omit the VM's function object, new-target and receiver
slots; explicit arguments start at `a0`, and the receiver prints as `this`.
Method prototypes now retain parameter and return types, including reference,
array and union descriptors. Static prototypes exclude the implicit receiver;
dynamic ABCs may omit the prototype entirely. `read_proto` decodes each format's
shorty codes, and `ProtoInfo.ref_types` lists only references, in return/argument
order. Bodyless typed methods are shown as declarations. Runtime `any` is not
treated as a recovered source type.

For recognized six-parameter `ViewPU` constructors, matching superclass and
initialization calls provide framework-based parameter names/type hints. These
signatures are explicitly marked as inferred; erased component parameter
interfaces remain `unknown`, and available debug names/real types take priority.
The printer declares remaining register temporaries and respects expression
precedence, including exponentiation and nullish/logical mixing. Calls preserve
an explicit receiver using `globalThis.Reflect.apply(...)` when member-call fusion
is not possible, without reading a user-defined `call` property on the function.

Control-flow cleanup uses post-dominators (`Cfg::postdoms`, including a virtual
exit) for branch joins, preserves values used after a branch or across iterations,
and folds temporaries only at safe evaluation sites. Shared branch tails, loop
condition side effects, self-loops, and simple single-handler try/catch blocks are
handled explicitly. `structure::polish` applies bounded folding, dead-store cleanup,
and conditional/nullish simplification for ordinary, nested and top-level bodies.

Literal-array lengths count tag/value pairs rather than decoded values. Both
literal readers now report the actual value count; `LiteralBufferIndex` carries a
`u32` payload. Array stores/spreads are retained instead of dropping their elements.

Modern async functions are normalized only after the complete enter/await/resume/
resolve/reject protocol has been validated. Exception edges participate in
dominance and loop discovery without being printed as ordinary fall-through edges.
Nested exception regions and proven duplicated cleanup sequences become
`try`/`catch`/`finally`, preserving return evaluation, rejection and overriding
completions. Unknown protocols retain the instruction listing and exception table.

When catchall exits cannot be expressed by that structured recovery, supported
reachable blocks can be rendered as an executable state loop. Each block retains
its innermost exception handler and accumulator value, including normal and
exceptional entries to shared cleanup. Unsupported opcodes and invalid handlers
still retain the diagnostic listing. Iterator object guards accept functions and
reject null; the VM hole has a separate identity from JavaScript `undefined`.
Discarded arithmetic, relational and loose-equality expressions retain coercion
side effects and exceptions; lifting and dead-store cleanup share this check.
`LiftedBody::is_concurrent` records a verified completion-notification protocol;
the printer emits `"use concurrent";` before declarations so SDK recompilation
preserves concurrent notifications for both synchronous and asynchronous methods.

Class recovery follows constructor offsets, literal buffers and immediate
`definemethod`/accessor attachments. It keeps class definitions at their original
initialization sites, groups instance/static members, and recovers `extends`,
`super` and static initializer blocks. ArkUI components retain their runtime
constructors, state accessors, lifecycle methods and nested rendering callbacks.
Dynamic prototype assignments remain at their original statement sites.
`definefieldbyname` and `definepropertybyname` preserve the accumulator and define
own data properties with writable/enumerable/configurable all true, using
`Object.defineProperty` rather than assignment. Complete static initializers
containing only receiver aliases and literal property definitions become static
fields, retaining declaration order. Dynamic values and function values remain
in static blocks. `lift::Expr::Object` represents an inline ordered object literal
whose values participate in data-flow, side-effect and equality traversals.
Imported superclasses use local import bindings, with namespace loads resolved by
module request index. `getunmappedargs` and `supercallspread` retain actual argument
forwarding. `lift::Expr::Array` represents inline arrays and preserves iterator
effects of spreads. Fresh empty-array spreads become array literals; general
`starrayspread` retains its start index and own-property writes instead of invoking
`push`. Empty catch protection blocks are removed after normalization.

`callruntime.definefieldbyvalue` takes its key from the first register and its
receiver from the second; `definefieldbyindex` uses the final immediate, excluding
the cache slot. Both retain the accumulator. Computed object callback stores
convert keys once and restore function names after the own-property write.
`copydataproperties` preserves the runtime's enumerable descriptor-copy operation,
including Symbol keys, accessor identities, nullish sources and destination return
values. The descriptor behavior follows the
[HarmonyOS 5 runtime implementation](https://github.com/openharmony/arkcompiler_ets_runtime/blob/OpenHarmony-5.0.0-Release/ecmascript/stubs/runtime_stubs-inl.h);
it is not rewritten as a standard JavaScript spread when accessors are present.

`AbcFile::read_debug_info(&MethodInfo)` returns `AbcResult<Option<file::debug::DebugInfo>>`.
`DebugInfo` exposes `parameter_names` and `locals`; each `LocalVariable` contains
`register`, `name`, optional `type_name`/`signature`, `start_pc` and exclusive `end_pc`. The parser handles PANDA's
separate line-program/constant-pool streams. Names are used only when parameter
copies and local register lifetimes are unambiguous; reused registers keep their
fallback names. Lexical frame identities connect closures to their captured
bindings, and repeated closure expressions retain their bodies.
Object-literal methods inherit their creation site's lexical environment and do
not also appear as orphan top-level functions. Nested expansion reserves one
count per method, separately from byte accounting; depth and size limits remain.
Closure nesting permits eight levels and up to 4,096 decoded instructions per
expanded body, retaining the 8,000-byte code limit, 256 unique closure expansion
budget and 1 MiB cumulative closure text limits. Audit output lists any remaining
limit-triggered placeholders separately from comments.
Object methods use the same eight-level depth bound and allow repeated cached
expansions. Their unique-method budget is 256 with a 64 KiB member-text bound;
all embedded bodies also count toward the 1 MiB output limit. Rest-argument copies
do not read the async machine, even when ISA accumulator flags describe them as
in/out; both narrow and wide forms preserve verified async normalization.

Instruction-specific lexical frame mappings distinguish reused depth/slot pairs
and retain bindings after branches whose environment stacks agree. Exception
handlers inherit a frame only when every protected instruction agrees on its
environment stack. Modern
namespace IIFEs keep their captured closures instead of using the legacy
mangled-name class heuristic. Lazy module loads use regular import slots;
dynamic imports, property deletion and quoted object keys are rendered explicitly.
Async normalization proves aliases across CFG edges and preserves optimized
resume jumps. Exception recovery only joins split exits whose register-copy tails
are proven nonthrowing, preserving pending calls inside their original try/catch.

Wide opcodes share the full narrow-opcode lifter while retaining their encoded
operand widths. Sendable module loads use import slots; Sendable class buffers
exclude the appended field-type index before splitting static/instance members.
Private data-only buffers with proven lexical frame identities use independent
WeakMaps, retaining brand checks and hidden state. Private methods/accessors and
ambiguous environments retain diagnostics. Instance initializer closures use
dynamic receivers. Namespace-local classes retain their declarations/accessors.
Environment allocations outside CFG cycles can use distinct captured bindings;
repeatable allocations remain conservative. For-in enumerates inherited keys,
and pending calls are flushed before loop transfers. New-object apply checks the
array operand and reads it by index through Reflect.construct.

`tests/hinote_recovery.rs` and its SDK fixtures cover these operations; the adjacent
Node comparison checks private isolation/branding, accessor behavior, inherited
enumeration, per-iteration calls and asynchronous completion. Synthetic tests also
cover sixteen-bit module indices, Sendable import slots and unknown private layouts.

The output remains pseudo-ArkTS. Stripped original names/types, generators, unknown
async layouts, and ambiguous lexical scopes (including some catch-only captures)
are not reconstructed exactly. ArkUI decorators and the original declarative
`build` DSL are not inferred from runtime code. Oversized or undecodable class
members remain visible as explicit unresolved bodies with a disassembly reference.

Instruction semantics were cross-checked against
[abc-decompiler's instruction decoder](https://github.com/ohos-decompiler/abc-decompiler/blob/master/jadx-core/src/main/java/jadx/core/dex/instructions/InsnDecoder.java)
and the
[OpenHarmony literal accessor](https://github.com/openharmony/arkcompiler_runtime_core/blob/master/libpandafile/literal_data_accessor-inl.h).
The implementation remains in-process Rust.

`tests/readability.rs` uses self-contained synthetic bytecode, so these regressions
run even without the ignored corpus. `tests/corpus.rs` resolves the optional corpus
relative to the workspace root, including when invoked by Cargo from this crate.

`tests/advanced_recovery.rs` uses optional local debug/release ABC fixtures compiled by
the real SDK; normal Cargo tests do not require the SDK or Node. The adjacent
fixture README documents regeneration and optional Node differential tests against
the original source, including rejection injection and ArkUI host stubs. The latter
exercise state/lifecycle/render callbacks; they are not device UI tests.

```bash
cargo run -p pedal-abc-bridge --example export_record -- input.abc RecordName output.ets
```

The `dump` example also accepts `ARK_DISASM_METHOD` to select a method by name, or
`ARK_DISASM_AT` to select an exact hexadecimal offset.

`tests/property_recovery.rs` uses optional local SDK debug/release fixtures for computed and
indexed properties, function names, descriptor copies, deep object methods, more
than twelve methods, and async rest parameters. Its fixture README describes
optional Node behavior checks; accumulator and operand-order checks also run on
synthetic bytecode in `tests/readability.rs`.

Additional references: [JD's class/dependency analysis](https://github.com/jd-opensource/arkdecompiler),
[PANDA debug data](https://github.com/openharmony/arkcompiler_runtime_core/blob/master/libpandafile/debug_data_accessor-inl.h),
and [Phoenix's semantics-preserving structural analysis](https://edmcman.github.io/papers/usenix13.pdf).

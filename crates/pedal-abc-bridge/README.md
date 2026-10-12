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

## Tests

`tests/corpus.rs` runs end-to-end against `test_examples/` when present (skipped
otherwise): modern abc parses + decodes 100% of methods, records decompile to
pseudo ArkTS, legacy abc parses with ≥80% decode coverage, HAP listings are
classified.

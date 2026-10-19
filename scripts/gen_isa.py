#!/usr/bin/env python3
"""Generate crates/pedal-abc-bridge/src/isa_gen.rs from OpenHarmony isa.yaml.

Regenerate with:
    python scripts/gen_isa.py target/research/isa.yaml

Parses `groups[].instructions[]` entries (sig / opcode_idx / format / prefix /
properties / acc) and the `prefixes:` table, then emits a static Rust table.
Format operand layouts follow runtime_core isa/isapi.rb `Format#encoding`:
the format name itself encodes `<field><bits>` pairs packed after the opcode
byte(s), e.g. `op_imm_16` -> imm:16 @bit 8; `pref_op_v8_8_v8_8` -> two 8-bit
register operands after a 2-byte (prefixed) opcode.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

PREFIXES = {}  # name -> opcode byte


def parse_uleb_skip(text: str) -> int:
    return 0


def parse_format(fmt: str):
    """Return (total_bits_after_opcode, is_prefixed, [(name, bits, bitoff)])."""
    prefixed = fmt.startswith("pref_")
    name = fmt.replace("pref_", "").replace("op_", "")
    parts = name.split("_")
    if parts == ["none"]:
        return 0, prefixed, []
    # pair up <field><bits>; names may contain digits (v8, v16, id16...)
    fields = []
    i = 0
    bits_sum = 0
    while i < len(parts):
        fname = parts[i]
        # width may be glued in the NEXT token when field name has no digit
        if i + 1 < len(parts) and parts[i + 1].isdigit():
            fbits = int(parts[i + 1])
            i += 2
        else:
            m = re.search(r"(\d+)$", fname)
            if not m:
                raise ValueError(f"bad format {fmt}")
            fbits = int(m.group(1))
            fname = fname[: m.start()]
            i += 1
        fields.append((fname, fbits, 8 * (2 if prefixed else 1) + bits_sum))
        bits_sum += fbits
    if bits_sum % 8 != 0:
        raise ValueError(f"format {fmt} not byte aligned")
    return bits_sum, prefixed, fields


def parse_sig_operands(sig: str):
    """`lda.str id:string_id, v:in` -> [(name, kind)] with roles v/imm/id."""
    ops = []
    head = sig.split(None, 1)
    if len(head) < 2:
        return ops
    for tok in head[1].split(","):
        tok = tok.strip()
        if not tok:
            continue
        name, _, decl = tok.partition(":")
        ops.append((name.strip(), decl.strip()))
    return ops


def esc(s: str) -> str:
    """Rust string escape"""
    return s.replace("\\", "\\\\").replace('"', '\\"').replace("\n", "\\n")


def main() -> int:
    src = Path(sys.argv[1] if len(sys.argv) > 1 else "target/research/isa.yaml")
    dst = Path(
        sys.argv[2] if len(sys.argv) > 2 else "crates/pedal-abc-bridge/src/isa_gen.rs"
    )
    text = src.read_text(encoding="utf-8")

    import yaml  # PyYAML

    doc = yaml.safe_load(text)
    for p in doc.get("prefixes", []):
        PREFIXES[p["name"]] = p["opcode_idx"]

    entries = []  # rows to emit
    seen = set()
    for group in doc.get("groups", []):
        for insn in group.get("instructions", []):
            sig = insn["sig"]
            mnem = sig.split(None, 1)[0]
            opcodes = insn.get("opcode_idx", [])
            formats = insn.get("format", [])
            if len(opcodes) != len(formats):
                raise ValueError(f"{sig}: opcode_idx/format length mismatch")
            prefix = insn.get("prefix")
            props = set(insn.get("properties", []) or [])
            group_props = set(group.get("properties", []) or [])
            acc = insn.get("acc", "none")
            for opc, fmt in zip(opcodes, formats):
                key = (prefix, opc)
                if key in seen:
                    # duplicate (later groups override older ones: keep first)
                    continue
                seen.add(key)
                bits, prefixed, fields = parse_format(fmt)
                if prefixed != (prefix is not None):
                    raise ValueError(f"{sig}: prefix/format mismatch {fmt}")
                sig_ops = parse_sig_operands(sig)
                entries.append(
                    {
                        "mnem": mnem,
                        "sig": sig,
                        "prefix": prefix,
                        "opcode": opc,
                        "fmt": fmt,
                        "size": (2 if prefixed else 1) + bits // 8,
                        "fields": fields,
                        "sig_ops": sig_ops,
                        "acc": acc,
                        "props": sorted(props | group_props),
                    }
                )

    lines = []
    lines.append("//! 自动生成：Panda ISA 指令表（由 scripts/gen_isa.py 从 isa.yaml 生成）。")
    lines.append("//!")
    lines.append("//! @author ljlVink")
    lines.append("")
    lines.append("use crate::isa::{InsFormat, InsStatic, OperandField};")
    lines.append("")
    lines.append("/// 全部指令（含前缀形式）")
    lines.append("pub static INSTRUCTIONS: &[InsStatic] = &[")
    for e in sorted(entries, key=lambda x: (x["prefix"] or "", x["opcode"])):
        mnem = e["mnem"]
        pref = f'Some(0x{PREFIXES[e["prefix"]]:02x})' if e["prefix"] else "None"
        fields = ", ".join(
            f'OperandField {{ name: "{esc(fn_)}", bit_off: {off}, bits: {b} }}'
            for fn_, b, off in e["fields"]
        )
        sig_ops = ", ".join(f'("{esc(n)}", "{esc(decl)}")' for n, decl in e["sig_ops"])
        is_jump = "jump" in e["props"] or "conditional" in e["props"]
        acc_read = "acc_read" in e["props"] or e["acc"].startswith(("in", "inout"))
        acc_write = "acc_write" in e["props"] or e["acc"].startswith(("out", "inout"))
        lines.append(
            f'    InsStatic {{ mnemonic: "{esc(mnem)}", prefix: {pref}, opcode: 0x{e["opcode"]:02x}, '
            f'size: {e["size"]}, sig: "{esc(e["sig"])}", format: InsFormat::from_name("{esc(e["fmt"])}"), '
            f"fields: &[{fields}], sig_ops: &[{sig_ops}], jump: {str(is_jump).lower()}, "
            f"acc_read: {str(acc_read).lower()}, acc_write: {str(acc_write).lower()} }},"
        )
    lines.append("];")
    lines.append("")
    dst.parent.mkdir(parents=True, exist_ok=True)
    dst.write_text("\n".join(lines) + "\n", encoding="utf-8")
    print(f"emitted {len(entries)} instructions -> {dst}")
    return 0


if __name__ == "__main__":
    sys.exit(main())

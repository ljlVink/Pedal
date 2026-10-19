#!/usr/bin/env python3
"""Learn the 0.1.0.7-era ecmascript opcode table empirically from legacy abc files.

These system abc files predate the Aug-2022 ISA renumbering; public runtime_core
tags (v3.1/v3.2) no longer carry their numbering. We recover it from the corpus:

1. Tiling constraints: every method's instruction bytes must decode to exactly
   code_size, and (for the vast majority) end with a 1-byte terminator
   (return=0x91 / returnundefined=0x92 / throw=0x8f).
2. Shortest-first solving: methods are tiled with a branch-and-bound over
   unknown opcode sizes; unambiguous tilings teach new sizes.
3. Semantic anchors: string-id resolution (`ldthisbyname`-style 4-byte loads
   whose id resolves to a plausible property name), argument counts, and shape
   matching against the modern ISA produce mnemonic/format mappings.

Output: JSON table {opcode: {size, mnemonic?, format?}} for the Rust decoder.

Usage: python scripts/gen_isa_legacy.py <file.abc>... > legacy_isa.json
"""

from __future__ import annotations

import json
import struct
import sys
from pathlib import Path


def sleb(d, o):
    r = s = 0
    while True:
        b = d[o]
        o += 1
        r |= (b & 0x7F) << s
        s += 7
        if not b & 0x80:
            if b & 0x40:
                r -= 1 << s
            return r, o
TERM = {0x91: "return", 0x92: "returnundefined", 0x8f: "throw"}


def uleb(d, o):
    r = s = 0
    while True:
        b = d[o]
        o += 1
        r |= (b & 0x7F) << s
        if not b & 0x80:
            return r, o
        s += 7


def rstr(d, o):
    h, o = uleb(d, o)
    ln = h >> 1
    return d[o : o + ln].decode("utf-8", "replace"), o + ln + 1


class Abc:
    def __init__(self, path):
        self.d = Path(path).read_bytes()
        d = self.d
        u = lambda o: struct.unpack_from("<I", d, o)[0]
        self.version = d[12:16]
        assert self.version == b"\x00\x01\x00\x07", f"{path}: not 0.1.0.7"
        self.foreign_off, self.foreign_size = u(0x14), u(0x18)
        self.num_classes, self.class_idx_off = u(0x20), u(0x24)
        self.num_litarr, self.litarr_off = u(0x30), u(0x34)
        self.num_idx, self.index_section_off = u(0x40), u(0x44)
        o = self.index_section_off
        st, en, cn, co, mn, mo, fn, fo, pn, po = struct.unpack_from("<10I", d, o)
        self.msl = [u(mo + 4 * i) for i in range(mn)]
        self.class_idx = [u(self.class_idx_off + 4 * i) for i in range(self.num_classes)]

    def is_foreign(self, off):
        return self.foreign_off <= off < self.foreign_off + self.foreign_size

    def methods(self):
        u = lambda o: struct.unpack_from("<I", d, o)[0]
        for off in self.class_idx:
            if self.is_foreign(off):
                continue
            d = self.d
            _, o = rstr(d, off)
            o += 4  # super
            acc, o = uleb(d, o)
            nf, o = uleb(d, o)
            nm, o = uleb(d, o)
            while True:
                t = d[o]
                o += 1
                if t == 0:
                    break
                if t == 0x01:
                    n, o = uleb(d, o)
                    o += 2 * n
                elif t == 0x02:
                    o += 1
                elif t == 0x07:
                    o += 4
                else:
                    o += 4
            for _ in range(nf):
                o += 8
                _, o = uleb(d, o)  # field access flags
                while d[o] != 0:
                    t = d[o]
                    if t == 0x01:
                        _, o = sleb(d, o + 1)
                    else:
                        o += 5
                o += 1
            for _ in range(nm):
                mstart = o
                o += 8
                _, o = uleb(d, o)  # access flags
                code_off = 0
                while True:
                    t = d[o]
                    o += 1
                    if t == 0:
                        break
                    if t == 0x01:
                        code_off = u(o)
                    o += 1 if t == 0x02 else 4
                if code_off:
                    nv, o2 = uleb(d, code_off)
                    na, o2 = uleb(d, o2)
                    cs, o2 = uleb(d, o2)
                    ts, o2 = uleb(d, o2)
                    yield mstart, d[o2 : o2 + cs], nv, na
                else:
                    yield mstart, b"", 0, 0


CAND = [1, 2, 3, 4, 5, 6, 8, 10]


CAND = [1, 2, 3, 4, 5, 6, 8, 10]


def tileable(b, learned):
    """严格平铺：路径上所有 opcode 均已知 size，且精确到达末尾、以终止符结尾"""
    n = len(b)
    if n == 0:
        return False
    reach = [False] * (n + 1)
    reach[n] = True
    for i in range(n - 1, -1, -1):
        x = b[i]
        s = learned.get(x)
        if s is None:
            continue
        j = i + s
        if j <= n and reach[j]:
            if j == n:
                if x in TERM:
                    reach[i] = True
            else:
                reach[i] = True
    return reach[0]


def coverage(bodies, learned):
    return sum(1 for b in bodies if tileable(b, learned))


def progress(b, learned):
    """宽松可达性：未知 opcode 允许任意候选时，方法能否铺满（进度信号）"""
    n = len(b)
    if n == 0:
        return False
    reach = [False] * (n + 1)
    reach[n] = True
    for i in range(n - 1, -1, -1):
        x = b[i]
        s = learned.get(x)
        options = (s,) if s else CAND
        for c in options:
            j = i + c
            if j <= n and reach[j]:
                if j == n:
                    if x in TERM:
                        reach[i] = True
                        break
                else:
                    reach[i] = True
                    break
    return reach[0]


def solve(sizes, methods, max_len=400):
    """贪心 + 覆盖验证：每轮提交使（严格可平铺数, 宽松可达数）增长最大的 (opcode, size)。"""
    from collections import Counter

    learned = dict(sizes)
    # 第一阶段：只用短方法训练（约束强、速度快）
    short = [m[1] for m in methods if 0 < len(m[1]) <= 28]
    bodies = short

    def score(bs):
        strict = coverage(bs, learned)
        loose = sum(1 for b in bs if progress(b, learned))
        return (strict, loose)

    for phase_bodies in (short, [m[1] for m in methods if 0 < len(m[1]) <= max_len]):
        bodies = phase_bodies
        cur = score(bodies)
        for _ in range(160):
            cands = Counter()
            for b in bodies:
                if not tileable(b, learned):
                    cands.update(set(b))
            if not cands:
                break
            best = None
            for x, freq in cands.most_common(30):
                if x in learned:
                    continue
                gains = []
                for s in CAND:
                    learned[x] = s
                    gains.append((score(bodies), s))
                del learned[x]
                gain, s = max(gains)
                if best is None or gain > best[0]:
                    best = (gain, x, s)
            if best is None or best[0] <= cur:
                break
            gain, x, s = best
            learned[x] = s
            cur = gain
    unresolved = [m for m in methods if m[1] and not tileable(m[1], learned)]
    return learned, unresolved, coverage([m[1] for m in methods if m[1]], learned)


def main():
    files = sys.argv[1:] or ["test_examples/system_framework/crypto_framework_ets.abc"]
    sizes = dict(TERM and {t: 1 for t in TERM})
    all_methods = []
    for f in files:
        abc = Abc(f)
        all_methods.extend(abc.methods())
    sizes, pending, ok = solve(sizes, all_methods)
    print(
        f"learned {len(sizes)} opcode sizes; tiled {ok}/{len(all_methods)}",
        file=sys.stderr,
    )
    json.dump({f"{k:#04x}": v for k, v in sorted(sizes.items())}, sys.stdout, indent=1)
    for m in pending[:5]:
        print("UNRESOLVED", m[1][:24].hex(" "), file=sys.stderr)


if __name__ == "__main__":
    main()

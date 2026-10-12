//! 调试 CLI：解析 abc/hap 并输出统计与反编译样例
//!
//! 用法：
//! - `dump <file.abc>`：解析单个 abc，输出头部/record/方法统计与若干反编译样例
//! - `dump <file.hap|file.hsp>`：列出包内条目并解析主 abc
//!
//! @author ljlVink

use pedal_abc_bridge::decompile;
use pedal_abc_bridge::disasm::{self, DisasmCtx};
use pedal_abc_bridge::file::AbcFile;
use pedal_abc_bridge::hap::HapArchive;

fn dump_abc(label: &str, data: Vec<u8>, sample: usize, full_disasm: bool) {
    let start = std::time::Instant::now();
    let file = match AbcFile::parse(data) {
        Ok(f) => f,
        Err(e) => {
            println!("[{label}] PARSE FAILED: {e}");
            return;
        }
    };
    println!(
        "[{label}] ver={}.{}.{}.{} classes={} regions={} foreign={:#x}+{:#x} lnps={} lits={} parse={}ms",
        file.header.version[0],
        file.header.version[1],
        file.header.version[2],
        file.header.version[3],
        file.classes.len(),
        file.index_headers.len(),
        file.header.foreign_off,
        file.header.foreign_size,
        file.header.num_lnps,
        file.header.num_literalarrays,
        start.elapsed().as_millis()
    );
    for (i, h) in file.index_headers.iter().enumerate().take(3) {
        println!(
            "  region {i}: [{:#x},{:#x}) class={}@{:#x} msl={}@{:#x} proto={}@{:#x}",
            h.start,
            h.end,
            h.class_idx_size,
            h.class_idx_off,
            h.method_idx_size,
            h.method_idx_off,
            h.proto_idx_size,
            h.proto_idx_off
        );
    }
    if let Some(c) = file.classes.iter().find(|c| !c.methods.is_empty()) {
        let m = &c.methods[0];
        println!(
            "  first method @{:#x} region_hit={} resolve_msl(0)={:?}",
            m.offset,
            file.region_of(m.offset).is_some(),
            file.resolve_msl(m.offset, 0)
        );
    }
    let mut total = 0usize;
    let mut decoded = 0usize;
    let mut methods_with_code = 0usize;
    let mut unknown: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for c in &file.classes {
        for m in &c.methods {
            let Some(code) = &m.code else { continue };
            methods_with_code += 1;
            total += 1;
            let legacy = file.header.kind == pedal_abc_bridge::file::HeaderKind::Legacy;
            match disasm::decode_with(code.instructions(&file.data), legacy) {
                Ok(_) => decoded += 1,
                Err(e) => *unknown.entry(format!("{e}")).or_insert(0) += 1,
            }
        }
    }
    println!(
        "  methods_with_code={methods_with_code} decoded_ok={decoded}/{total} ({:.1}%)",
        if total > 0 {
            decoded as f64 / total as f64 * 100.0
        } else {
            100.0
        }
    );
    let mut unk: Vec<_> = unknown.into_iter().collect();
    unk.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
    for (e, n) in unk.iter().take(8) {
        println!("    unknown x{n}: {e}");
    }
    // 定点反汇编调试：ARK_DISASM_AT=<method_off>
    if let Ok(at) = std::env::var("ARK_DISASM_AT")
        && let Ok(off) = u32::from_str_radix(at.trim_start_matches("0x"), 16)
        && let Ok(m) = file.method_at(off)
        && let Some(code) = &m.code
    {
        let ctx = DisasmCtx {
            file: &file,
            method_off: m.offset,
        };
        match disasm::disassemble_method(
            &ctx,
            &m.name,
            code.instructions(&file.data),
            code.num_vregs,
            code.num_args,
            &code.try_blocks,
        ) {
            Ok(text) => println!("{text}"),
            Err(e) => println!("disasm failed: {e}"),
        }
        if std::env::var("ARK_SIG").is_ok() {
            let legacy = file.header.kind == pedal_abc_bridge::file::HeaderKind::Legacy;
            if let Ok(ins) = disasm::decode_with(code.instructions(&file.data), legacy) {
                for x in ins.iter() {
                    let fields: Vec<String> = x
                        .sig_values()
                        .iter()
                        .map(|(n, _, v)| format!("{n}={v}"))
                        .collect();
                    println!("  {:#x} {} [{}]", x.pc, x.mnemonic(), fields.join(", "));
                }
            }
        }
    }

    // 样例反编译
    let mut shown = 0;
    for c in &file.classes {
        if shown >= sample {
            break;
        }
        let code_methods = c
            .methods
            .iter()
            .filter(|m| m.code.as_ref().map(|c| c.code_size > 8).unwrap_or(false))
            .count();
        if code_methods == 0 {
            continue;
        }
        if full_disasm {
            for m in c.methods.iter().take(3) {
                let Some(code) = &m.code else { continue };
                if code.code_size < 8 {
                    continue;
                }
                let ctx = DisasmCtx {
                    file: &file,
                    method_off: m.offset,
                };
                match disasm::disassemble_method(
                    &ctx,
                    &m.name,
                    code.instructions(&file.data),
                    code.num_vregs,
                    code.num_args,
                    &code.try_blocks,
                ) {
                    Ok(text) => {
                        println!("---- disasm {} ----", m.name);
                        for l in text.lines().take(50) {
                            println!("{l}");
                        }
                    }
                    Err(e) => println!("disasm failed: {e}"),
                }
            }
        }
        match decompile::decompile_record(&file, c) {
            Ok(rec) => {
                if let Ok(want) = std::env::var("ARK_FULL") {
                    if c.name.contains(&want) {
                        println!("{}", rec.source);
                    }
                    continue;
                }
                if rec.source.len() > 120 {
                    println!("---- record {} ({code_methods} methods) ----", rec.name);
                    for l in rec.source.lines().take(70) {
                        println!("{l}");
                    }
                    shown += 1;
                }
            }
            Err(e) => println!("---- record {} FAILED: {e}", c.name),
        }
    }
    if full_disasm && let Some(c) = file.classes.iter().find(|c| !c.methods.is_empty()) {
        let m = &c.methods[0];
        if let Some(code) = &m.code {
            let ctx = DisasmCtx {
                file: &file,
                method_off: m.offset,
            };
            match disasm::disassemble_method(
                &ctx,
                &m.name,
                code.instructions(&file.data),
                code.num_vregs,
                code.num_args,
                &code.try_blocks,
            ) {
                Ok(text) => {
                    println!("---- disasm sample: {} ----", m.name);
                    for l in text.lines().take(40) {
                        println!("{l}");
                    }
                }
                Err(e) => println!("disasm failed: {e}"),
            }
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("usage: dump <file.abc|file.hap|file.hsp> [sample_count] [disasm]");
        std::process::exit(2);
    }
    let path = &args[1];
    let sample: usize = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(2);
    let full_disasm = args.get(3).map(|s| s == "disasm").unwrap_or(false);
    let data = std::fs::read(path).expect("read file");
    if HapArchive::is_package(path) {
        let hap = HapArchive::parse(data).expect("parse hap");
        println!(
            "[{}] entries={} abc={}",
            path,
            hap.entries.len(),
            hap.entries.iter().filter(|e| e.is_abc).count()
        );
        for e in hap.entries.iter().filter(|e| e.is_abc).take(4) {
            println!("  abc entry: {} ({} bytes)", e.path, e.size);
        }
        for e in hap.entries.iter().filter(|e| e.is_abc).take(2) {
            let bytes = hap.read_entry(&e.path).expect("read entry");
            dump_abc(&format!("{}/{}", path, e.path), bytes, sample, full_disasm);
        }
    } else {
        dump_abc(path, data, sample, full_disasm);
    }
}

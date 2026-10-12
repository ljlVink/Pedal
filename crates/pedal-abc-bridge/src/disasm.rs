//! 指令解码与反汇编文本渲染
//!
//! 解码规则（与 runtime_core `isapi.rb` 一致）：
//! - 首字节为操作码；`0xfb..0xfe` 为前缀字节，其后一字节才是操作码；
//! - 操作数按格式名在操作码（或前缀+操作码）之后按位紧 packing，
//!   多字节值小端。
//!
//! @author ljlVink

use crate::error::{AbcError, AbcResult};
use crate::file::{AbcFile, TryBlock};
use crate::isa::{self, InsStatic};

/// 一条已解码指令
#[derive(Debug, Clone)]
pub struct Ins {
    /// 指令 pc（方法指令区起点为 0）
    pub pc: usize,
    /// 指令长度
    pub size: u8,
    /// 前缀字节
    pub prefix: Option<u8>,
    /// 操作码
    pub opcode: u8,
    /// 静态元数据
    pub meta: &'static InsStatic,
    /// 原始字节（最多 16 字节，零填充）
    raw: [u8; 16],
}

impl Ins {
    /// 提取指定位布局字段
    pub fn field(&self, name: &str) -> Option<u64> {
        let f = self.meta.fields.iter().find(|f| f.name == name)?;
        Some(extract_bits(&self.raw, f.bit_off, f.bits))
    }

    /// 助记符（sig 头已含前缀命名空间，如 `callruntime.x` / `throw.x` / `wide.x`）
    pub fn mnemonic(&self) -> String {
        self.meta.mnemonic.to_string()
    }

    /// sig 操作数个数字段（v/imm/id），按 sig 顺序
    pub fn sig_values(&self) -> Vec<(&'static str, &'static str, u64)> {
        // sig 操作数与格式字段按位置一一对应（isa.yaml 约定两者同序），
        // sig 名（string_id/method_id/v…）与编码字段名���id/id1/imm1/v8…）不同。
        let mut out = Vec::new();
        for (i, (name, decl)) in self.meta.sig_ops.iter().enumerate() {
            let val = self
                .meta
                .fields
                .get(i)
                .map(|f| extract_bits(&self.raw, f.bit_off, f.bits))
                .unwrap_or(0);
            out.push((*name, *decl, val));
        }
        out
    }

    /// 跳转目标（相对 imm），返回绝对 pc
    pub fn jump_target(&self) -> Option<usize> {
        if !self.meta.jump {
            return None;
        }
        // 实测（arkts_probe 12.0.2 全量 + HiNote widgets 32/32）：
        // 目标 = pc + imm（等于 code_len 表示跳到方法尾）。
        // imm 按 sig 声明宽度符号扩展：imm8/imm16 的回边（imm ≥ 0x80/0x8000）
        // 是负数 -- fib@probe: jmp imm=226 → -30 → 0x18 = while 条件回边
        let f = self.meta.fields.iter().find(|f| f.name == "imm")?;
        let imm = extract_bits(&self.raw, f.bit_off, f.bits);
        // 按实际编码位宽符号扩展（sig 声明是最大宽度 i32；短格式 imm8 的
        // 回边 imm ≥ 0x80 是负数，如 fib@probe: jmp 226 → -30 → 0x18）
        let signed = match f.bits {
            0..=8 => imm as u8 as i8 as i64,
            9..=16 => imm as u16 as i16 as i64,
            _ => imm as u32 as i32 as i64,
        };
        Some((self.pc as i64 + signed) as usize)
    }
}

/// 解码整个方法指令区（按现代 ISA）
pub fn decode(code: &[u8]) -> AbcResult<Vec<Ins>> {
    decode_with(code, false)
}

/// 解码方法指令区；`legacy` 时优先使用 0.1.0.x 覆盖表
pub fn decode_with(code: &[u8], legacy: bool) -> AbcResult<Vec<Ins>> {
    let mut out = Vec::with_capacity(code.len() / 3 + 4);
    let mut pc = 0usize;
    while pc < code.len() {
        let b0 = code[pc];
        let (prefix, opcode) = if isa::is_prefix(b0) {
            if pc + 1 >= code.len() {
                return Err(AbcError::UnknownOpcode {
                    prefix: None,
                    opcode: b0,
                    pc,
                });
            }
            (Some(b0), code[pc + 1])
        } else {
            (None, b0)
        };
        let meta = if legacy && prefix.is_none() {
            isa::lookup_legacy(opcode)
        } else {
            None
        }
        .or_else(|| isa::lookup(prefix, opcode))
        .ok_or(AbcError::UnknownOpcode { prefix, opcode, pc })?;
        let size = meta.size as usize;
        if pc + size > code.len() {
            return Err(AbcError::UnknownOpcode { prefix, opcode, pc });
        }
        let mut raw = [0u8; 16];
        raw[..size.min(16)].copy_from_slice(&code[pc..pc + size.min(16)]);
        out.push(Ins {
            pc,
            size: size as u8,
            prefix,
            opcode,
            meta,
            raw,
        });
        pc += size;
    }
    Ok(out)
}

/// 从指令原始字节中按位提取小端字段
fn extract_bits(raw: &[u8; 16], bit_off: u32, bits: u32) -> u64 {
    let mut v: u128 = 0;
    for (i, b) in raw.iter().enumerate().take(16) {
        v |= (*b as u128) << (8 * i);
    }
    if bits == 0 {
        return 0;
    }
    let mask: u128 = if bits >= 64 {
        u64::MAX as u128
    } else {
        (1u128 << bits) - 1
    };
    ((v >> bit_off) & mask) as u64
}

/// 反汇编渲染上下文（id 解析回调）
pub struct DisasmCtx<'a> {
    /// 宿主文件（解析字符串/方法/字面量）
    pub file: &'a AbcFile,
    /// 方法偏移（用于 region 解析）
    pub method_off: u32,
}

impl<'a> DisasmCtx<'a> {
    /// 渲染一条指令操作数
    fn render_operands(&self, ins: &Ins, label_of: &dyn Fn(usize) -> String) -> String {
        let mut parts = Vec::new();
        for (name, decl, val) in ins.sig_values() {
            let s = if ins.meta.jump && name == "imm" {
                label_of(ins.jump_target().unwrap_or(0))
            } else if name.ends_with("string_id") {
                match self.file.resolve_msl(self.method_off, val as u32) {
                    Ok(off) => match self.file.string_at(off) {
                        Ok(s) => format!("{:?}", s),
                        Err(_) => format!("str@{}", val),
                    },
                    Err(_) => format!("str@{}", val),
                }
            } else if name.ends_with("method_id") {
                match self.file.resolve_msl(self.method_off, val as u32) {
                    Ok(off) => match self.method_display(off) {
                        Some(s) => s,
                        None => format!("method@{val}"),
                    },
                    Err(_) => format!("method@{val}"),
                }
            } else if name.ends_with("literalarray_id") {
                format!("litarr@{}", val)
            } else if name.ends_with("type_id") {
                format!("type@{}", val)
            } else if name.ends_with("field_id") {
                format!("field@{}", val)
            } else if name.starts_with('v') {
                format!("v{val}")
            } else {
                format!("{}", signed_imm(decl, val))
            };
            parts.push(s);
        }
        parts.join(", ")
    }

    fn method_display(&self, off: u32) -> Option<String> {
        let mut r = crate::reader::Reader::new(self.file.bytes());
        r.seek(off as usize).ok()?;
        let _class_idx = r.u16().ok()?;
        let _proto_idx = r.u16().ok()?;
        let name_off = r.u32().ok()?;
        let name = self.file.string_at(name_off).unwrap_or_default();
        Some(name)
    }
}

/// 立即数按 sig 声明渲染为有符号
fn signed_imm(decl: &str, val: u64) -> i64 {
    if decl.contains('i') && !decl.contains('u') {
        match decl {
            "i8" => val as u8 as i8 as i64,
            "i16" => val as u16 as i16 as i64,
            "i32" => val as u32 as i32 as i64,
            "i64" => val as i64,
            _ => val as i64,
        }
    } else {
        val as i64
    }
}

/// 渲染单条指令文本
pub fn render_ins(ctx: &DisasmCtx, ins: &Ins) -> String {
    let ops = ctx.render_operands(ins, &|pc| format!("#{pc}"));
    if ops.is_empty() {
        ins.mnemonic().to_string()
    } else {
        format!("{} {}", ins.mnemonic(), ops)
    }
}

/// 反汇编整个方法（pandasm 风格文本）
pub fn disassemble_method(
    ctx: &DisasmCtx,
    name: &str,
    code_bytes: &[u8],
    num_vregs: u32,
    num_args: u32,
    try_blocks: &[TryBlock],
) -> AbcResult<String> {
    let legacy = ctx.file.header.kind == crate::file::HeaderKind::Legacy;
    let ins = decode_with(code_bytes, legacy)?;
    // 标签：跳转目标 + catch handler
    let mut targets: Vec<usize> = ins
        .iter()
        .filter_map(|i| i.jump_target())
        .filter(|&t| t <= code_bytes.len())
        .collect();
    for tb in try_blocks {
        for cb in &tb.catch_blocks {
            targets.push(cb.handler_pc as usize);
        }
    }
    targets.sort_unstable();
    targets.dedup();
    let label_targets = targets.clone();
    let label_of = move |pc: usize| -> String {
        if label_targets.binary_search(&pc).is_ok() {
            format!("L_{pc:04x}")
        } else {
            format!("#{pc}")
        }
    };
    let mut out = String::with_capacity(ins.len() * 24 + 256);
    out.push_str(&format!(
        ".function any {}() <static> {{\n",
        pandasm_function_name(name)
    ));
    out.push_str(&format!(
        " /* num_vregs = {} num_args = {} */\n",
        num_vregs, num_args
    ));
    for tb in try_blocks {
        for cb in &tb.catch_blocks {
            out.push_str(&format!(
                " .catchall :try_{:04x}, begin: L_{:04x}, end: L_{:04x} -> L_{:04x}\n",
                tb.start_pc,
                tb.start_pc,
                tb.start_pc + tb.length,
                cb.handler_pc
            ));
        }
    }
    for i in &ins {
        if targets.binary_search(&i.pc).is_ok() {
            out.push_str(&format!("L_{:04x}:\n", i.pc));
        }
        let ops = ctx.render_operands(i, &label_of);
        if ops.is_empty() {
            out.push_str(&format!("  {}\n", i.mnemonic()));
        } else {
            out.push_str(&format!("  {} {}\n", i.mnemonic(), ops));
        }
    }
    out.push_str("}\n");
    Ok(out)
}

/// pandasm 函数名：名字中的特殊字符转义
fn pandasm_function_name(name: &str) -> String {
    if name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.' || c == '$')
    {
        name.to_string()
    } else {
        format!("{name:?}")
    }
}

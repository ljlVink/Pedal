//! Panda ISA 指令元数据：由 `isa_gen.rs` 静态表提供，本模块定义类型与查找
//!
//! @author ljlVink

use std::collections::HashMap;
use std::sync::LazyLock;

/// 前缀字节（与 isa.yaml `prefixes` 一致）
pub const PREFIX_THROW: u8 = 0xfe;
/// 宽操作数前缀
pub const PREFIX_WIDE: u8 = 0xfd;
/// 已废弃指令前缀
pub const PREFIX_DEPRECATED: u8 = 0xfc;
/// 运行时调用前缀
pub const PREFIX_CALLRUNTIME: u8 = 0xfb;

/// 指令格式名（对应 runtime_core 格式定义）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InsFormat {
    /// 无操作数
    None,
    /// 具名格式（解码时按字段表处理）
    Named,
}

impl InsFormat {
    /// 名字到枚举（当前所有带操作数格式统一为 Named，字段由表描述）
    pub const fn from_name(_: &str) -> Self {
        InsFormat::Named
    }
}

/// 操作数角色
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperandKind {
    /// 寄存器 v
    V,
    /// 立即数 imm
    Imm,
    /// 常量池/区域索引 id
    Id,
}

/// 指令内一个编码字段的位布局
#[derive(Debug)]
pub struct OperandField {
    /// 字段名（v8/v16/imm/id…）
    pub name: &'static str,
    /// 相对指令起始的比特偏移（含操作码字节/前缀）
    pub bit_off: u32,
    pub bits: u32,
}

/// 静态指令描述
#[derive(Debug)]
pub struct InsStatic {
    /// 助记符（不含前缀点号）
    pub mnemonic: &'static str,
    /// 前缀字节
    pub prefix: Option<u8>,
    /// 操作码字节
    pub opcode: u8,
    /// 指令总字节数
    pub size: u8,
    /// 完整 sig（含操作数声明）
    pub sig: &'static str,
    /// 格式
    pub format: InsFormat,
    /// 编码字段表
    pub fields: &'static [OperandField],
    /// sig 声明的操作数（名字，声明）
    pub sig_ops: &'static [(&'static str, &'static str)],
    /// 是否为跳转（含条件跳转）
    pub jump: bool,
    /// 读累加器
    pub acc_read: bool,
    /// 写累加器
    pub acc_write: bool,
}

static TABLE: LazyLock<HashMap<(u8, u8), &'static InsStatic>> = LazyLock::new(|| {
    crate::isa_gen::INSTRUCTIONS
        .iter()
        .map(|i| ((i.prefix.unwrap_or(0), i.opcode), i))
        .collect()
});
/// 0.1.0.x（旧编号）ISA 覆盖表：由 scripts/gen_isa_legacy.py 从语料学习并用
/// 平铺覆盖率验证（47.2% → 90.4%）。旧文件的操作码编号与现代表不同，
/// 命中此表时完全替代现代表条目。
static LEGACY_TABLE: LazyLock<HashMap<u8, &'static InsStatic>> =
    LazyLock::new(|| LEGACY_INSTRUCTIONS.iter().map(|i| (i.opcode, i)).collect());

/// 旧编号指令（格式字段为保守推断：imm8 起始、其余并入 imm）
static LEGACY_INSTRUCTIONS: &[InsStatic] = &[
    legacy_ins(0x21, "op21", 3, &[]),
    legacy_ins(0x7a, "op7a", 4, &[]),
    legacy_ins(0x7d, "op7d", 4, &[]),
    legacy_ins(0x7f, "ldthisbyname", 4, &[("imm", "u8"), ("string_id", "")]),
    legacy_ins(0x84, "op84", 8, &[]),
    legacy_ins(0x85, "op85", 8, &[]),
    legacy_ins(0x88, "stthisbyname", 4, &[("imm", "u8"), ("string_id", "")]),
    legacy_ins(0x8b, "op8b", 5, &[]),
    legacy_ins(0x8f, "throw", 1, &[]),
    legacy_ins(0x91, "return", 1, &[]),
    legacy_ins(0x92, "returnundefined", 1, &[]),
    legacy_ins(0x93, "op93", 2, &[]),
    legacy_ins(0xaa, "opaa", 3, &[]),
];

/// 构造旧编号 InsStatic（无前缀；字段布局 imm8/id16 由 size 推断）
const fn legacy_ins(
    opcode: u8,
    mnemonic: &'static str,
    size: u8,
    sig_ops: &'static [(&'static str, &'static str)],
) -> InsStatic {
    InsStatic {
        mnemonic,
        prefix: None,
        opcode,
        size,
        sig: mnemonic,
        format: InsFormat::Named,
        fields: legacy_fields(size, sig_ops),
        sig_ops,
        jump: false,
        acc_read: true,
        acc_write: true,
    }
}

/// 由 size 与 sig 推断字段布局（const 上下文受限，改为固定两档）
const fn legacy_fields(
    size: u8,
    sig_ops: &'static [(&'static str, &'static str)],
) -> &'static [OperandField] {
    // 4 字节 imm8 + id16；其他布局给一个整段 imm
    if size == 4 && sig_ops.len() == 2 {
        LEGACY_F_IMM8_ID16
    } else if size >= 2 {
        match size {
            2 => LEGACY_F_IMM8,
            3 => LEGACY_F_IMM16,
            5 => LEGACY_F_IMM32,
            _ => LEGACY_F_NONE,
        }
    } else {
        LEGACY_F_NONE
    }
}

static LEGACY_F_NONE: &[OperandField] = &[];
static LEGACY_F_IMM8: &[OperandField] = &[OperandField {
    name: "imm",
    bit_off: 8,
    bits: 8,
}];
static LEGACY_F_IMM16: &[OperandField] = &[OperandField {
    name: "imm",
    bit_off: 8,
    bits: 16,
}];
static LEGACY_F_IMM32: &[OperandField] = &[OperandField {
    name: "imm",
    bit_off: 8,
    bits: 32,
}];
static LEGACY_F_IMM8_ID16: &[OperandField] = &[
    OperandField {
        name: "imm",
        bit_off: 8,
        bits: 8,
    },
    OperandField {
        name: "id",
        bit_off: 16,
        bits: 16,
    },
];

/// 旧编号查找（无前缀形式）
pub fn lookup_legacy(opcode: u8) -> Option<&'static InsStatic> {
    LEGACY_TABLE.get(&opcode).copied()
}

/// 按前缀 + 操作码查找指令定义
pub fn lookup(prefix: Option<u8>, opcode: u8) -> Option<&'static InsStatic> {
    TABLE.get(&(prefix.unwrap_or(0), opcode)).copied()
}

/// 判断字节是否为前缀字节
pub fn is_prefix(b: u8) -> bool {
    matches!(
        b,
        PREFIX_THROW | PREFIX_WIDE | PREFIX_DEPRECATED | PREFIX_CALLRUNTIME
    )
}

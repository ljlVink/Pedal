//! 文本 / Hex / ArkTS 反汇编视图切换
//!
//! @author sky
//! @author ljlVink

/// 活跃视图
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ActiveView {
    Decompiled,
    Hex,
    /// ArkTS 反汇编视图（.ets tab 专属）
    Disasm,
}

impl ActiveView {
    /// ArkTS tab 的视图循环：源码 ↔ 反汇编
    pub fn next_ark(self) -> Self {
        match self {
            Self::Disasm => Self::Decompiled,
            _ => Self::Disasm,
        }
    }
}

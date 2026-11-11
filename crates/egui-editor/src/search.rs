//! 文本匹配区间
//!
//! @author sky

/// 查找匹配项（字节偏移）
#[derive(Clone)]
pub struct FindMatch {
    /// 起始字节偏移
    pub start: usize,
    /// 结束字节偏移
    pub end: usize,
}

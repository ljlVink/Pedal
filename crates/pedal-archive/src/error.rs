//! 统一错误类型
//!
//! @author sky
//! @author ljlVink

/// 归档 crate 统一错误类型
#[derive(Debug)]
pub enum ArchiveError {
    /// IO 错误
    Io(std::io::Error),
    /// 归档解析错误
    Parse(String),
}

impl std::fmt::Display for ArchiveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "{e}"),
            Self::Parse(msg) => write!(f, "Parse error: {msg}"),
        }
    }
}

impl std::error::Error for ArchiveError {}

impl ArchiveError {
    /// 快捷构造 Parse 变体
    pub fn parse(e: impl std::fmt::Display) -> Self {
        Self::Parse(e.to_string())
    }
}

impl From<std::io::Error> for ArchiveError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

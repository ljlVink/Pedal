//! 错误类型：覆盖容器解析、指令解码、反编译各阶段
//!
//! @author ljlVink

use std::fmt;

/// ABC 解析 / 反编译错误
#[derive(Debug)]
pub enum AbcError {
    /// 输入不是合法的 PANDA abc 文件
    NotAnAbcFile,
    /// 不支持的 abc 版本（四个字节版本号）
    UnsupportedVersion([u8; 4]),
    /// 偏移越界（位置，剩余长度）
    OutOfBounds {
        /// 请求的读取位置
        offset: usize,
        /// 实际数据长度
        len: usize,
        /// 附加说明
        what: &'static str,
    },
    /// 非法 uleb/sleb 编码
    BadLeb128,
    /// 字符串非正常终止
    UnterminatedString,
    /// 未知指令操作码
    UnknownOpcode {
        /// 前缀字节（throw/wide/deprecated/callruntime）
        prefix: Option<u8>,
        /// 操作码字节
        opcode: u8,
        /// 出现位置
        pc: usize,
    },
    /// TaggedValue 标签未知
    UnknownTag {
        /// 标签值
        tag: u8,
        /// 上下文（class/field/method/literal…）
        what: &'static str,
    },
    /// 索引超出区域范围
    IndexOutOfRange {
        /// 索引
        index: usize,
        /// 区域长度
        len: usize,
    },
    /// zip（HAP/HSP）读取失败
    Zip(String),
    /// IO 失败
    Io(String),
}

impl fmt::Display for AbcError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AbcError::NotAnAbcFile => write!(f, "not a PANDA abc file"),
            AbcError::UnsupportedVersion(v) => write!(
                f,
                "unsupported abc version {}.{}.{}.{}",
                v[0], v[1], v[2], v[3]
            ),
            AbcError::OutOfBounds { offset, len, what } => {
                write!(
                    f,
                    "read out of bounds at {offset:#x} (len {len:#x}) while {what}"
                )
            }
            AbcError::BadLeb128 => write!(f, "malformed leb128"),
            AbcError::UnterminatedString => write!(f, "unterminated mutf-8 string"),
            AbcError::UnknownOpcode { prefix, opcode, pc } => {
                let p = prefix.map(|b| format!("{b:#04x} ")).unwrap_or_default();
                write!(f, "unknown opcode {p}{opcode:#04x} at pc {pc:#x}")
            }
            AbcError::UnknownTag { tag, what } => {
                write!(f, "unknown {what} tag {tag:#04x}")
            }
            AbcError::IndexOutOfRange { index, len } => {
                write!(f, "index {index} out of range (len {len})")
            }
            AbcError::Zip(e) => write!(f, "zip error: {e}"),
            AbcError::Io(e) => write!(f, "io error: {e}"),
        }
    }
}

impl std::error::Error for AbcError {}

impl From<std::io::Error> for AbcError {
    fn from(e: std::io::Error) -> Self {
        AbcError::Io(e.to_string())
    }
}

/// 模块内通用结果类型
pub type AbcResult<T> = Result<T, AbcError>;

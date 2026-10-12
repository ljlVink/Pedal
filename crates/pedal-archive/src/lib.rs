//! 通用归档 crate：ZIP/JAR 读写、修改追踪与虚拟归档构建
//!
//! 无 UI 依赖；公开操作均返回 `Result<T, ArchiveError>`。
//!
//! @author sky
//! @author ljlVink

pub mod error;
pub mod jar;

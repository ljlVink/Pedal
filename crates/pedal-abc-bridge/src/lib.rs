//! Ark 字节码 bridge：ABC 容器解析、指令反汇编、伪 ArkTS 反编译、HAP/HSP 包读取
//!
//! 不依赖 UI；所有对外操作返回 `Result<T, AbcError>`。
//!
//! @author ljlVink

pub mod decompile;
pub mod disasm;
pub mod error;
pub mod file;
pub mod hap;
pub mod isa;
mod isa_gen;
pub mod reader;

#[cfg(test)]
#[path = "../tests/common/mod.rs"]
mod test_support;

pub use error::AbcError;

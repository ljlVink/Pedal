//! PANDA 调试信息：参数名与行号程序中的局部变量生命周期
//!
//! @author ljlVink

use std::collections::HashMap;

use super::{AbcFile, MethodInfo};
use crate::error::{AbcError, AbcResult};
use crate::reader::Reader;

/// 调试信息中的一个局部变量；结束地址不包含在作用域内。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalVariable {
    /// 物理寄存器；负值（例如累加器）不会映射为普通局部变量。
    pub register: i32,
    /// 编译时保存的名字。
    pub name: String,
    /// 调试信息保存的类型名称；空偏移表示没有类型信息。
    pub type_name: Option<String>,
    /// 扩展局部变量记录保存的类型签名；普通记录或空偏移表示没有签名。
    pub signature: Option<String>,
    /// 作用域起始字节码地址。
    pub start_pc: u32,
    /// 作用域结束字节码地址。
    pub end_pc: u32,
}

/// 可选调试名字；发布构建可能完全没有名字信息。
#[derive(Debug, Clone, Default)]
pub struct DebugInfo {
    /// 声明顺序中的参数名；空偏移表示未知名称。
    pub parameter_names: Vec<Option<String>>,
    /// 行号程序记录的局部变量作用域。
    pub locals: Vec<LocalVariable>,
}

impl AbcFile {
    /// 读取方法调试信息；不存在调试信息时返回 `None`。
    ///
    /// PANDA 把行号操作码/寄存器与常量池操作数放在两个独立字节流中，
    /// 不能按 DEX 的单流格式读取。格式参考 OpenHarmony libpandafile。
    pub fn read_debug_info(&self, method: &MethodInfo) -> AbcResult<Option<DebugInfo>> {
        if method.debug_info_off == 0 {
            return Ok(None);
        }
        let mut r = Reader::new(self.bytes());
        r.seek(method.debug_info_off as usize)?;
        let _line_start = u32_leb(&mut r)?;
        let num_params = u32_leb(&mut r)? as usize;
        if num_params > r.remaining() {
            return Err(invalid("debug parameter count"));
        }
        let mut parameter_names = Vec::with_capacity(num_params);
        for _ in 0..num_params {
            let off = u32_leb(&mut r)?;
            parameter_names.push(if off == 0 {
                None
            } else {
                Some(self.string_at(off)?)
            });
        }
        let pool_size = u32_leb(&mut r)? as usize;
        let pool_start = r.pos();
        r.skip(pool_size)?;
        let pool = &self.bytes()[pool_start..r.pos()];
        let index = u32_leb(&mut r)?;
        if index >= self.header.num_lnps || self.header.lnp_idx_off == 0 {
            return Err(AbcError::IndexOutOfRange {
                index: index as usize,
                len: self.header.num_lnps as usize,
            });
        }
        let table_entry = (self.header.lnp_idx_off as usize)
            .checked_add(index as usize * 4)
            .ok_or_else(|| invalid("debug program index"))?;
        let program_off = r.u32_at(table_entry)?;
        r.seek(program_off as usize)?;
        let code_size = method.code.as_ref().map_or(0, |c| c.code_size);
        let locals = read_program(self, &mut r, pool, code_size)?;
        Ok(Some(DebugInfo {
            parameter_names,
            locals,
        }))
    }
}

fn invalid(what: &'static str) -> AbcError {
    AbcError::UnknownTag { tag: 0, what }
}

fn u32_leb(r: &mut Reader<'_>) -> AbcResult<u32> {
    u32::try_from(r.uleb()?).map_err(|_| AbcError::BadLeb128)
}

fn register(r: &mut Reader<'_>) -> AbcResult<i32> {
    i32::try_from(r.sleb()?).map_err(|_| AbcError::BadLeb128)
}

fn advance(pc: &mut u32, amount: u32, code_size: u32) -> AbcResult<()> {
    *pc = pc
        .checked_add(amount)
        .filter(|p| *p <= code_size)
        .ok_or_else(|| invalid("debug program address"))?;
    Ok(())
}

fn read_program(
    file: &AbcFile,
    program: &mut Reader<'_>,
    pool: &[u8],
    code_size: u32,
) -> AbcResult<Vec<LocalVariable>> {
    let mut constants = Reader::new(pool);
    let mut locals: Vec<LocalVariable> = Vec::new();
    let mut active = HashMap::<i32, usize>::new();
    let mut previous = HashMap::<i32, usize>::new();
    let mut pc = 0;
    loop {
        match program.u8()? {
            0 => break,
            1 => advance(&mut pc, u32_leb(&mut constants)?, code_size)?,
            2 => {
                constants.sleb()?;
            }
            op @ (3 | 4) => {
                let register = register(program)?;
                let name_off = u32_leb(&mut constants)?;
                let type_off = u32_leb(&mut constants)?;
                let signature_off = if op == 4 { u32_leb(&mut constants)? } else { 0 };
                if let Some(old) = active.remove(&register) {
                    locals[old].end_pc = pc;
                }
                let index = locals.len();
                locals.push(LocalVariable {
                    register,
                    name: if name_off == 0 {
                        String::new()
                    } else {
                        file.string_at(name_off)?
                    },
                    type_name: (type_off != 0)
                        .then(|| file.string_at(type_off))
                        .transpose()?,
                    signature: (signature_off != 0)
                        .then(|| file.string_at(signature_off))
                        .transpose()?,
                    start_pc: pc,
                    end_pc: code_size,
                });
                active.insert(register, index);
                previous.insert(register, index);
            }
            5 => {
                let register = register(program)?;
                let index = active
                    .remove(&register)
                    .ok_or_else(|| invalid("debug END_LOCAL without START_LOCAL"))?;
                locals[index].end_pc = pc;
            }
            6 => {
                let register = register(program)?;
                if active.contains_key(&register) {
                    return Err(invalid("debug RESTART_LOCAL while active"));
                }
                let old = *previous
                    .get(&register)
                    .ok_or_else(|| invalid("debug RESTART_LOCAL without START_LOCAL"))?;
                let index = locals.len();
                locals.push(LocalVariable {
                    start_pc: pc,
                    end_pc: code_size,
                    ..locals[old].clone()
                });
                active.insert(register, index);
                previous.insert(register, index);
            }
            7 | 8 => {}
            9..=11 => {
                u32_leb(&mut constants)?;
            }
            special => advance(&mut pc, u32::from(special - 12) / 15, code_size)?,
        }
    }
    Ok(locals)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::file::{CodeInfo, FileHeader, FunctionKind, HeaderKind};

    fn uleb(mut value: u32, bytes: &mut Vec<u8>) {
        loop {
            let mut byte = (value & 127) as u8;
            value >>= 7;
            if value != 0 {
                byte |= 128;
            }
            bytes.push(byte);
            if value == 0 {
                return;
            }
        }
    }

    pub(crate) fn fixture(program: &[u8], pool: &[u8]) -> (AbcFile, MethodInfo) {
        let mut data = vec![0; 80];
        let name_off = data.len() as u32;
        data.extend_from_slice(&[11, b't', b'o', b't', b'a', b'l', 0]);
        for text in ["number", "Array<string>"] {
            uleb((text.len() as u32) * 2 + 1, &mut data);
            data.extend_from_slice(text.as_bytes());
            data.push(0);
        }
        let debug_info_off = data.len() as u32;
        data.extend_from_slice(&[1, 1]);
        uleb(name_off, &mut data);
        uleb(pool.len() as u32, &mut data);
        data.extend_from_slice(pool);
        data.push(0);
        let program_off = data.len() as u32;
        data[64..68].copy_from_slice(&program_off.to_le_bytes());
        data.extend_from_slice(program);
        let file = AbcFile {
            header: FileHeader {
                version: [12, 0, 6, 0],
                file_size: data.len() as u32,
                foreign_off: 0,
                foreign_size: 0,
                num_classes: 0,
                class_idx_off: 0,
                num_lnps: 1,
                lnp_idx_off: 64,
                num_literalarrays: 0,
                literalarray_idx_off: 0,
                num_index_regions: 0,
                index_section_off: 0,
                kind: HeaderKind::Modern,
            },
            data,
            class_index: vec![],
            index_headers: vec![],
            classes: vec![],
        };
        let method = MethodInfo {
            offset: 0,
            class_off: 0,
            proto_off: 0,
            name: "fixture".into(),
            header_index: 0,
            kind: FunctionKind::Function,
            code: Some(CodeInfo {
                offset: 0,
                num_vregs: 2,
                num_args: 4,
                code_size: 12,
                ins_off: 0,
                try_blocks: vec![],
            }),
            debug_info_off,
            foreign: false,
        };
        (file, method)
    }

    #[test]
    fn debug_program_uses_separate_operand_streams() {
        let (file, method) = fixture(&[9, 3, 1, 1, 5, 1, 0], &[0, 80, 0, 7]);
        let info = file.read_debug_info(&method).unwrap().unwrap();
        assert_eq!(info.parameter_names, vec![Some("total".into())]);
        assert_eq!(
            info.locals,
            vec![LocalVariable {
                register: 1,
                name: "total".into(),
                type_name: None,
                signature: None,
                start_pc: 0,
                end_pc: 7
            }]
        );
    }

    #[test]
    fn debug_extended_restart_and_special_address() {
        let (file, method) = fixture(&[4, 0x7f, 46, 5, 0x7f, 6, 0x7f, 0], &[80, 87, 95]);
        let info = file.read_debug_info(&method).unwrap().unwrap();
        assert_eq!(info.locals.len(), 2);
        assert_eq!(info.locals[0].register, -1);
        assert_eq!((info.locals[0].start_pc, info.locals[0].end_pc), (0, 2));
        assert_eq!((info.locals[1].start_pc, info.locals[1].end_pc), (2, 12));
        for local in &info.locals {
            assert_eq!(local.type_name.as_deref(), Some("number"));
            assert_eq!(local.signature.as_deref(), Some("Array<string>"));
        }
    }

    #[test]
    fn debug_local_type_and_absent_extended_signature() {
        let (file, method) = fixture(&[3, 0, 4, 1, 0], &[80, 87, 80, 0, 0]);
        let info = file.read_debug_info(&method).unwrap().unwrap();
        assert_eq!(info.locals[0].type_name.as_deref(), Some("number"));
        assert_eq!(info.locals[0].signature, None);
        assert_eq!(info.locals[1].type_name, None);
        assert_eq!(info.locals[1].signature, None);
    }

    #[test]
    fn invalid_debug_type_and_signature_string_offsets_are_rejected() {
        for operands in [[80, u32::MAX, 0], [80, 87, u32::MAX]] {
            let mut pool = Vec::new();
            for offset in operands {
                uleb(offset, &mut pool);
            }
            let (file, method) = fixture(&[4, 0, 0], &pool);
            assert!(file.read_debug_info(&method).is_err());
        }
        let mut pool = vec![80];
        uleb(u32::MAX, &mut pool);
        let (file, method) = fixture(&[3, 0, 0], &pool);
        assert!(file.read_debug_info(&method).is_err());
    }

    #[test]
    fn missing_debug_is_distinct_from_invalid_debug() {
        let (file, mut method) = fixture(&[3, 0, 0], &[]);
        assert!(file.read_debug_info(&method).is_err());
        method.debug_info_off = 0;
        assert!(file.read_debug_info(&method).unwrap().is_none());
    }

    #[test]
    fn truncated_and_out_of_code_programs_are_rejected() {
        for (program, pool) in [(&[3, 0][..], &[80, 0][..]), (&[1, 0], &[13])] {
            let (file, method) = fixture(program, pool);
            assert!(file.read_debug_info(&method).is_err());
        }
    }
}

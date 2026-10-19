//! 方法原型索引、shorty 与引用类型解析。
//!
//! @author ljlVink

use super::{AbcFile, BasicType, HeaderKind, ProtoInfo, Type};
use crate::error::{AbcError, AbcResult};
use crate::reader::Reader;

impl AbcFile {
    pub(super) fn resolve_method_proto(&self, method_off: u32, index: u32) -> AbcResult<u32> {
        // 动态运行时允许省略 proto；静态运行时的同一索引必须解析。
        if self.header.kind == HeaderKind::Modern && index == u16::MAX as u32 {
            return Ok(0);
        }
        let region = self
            .region_of(method_off)
            .ok_or(AbcError::IndexOutOfRange {
                index: method_off as usize,
                len: self.index_headers.len(),
            })?;
        let off = self.region_u32(region.proto_idx_off, region.proto_idx_size, index)?;
        self.check_proto_offset(off, "method proto")?;
        Ok(off)
    }

    fn check_proto_offset(&self, off: u32, what: &'static str) -> AbcResult<()> {
        if off < 64 || off as usize >= self.data.len() {
            return Err(AbcError::OutOfBounds {
                offset: off as usize,
                len: self.data.len(),
                what,
            });
        }
        Ok(())
    }

    /// 解析真实方法签名；参数不包含静态运行时实例方法的隐式 this。
    pub fn read_proto(&self, off: u32) -> AbcResult<ProtoInfo> {
        self.check_proto_offset(off, "proto shorty")?;
        let mut reader = Reader::new(&self.data);
        reader.seek(off as usize)?;
        let mut types = Vec::new();
        // 每个 u16 从低位开始容纳四个类型码；终止码所在整个 u16 都属于 shorty。
        'shorty: loop {
            let word = reader.u16()?;
            for shift in [0, 4, 8, 12] {
                let code = ((word >> shift) & 0xf) as u8;
                if code == 0 {
                    break 'shorty;
                }
                let ty = self.shorty_type(code)?;
                if !types.is_empty() && ty == Type::Basic(BasicType::Void) {
                    return Err(AbcError::UnknownTag {
                        tag: code,
                        what: "void proto parameter",
                    });
                }
                types.push(ty);
            }
        }
        if types.is_empty() {
            return Err(AbcError::UnknownTag {
                tag: 0,
                what: "missing proto return type",
            });
        }

        let mut ref_types = Vec::new();
        for ty in &mut types {
            if matches!(ty, Type::Ref(_)) {
                let index = reader.u16()? as u32;
                let resolved = self.resolve_type(off, index)?;
                let Type::Ref(class_off) = resolved else {
                    return Err(AbcError::UnknownTag {
                        tag: index as u8,
                        what: "primitive proto reference",
                    });
                };
                self.check_proto_offset(class_off, "proto reference class")?;
                *ty = resolved;
                ref_types.push(class_off);
            }
        }
        let ret = types.remove(0);
        Ok(ProtoInfo {
            ret,
            params: types,
            ref_types,
        })
    }

    fn shorty_type(&self, code: u8) -> AbcResult<Type> {
        // shorty 与字段类型的编码相差两位，不能复用 basic_from_code。
        // static_core/libarkfile/types.yaml 的 tagged/reference 顺序与动态版相反。
        let basic = match code {
            0x01 => BasicType::Void,
            0x02 => BasicType::U1,
            0x03 => BasicType::I8,
            0x04 => BasicType::U8,
            0x05 => BasicType::I16,
            0x06 => BasicType::U16,
            0x07 => BasicType::I32,
            0x08 => BasicType::U32,
            0x09 => BasicType::F32,
            0x0a => BasicType::F64,
            0x0b => BasicType::I64,
            0x0c => BasicType::U64,
            0x0d if self.header.kind == HeaderKind::Modern => return Ok(Type::Ref(0)),
            0x0e if self.header.kind == HeaderKind::Legacy => return Ok(Type::Ref(0)),
            0x0d | 0x0e => BasicType::Any,
            other => {
                return Err(AbcError::UnknownTag {
                    tag: other,
                    what: "proto shorty",
                });
            }
        };
        Ok(Type::Basic(basic))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::file::{FileHeader, IndexHeader};

    const PROTO: u32 = 128;
    const METHOD: u32 = 176;

    fn fixture(kind: HeaderKind, codes: &[u8], references: &[u16]) -> AbcFile {
        let mut data = vec![0; 384];
        for (index, offset) in [256u32, 288, 5].into_iter().enumerate() {
            data[64 + index * 4..68 + index * 4].copy_from_slice(&offset.to_le_bytes());
        }
        data[80..84].copy_from_slice(&PROTO.to_le_bytes());
        let mut shorty = codes.to_vec();
        shorty.push(0);
        let mut cursor = PROTO as usize;
        for chunk in shorty.chunks(4) {
            let word = chunk
                .iter()
                .enumerate()
                .fold(0u16, |word, (i, &code)| word | ((code as u16) << (4 * i)));
            data[cursor..cursor + 2].copy_from_slice(&word.to_le_bytes());
            cursor += 2;
        }
        for index in references {
            data[cursor..cursor + 2].copy_from_slice(&index.to_le_bytes());
            cursor += 2;
        }
        for (offset, text) in [(256, "Ldemo/Result;"), (288, "Ldemo/Input;"), (320, "test")] {
            data[offset] = (text.len() * 2 + 1) as u8;
            data[offset + 1..offset + text.len() + 1].copy_from_slice(text.as_bytes());
        }
        data[METHOD as usize + 4..METHOD as usize + 8].copy_from_slice(&320u32.to_le_bytes());
        let mut header = FileHeader {
            version: [12, 0, 6, 0],
            file_size: data.len() as u32,
            foreign_off: 0,
            foreign_size: 0,
            num_classes: 0,
            class_idx_off: 0,
            num_lnps: 0,
            lnp_idx_off: 0,
            num_literalarrays: 0,
            literalarray_idx_off: 0,
            num_index_regions: 1,
            index_section_off: 0,
            kind,
        };
        if kind == HeaderKind::Legacy {
            header.version = [0, 1, 0, 7];
        }
        AbcFile {
            data,
            header,
            class_index: Vec::new(),
            index_headers: vec![IndexHeader {
                start: 64,
                end: 384,
                class_idx_size: 3,
                class_idx_off: 64,
                method_idx_size: 0,
                method_idx_off: 0,
                field_idx_size: 0,
                field_idx_off: 0,
                proto_idx_size: 1,
                proto_idx_off: 80,
            }],
            classes: Vec::new(),
        }
    }

    #[test]
    fn shorty_primitives_are_distinct_from_field_type_codes() {
        let file = fixture(HeaderKind::Modern, &(1..=12).collect::<Vec<_>>(), &[]);
        let proto = file.read_proto(PROTO).unwrap();
        assert_eq!(proto.ret, Type::Basic(BasicType::Void));
        assert_eq!(
            proto.params,
            [
                BasicType::U1,
                BasicType::I8,
                BasicType::U8,
                BasicType::I16,
                BasicType::U16,
                BasicType::I32,
                BasicType::U32,
                BasicType::F32,
                BasicType::F64,
                BasicType::I64,
                BasicType::U64,
            ]
            .map(Type::Basic)
        );
        assert!(proto.ref_types.is_empty());
        assert_eq!(super::super::type_from_u32(0), Type::Basic(BasicType::U1));
    }

    #[test]
    fn references_include_return_value_and_use_generation_specific_codes() {
        for (kind, reference, any) in [
            (HeaderKind::Modern, 0xd, 0xe),
            (HeaderKind::Legacy, 0xe, 0xd),
        ] {
            // 四个类型恰好填满一个 word，终止 word 之后才开始引用索引。
            let file = fixture(kind, &[reference, 7, reference, any], &[0, 1]);
            let proto = file.read_proto(PROTO).unwrap();
            assert_eq!(proto.ret, Type::Ref(256));
            assert_eq!(
                proto.params,
                [
                    Type::Basic(BasicType::I32),
                    Type::Ref(288),
                    Type::Basic(BasicType::Any)
                ]
            );
            assert_eq!(proto.ref_types, [256, 288]);
        }
    }

    #[test]
    fn method_resolves_proto_and_declaring_class() {
        for kind in [HeaderKind::Legacy, HeaderKind::Modern] {
            let file = fixture(kind, &[1, 7], &[]);
            let method = file.method_at(METHOD).unwrap();
            assert_eq!(method.class_off, 256);
            assert_eq!(method.proto_off, PROTO);
            assert_eq!(file.read_proto(method.proto_off).unwrap().params.len(), 1);
        }
    }

    #[test]
    fn only_dynamic_methods_accept_absent_proto_index() {
        for kind in [HeaderKind::Legacy, HeaderKind::Modern] {
            let mut file = fixture(kind, &[1], &[]);
            file.data[METHOD as usize + 2..METHOD as usize + 4]
                .copy_from_slice(&u16::MAX.to_le_bytes());
            if kind == HeaderKind::Modern {
                assert_eq!(file.method_at(METHOD).unwrap().proto_off, 0);
            } else {
                assert!(matches!(
                    file.method_at(METHOD),
                    Err(AbcError::IndexOutOfRange { .. })
                ));
            }
        }
    }

    #[test]
    fn foreign_method_has_no_tag_stream() {
        let mut file = fixture(HeaderKind::Legacy, &[1, 7], &[]);
        file.header.foreign_off = METHOD;
        file.header.foreign_size = 9;
        // ForeignMethod 的 access_flags 后面是下一个实体，并没有 0 终止标签。
        file.data[METHOD as usize + 9..].fill(0xff);
        // 名称仍需位于有效区域，以免把测试变成字符串错误。
        file.data[320..326].copy_from_slice(&[9, b't', b'e', b's', b't', 0]);
        let method = file.method_at(METHOD).unwrap();
        assert!(method.foreign);
        assert!(method.code.is_none());
        assert_eq!(method.proto_off, PROTO);
    }

    #[test]
    fn proto_references_use_the_proto_region_not_the_method_region() {
        let mut file = fixture(HeaderKind::Modern, &[0xd], &[0]);
        file.index_headers[0].end = METHOD;
        let mut second = file.index_headers[0];
        second.start = METHOD;
        second.end = file.data.len() as u32;
        second.class_idx_off = 68;
        second.class_idx_size = 1;
        file.index_headers.push(second);
        let method = file.method_at(METHOD).unwrap();
        assert_eq!(method.class_off, 288);
        assert_eq!(
            file.read_proto(method.proto_off).unwrap().ret,
            Type::Ref(256)
        );
    }

    #[test]
    fn invalid_shorties_references_and_offsets_return_errors() {
        for codes in [&[][..], &[0xf], &[1, 1]] {
            assert!(
                fixture(HeaderKind::Modern, codes, &[])
                    .read_proto(PROTO)
                    .is_err()
            );
        }
        for index in [2, 3, u16::MAX] {
            assert!(
                fixture(HeaderKind::Modern, &[0xd], &[index])
                    .read_proto(PROTO)
                    .is_err()
            );
        }
        let mut file = fixture(HeaderKind::Modern, &[0xd], &[0]);
        file.data[64..68].copy_from_slice(&999u32.to_le_bytes());
        assert!(file.read_proto(PROTO).is_err());
        for offset in [0, 1, 63, 384, u32::MAX] {
            assert!(file.read_proto(offset).is_err());
        }
        file.data[80..84].copy_from_slice(&0u32.to_le_bytes());
        assert!(file.method_at(METHOD).is_err());
    }

    #[test]
    fn truncated_shorty_and_reference_list_return_errors() {
        for (codes, length) in [(&[1, 2, 3, 4][..], 130), (&[0xd][..], 131)] {
            let mut file = fixture(HeaderKind::Modern, codes, &[0]);
            file.data.truncate(length);
            assert!(file.read_proto(PROTO).is_err());
        }
    }

    #[test]
    fn compiler_dynamic_fixtures_preserve_missing_type_information() {
        for path in [
            "advanced_recovery/debug.abc",
            "advanced_recovery/release.abc",
        ] {
            let bytes = crate::test_support::read_fixture(path);
            let file = AbcFile::parse(bytes).unwrap();
            assert!(!file.classes.is_empty());
            for method in file.classes.iter().flat_map(|class| &class.methods) {
                assert_eq!(method.proto_off, 0, "{}", method.name);
            }
        }
    }
}

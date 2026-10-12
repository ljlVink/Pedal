//! PANDA abc 容器解析：头部、索引区域、record/class、field、method、proto、code、literal
//!
//! 同时支持两类布局：
//! - 现代（≥9.0.0.0，es2abc）：头部 60/64 字节，`index_section_off` 指向连续的
//!   10 字段 `IndexHeader` 数组（method 区域数组同时存放 method/string/literalarray 偏移）。
//! - 旧版（0.1.0.x）：字段顺序不同，索引区结构见 `HeaderKind::Legacy`（布局由
//!   版本探测决定）。
//!
//! @author ljlVink

use crate::error::{AbcError, AbcResult};
use crate::reader::Reader;

/// 文件头（两种布局的并集，按 kind 解释）
#[derive(Debug, Clone)]
pub struct FileHeader {
    /// 四字节版本号
    pub version: [u8; 4],
    /// 文件大小
    pub file_size: u32,
    /// 外部（foreign）区域起点
    pub foreign_off: u32,
    /// 外部区域大小
    pub foreign_size: u32,
    /// 本文件定义的 class（record）数量
    pub num_classes: u32,
    /// class 索引区域偏移
    pub class_idx_off: u32,
    /// 行号程序数量
    pub num_lnps: u32,
    /// 行号程序索引偏移
    pub lnp_idx_off: u32,
    /// literalarray 数量
    pub num_literalarrays: u32,
    /// literalarray 索引偏移
    pub literalarray_idx_off: u32,
    /// 索引区域数量
    pub num_index_regions: u32,
    /// 索引区域表偏移
    pub index_section_off: u32,
    /// 布局种类
    pub kind: HeaderKind,
}

/// 头部/索引布局种类
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeaderKind {
    /// 0.1.0.x：num_lnps 在 class_idx_off 之后，index_section_off 指向偏移表
    Legacy,
    /// ≥9.x 现代布局：连续 IndexHeader 数组
    Modern,
}

/// 索引区域头（10 字段）
#[derive(Debug, Clone, Copy)]
pub struct IndexHeader {
    /// 区域起始偏移
    pub start: u32,
    /// 区域结束偏移
    pub end: u32,
    /// class 区域索引条目数
    pub class_idx_size: u32,
    /// class 区域索引偏移
    pub class_idx_off: u32,
    /// method/string/literalarray 区域索引条目数
    pub method_idx_size: u32,
    /// method/string/literalarray 区域索引偏移
    pub method_idx_off: u32,
    /// field 区域索引条目数
    pub field_idx_size: u32,
    /// field 区域索引偏移
    pub field_idx_off: u32,
    /// proto 区域索引条目数
    pub proto_idx_size: u32,
    /// proto 区域索引偏移
    pub proto_idx_off: u32,
}

/// 基础类型编码（Type u32 < 0x0d 时）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BasicType {
    /// u1
    U1,
    /// i8
    I8,
    /// u8
    U8,
    /// i16
    I16,
    /// u16
    U16,
    /// i32
    I32,
    /// u32
    U32,
    /// f32
    F32,
    /// f64
    F64,
    /// i64
    I64,
    /// u64
    U64,
    /// any（动态类型）
    Any,
}

/// Type：基础类型或 class 引用
#[derive(Debug, Clone, Copy)]
pub enum Type {
    /// 基础类型
    Basic(BasicType),
    /// class 偏移
    Ref(u32),
}

/// 函数种类（MethodIndexData 16..24 位）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FunctionKind {
    /// 普通函数
    Function,
    /// 箭头函数
    NcFunction,
    /// 生成器
    GeneratorFunction,
    /// 异步函数
    AsyncFunction,
    /// 异步生成器
    AsyncGeneratorFunction,
    /// 异步箭头函数
    AsyncNcFunction,
    /// 并发函数（@Concurrent）
    ConcurrentFunction,
    /// 未知值
    Unknown(u8),
}

/// FunctionKind 解码
pub fn function_kind(v: u8) -> FunctionKind {
    match v {
        0x1 => FunctionKind::Function,
        0x2 => FunctionKind::NcFunction,
        0x3 => FunctionKind::GeneratorFunction,
        0x4 => FunctionKind::AsyncFunction,
        0x5 => FunctionKind::AsyncGeneratorFunction,
        0x6 => FunctionKind::AsyncNcFunction,
        0x7 => FunctionKind::ConcurrentFunction,
        other => FunctionKind::Unknown(other),
    }
}

/// catch 块
#[derive(Debug, Clone, Copy)]
pub struct CatchBlock {
    /// 类型索引（0 = 捕获所有）
    pub type_idx: u32,
    /// 处理器首指令 pc
    pub handler_pc: u32,
    /// 处理器代码大小
    pub code_size: u32,
}

/// try 块
#[derive(Debug, Clone)]
pub struct TryBlock {
    /// 起始 pc
    pub start_pc: u32,
    /// 覆盖长度（字节）
    pub length: u32,
    /// 关联 catch 块
    pub catch_blocks: Vec<CatchBlock>,
}

/// 方法体
#[derive(Debug, Clone)]
pub struct CodeInfo {
    /// 偏移
    pub offset: u32,
    /// 寄存器数（不含参数）
    pub num_vregs: u32,
    /// 参数个数
    pub num_args: u32,
    /// 指令字节数
    pub code_size: u32,
    /// 指令区起始偏移（绝对）
    pub ins_off: u32,
    /// try 块
    pub try_blocks: Vec<TryBlock>,
}

impl CodeInfo {
    /// 指令字节切片
    pub fn instructions<'a>(&self, data: &'a [u8]) -> &'a [u8] {
        let end = (self.ins_off as usize + self.code_size as usize).min(data.len());
        &data[self.ins_off as usize..end]
    }
}

/// 方法（含外部方法）
#[derive(Debug, Clone)]
pub struct MethodInfo {
    /// 方法结构偏移
    pub offset: u32,
    /// 所属 class 偏移（0 = 无）
    pub class_off: u32,
    /// proto 偏移（0 = 无）
    pub proto_off: u32,
    /// 方法名
    pub name: String,
    /// 所在索引头（header_index）
    pub header_index: u16,
    /// 函数种类
    pub kind: FunctionKind,
    /// 方法体
    pub code: Option<CodeInfo>,
    /// 调试信息偏移
    pub debug_info_off: u32,
    /// 是否外部方法（foreign 区）
    pub foreign: bool,
}

/// 字段
#[derive(Debug, Clone)]
pub struct FieldInfo {
    /// 字段结构偏移
    pub offset: u32,
    /// 所属 class 偏移
    pub class_off: u32,
    /// 类型
    pub ty: Type,
    /// 字段名
    pub name: String,
    /// INT_VALUE 标签值
    pub int_value: Option<i64>,
    /// VALUE 标签值（FLOAT/ID）
    pub value: Option<u32>,
}

/// class（ES 文件中即 record / 源文件）
#[derive(Debug, Clone)]
pub struct ClassInfo {
    /// class 结构偏移
    pub offset: u32,
    /// record 名（TypeDescriptor 或源文件路径）
    pub name: String,
    /// 访问标记
    pub access_flags: u32,
    /// 源文件名
    pub source_file: Option<String>,
    /// 字段
    pub fields: Vec<FieldInfo>,
    /// 方法
    pub methods: Vec<MethodInfo>,
    /// moduleRecordIdx 字段值（ESM record）
    pub module_record_idx: Option<u32>,
}

/// proto（方法签名）
#[derive(Debug, Clone)]
pub struct ProtoInfo {
    /// 返回类型
    pub ret: Type,
    /// 参数类型
    pub params: Vec<Type>,
    /// 参数引用类型（class 偏移），与 params 中 Ref 一一对应
    pub ref_types: Vec<u32>,
}

/// literal 值
#[derive(Debug, Clone)]
pub enum LiteralValue {
    /// bool / int8（tag 0x00/0x01）
    Bool(bool),
    /// int8
    Int8(i8),
    /// int16
    Int16(i16),
    /// int32
    Int32(i32),
    /// uint32
    Uint32(u32),
    /// uint64
    Uint64(u64),
    /// float
    Float(f32),
    /// double
    Double(f64),
    /// 字符串（文件内偏移）
    String(u32),
    /// 方法偏移
    Method(u32),
    /// 生成器方法偏移
    GeneratorMethod(u32),
    /// 异步生成器方法偏移
    AsyncGeneratorMethod(u32),
    /// accessor（getter/setter 对偏移）
    Accessor(u32),
    /// 方法附属值
    MethodAffiliate(u16),
    /// u1 数组
    ArrayU1(Vec<u8>),
    /// u8 数组
    ArrayU8(Vec<u8>),
    /// i8 数组
    ArrayI8(Vec<i8>),
    /// u16 数组
    ArrayU16(Vec<u16>),
    /// i16 数组
    ArrayI16(Vec<i16>),
    /// u32 数组
    ArrayU32(Vec<u32>),
    /// i32 数组
    ArrayI32(Vec<i32>),
    /// u64 数组
    ArrayU64(Vec<u64>),
    /// i64 数组
    ArrayI64(Vec<i64>),
    /// f32 数组
    ArrayF32(Vec<f32>),
    /// f64 数组
    ArrayF64(Vec<f64>),
    /// 字符串偏移数组
    ArrayString(Vec<u32>),
    /// literalbuffer 索引
    LiteralBufferIndex(u16),
    /// literalarray 偏移
    LiteralArray(u32),
    /// 内建类型索引
    BuiltinTypeIndex(u16),
    /// getter 方法偏移
    Getter(u32),
    /// setter 方法偏移
    Setter(u32),
    /// implements
    EtsImplements(u32),
    /// null 值
    Null,
}

/// ESM 模块 record：import/export 信息
#[derive(Debug, Clone, Default)]
pub struct ModuleRecord {
    /// 模块请求（import 说明符）
    pub requests: Vec<String>,
    /// 具名导入
    pub regular_imports: Vec<RegularImport>,
    /// 命名空间导入（import * as ns）
    pub namespace_imports: Vec<NamespaceImport>,
    /// 本地导出
    pub local_exports: Vec<LocalExport>,
    /// 间接导出（re-export）
    pub indirect_exports: Vec<IndirectExport>,
    /// star 导出（export * from）
    pub star_exports: Vec<u16>,
}

/// 具名导入：import { import_name as local_name } from requests[module_idx]
#[derive(Debug, Clone)]
pub struct RegularImport {
    /// 本地绑定名
    pub local_name: String,
    /// 导出名
    pub import_name: String,
    /// 模块请求下标
    pub module_idx: u16,
}

/// 命名空间导入：import * as local_name from requests[module_idx]
#[derive(Debug, Clone)]
pub struct NamespaceImport {
    /// 本地绑定名
    pub local_name: String,
    /// 模块请求下标
    pub module_idx: u16,
}

/// 本地导出：export { local_name as export_name }
#[derive(Debug, Clone)]
pub struct LocalExport {
    /// 本地绑定名
    pub local_name: String,
    /// 导出名
    pub export_name: String,
}

/// 间接导出：export { import_name as export_name } from requests[module_idx]
#[derive(Debug, Clone)]
pub struct IndirectExport {
    /// 导出名
    pub export_name: String,
    /// 来源模块导入名
    pub import_name: String,
    /// 模块请求下标
    pub module_idx: u16,
}

/// 完整 abc 文件
pub struct AbcFile {
    /// 原始字节
    pub data: Vec<u8>,
    /// 头部
    pub header: FileHeader,
    /// class 索引（偏移数组，含 foreign 区元素）
    pub class_index: Vec<u32>,
    /// 索引区域头
    pub index_headers: Vec<IndexHeader>,
    /// 本文件定义的 class（record）
    pub classes: Vec<ClassInfo>,
}

impl AbcFile {
    /// 底层字节
    pub fn bytes(&self) -> &[u8] {
        &self.data
    }

    /// 解析 abc 字节
    pub fn parse(data: Vec<u8>) -> AbcResult<Self> {
        if data.len() < 64 || &data[..8] != b"PANDA\0\0\0" {
            return Err(AbcError::NotAnAbcFile);
        }
        let mut r = Reader::new(&data);
        r.seek(12)?;
        let version = [r.u8()?, r.u8()?, r.u8()?, r.u8()?];
        let kind = if version[0] == 0 {
            HeaderKind::Legacy
        } else {
            HeaderKind::Modern
        };
        let header = match kind {
            HeaderKind::Modern => {
                let file_size = r.u32()?;
                let foreign_off = r.u32()?;
                let foreign_size = r.u32()?;
                let num_classes = r.u32()?;
                let class_idx_off = r.u32()?;
                let num_lnps = r.u32()?;
                let lnp_idx_off = r.u32()?;
                let num_literalarrays = r.u32()?;
                let literalarray_idx_off = r.u32()?;
                let num_index_regions = r.u32()?;
                let index_section_off = r.u32()?;
                FileHeader {
                    version,
                    file_size,
                    foreign_off,
                    foreign_size,
                    num_classes,
                    class_idx_off,
                    num_lnps,
                    lnp_idx_off,
                    num_literalarrays,
                    literalarray_idx_off,
                    num_index_regions,
                    index_section_off,
                    kind,
                }
            }
            HeaderKind::Legacy => {
                // 0.1.0.x（72 字节头，LayoutScout 考古结论）：
                // @0x1C reserved、@0x20 num_classes（foreign+local）、@0x24 class_idx_off(=0x48)、
                // @0x28 module_idx 字节大小、@0x2C module_idx_off、
                // @0x30 num_literalarrays、@0x34 literalarray_idx_off（至文件尾）、
                // @0x38 num_lnps、@0x3C lnp_idx_off、@0x40 num_indexes、@0x44 index_section_off
                let file_size = r.u32()?;
                let foreign_off = r.u32()?;
                let foreign_size = r.u32()?;
                let _reserved = r.u32()?;
                let num_classes = r.u32()?;
                let class_idx_off = r.u32()?;
                let _module_idx_size = r.u32()?;
                let _module_idx_off = r.u32()?;
                let num_literalarrays = r.u32()?;
                let literalarray_idx_off = r.u32()?;
                let num_lnps = r.u32()?;
                let _lnp_idx_off = r.u32()?;
                let num_index_regions = r.u32()?;
                let index_section_off = r.u32()?;
                FileHeader {
                    version,
                    file_size,
                    foreign_off,
                    foreign_size,
                    num_classes,
                    class_idx_off,
                    num_lnps,
                    lnp_idx_off: 0,
                    num_literalarrays,
                    literalarray_idx_off,
                    num_index_regions,
                    index_section_off,
                    kind,
                }
            }
        };
        let mut file = AbcFile {
            class_index: Vec::new(),
            index_headers: Vec::new(),
            classes: Vec::new(),
            data,
            header,
        };
        file.read_class_index()?;
        file.read_index_headers()?;
        file.read_classes()?;
        Ok(file)
    }

    fn read_class_index(&mut self) -> AbcResult<()> {
        if self.header.class_idx_off == 0 || self.header.num_classes == 0 {
            return Ok(());
        }
        let n = self.header.num_classes as usize;
        let off = self.header.class_idx_off as usize;
        if off + n * 4 > self.data.len() {
            return Err(AbcError::OutOfBounds {
                offset: off,
                len: self.data.len(),
                what: "class index",
            });
        }
        self.class_index = Vec::with_capacity(n);
        for i in 0..n {
            let o = u32::from_le_bytes(self.data[off + i * 4..off + i * 4 + 4].try_into().unwrap());
            self.class_index.push(o);
        }
        Ok(())
    }

    fn read_index_headers(&mut self) -> AbcResult<()> {
        let (num, off) = (self.header.num_index_regions, self.header.index_section_off);
        if num == 0 || off == 0 {
            return Ok(());
        }
        let base = off as usize;
        // 先按“偏移表 → 分散 IndexHeader”尝试；失败则按“连续数组”解析
        let mut headers = Vec::new();
        let mut cursor = base;
        let mut ok_offsets = true;
        let mut offsets = Vec::with_capacity(num as usize);
        for _ in 0..num {
            if cursor + 4 > self.data.len() {
                ok_offsets = false;
                break;
            }
            let v = u32::from_le_bytes(self.data[cursor..cursor + 4].try_into().unwrap());
            cursor += 4;
            offsets.push(v);
        }
        if ok_offsets {
            for &ho in &offsets {
                if ho == 0 || ho as usize + 40 > self.data.len() {
                    ok_offsets = false;
                    break;
                }
                match Self::read_index_header_at(ho as usize)(&self.data) {
                    Some(h) => headers.push(h),
                    None => {
                        ok_offsets = false;
                        break;
                    }
                }
            }
        }
        if !ok_offsets {
            headers.clear();
            let mut p = base;
            for _ in 0..num {
                if p + 40 > self.data.len() {
                    break;
                }
                if let Some(h) = Self::read_index_header_at(p)(&self.data) {
                    headers.push(h);
                } else {
                    return Err(AbcError::OutOfBounds {
                        offset: p,
                        len: self.data.len(),
                        what: "index header",
                    });
                }
                p += 40;
            }
        }
        headers.sort_by_key(|h| h.start);
        self.index_headers = headers;
        Ok(())
    }

    fn read_index_header_at(p: usize) -> impl Fn(&[u8]) -> Option<IndexHeader> {
        move |data: &[u8]| {
            if p + 40 > data.len() {
                return None;
            }
            let w =
                |i: usize| u32::from_le_bytes(data[p + i * 4..p + i * 4 + 4].try_into().unwrap());
            let h = IndexHeader {
                start: w(0),
                end: w(1),
                class_idx_size: w(2),
                class_idx_off: w(3),
                method_idx_size: w(4),
                method_idx_off: w(5),
                field_idx_size: w(6),
                field_idx_off: w(7),
                proto_idx_size: w(8),
                proto_idx_off: w(9),
            };
            if h.start >= h.end || h.end as usize > data.len() || h.start < 64 {
                return None;
            }
            // size 字段不应超过文件长度（放宽：允许大计数但校验区域在文件内）
            if h.class_idx_size as usize * 4 > data.len()
                || h.method_idx_size as usize * 4 > data.len()
            {
                return None;
            }
            Some(h)
        }
    }

    /// 找到包含 offset 的索引区域头
    pub fn region_of(&self, offset: u32) -> Option<&IndexHeader> {
        self.index_headers
            .binary_search_by(|h| {
                if offset < h.start {
                    std::cmp::Ordering::Greater
                } else if offset >= h.end {
                    std::cmp::Ordering::Less
                } else {
                    std::cmp::Ordering::Equal
                }
            })
            .ok()
            .map(|i| &self.index_headers[i])
    }

    /// 解析 region 索引数组中的 u32
    pub fn region_u32(&self, table_off: u32, size: u32, idx: u32) -> AbcResult<u32> {
        if idx >= size {
            return Err(AbcError::IndexOutOfRange {
                index: idx as usize,
                len: size as usize,
            });
        }
        let p = table_off as usize + idx as usize * 4;
        if p + 4 > self.data.len() {
            return Err(AbcError::OutOfBounds {
                offset: p,
                len: self.data.len(),
                what: "region entry",
            });
        }
        Ok(u32::from_le_bytes(self.data[p..p + 4].try_into().unwrap()))
    }

    /// 通过 method/string/literalarray 区域索引解析偏移
    pub fn resolve_msl(&self, from_offset: u32, idx: u32) -> AbcResult<u32> {
        let hdr = self
            .region_of(from_offset)
            .ok_or(AbcError::IndexOutOfRange {
                index: from_offset as usize,
                len: self.index_headers.len(),
            })?;
        self.region_u32(hdr.method_idx_off, hdr.method_idx_size, idx)
    }

    /// 通过 class 区域索引解析 Type
    pub fn resolve_type(&self, from_offset: u32, idx: u32) -> AbcResult<Type> {
        let hdr = self
            .region_of(from_offset)
            .ok_or(AbcError::IndexOutOfRange {
                index: from_offset as usize,
                len: self.index_headers.len(),
            })?;
        let raw = self.region_u32(hdr.class_idx_off, hdr.class_idx_size, idx)?;
        Ok(type_from_u32(raw))
    }

    /// class 偏移 → 名字（缓存省略，直接解析）
    pub fn class_name(&self, class_off: u32) -> AbcResult<String> {
        if class_off == 0 {
            return Ok(String::new());
        }
        let mut r = Reader::new(&self.data);
        r.seek(class_off as usize)?;
        r.string()
    }

    /// 是否 foreign 区域
    pub fn is_foreign(&self, offset: u32) -> bool {
        self.header.foreign_size > 0
            && offset >= self.header.foreign_off
            && offset < self.header.foreign_off + self.header.foreign_size
    }

    fn read_classes(&mut self) -> AbcResult<()> {
        let offsets = self.class_index.clone();
        for off in offsets {
            if self.is_foreign(off) {
                // ForeignClass：仅名字
                let mut r = Reader::new(&self.data);
                r.seek(off as usize)?;
                let name = r.string()?;
                self.classes.push(ClassInfo {
                    offset: off,
                    name,
                    access_flags: 0,
                    source_file: None,
                    fields: Vec::new(),
                    methods: Vec::new(),
                    module_record_idx: None,
                });
                continue;
            }
            let class = self.read_class(off)?;
            self.classes.push(class);
        }
        Ok(())
    }

    fn read_class(&self, off: u32) -> AbcResult<ClassInfo> {
        let mut r = Reader::new(&self.data);
        r.seek(off as usize)?;
        let name = r.string()?;
        let _reserved = r.u32()?;
        let access_flags = r.uleb()? as u32;
        let num_fields = r.uleb()? as usize;
        let num_methods = r.uleb()? as usize;
        let mut source_file = None;
        let mut module_record_idx = None;
        // class_data TaggedValue 流
        loop {
            let tag = r.u8()?;
            match tag {
                0x00 => break,
                0x01 => {
                    // 接口列表（旧版）：uleb 数量 + u16 索引
                    let n = r.uleb()? as usize;
                    r.skip(n * 2)?;
                }
                0x02 => {
                    r.skip(1)?;
                }
                0x07 => {
                    let off = r.u32()?;
                    source_file = Some(r.string_at(off as usize)?);
                }
                0x03..=0x06 => {
                    r.skip(4)?;
                }
                other => {
                    return Err(AbcError::UnknownTag {
                        tag: other,
                        what: "class",
                    });
                }
            }
        }
        let mut fields = Vec::with_capacity(num_fields.min(r.remaining()).min(65536));
        for _ in 0..num_fields {
            fields.push(self.read_field(&mut r)?);
        }
        let mut methods = Vec::with_capacity(num_methods.min(r.remaining()).min(65536));
        for _ in 0..num_methods {
            methods.push(self.read_method(&mut r, false)?);
        }
        for f in &fields {
            if f.name == "moduleRecordIdx" {
                module_record_idx = f.value;
            }
        }
        Ok(ClassInfo {
            offset: off,
            name,
            access_flags,
            source_file,
            fields,
            methods,
            module_record_idx,
        })
    }

    fn read_field(&self, r: &mut Reader) -> AbcResult<FieldInfo> {
        let offset = r.pos() as u32;
        let class_idx = r.u16()? as u32;
        let type_idx = r.u16()? as u32;
        let name_off = r.u32()?;
        let _reserved = r.uleb()?;
        let mut int_value = None;
        let mut value = None;
        loop {
            let tag = r.u8()?;
            match tag {
                0x00 => break,
                0x01 => int_value = Some(r.sleb()?),
                0x02 => value = Some(r.u32()?),
                _ => r.skip(4)?,
            }
        }
        let name = r.string_at(name_off as usize)?;
        let ty = self
            .resolve_type(offset, type_idx)
            .unwrap_or(Type::Basic(BasicType::Any));
        let _ = class_idx;
        Ok(FieldInfo {
            offset,
            class_off: 0,
            ty,
            name,
            int_value,
            value,
        })
    }

    fn read_method(&self, r: &mut Reader, foreign: bool) -> AbcResult<MethodInfo> {
        let offset = r.pos() as u32;
        let class_idx = r.u16()? as u32;
        let proto_idx = r.u16()? as u32;
        let name_off = r.u32()?;
        // 现代：uleb = MethodIndexData（header_index | function_kind<<16）
        // 旧版（0.1.0.x）：uleb = 访问标记，无 function kind 信息
        let index_data = r.uleb()? as u32;
        let name = r.string_at(name_off as usize)?;
        let (header_index, kind) = if self.header.kind == HeaderKind::Legacy {
            (0u16, FunctionKind::Function)
        } else {
            (
                (index_data & 0xffff) as u16,
                function_kind(((index_data >> 16) & 0xff) as u8),
            )
        };
        let mut code = None;
        let mut debug_info_off = 0;
        loop {
            let tag = r.u8()?;
            match tag {
                0x00 => break,
                0x01 => {
                    let off = r.u32()?;
                    code = Some(self.read_code(off)?);
                }
                0x02 => {
                    r.skip(1)?;
                }
                0x05 => debug_info_off = r.u32()?,
                0x06 => {
                    let _annotation_off = r.u32()?;
                }
                0x09 if foreign => {
                    // ForeignMethod 无额外数据
                }
                _ => r.skip(4)?,
            }
        }
        let _ = class_idx;
        let _ = proto_idx;
        Ok(MethodInfo {
            offset,
            class_off: 0,
            proto_off: 0,
            name,
            header_index,
            kind,
            code,
            debug_info_off,
            foreign,
        })
    }

    /// 按偏移解析方法池中的方法（闭包/类成员递归反编译用）
    pub fn method_at(&self, off: u32) -> AbcResult<MethodInfo> {
        let mut r = Reader::new(&self.data);
        r.seek(off as usize)?;
        self.read_method(&mut r, false)
    }

    /// 解析 Code 结构
    pub fn read_code(&self, off: u32) -> AbcResult<CodeInfo> {
        let mut r = Reader::new(&self.data);
        r.seek(off as usize)?;
        let num_vregs = r.uleb()? as u32;
        let num_args = r.uleb()? as u32;
        let code_size = r.uleb()? as u32;
        let tries_size = r.uleb()? as u32;
        let ins_off = r.pos() as u32;
        r.skip(code_size as usize)?;
        let mut try_blocks = Vec::new();
        for _ in 0..tries_size {
            let start_pc = r.uleb()? as u32;
            let length = r.uleb()? as u32;
            let num_catches = r.uleb()? as usize;
            let mut catch_blocks = Vec::with_capacity(num_catches.min(r.remaining()).min(65536));
            for _ in 0..num_catches {
                let type_idx = r.uleb()? as u32;
                let handler_pc = r.uleb()? as u32;
                let code_size = r.uleb()? as u32;
                catch_blocks.push(CatchBlock {
                    type_idx,
                    handler_pc,
                    code_size,
                });
            }
            try_blocks.push(TryBlock {
                start_pc,
                length,
                catch_blocks,
            });
        }
        Ok(CodeInfo {
            offset: off,
            num_vregs,
            num_args,
            code_size,
            ins_off,
            try_blocks,
        })
    }

    /// 解析 proto（签名）
    pub fn read_proto(&self, off: u32) -> AbcResult<ProtoInfo> {
        let mut r = Reader::new(&self.data);
        r.seek(off as usize)?;
        // shorty：4 位一组，0 结尾；随后 ref 类型 u16 索引列表
        let mut ret = Type::Basic(BasicType::Any);
        let mut params = Vec::new();
        let elem = |v: u16, idx: usize, out: &mut Vec<Type>| -> bool {
            let shift = (idx % 4) * 4;
            let t = (v >> shift) & 0xf;
            if t == 0 {
                return false;
            }
            out.push(Type::Basic(basic_from_code(t)));
            true
        };
        let mut all = Vec::new();
        let mut v = r.u16()?;
        let mut idx = 0;
        loop {
            if !elem(v, idx, &mut all) {
                break;
            }
            idx += 1;
            if idx % 4 == 0 {
                v = r.u16()?;
            }
        }
        if let Some(first) = all.first().copied() {
            ret = first;
            params = all[1..].to_vec();
        }
        // ref 类型（非基础类型的参数/返回值）
        let mut ref_types = Vec::new();
        for t in std::iter::once(ret).chain(params.iter().copied()) {
            if matches!(t, Type::Basic(_)) {
                ref_types.push(0);
            } else {
                let idx = r.u16()? as u32;
                let resolved = self
                    .resolve_type(off, idx)
                    .unwrap_or(Type::Basic(BasicType::Any));
                ref_types.push(match resolved {
                    Type::Ref(o) => o,
                    Type::Basic(_) => 0,
                });
            }
        }
        Ok(ProtoInfo {
            ret,
            params,
            ref_types,
        })
    }

    /// 解析 literal 数组
    /// 截断式读取：超过 4096 个元素时保留前 4096 个，返回值附带总数。
    /// 防御超长字面量表（数值大表）被完整物化导致的内存/CPU 失控
    pub fn read_literal_array_truncated(&self, off: u32) -> AbcResult<TruncatedLiteralArray> {
        let mut r = Reader::new(&self.data);
        r.seek(off as usize)?;
        let num = r.u32()? as usize;
        let keep = num.min(LITERAL_RENDER_LIMIT);
        let mut values = Vec::with_capacity(keep);
        for _ in 0..keep {
            values.push(read_literal_value(&mut r)?);
        }
        Ok(TruncatedLiteralArray { values, total: num })
    }

    pub fn read_literal_array(&self, off: u32) -> AbcResult<Vec<LiteralValue>> {
        let mut r = Reader::new(&self.data);
        r.seek(off as usize)?;
        let num = r.u32()? as usize;
        let mut out = Vec::with_capacity(num.min(4096));
        for _ in 0..num {
            out.push(read_literal_value(&mut r)?);
        }
        Ok(out)
    }

    /// 读取字符串（绝对偏移）
    pub fn string_at(&self, off: u32) -> AbcResult<String> {
        Reader::new(&self.data).string_at(off as usize)
    }

    /// 解析模块 record（moduleRecordIdx 指向的原始结构，非 tagged literal）
    pub fn read_module_record(&self, off: u32) -> AbcResult<ModuleRecord> {
        let mut r = Reader::new(&self.data);
        r.seek(off as usize)?;
        let _num_literals = r.u32()?;
        let mut requests = Vec::new();
        let n = r.u32()? as usize;
        for _ in 0..n {
            let off = r.u32()?;
            requests.push(r.string_at(off as usize)?);
        }
        let mut regular_imports = Vec::new();
        let n = r.u32()? as usize;
        for _ in 0..n {
            let local_off = r.u32()?;
            let import_off = r.u32()?;
            let module_idx = r.u16()?;
            regular_imports.push(RegularImport {
                local_name: r.string_at(local_off as usize)?,
                import_name: r.string_at(import_off as usize)?,
                module_idx,
            });
        }
        let mut namespace_imports = Vec::new();
        let n = r.u32()? as usize;
        for _ in 0..n {
            let local_off = r.u32()?;
            let module_idx = r.u16()?;
            namespace_imports.push(NamespaceImport {
                local_name: r.string_at(local_off as usize)?,
                module_idx,
            });
        }
        let mut local_exports = Vec::new();
        let n = r.u32()? as usize;
        for _ in 0..n {
            let local_off = r.u32()?;
            let export_off = r.u32()?;
            local_exports.push(LocalExport {
                local_name: r.string_at(local_off as usize)?,
                export_name: r.string_at(export_off as usize)?,
            });
        }
        let mut indirect_exports = Vec::new();
        let n = r.u32()? as usize;
        for _ in 0..n {
            let export_off = r.u32()?;
            let import_off = r.u32()?;
            let module_idx = r.u16()?;
            indirect_exports.push(IndirectExport {
                export_name: r.string_at(export_off as usize)?,
                import_name: r.string_at(import_off as usize)?,
                module_idx,
            });
        }
        let mut star_exports = Vec::new();
        let n = r.u32()? as usize;
        for _ in 0..n {
            star_exports.push(r.u16()?);
        }
        Ok(ModuleRecord {
            requests,
            regular_imports,
            namespace_imports,
            local_exports,
            indirect_exports,
            star_exports,
        })
    }
}

/// Type u32 解码
pub fn type_from_u32(raw: u32) -> Type {
    if raw < 0x0d {
        Type::Basic(basic_from_code(raw as u16))
    } else {
        Type::Ref(raw)
    }
}

/// 4 位类型码 → 基础类型
pub fn basic_from_code(code: u16) -> BasicType {
    match code {
        0x00 => BasicType::U1,
        0x01 => BasicType::I8,
        0x02 => BasicType::U8,
        0x03 => BasicType::I16,
        0x04 => BasicType::U16,
        0x05 => BasicType::I32,
        0x06 => BasicType::U32,
        0x07 => BasicType::F32,
        0x08 => BasicType::F64,
        0x09 => BasicType::I64,
        0x0a => BasicType::U64,
        0x0c => BasicType::Any,
        _ => BasicType::Any,
    }
}

/// literal 渲染截断上限
pub const LITERAL_RENDER_LIMIT: usize = 4096;

/// 截断式 literal 数组：保留前 N 个元素 + 总数
pub struct TruncatedLiteralArray {
    pub values: Vec<LiteralValue>,
    pub total: usize,
}

/// 读取单个 tagged literal 值
fn read_literal_value(r: &mut Reader) -> AbcResult<LiteralValue> {
    let tag = r.u8()?;
    let v = match tag {
        0x00 => LiteralValue::Int8(r.u8()? as i8),
        0x01 => LiteralValue::Bool(r.u8()? != 0),
        0x02 => LiteralValue::Int32(r.u32()? as i32),
        0x03 => LiteralValue::Float(f32::from_bits(r.u32()?)),
        0x04 => LiteralValue::Double(f64::from_bits(r.u64()?)),
        0x05 => LiteralValue::String(r.u32()?),
        0x06 => LiteralValue::Method(r.u32()?),
        0x07 => LiteralValue::GeneratorMethod(r.u32()?),
        0x08 => LiteralValue::Accessor(r.u32()?),
        0x09 => LiteralValue::MethodAffiliate(r.u16()?),
        0x0a => {
            let n = (r.u32()? as usize)
                .min(r.remaining())
                .min(LITERAL_RENDER_LIMIT);
            let mut a = Vec::with_capacity(n);
            for _ in 0..n {
                a.push(r.u8()?);
            }
            LiteralValue::ArrayU1(a)
        }
        0x0b => {
            let n = (r.u32()? as usize)
                .min(r.remaining())
                .min(LITERAL_RENDER_LIMIT);
            let mut a = Vec::with_capacity(n);
            for _ in 0..n {
                a.push(r.u8()?);
            }
            LiteralValue::ArrayU8(a)
        }
        0x0c => {
            let n = (r.u32()? as usize)
                .min(r.remaining())
                .min(LITERAL_RENDER_LIMIT);
            let mut a = Vec::with_capacity(n);
            for _ in 0..n {
                a.push(r.u8()? as i8);
            }
            LiteralValue::ArrayI8(a)
        }
        0x0d => {
            let n = (r.u32()? as usize)
                .min(r.remaining())
                .min(LITERAL_RENDER_LIMIT);
            let mut a = Vec::with_capacity(n);
            for _ in 0..n {
                a.push(r.u16()?);
            }
            LiteralValue::ArrayU16(a)
        }
        0x0e => {
            let n = (r.u32()? as usize)
                .min(r.remaining())
                .min(LITERAL_RENDER_LIMIT);
            let mut a = Vec::with_capacity(n);
            for _ in 0..n {
                a.push(r.u16()? as i16);
            }
            LiteralValue::ArrayI16(a)
        }
        0x0f => {
            let n = (r.u32()? as usize)
                .min(r.remaining())
                .min(LITERAL_RENDER_LIMIT);
            let mut a = Vec::with_capacity(n);
            for _ in 0..n {
                a.push(r.u32()?);
            }
            LiteralValue::ArrayU32(a)
        }
        0x10 => {
            let n = (r.u32()? as usize)
                .min(r.remaining())
                .min(LITERAL_RENDER_LIMIT);
            let mut a = Vec::with_capacity(n);
            for _ in 0..n {
                a.push(r.u32()? as i32);
            }
            LiteralValue::ArrayI32(a)
        }
        0x11 => {
            let n = (r.u32()? as usize)
                .min(r.remaining())
                .min(LITERAL_RENDER_LIMIT);
            let mut a = Vec::with_capacity(n);
            for _ in 0..n {
                a.push(r.u64()?);
            }
            LiteralValue::ArrayU64(a)
        }
        0x12 => {
            let n = (r.u32()? as usize)
                .min(r.remaining())
                .min(LITERAL_RENDER_LIMIT);
            let mut a = Vec::with_capacity(n);
            for _ in 0..n {
                a.push(r.sleb()?);
            }
            LiteralValue::ArrayI64(a)
        }
        0x13 => {
            let n = (r.u32()? as usize)
                .min(r.remaining())
                .min(LITERAL_RENDER_LIMIT);
            let mut a = Vec::with_capacity(n);
            for _ in 0..n {
                a.push(f32::from_bits(r.u32()?));
            }
            LiteralValue::ArrayF32(a)
        }
        0x14 => {
            let n = (r.u32()? as usize)
                .min(r.remaining())
                .min(LITERAL_RENDER_LIMIT);
            let mut a = Vec::with_capacity(n);
            for _ in 0..n {
                a.push(f64::from_bits(r.u64()?));
            }
            LiteralValue::ArrayF64(a)
        }
        0x15 => {
            let n = (r.u32()? as usize)
                .min(r.remaining())
                .min(LITERAL_RENDER_LIMIT);
            let mut a = Vec::with_capacity(n);
            for _ in 0..n {
                a.push(r.u32()?);
            }
            LiteralValue::ArrayString(a)
        }
        0x16 => LiteralValue::AsyncGeneratorMethod(r.u32()?),
        0x17 => LiteralValue::LiteralBufferIndex(r.u16()?),
        0x18 => LiteralValue::LiteralArray(r.u32()?),
        0x19 => LiteralValue::BuiltinTypeIndex(r.u16()?),
        0x1a => LiteralValue::Getter(r.u32()?),
        0x1b => LiteralValue::Setter(r.u32()?),
        0x1c => LiteralValue::EtsImplements(r.u32()?),
        0xff => LiteralValue::Null,
        other => {
            return Err(AbcError::UnknownTag {
                tag: other,
                what: "literal",
            });
        }
    };
    Ok(v)
}

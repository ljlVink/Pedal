//! HAP/HSP 应用包读取：zip 容器内枚举 abc/an/so/资源条目
//!
//! @author ljlVink

use crate::error::{AbcError, AbcResult};
use std::io::Read;

/// 包内一个条目
#[derive(Debug, Clone)]
pub struct HapEntry {
    /// 条目路径（包内相对路径）
    pub path: String,
    /// 未压缩大小
    pub size: u64,
    /// 是否 abc 字节码
    pub is_abc: bool,
    /// 是否 AOT ELF（.an）
    pub is_an: bool,
    /// 是否原生库（.so）
    pub is_so: bool,
    /// 是否描述文件（module.json / pack.info 等）
    pub is_descriptor: bool,
}

/// HAP/HSP 包
pub struct HapArchive {
    /// zip 数据（保存以便按需读取条目）
    pub data: Vec<u8>,
    /// 条目清单
    pub entries: Vec<HapEntry>,
}

impl HapArchive {
    /// 判断文件名是否为支持的包
    pub fn is_package(path: &str) -> bool {
        path.ends_with(".hap")
            || path.ends_with(".hsp")
            || path.ends_with(".har")
            || path.ends_with(".app")
    }

    /// 解析包（读取中央目录）
    pub fn parse(data: Vec<u8>) -> AbcResult<Self> {
        let cursor = std::io::Cursor::new(&data);
        let mut zip = zip::ZipArchive::new(cursor).map_err(|e| AbcError::Zip(e.to_string()))?;
        let mut entries = Vec::with_capacity(zip.len());
        for i in 0..zip.len() {
            let entry = zip
                .by_index_raw(i)
                .map_err(|e| AbcError::Zip(e.to_string()))?;
            let path = entry.name().to_string();
            let lower = path.to_lowercase();
            entries.push(HapEntry {
                is_abc: lower.ends_with(".abc"),
                is_an: lower.ends_with(".an"),
                is_so: lower.ends_with(".so"),
                is_descriptor: lower.ends_with("module.json")
                    || lower.ends_with("pack.info")
                    || lower.ends_with("config.json"),
                size: entry.size(),
                path,
            });
        }
        Ok(HapArchive { data, entries })
    }

    /// 读取条目内容
    pub fn read_entry(&self, path: &str) -> AbcResult<Vec<u8>> {
        let cursor = std::io::Cursor::new(&self.data);
        let mut zip = zip::ZipArchive::new(cursor).map_err(|e| AbcError::Zip(e.to_string()))?;
        let mut entry = zip
            .by_name(path)
            .map_err(|e| AbcError::Zip(e.to_string()))?;
        let mut out = Vec::with_capacity(entry.size() as usize);
        entry
            .read_to_end(&mut out)
            .map_err(|e| AbcError::Zip(e.to_string()))?;
        Ok(out)
    }

    /// 主 abc（ets/modules.abc）路径
    pub fn main_abc(&self) -> Option<&str> {
        self.entries
            .iter()
            .find(|e| e.is_abc && e.path.ends_with("modules.abc"))
            .map(|e| e.path.as_str())
            .or_else(|| {
                self.entries
                    .iter()
                    .find(|e| e.is_abc)
                    .map(|e| e.path.as_str())
            })
    }
}

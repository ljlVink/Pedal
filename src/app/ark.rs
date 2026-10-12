//! ArkTS（abc）工作区：HAP/HSP/ABC 打开、虚拟 JAR 条目构建、反编译源与反汇编
//!
//! 打开流程：后台线程解析包 → 解析全部 abc → 全量反编译 record 为伪 ArkTS →
//! 组装虚拟 JarArchive（record 源码即文本条目），复用 JAR 版 UI（树/Tab/搜索/导出）。
//!
//! @author sky
//! @author ljlVink

use crate::task::Task;
use pedal_abc_bridge::decompile;
use pedal_abc_bridge::disasm::{self, DisasmCtx};
use pedal_abc_bridge::error::AbcError;
use pedal_abc_bridge::file::AbcFile;
use pedal_abc_bridge::hap::HapArchive;
use pedal_archive::jar::{JarArchive, LoadProgress};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// 原始条目加载的大小上限（超过则不进内存，hex 视图显示提示）
const RAW_ENTRY_LIMIT: u64 = 2 * 1024 * 1024;

/// Ark 工作区元数据（LoadedState.ark）
pub(crate) struct ArkMeta {
    /// abc 文件（条目路径 → 解析结果）
    pub abcs: Vec<(String, AbcFile)>,
    /// 反编译源码（虚拟条目路径 → 伪 ArkTS）
    pub sources: HashMap<String, String>,
    /// 反汇编文本缓存（.ets 条目路径 → pandasm 文本，懒生成）
    pub disasm: HashMap<String, String>,
    /// .ets 条目路径 → (abc 条目序号, class 偏移)
    pub disasm_targets: HashMap<String, (usize, u32)>,
}

impl ArkMeta {
    /// 懒生成一条反汇编文本
    pub fn build_disasm_entry(&mut self, entry_path: &str) -> Option<String> {
        if let Some(text) = self.disasm.get(entry_path) {
            return Some(text.clone());
        }
        let (abc_idx, class_off) = *self.disasm_targets.get(entry_path)?;
        let (abc_path, file) = self.abcs.get(abc_idx)?;
        let class = file.classes.iter().find(|c| c.offset == class_off)?;
        let text = build_disasm(file, class);
        self.disasm.insert(entry_path.to_string(), text.clone());
        let _ = abc_path;
        Some(text)
    }
}

/// Ark 加载状态
pub(crate) struct ArkLoadingState {
    /// 文件名
    pub name: String,
    /// 进度
    pub progress: Arc<LoadProgress>,
    /// 后台任务
    pub task: Task<Result<(JarArchive, ArkMeta), AbcError>>,
}

/// 判断是否为 ArkTS 工作区文件
pub(crate) fn is_ark_file(path: &Path) -> bool {
    let lower = path.to_string_lossy().to_lowercase();
    lower.ends_with(".hap")
        || lower.ends_with(".hsp")
        || lower.ends_with(".abc")
        || lower.ends_with(".har")
        || lower.ends_with(".app")
}

/// 后台打开 ArkTS 文件（HAP/HSP/独立 abc）
pub(crate) fn spawn_open_ark(
    path: &Path,
    progress: Arc<LoadProgress>,
) -> Task<Result<(JarArchive, ArkMeta), AbcError>> {
    let path = path.to_path_buf();
    Task::spawn(move || open_ark(&path, &progress))
}

/// 解析并构建虚拟归档
fn open_ark(path: &Path, progress: &LoadProgress) -> Result<(JarArchive, ArkMeta), AbcError> {
    let data = std::fs::read(path).map_err(|e| AbcError::Io(e.to_string()))?;
    let file_size = data.len() as u64;
    let hash: String = {
        use sha2::Digest;
        sha2::Sha256::digest(&data)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect()
    };
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();

    let mut entries: HashMap<String, Vec<u8>> = HashMap::new();
    let mut abcs: Vec<(String, AbcFile)> = Vec::new();

    let is_package = HapArchive::is_package(&path.to_string_lossy());
    if is_package {
        let hap = HapArchive::parse(data)?;
        // 原始条目（限制大小）
        for e in &hap.entries {
            if e.size <= RAW_ENTRY_LIMIT {
                if let Ok(bytes) = hap.read_entry(&e.path) {
                    entries.insert(e.path.clone(), bytes);
                }
            } else {
                // 占位：标记为大型二进制，hex 视图显示空
                entries.insert(
                    e.path.clone(),
                    format!("<large entry {} bytes>", e.size).into_bytes(),
                );
            }
        }
        // abc 条目
        for e in hap.entries.iter().filter(|e| e.is_abc) {
            let bytes = hap.read_entry(&e.path)?;
            match AbcFile::parse(bytes) {
                Ok(f) => abcs.push((e.path.clone(), f)),
                Err(err) => {
                    log::warn!("abc 解析失败 {}: {err}", e.path);
                    entries.insert(
                        format!("{}.error.txt", e.path),
                        format!("abc parse failed: {err}").into_bytes(),
                    );
                }
            }
        }
    } else {
        let f = AbcFile::parse(data)?;
        let stem = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "module".into());
        let entry_path = format!("{stem}.abc");
        // 原始字节即条目（hex 视图）
        let raw = {
            let mut r = f.data.clone();
            r.truncate(RAW_ENTRY_LIMIT as usize);
            r
        };
        entries.insert(entry_path.clone(), raw);
        abcs.push((entry_path, f));
    }

    // 全量反编译 + 反汇编（按 record 切块多线程）
    let total: u32 = abcs.iter().map(|(_, f)| f.classes.len() as u32).sum();
    progress
        .total
        .store(total.max(1), std::sync::atomic::Ordering::Relaxed);
    // 并行度封顶 4：GUI 应用做良好公民，避免抢占前台程序
    let threads = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
        .clamp(1, 4);
    // 展平 (abc_idx, abc_path, file, class) 四元组便于切块
    let jobs: Vec<(
        usize,
        &str,
        &pedal_abc_bridge::file::AbcFile,
        &pedal_abc_bridge::file::ClassInfo,
    )> = abcs
        .iter()
        .enumerate()
        .flat_map(|(i, (p, f))| f.classes.iter().map(move |c| (i, p.as_str(), f, c)))
        .collect();
    let chunk = jobs.len().div_ceil(threads);
    let results: Vec<Vec<(String, String, usize, u32)>> = if chunk == 0 || threads == 1 {
        // 退化：单线程直接算，避免线程开销
        vec![run_chunk(&jobs, progress)]
    } else {
        std::thread::scope(|scope| {
            let handles: Vec<_> = jobs
                .chunks(chunk)
                .map(|slice| scope.spawn(move || run_chunk(slice, progress)))
                .collect();
            handles
                .into_iter()
                .map(|h| h.join().unwrap_or_default())
                .collect()
        })
    };
    let mut sources = HashMap::new();
    let mut disasm_targets: HashMap<String, (usize, u32)> = HashMap::new();
    for part in results {
        for (src_path, source, abc_idx, class_off) in part {
            entries.insert(src_path.clone(), source.clone().into_bytes());
            // 反汇编以 .ets 条目为键，编辑器内“源码|反汇编”切换懒生成
            disasm_targets.insert(src_path.clone(), (abc_idx, class_off));
            sources.insert(src_path, source);
        }
    }

    log::info!(
        "ArkTS workspace ready: {} records, {} abc files, {} sources",
        sources.len(),
        abcs.len(),
        sources.len()
    );
    let jar = JarArchive::from_entries(name, PathBuf::from(path), hash, file_size, entries);
    Ok((
        jar,
        ArkMeta {
            abcs,
            sources,
            disasm: HashMap::new(),
            disasm_targets,
        },
    ))
}

/// 反编译一个 record 分块，返回 (src_path, 源码, abc_idx, class 偏移)
fn run_chunk(
    jobs: &[(
        usize,
        &str,
        &pedal_abc_bridge::file::AbcFile,
        &pedal_abc_bridge::file::ClassInfo,
    )],
    progress: &pedal_archive::jar::LoadProgress,
) -> Vec<(String, String, usize, u32)> {
    let mut out = Vec::with_capacity(jobs.len());
    let mut local_done = 0u32;
    for (abc_idx, abc_path, file, class) in jobs {
        let display = display_record_name(&class.name);
        let src_path = format!("{abc_path}/{display}.ets");
        let source = decompile::decompile_record(file, class)
            .map(|r| r.source)
            .unwrap_or_else(|e| format!("// decompile failed: {e}\n"));
        out.push((src_path, source, *abc_idx, class.offset));
        local_done += 1;
        if local_done.is_multiple_of(16) {
            progress
                .current
                .fetch_add(16, std::sync::atomic::Ordering::Relaxed);
        }
    }
    progress
        .current
        .fetch_add(local_done % 16, std::sync::atomic::Ordering::Relaxed);
    out
}

/// record 显示名：`L&@app/foo/Bar&3.4.0-501;` → `@app/foo/Bar`
pub fn display_record_name(name: &str) -> String {
    let mut s = name;
    if let Some(stripped) = s.strip_prefix('L') {
        s = stripped;
    }
    if let Some(stripped) = s.strip_suffix(';') {
        s = stripped;
    }
    // 版本后缀 `&N.N.N-NNN`
    if let Some(pos) = s.rfind('&') {
        let tail = &s[pos + 1..];
        let versionish = !tail.is_empty()
            && tail
                .chars()
                .all(|c| c.is_ascii_digit() || c == '.' || c == '-');
        if versionish {
            s = &s[..pos];
        }
    }
    s.to_string()
}

/// 整个 record 的 pandasm 反汇编文本
fn build_disasm(file: &AbcFile, class: &pedal_abc_bridge::file::ClassInfo) -> String {
    let mut out = String::with_capacity(4096);
    out.push_str(&format!(".record {}\n", display_record_name(&class.name)));
    for m in &class.methods {
        let Some(code) = &m.code else {
            continue;
        };
        let ctx = DisasmCtx {
            file,
            method_off: m.offset,
        };
        match disasm::disassemble_method(
            &ctx,
            &m.name,
            code.instructions(&file.data),
            code.num_vregs,
            code.num_args,
            &code.try_blocks,
        ) {
            Ok(text) => out.push_str(&text),
            Err(e) => out.push_str(&format!("// disasm failed @{}: {e}\n", m.name)),
        }
    }
    out
}

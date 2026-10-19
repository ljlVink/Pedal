//! ArkTS（abc）工作区：HAP/HSP/ABC 打开、虚拟 JAR 条目构建、反编译源与反汇编
//!
//! 打开流程：后台线程解析包 → 解析全部 abc → 全量反编译 record 为伪 ArkTS →
//! 组装虚拟 JarArchive（record 源码即文本条目），复用 JAR 版 UI（树/Tab/搜索/导出）。
//!
//! @author sky
//! @author ljlVink

use crate::task::{Poll, Pollable, Task};
use pedal_abc_bridge::decompile;
use pedal_abc_bridge::disasm::{self, DisasmCtx};
use pedal_abc_bridge::error::AbcError;
use pedal_abc_bridge::file::AbcFile;
use pedal_abc_bridge::hap::HapArchive;
use pedal_archive::jar::{JarArchive, LoadProgress};
use rust_i18n::t;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

mod record_names;
use record_names::{display_record_name, record_entry_name};

/// 原始条目加载的大小上限（超过则不进内存，hex 视图显示提示）
const RAW_ENTRY_LIMIT: u64 = 2 * 1024 * 1024;

/// Ark 工作区元数据（LoadedState.ark）
pub(crate) struct ArkMeta {
    /// abc 文件（条目路径 → 解析结果）
    pub abcs: Vec<(String, Arc<AbcFile>)>,
    /// 反编译源码（虚拟条目路径 → 伪 ArkTS）
    pub sources: HashMap<String, String>,
    /// 反汇编文本缓存（.ets 条目路径 → pandasm 文本，懒生成）
    pub disasm: HashMap<String, String>,
    /// .ets 条目路径 → (abc 条目序号, class 偏移)
    pub disasm_targets: HashMap<String, (usize, u32)>,
    /// 需要与可导出路径区分的可读文件名（联合类型等）。
    pub record_labels: HashMap<String, String>,
    /// 当前工作区的反汇编后台任务；关闭工作区时丢弃接收端。
    disasm_tasks: HashMap<String, Task<Option<String>>>,
}

impl ArkMeta {
    /// 为尚未缓存的条目启动后台反汇编，重复请求复用正在执行的任务。
    pub fn request_disasm_entry(&mut self, entry_path: &str) -> bool {
        if self.disasm.contains_key(entry_path) || self.disasm_tasks.contains_key(entry_path) {
            return true;
        }
        let Some(&(abc_idx, class_off)) = self.disasm_targets.get(entry_path) else {
            return false;
        };
        let Some((_, file)) = self.abcs.get(abc_idx) else {
            return false;
        };
        let file = Arc::clone(file);
        self.disasm_tasks.insert(
            entry_path.to_string(),
            Task::spawn(move || {
                let class = file.classes.iter().find(|c| c.offset == class_off)?;
                Some(build_disasm(&file, class))
            }),
        );
        true
    }

    /// 非阻塞收集完成结果并缓存；None 表示任务断开或目标不存在。
    pub fn poll_disasm_entries(&mut self) -> Vec<(String, Option<String>)> {
        let completed: Vec<_> = self
            .disasm_tasks
            .iter()
            .filter_map(|(path, task)| match task.poll() {
                Poll::Ready(text) => Some((path.clone(), text)),
                Poll::Lost => Some((path.clone(), None)),
                Poll::Pending => None,
            })
            .collect();
        for (path, text) in &completed {
            self.disasm_tasks.remove(path);
            if let Some(text) = text {
                self.disasm.insert(path.clone(), text.clone());
            }
        }
        completed
    }

    /// 是否需要继续轮询当前工作区的反汇编任务。
    pub fn has_pending_disasm(&self) -> bool {
        !self.disasm_tasks.is_empty()
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
pub(super) fn open_ark(
    path: &Path,
    progress: &LoadProgress,
) -> Result<(JarArchive, ArkMeta), AbcError> {
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
    let mut abcs: Vec<(String, Arc<AbcFile>)> = Vec::new();

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
                Ok(f) => abcs.push((e.path.clone(), Arc::new(f))),
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
        abcs.push((entry_path, Arc::new(f)));
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
        .flat_map(|(i, (p, f))| {
            f.classes
                .iter()
                .map(move |c| (i, p.as_str(), f.as_ref(), c))
        })
        .collect();
    let chunk = jobs.len().div_ceil(threads);
    let results: Vec<Vec<RecordSource>> = if chunk == 0 || threads == 1 {
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
    let mut record_labels = HashMap::new();
    let mut disasm_targets: HashMap<String, (usize, u32)> = HashMap::new();
    for part in results {
        for RecordSource {
            src_path,
            source,
            abc_idx,
            class_off,
            label,
        } in part
        {
            entries.insert(src_path.clone(), source.clone().into_bytes());
            // 反汇编以 .ets 条目为键，编辑器内“源码|反汇编”切换懒生成
            disasm_targets.insert(src_path.clone(), (abc_idx, class_off));
            if let Some(label) = label {
                record_labels.insert(src_path.clone(), label);
            }
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
            record_labels,
            disasm_tasks: HashMap::new(),
        },
    ))
}

/// 一个 record 的源码条目与原始字节码目标。
struct RecordSource {
    src_path: String,
    source: String,
    abc_idx: usize,
    class_off: u32,
    label: Option<String>,
}

/// 反编译一个 record 分块。
fn run_chunk(
    jobs: &[(
        usize,
        &str,
        &pedal_abc_bridge::file::AbcFile,
        &pedal_abc_bridge::file::ClassInfo,
    )],
    progress: &pedal_archive::jar::LoadProgress,
) -> Vec<RecordSource> {
    let mut out = Vec::with_capacity(jobs.len());
    let mut local_done = 0u32;
    for (abc_idx, abc_path, file, class) in jobs {
        let display = display_record_name(&class.name);
        let (entry_name, label) = record_entry_name(&class.name, class.offset);
        let src_path = format!("{abc_path}/{entry_name}.ets");
        let mut source = decompile::decompile_record(file, class)
            .map(|r| r.source)
            .unwrap_or_else(|e| format!("// decompile failed: {e}\n"));
        if source.trim().is_empty() {
            let reason = if file.is_foreign(class.offset) {
                t!("editor.ark_external_record")
            } else if class.methods.iter().all(|m| m.code.is_none()) {
                t!("editor.ark_declaration_record")
            } else {
                t!("editor.ark_empty_source")
            };
            source = format!("// {display}\n// {reason}\n");
            for field in &class.fields {
                source.push_str(&format!(
                    "// {}\n",
                    t!("editor.ark_field", name = &field.name)
                ));
            }
            for method in &class.methods {
                source.push_str(&format!(
                    "// {}\n",
                    t!("editor.ark_method", name = &method.name)
                ));
            }
        }
        out.push(RecordSource {
            src_path,
            source,
            abc_idx: *abc_idx,
            class_off: class.offset,
            label,
        });
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

#[cfg(test)]
mod disasm_tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    fn record_names_preserve_array_dimensions_and_plain_module_names() {
        assert_eq!(display_record_name("Lstd/core/String;"), "std/core/String");
        assert_eq!(
            display_record_name("[Lstd/core/String;"),
            "std/core/String[]"
        );
        assert_eq!(
            display_record_name("[[Lstd/core/String;"),
            "std/core/String[][]"
        );
        assert_eq!(
            display_record_name("[L@ohos/multimedia/camera/camera/CameraPosition;"),
            "@ohos/multimedia/camera/camera/CameraPosition[]"
        );
        assert_eq!(display_record_name("[L@app/Foo&3.4.0-501;"), "@app/Foo[]");
        for plain in [
            "Logger",
            "Local/index",
            "[Label",
            "[L;",
            "_ESSlotNumberAnnotation",
        ] {
            assert_eq!(display_record_name(plain), plain);
        }
    }

    fn fixture_workspace() -> ArkMeta {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("crates/pedal-abc-bridge/tests/fixtures/advanced_recovery/debug.abc");
        open_ark(&path, &LoadProgress::new()).unwrap().1
    }

    fn await_disasm(ark: &mut ArkMeta, entry: &str) -> String {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some((path, result)) = ark.poll_disasm_entries().into_iter().next() {
                assert_eq!(path, entry);
                return result.expect("fixture disassembly succeeds");
            }
            assert!(Instant::now() < deadline, "disassembly task timed out");
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    #[test]
    fn real_record_disassembly_reuses_pending_task_and_cached_result() {
        let mut ark = fixture_workspace();
        let entry = ark
            .disasm_targets
            .keys()
            .find(|path| path.contains("AdvancedRecovery"))
            .unwrap()
            .clone();
        assert!(ark.request_disasm_entry(&entry));
        assert!(ark.request_disasm_entry(&entry));
        assert_eq!(ark.disasm_tasks.len(), 1);
        let text = await_disasm(&mut ark, &entry);
        assert!(text.contains(".record AdvancedRecovery"), "{text}");
        assert!(text.contains("func_main_0"), "{text}");
        assert_eq!(ark.disasm.get(&entry), Some(&text));
        assert!(ark.request_disasm_entry(&entry));
        assert!(!ark.has_pending_disasm());
        assert!(ark.poll_disasm_entries().is_empty());
    }

    #[test]
    fn closing_workspace_does_not_deliver_into_same_path_in_new_workspace() {
        let mut old = fixture_workspace();
        let mut current = fixture_workspace();
        let entry = "shared.ets";
        let old_target = *old
            .disasm_targets
            .iter()
            .find(|(path, _)| path.contains("AdvancedRecovery"))
            .unwrap()
            .1;
        let current_class = current.abcs[0]
            .1
            .classes
            .iter()
            .find(|class| class.methods.is_empty())
            .unwrap();
        let expected = format!(".record {}\n", display_record_name(&current_class.name));
        let current_target = (0, current_class.offset);
        old.disasm_targets.insert(entry.into(), old_target);
        current.disasm_targets.insert(entry.into(), current_target);
        assert!(old.request_disasm_entry(entry));
        assert!(old.has_pending_disasm());
        drop(old);
        assert!(!current.disasm.contains_key(entry));
        assert!(current.poll_disasm_entries().is_empty());
        assert!(current.request_disasm_entry(entry));
        let text = await_disasm(&mut current, entry);
        assert_eq!(text, expected);
        assert_eq!(current.disasm.get(entry), Some(&expected));
        assert!(!current.has_pending_disasm());
    }
}

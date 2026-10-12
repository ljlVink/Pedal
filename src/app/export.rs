//! 导出功能：反编译源码导出、归档导出
//!
//! @author sky
//! @author ljlVink

use super::App;
use crate::task::{Poll, Pollable, Task};
use pedal_archive::jar::LoadProgress;
use rust_i18n::t;
use std::path::PathBuf;
use std::sync::Arc;

/// JAR 写出模式
pub(crate) enum JarWriteMode {
    /// 另存为新的 JAR 文件
    ExportCopy,
    /// 覆盖当前打开的源 JAR 文件
    OverwriteSource,
}

/// JAR 后台导出状态
pub(crate) struct ExportingState {
    /// 目标文件路径（用于完成后显示）
    pub dest: PathBuf,
    /// 已修改条目数（用于完成后显示）
    pub modified_count: usize,
    /// 写出模式
    pub mode: JarWriteMode,
    /// 导出进度
    pub progress: Arc<LoadProgress>,
    /// 后台导出任务
    pub task: Task<Result<usize, String>>,
}

impl App {
    /// 当前是否为 ArkTS 虚拟归档工作区（只读，不支持写回）
    fn is_ark_workspace(&self) -> bool {
        self.workspace.loaded().is_some_and(|s| s.ark.is_some())
    }

    fn start_jar_write(
        &mut self,
        dest: PathBuf,
        modified_count: usize,
        snapshot: Vec<(String, Vec<u8>)>,
        mode: JarWriteMode,
    ) {
        let progress = Arc::new(LoadProgress::new());
        let p = progress.clone();
        let out = dest.clone();
        let task = Task::spawn(move || {
            pedal_archive::jar::write_jar(&snapshot, &out, &p).map_err(|e| e.to_string())
        });
        self.exporting = Some(ExportingState {
            dest,
            modified_count,
            mode,
            progress,
            task,
        });
    }

    /// 导出修改后的 JAR 到用户选择的路径
    ///
    /// 在主线程快照条目数据，然后在后台线程写出 ZIP，
    /// 通过 `LoadProgress` 原子量回报进度。
    pub fn export_jar(&mut self) {
        if self.is_ark_workspace() {
            self.toasts.warning(t!("layout.ark_readonly"));
            return;
        }
        let Some(jar) = self.workspace.jar() else {
            self.toasts.warning(t!("layout.export_no_jar"));
            return;
        };
        if self.exporting.is_some() {
            self.toasts.warning(t!("layout.export_in_progress"));
            return;
        }
        let unsaved = self.layout.editor.unsaved_paths();
        if !unsaved.is_empty() {
            self.toasts
                .warning(t!("layout.export_unsaved", count = unsaved.len()));
            return;
        }
        let default_name = jar
            .path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "output.jar".to_owned());
        let Some(dest) = rfd::FileDialog::new()
            .set_file_name(&default_name)
            .add_filter("JAR", &["jar"])
            .save_file()
        else {
            return;
        };
        let snapshot = jar.snapshot_entries();
        let modified_count = jar.modified_count();
        self.start_jar_write(dest, modified_count, snapshot, JarWriteMode::ExportCopy);
    }

    /// 保存并覆盖当前打开的源 JAR 文件
    pub fn save_jar_overwrite_source(&mut self) {
        if self.is_ark_workspace() {
            self.toasts.warning(t!("layout.ark_readonly"));
            return;
        }
        let Some(jar) = self.workspace.jar() else {
            self.toasts.warning(t!("layout.export_no_jar"));
            return;
        };
        if self.exporting.is_some() {
            self.toasts.warning(t!("layout.save_source_in_progress"));
            return;
        }
        let unsaved = self.layout.editor.unsaved_paths();
        if !unsaved.is_empty() {
            self.toasts
                .warning(t!("layout.export_unsaved", count = unsaved.len()));
            return;
        }
        let modified_count = jar.modified_count();
        if modified_count == 0 {
            self.toasts.info(t!("layout.save_source_no_changes"));
            return;
        }
        let dest = jar.path.clone();
        let snapshot = jar.snapshot_entries();
        self.start_jar_write(
            dest,
            modified_count,
            snapshot,
            JarWriteMode::OverwriteSource,
        );
    }

    /// 轮询后台 JAR 导出是否完成
    pub(crate) fn poll_export_jar(&mut self) {
        let Some(state) = &self.exporting else {
            return;
        };
        let result = match state.task.poll() {
            Poll::Ready(r) => r,
            Poll::Pending => return,
            Poll::Lost => {
                self.exporting = None;
                return;
            }
        };
        let state = self.exporting.take().unwrap();
        let dest = state.dest;
        let modified = state.modified_count;
        let mode = state.mode;
        match result {
            Ok(count) => {
                let display = dest.to_string_lossy();
                match mode {
                    JarWriteMode::ExportCopy => {
                        self.toasts.info(t!(
                            "layout.export_jar_complete",
                            path = display,
                            count = count,
                            modified = modified
                        ));
                        log::info!(
                            "Exported JAR ({count} entries, {modified} modified) to {display}"
                        );
                    }
                    JarWriteMode::OverwriteSource => {
                        if let Some(jar) = self.workspace.jar_mut()
                            && let Err(error) = jar.commit_modified_from_file(&dest)
                        {
                            log::warn!("Failed to refresh saved source JAR metadata: {error}");
                            jar.clear_modified();
                        }
                        self.toasts.info(t!(
                            "layout.save_source_complete",
                            path = display,
                            count = count,
                            modified = modified
                        ));
                        log::info!(
                            "Saved source JAR ({count} entries, {modified} modified) to {display}"
                        );
                    }
                }
            }
            Err(e) => match mode {
                JarWriteMode::ExportCopy => {
                    self.toasts.error(t!("layout.export_failed", error = e));
                    log::error!("Export JAR failed: {e}");
                }
                JarWriteMode::OverwriteSource => {
                    self.toasts
                        .error(t!("layout.save_source_failed", error = e));
                    log::error!("Save source JAR failed: {e}");
                }
            },
        }
    }

    /// 导出反编译源码到用户选择的目录
    ///
    /// ArkTS 工作区导出内存中的伪 ArkTS 源码；普通归档无源码可导出。
    pub fn export_decompiled(&mut self) {
        if !self.is_ark_workspace() {
            self.toasts.warning(t!("layout.export_no_jar"));
            return;
        }
        self.export_ark_decompiled();
    }

    /// 导出 ArkTS 反编译源码（伪 ArkTS）到用户选择的目录
    pub fn export_ark_decompiled(&mut self) {
        let Some(loaded) = self.workspace.loaded() else {
            return;
        };
        let Some(ark) = &loaded.ark else {
            return;
        };
        let Some(dest) = rfd::FileDialog::new().pick_folder() else {
            return;
        };
        let mut count = 0usize;
        let mut failed = 0usize;
        for (path, source) in ark.sources.iter() {
            let out_path = dest.join(path.replace(':', "_"));
            if let Some(parent) = out_path.parent()
                && std::fs::create_dir_all(parent).is_err()
            {
                failed += 1;
                continue;
            }
            match std::fs::write(&out_path, source) {
                Ok(()) => count += 1,
                Err(_) => failed += 1,
            }
        }
        let display = dest.to_string_lossy();
        if failed == 0 {
            self.toasts
                .info(t!("layout.export_complete", path = display, count = count));
        } else {
            self.toasts.warning(t!(
                "layout.export_ark_partial",
                path = display,
                count = count,
                failed = failed
            ));
        }
        log::info!("Exported ArkTS sources: {count} ok, {failed} failed -> {display}");
    }
}

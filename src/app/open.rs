//! 文件打开：归档加载、拖拽处理、explorer 点击、独立文件打开
//!
//! @author sky
//! @author ljlVink

use super::App;
use super::ark::{self, ArkLoadingState};
use super::workspace::{LoadedState, LoadingState, Workspace};
use crate::task::{Poll, Pollable, Task};
use crate::ui::editor::EditorArea;
use crate::ui::explorer::tree;
use eframe::egui;
use egui_shell::components::SettingsFile;
use pedal_archive::error::ArchiveError;
use pedal_archive::jar::{JarArchive, LoadProgress};
use rust_i18n::t;
use std::path::Path;
use std::sync::Arc;

impl App {
    /// 处理 explorer 中点击的文件
    pub(crate) fn handle_pending_open(&mut self) {
        let Some(entry_path) = self.layout.file_panel.pending_open.take() else {
            return;
        };
        // 已打开的 tab 直接聚焦
        if self.layout.editor.focus_tab(&entry_path) {
            return;
        }
        // ArkTS：.panda 反汇编条目懒生成
        if entry_path.ends_with(".panda") {
            let cached = {
                let Some(loaded) = self.workspace.loaded() else {
                    return;
                };
                let Some(ark) = &loaded.ark else {
                    return;
                };
                ark.disasm.get(&entry_path).cloned()
            };
            if cached.is_none() {
                let text = {
                    let Some(loaded) = self.workspace.loaded_mut() else {
                        return;
                    };
                    let Some(ark) = loaded.ark.as_mut() else {
                        return;
                    };
                    ark.build_disasm_entry(&entry_path)
                };
                if let Some(text) = text
                    && let Some(jar) = self.workspace.jar_mut()
                {
                    jar.put(&entry_path, text.into_bytes());
                }
            }
        }
        let Some(jar) = self.workspace.jar() else {
            return;
        };
        let Some(raw) = jar.get(&entry_path) else {
            return;
        };
        let bytes = raw.to_vec();
        let tab = Self::create_tab(&entry_path, &bytes);
        self.layout.editor.open_tab(tab);
    }

    /// 处理拖拽到窗口的文件
    pub(crate) fn handle_dropped_files(&mut self, ctx: &egui::Context) {
        let dropped = ctx.input(|i| i.raw.dropped_files.clone());
        if dropped.is_empty() {
            return;
        }
        // 多 pass 去重
        let frame = ctx.cumulative_frame_nr();
        let cache_id = egui::Id::new("drop_frame");
        let last: Option<u64> = ctx.data(|d| d.get_temp(cache_id));
        if last == Some(frame) {
            return;
        }
        ctx.data_mut(|d| d.insert_temp(cache_id, frame));
        if let Some(path) = dropped.into_iter().find_map(|file| file.path) {
            if is_jar_file(&path) || ark::is_ark_file(&path) {
                self.request_open_jar(&path);
            } else {
                self.open_standalone_file(&path);
            }
        }
    }

    /// 检查后台归档加载是否完成
    pub(crate) fn check_loading(&mut self) {
        let result = {
            let Workspace::Loading(loading) = &mut self.workspace else {
                return;
            };
            match loading.task.poll() {
                Poll::Ready(result) => result,
                Poll::Pending => return,
                Poll::Lost => Err(ArchiveError::parse("JAR loading task disconnected")),
            }
        };
        match result {
            Ok(jar) => self.finish_loading_jar(jar),
            Err(e) => {
                log::error!("Failed to open JAR: {e}");
                self.toasts.error(t!("layout.open_jar_failed", error = e));
                self.workspace = Workspace::Empty;
            }
        }
    }

    /// 轮询 ArkTS 工作区后台加载
    pub(crate) fn check_ark_loading(&mut self) {
        let (name, result) = {
            let Workspace::ArkLoading(loading) = &mut self.workspace else {
                return;
            };

            match loading.task.poll() {
                Poll::Pending => return,
                Poll::Lost => (loading.name.clone(), Err("task disconnected".to_string())),
                Poll::Ready(r) => (loading.name.clone(), r.map_err(|e| e.to_string())),
            }
        };
        match result {
            Ok((jar, meta)) => {
                let paths = jar.paths();
                self.layout
                    .file_panel
                    .set_roots(crate::ui::explorer::tree::build_tree(&jar.name, &paths));
                self.layout.file_panel.selected = None;
                self.settings.add_recent(&jar.path, &jar.name);
                if let Err(e) = self.settings.save() {
                    log::warn!("保存最近打开记录失败: {e}");
                }
                let mut loaded = LoadedState::new(jar);
                loaded.ark = Some(meta);
                self.workspace = Workspace::Loaded(Box::new(loaded));
                self.layout.explorer_visible = true;
                let _ = name;
            }
            Err(e) => {
                log::error!("Failed to open ArkTS file {name}: {e}");
                self.toasts.error(t!("layout.open_jar_failed", error = e));
                self.workspace = Workspace::Empty;
            }
        }
    }

    fn finish_loading_jar(&mut self, jar: JarArchive) {
        let paths = jar.paths();
        self.layout
            .file_panel
            .set_roots(tree::build_tree(&jar.name, &paths));
        self.layout.file_panel.selected = None;
        self.settings.add_recent(&jar.path, &jar.name);
        if let Err(e) = self.settings.save() {
            log::warn!("保存最近打开记录失败: {e}");
        }
        self.workspace = Workspace::Loaded(Box::new(LoadedState::new(jar)));
        self.layout.explorer_visible = true;
    }

    /// 从最近打开列表移除指定路径并持久化
    pub fn remove_recent(&mut self, path: &Path) {
        self.settings.remove_recent(path);
        if let Err(e) = self.settings.save() {
            log::warn!("保存最近打开记录失败: {e}");
        }
    }

    /// 在后台线程打开 JAR 类归档文件
    pub fn open_jar(&mut self, path: &Path) {
        // 前置检查：文件不存在时直接提示，不清空旧状态
        if !path.exists() {
            self.toasts
                .error(t!("layout.file_not_found", path = path.display()));
            self.remove_recent(path);
            return;
        }
        if ark::is_ark_file(path) {
            self.open_ark(path);
            return;
        }
        let progress = Arc::new(LoadProgress::new());
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let path = path.to_path_buf();
        let p = progress.clone();
        let task = Task::spawn(move || JarArchive::open_with_progress(&path, &p));
        self.layout.file_panel.roots = Vec::new();
        self.layout.file_panel.selected = None;
        self.layout.editor = EditorArea::new();
        self.exporting = None;
        self.workspace = Workspace::Loading(LoadingState {
            name,
            progress,
            task,
        });
    }

    /// 在后台线程打开 ArkTS 工作区（abc/HAP/HSP）
    pub fn open_ark(&mut self, path: &Path) {
        let progress = Arc::new(LoadProgress::new());
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let task = ark::spawn_open_ark(path, progress.clone());
        self.layout.file_panel.roots = Vec::new();
        self.layout.file_panel.selected = None;
        self.layout.editor = EditorArea::new();
        self.exporting = None;
        self.workspace = Workspace::ArkLoading(ArkLoadingState {
            name,
            progress,
            task,
        });
    }

    /// 打开文件选择器，返回 (路径, 是否归档类文件)
    pub(crate) fn pick_file() -> Option<(std::path::PathBuf, bool)> {
        let jar_label = t!("layout.java_archive");
        let ark_label = t!("layout.ark_archive");
        rfd::FileDialog::new()
            .add_filter(&*jar_label, &["jar", "zip", "war", "ear"])
            .add_filter(&*ark_label, &["hap", "hsp", "abc", "har", "app"])
            .pick_file()
            .map(|path| {
                let is_archive = is_jar_file(&path) || ark::is_ark_file(&path);
                (path, is_archive)
            })
    }

    /// 启动新进程实例打开指定文件
    pub(crate) fn spawn_new_window(&mut self, path: &Path) {
        match std::env::current_exe() {
            Ok(exe) => {
                if let Err(e) = std::process::Command::new(&exe).arg(path).spawn() {
                    log::error!("Failed to spawn new window: {e}");
                    self.toasts
                        .error(t!("layout.spawn_window_failed", error = e));
                }
            }
            Err(e) => {
                log::error!("Cannot determine executable path: {e}");
                self.toasts
                    .error(t!("layout.spawn_window_failed", error = e));
            }
        }
    }

    /// 打开文件对话框选择归档或独立文件
    pub fn open_jar_dialog(&mut self) {
        if let Some((path, is_jar)) = Self::pick_file() {
            if is_jar {
                self.open_jar(&path);
            } else {
                self.open_standalone_file(&path);
            }
        }
    }

    /// 打开独立文件（非归档条目）为 tab，保存时直接写回磁盘
    pub fn open_standalone_file(&mut self, path: &Path) {
        let path_str = path.to_string_lossy().to_string();
        // 已打开则聚焦
        if self.layout.editor.focus_tab(&path_str) {
            return;
        }
        let bytes = match std::fs::read(path) {
            Ok(b) => b,
            Err(e) => {
                log::error!("Failed to read file: {path_str}: {e}");
                self.toasts.error(t!("layout.open_file_failed", error = e));
                return;
            }
        };
        let mut tab = Self::create_tab(&path_str, &bytes);
        tab.standalone_path = Some(path.to_path_buf());
        self.layout.editor.open_tab(tab);
    }
}

/// 判断路径是否为 JAR 类归档文件
fn is_jar_file(path: &Path) -> bool {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    matches!(
        ext.to_ascii_lowercase().as_str(),
        "jar" | "zip" | "war" | "ear"
    )
}

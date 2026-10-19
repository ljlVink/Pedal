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
        let Some(jar) = self.workspace.jar() else {
            return;
        };
        let Some(raw) = jar.get(&entry_path) else {
            return;
        };
        let bytes = raw.to_vec();
        let mut tab = Self::create_tab(&entry_path, &bytes);
        if let Some(ark) = self
            .workspace
            .loaded()
            .and_then(|s| s.ark.as_ref())
            .filter(|ark| ark.disasm_targets.contains_key(&entry_path))
        {
            tab.is_ark = true;
            tab.is_text = false;
            if let Some(label) = ark.record_labels.get(&entry_path) {
                tab.title = label.clone();
            }
        }
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
                let mut roots = tree::build_tree(&jar.name, &paths);
                tree::apply_file_labels(&mut roots, &meta.record_labels);
                self.layout.file_panel.set_roots(roots);
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
        let ark_label = t!("layout.ark_archive");
        rfd::FileDialog::new()
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::Settings;
    use crate::ui::editor::view_toggle::ActiveView;
    use crate::ui::layout::Layout;

    fn app_with_workspace(loaded: LoadedState) -> App {
        let settings = Settings::default();
        App {
            layout: Layout::new(&settings),
            settings,
            toasts: Default::default(),
            pending_confirm: None,
            workspace: Workspace::Loaded(Box::new(loaded)),
            exporting: None,
        }
    }

    #[test]
    fn abc_explorer_opens_readonly_source_and_enables_disassembly() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("crates/pedal-abc-bridge/tests/fixtures/advanced_recovery/debug.abc");
        let (jar, meta) = ark::open_ark(&path, &LoadProgress::new()).unwrap();
        assert!(
            meta.sources
                .values()
                .all(|source| !source.trim().is_empty())
        );
        let mut loaded = LoadedState::new(jar);
        loaded.ark = Some(meta);
        let mut app = app_with_workspace(loaded);

        let path = "debug.abc/AdvancedRecovery.ets";
        app.layout.file_panel.pending_open = Some(path.into());
        app.handle_pending_open();
        let tab = app.layout.editor.focused_tab().unwrap();
        assert_eq!(tab.entry_path.as_deref(), Some(path));
        assert!(tab.decompiled.contains("class RecoveryBase"));
        assert!(tab.is_ark);
        assert!(!tab.is_text);
        assert!(tab.active_view == ActiveView::Decompiled);
        app.layout.editor.cycle_view();
        assert!(app.layout.editor.focused_tab().unwrap().active_view == ActiveView::Disasm);

        // 编译器注解也能打开，有说明和成员名称，不再显示空白。
        app.layout.file_panel.pending_open = Some("debug.abc/_ESSlotNumberAnnotation.ets".into());
        app.handle_pending_open();
        let tab = app.layout.editor.focused_tab().unwrap();
        assert!(tab.is_ark);
        assert!(tab.decompiled.contains("_ESSlotNumberAnnotation"));
        assert!(tab.decompiled.lines().count() >= 2);

        // 原始 ABC 仍是二进制，不被误判为可编辑源码。
        app.layout.file_panel.pending_open = Some("debug.abc".into());
        app.handle_pending_open();
        let tab = app.layout.editor.focused_tab().unwrap();
        assert!(!tab.is_ark);
        assert!(tab.active_view == ActiveView::Hex);
    }

    #[test]
    fn regular_archive_ets_and_panda_entries_still_open_as_text() {
        let entries = [
            ("Index.ets".into(), b"const greeting = 'hello';".to_vec()),
            ("Index.panda".into(), b".record Index".to_vec()),
        ]
        .into_iter()
        .collect();
        let jar = JarArchive::from_entries(
            "source.zip".into(),
            "source.zip".into(),
            String::new(),
            0,
            entries,
        );
        let mut app = app_with_workspace(LoadedState::new(jar));
        for path in ["Index.ets", "Index.panda"] {
            app.layout.file_panel.pending_open = Some(path.into());
            app.handle_pending_open();
            let tab = app.layout.editor.focused_tab().unwrap();
            assert_eq!(tab.entry_path.as_deref(), Some(path));
            assert!(tab.is_text);
            assert!(!tab.is_ark);
            assert!(!tab.decompiled.is_empty());
        }
    }

    #[test]
    fn explorer_click_opens_in_the_same_frame_without_restoring_old_selection() {
        let entries = [
            ("First.ets".into(), b"const first = 1;".to_vec()),
            ("Second.ets".into(), b"const second = 2;".to_vec()),
        ]
        .into_iter()
        .collect();
        let jar = JarArchive::from_entries(
            "source.zip".into(),
            "source.zip".into(),
            String::new(),
            0,
            entries,
        );
        let roots = tree::build_tree(&jar.name, &jar.paths());
        let mut app = app_with_workspace(LoadedState::new(jar));
        app.layout.file_panel.set_roots(roots);
        app.layout.explorer_visible = true;
        app.layout.file_panel.pending_open = Some("First.ets".into());
        app.handle_pending_open();

        let ctx = egui::Context::default();
        let mut fonts = egui::FontDefinitions::default();
        fonts.families.insert(
            crate::appearance::codicon::family(),
            fonts.families[&egui::FontFamily::Proportional].clone(),
        );
        ctx.set_fonts(fonts);
        let shell = crate::appearance::theme::shell_theme();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0));
        let _ = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(screen),
                ..Default::default()
            },
            |ui| app.render(ui, &shell),
        );
        let row = ctx
            .read_response(egui::Id::new("tree_row").with(2usize))
            .unwrap()
            .rect;
        let pos = egui::pos2(row.left() + 80.0, row.center().y);
        for pressed in [true, false] {
            let _ = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(screen),
                    events: vec![
                        egui::Event::PointerMoved(pos),
                        egui::Event::PointerButton {
                            pos,
                            button: egui::PointerButton::Primary,
                            pressed,
                            modifiers: egui::Modifiers::NONE,
                        },
                    ],
                    ..Default::default()
                },
                |ui| app.render(ui, &shell),
            );
        }
        assert_eq!(
            app.layout
                .editor
                .focused_tab()
                .unwrap()
                .entry_path
                .as_deref(),
            Some("Second.ets")
        );
        assert_eq!(
            app.layout.file_panel.selected.as_deref(),
            Some("Second.ets")
        );
        assert!(app.layout.file_panel.pending_open.is_none());
        assert_eq!(app.layout.editor.dock_state.iter_all_tabs().count(), 2);
    }

    #[test]
    fn camera_union_records_open_search_and_export_without_descriptor_directories() {
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("test_examples/system_framework/camera_framework_taihe_abc.abc");
        if !fixture.exists() {
            eprintln!("optional camera framework corpus is absent");
            return;
        }
        let (jar, meta) = ark::open_ark(&fixture, &LoadProgress::new()).unwrap();
        assert_eq!(meta.record_labels.len(), 4);
        assert_eq!(meta.sources.len(), meta.abcs[0].1.classes.len());
        let labels = meta.record_labels.clone();
        let mut roots = tree::build_tree(&jar.name, &jar.paths());
        tree::apply_file_labels(&mut roots, &labels);
        let filter_index = tree::build_filter_index(&roots);
        assert!(!filter_index.iter().any(|e| e.label_lower.contains("{ul")));
        let mut loaded = LoadedState::new(jar);
        loaded.ark = Some(meta);
        let mut app = app_with_workspace(loaded);
        app.rebuild_search_index();

        let output = std::env::temp_dir().join(format!(
            "pedal-union-export-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&output).unwrap();
        for (path, label) in &labels {
            assert_eq!(
                path.matches('/').count(),
                1,
                "union must be one leaf: {path}"
            );
            assert!(!path.contains('|'));
            assert!(filter_index.iter().any(|e| e.path == *path
                && !e.is_folder
                && e.label_lower == label.to_ascii_lowercase()));
            app.layout.file_panel.pending_open = Some(path.clone());
            app.handle_pending_open();
            let tab = app.layout.editor.focused_tab().unwrap();
            assert_eq!(tab.title, *label);
            assert_eq!(tab.entry_path.as_deref(), Some(path.as_str()));
            assert!(tab.is_ark && !tab.decompiled.is_empty());
            assert!(tab.decompiled.contains(" | "));
            let loaded = app.workspace.loaded().unwrap();
            let ark = loaded.ark.as_ref().unwrap();
            let (abc_idx, class_off) = ark.disasm_targets[path];
            assert!(
                ark.abcs[abc_idx]
                    .1
                    .classes
                    .iter()
                    .any(|c| c.offset == class_off && c.name.starts_with("{U"))
            );
            assert!(
                loaded
                    .search_index
                    .as_ref()
                    .unwrap()
                    .entries
                    .iter()
                    .any(|e| e.entry_path == *path && format!("{}.ets", e.class_name) == *label)
            );
            // 使用实际虚拟路径执行磁盘写入，覆盖 Windows 文件名限制。
            let exported = output.join(path);
            std::fs::create_dir_all(exported.parent().unwrap()).unwrap();
            std::fs::write(&exported, &ark.sources[path]).unwrap();
            assert_eq!(
                std::fs::read_to_string(&exported).unwrap(),
                ark.sources[path]
            );
            std::fs::remove_file(exported).unwrap();
        }
        std::fs::remove_dir(output.join("camera_framework_taihe_abc.abc")).unwrap();
        std::fs::remove_dir(output).unwrap();
        assert!(
            labels
                .values()
                .any(|label| label == "(BusinessError | null).ets")
        );
        assert!(
            labels
                .values()
                .any(|label| label == "(Image | Picture).ets")
        );
        assert!(labels.values().any(|label| label == "(null | Object).ets"));
    }
}

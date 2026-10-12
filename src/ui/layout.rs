//! 主布局：UI 状态 + 渲染
//!
//! @author sky
//! @author ljlVink

use crate::app::workspace::{LoadedState, Workspace};
use crate::app::{App, ConfirmAction};
use crate::appearance::theme;
use crate::settings::{self, Settings};
use eframe::egui;
use egui_keybind::KeyMap;
use egui_shell::components::{SettingsFile, SettingsPanel};
use rust_i18n::t;
use std::sync::atomic::Ordering;

use super::editor::EditorArea;
use super::explorer::{FileAction, FilePanel, tree};
use super::search::SearchDialog;
use super::status_bar::StatusBar;

/// 布局区域矩形
struct LayoutRects {
    /// 资源管理器面板区域
    explorer: egui::Rect,
    /// 编辑器区域
    editor: egui::Rect,
    /// 状态栏区域
    status: egui::Rect,
}

tabookit::class! {
    /// UI 布局状态
    pub struct Layout {
        /// 文件资源管理器面板
        pub file_panel: FilePanel,
        /// 编辑器区域（tab 管理 + 代码视图）
        pub editor: EditorArea,
        /// 底部状态栏
        pub status_bar: StatusBar,
        /// 搜索浮动面板
        pub search: SearchDialog,
        /// 设置面板
        pub settings_panel: SettingsPanel<Settings>,
        /// 快捷键映射
        keys: KeyMap<App>,
        /// Explorer 面板当前宽度（可拖拽调整）
        explorer_width: f32,
        /// Explorer 面板是否可见
        pub explorer_visible: bool,
        /// FPS 叠加层开关（F12）
        show_fps: bool,
        /// 关于对话框是否打开
        pub about_open: bool,
    }

    pub fn new(settings: &Settings) -> Self {
        let keys = super::keybindings::build_keymap(&settings.keymap);
        Self {
            file_panel: FilePanel::new(),
            editor: EditorArea::new(),
            status_bar: StatusBar::default(),
            search: SearchDialog::new(),
            settings_panel: settings::new_panel(),
            keys,
            explorer_width: theme::FILE_PANEL_WIDTH,
            explorer_visible: false,
            show_fps: false,
            about_open: false,
        }
    }
}

// ─── 渲染逻辑（impl App 定义在 UI 模块，访问 self.layout.* 进行绘制）───

impl App {
    /// 在 CentralPanel 内绘制完整布局
    pub fn render(&mut self, ui: &mut egui::Ui, shell_theme: &egui_shell::ShellTheme) {
        self.intercept_close(ui.ctx());
        self.handle_dropped_files(ui.ctx());
        self.handle_pending_open();
        self.handle_pending_reveal();
        self.poll_tasks();
        self.dispatch_keybinds(ui.ctx());
        self.render_panels(ui);
        self.render_overlays(ui, shell_theme);
    }

    /// 拦截窗口关闭请求（有未保存变更时弹确认）
    fn intercept_close(&mut self, ctx: &egui::Context) {
        if ctx.input(|i| i.viewport().close_requested()) && self.has_unsaved_changes() {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.pending_confirm = Some(ConfirmAction::Close);
        }
    }

    /// 轮询所有后台任务
    fn poll_tasks(&mut self) {
        self.check_loading();
        self.check_ark_loading();
        let loading = matches!(
            self.workspace,
            Workspace::Loading(_) | Workspace::ArkLoading(_)
        );
        if !loading {
            self.poll_export_jar();
            self.rebuild_search_index();
        }
    }

    /// 分发快捷键（录制中跳过）
    fn dispatch_keybinds(&mut self, ctx: &egui::Context) {
        if egui_shell::components::panel::is_recording_keybind(ctx) {
            return;
        }
        let view_before = self.layout.editor.focused_view();
        let mut keys = std::mem::take(&mut self.layout.keys);
        keys.dispatch(ctx, self);
        self.layout.keys = keys;
        // Tab 切换视图后清除焦点，防止 begin_pass 焦点导航产生的闪烁
        if self.layout.editor.focused_view() != view_before {
            ctx.memory_mut(|m| m.stop_text_input());
        }
        self.collect_blocked_close();
    }

    /// 渲染三大面板（explorer + editor + status bar）
    fn render_panels(&mut self, ui: &mut egui::Ui) {
        let explorer_effective = self.layout.explorer_visible && self.workspace.is_loaded();
        let t = self.explorer_anim_t(ui, explorer_effective);
        let rects = compute_rects(ui.max_rect(), self.layout.explorer_width * t, t > 0.0);
        if t > 0.0 {
            self.render_explorer(ui, rects.explorer);
        }
        if self.layout.explorer_visible && t >= 1.0 {
            self.render_resize_handle(ui, &rects);
        }
        self.render_editor(ui, rects.editor);
        self.collect_blocked_close();
        self.sync_explorer_selection();
        self.render_status_bar(ui, rects.status);
    }

    /// 渲染浮动层（搜索、设置、通知、FPS、确认弹窗）
    fn render_overlays(&mut self, ui: &mut egui::Ui, shell_theme: &egui_shell::ShellTheme) {
        let search_index = self.workspace.loaded().and_then(|s| s.search_index.clone());
        let has_jar = self.workspace.is_loaded();
        self.layout.search.sync_state(search_index, has_jar, false);
        self.layout.search.render(ui.ctx(), shell_theme);
        if let Some(req) = self.layout.search.take_pending_open()
            && !self
                .layout
                .editor
                .focus_tab_at(&req.entry_path, Some(req.line))
        {
            let path = req.entry_path.clone();
            self.layout.file_panel.pending_open = Some(req.entry_path);
            self.handle_pending_open();
            // 新开的 tab 也需要设置滚动目标
            self.layout.editor.focus_tab_at(&path, Some(req.line));
        }
        let output = settings::show(&mut self.layout.settings_panel, ui.ctx(), shell_theme);
        if let Some(new_settings) = output.settings {
            self.apply_settings(new_settings);
            ui.ctx().request_repaint();
        }
        self.toasts.show(ui.ctx());
        self.render_about(ui.ctx());
        if ui.input(|i| i.key_pressed(egui::Key::F12)) {
            self.layout.show_fps = !self.layout.show_fps;
        }
        if self.layout.show_fps {
            self.render_fps_overlay(ui);
        }
        self.render_confirm(ui.ctx());
    }

    /// 关于对话框
    fn render_about(&mut self, ctx: &egui::Context) {
        if !self.layout.about_open {
            return;
        }
        let screen = ctx.content_rect();
        let size = egui::vec2(400.0, 250.0);
        let pos = (screen.center() - size / 2.0).max(screen.left_top());
        let mut open = true;
        let mut close_clicked = false;
        egui::Window::new(t!("menu.about"))
            .resizable(false)
            .collapsible(false)
            .default_pos(pos)
            .fixed_size(size)
            .open(&mut open)
            .show(ctx, |ui| {
                ui.add_space(6.0);
                ui.heading("Pedal");
                ui.label(format!("version {}", env!("CARGO_PKG_VERSION")));
                ui.add_space(8.0);
                ui.label(t!("about.description"));
                ui.add_space(8.0);
                ui.label(t!("about.license"));
                ui.with_layout(egui::Layout::bottom_up(egui::Align::RIGHT), |ui| {
                    if ui.button(t!("about.close")).clicked() {
                        close_clicked = true;
                    }
                });
            });
        self.layout.about_open = open && !close_clicked;
    }

    /// 应用新设置（keybind / 语言 / 持久化）
    fn apply_settings(&mut self, new_settings: Settings) {
        self.layout.keys = super::keybindings::build_keymap(&new_settings.keymap);
        if new_settings.language != self.settings.language {
            rust_i18n::set_locale(new_settings.language.code());
        }
        self.settings = new_settings;
        if let Err(e) = self.settings.save() {
            log::warn!("配置保存失败: {e}");
        }
    }

    /// 收集 egui_dock 渲染期间产生的 blocked_close 事件
    fn collect_blocked_close(&mut self) {
        if let Some(action) = self.layout.editor.blocked_close.take()
            && self.pending_confirm.is_none()
        {
            self.pending_confirm = Some(ConfirmAction::TabClose(action));
        }
    }

    /// 左上角 FPS 叠加层（调试用）
    fn render_fps_overlay(&self, ui: &egui::Ui) {
        let dt = ui.input(|i| i.stable_dt);
        let fps = if dt > 0.0 { 1.0 / dt } else { 0.0 };
        let text = format!("{fps:.0} fps  {:.1} ms", dt * 1000.0);
        let pos = ui.max_rect().left_top() + egui::vec2(8.0, 8.0);
        let font = egui::FontId::monospace(11.0);
        let painter = ui.painter();
        // 半透明背景
        let galley = painter.layout_no_wrap(text, font, egui::Color32::WHITE);
        let bg = egui::Rect::from_min_size(
            pos - egui::vec2(4.0, 2.0),
            galley.size() + egui::vec2(8.0, 4.0),
        );
        painter.rect_filled(bg, 3.0, egui::Color32::from_black_alpha(160));
        painter.galley(pos, galley, egui::Color32::WHITE);
    }

    /// 获取 explorer 折叠/展开动画插值（同一帧内缓存）
    fn explorer_anim_t(&self, ui: &egui::Ui, visible: bool) -> f32 {
        let anim_id = ui.id().with("explorer_anim");
        self.cached_anim_t(ui, anim_id, visible)
    }

    /// 渲染 explorer 面板
    fn render_explorer(&mut self, ui: &mut egui::Ui, rect: egui::Rect) {
        egui_shell::components::widget::island::paint(ui, rect, &theme::ISLAND);
        let (tab_modified, jar_modified) = self.split_modified_entries();
        let mut child = ui.new_child(
            egui::UiBuilder::new()
                .id(egui::Id::new("explorer_island"))
                .max_rect(rect),
        );
        child.set_clip_rect(rect);
        self.layout
            .file_panel
            .render(&mut child, &tab_modified, &jar_modified);
        self.handle_file_panel_action();
        egui_shell::components::widget::island::paint_corner_mask(ui, rect, &theme::ISLAND);
    }

    fn handle_file_panel_action(&mut self) {
        let Some(action) = self.layout.file_panel.take_file_action() else {
            return;
        };
        match action {
            FileAction::Delete(path) => self.delete_jar_entry(path),
            FileAction::Rename { old_path, new_path } => self.rename_jar_entry(old_path, new_path),
        }
    }

    fn delete_jar_entry(&mut self, path: String) {
        if self
            .layout
            .editor
            .unsaved_paths()
            .iter()
            .any(|entry| entry == &path)
        {
            self.toasts.warning(t!("layout.entry_unsaved", path = path));
            return;
        }
        let Some((jar_name, paths)) = self.delete_loaded_jar_entry(&path) else {
            self.toasts.error(t!("layout.entry_not_found", path = path));
            return;
        };
        self.layout.editor.close_entry_tab(&path);
        if self.layout.file_panel.selected.as_deref() == Some(&path) {
            self.layout.file_panel.selected = None;
        }
        self.rebuild_file_tree(&jar_name, &paths);
        self.toasts.info(t!("layout.entry_deleted", path = path));
    }

    fn rename_jar_entry(&mut self, old_path: String, new_path: String) {
        let Some(new_path) = normalize_entry_path(&new_path) else {
            self.toasts.error(t!("layout.entry_invalid_path"));
            return;
        };
        if old_path == new_path {
            return;
        }
        if self
            .layout
            .editor
            .unsaved_paths()
            .iter()
            .any(|entry| entry == &old_path)
        {
            self.toasts
                .warning(t!("layout.entry_unsaved", path = old_path));
            return;
        }
        let Some((jar_name, paths)) = self.rename_loaded_jar_entry(&old_path, &new_path) else {
            self.toasts
                .error(t!("layout.entry_rename_failed", path = old_path));
            return;
        };
        self.layout.editor.rename_entry_tab(&old_path, &new_path);
        if self.layout.file_panel.selected.as_deref() == Some(&old_path) {
            self.layout.file_panel.selected = Some(new_path.clone());
            self.layout.file_panel.scroll_to_selected = true;
        }
        self.rebuild_file_tree(&jar_name, &paths);
        tree::reveal(&mut self.layout.file_panel.roots, &new_path);
        self.toasts
            .info(t!("layout.entry_renamed", old = old_path, new = new_path));
    }

    fn delete_loaded_jar_entry(&mut self, path: &str) -> Option<(String, Vec<String>)> {
        let loaded = self.workspace.loaded_mut()?;
        if !loaded.jar.delete(path) {
            return None;
        }
        let paths = refresh_loaded_entry_indexes(loaded);
        Some((loaded.jar.name.clone(), paths))
    }

    fn rename_loaded_jar_entry(
        &mut self,
        old_path: &str,
        new_path: &str,
    ) -> Option<(String, Vec<String>)> {
        let loaded = self.workspace.loaded_mut()?;
        if loaded.jar.get(old_path).is_none() || loaded.jar.get(new_path).is_some() {
            return None;
        }
        if !loaded.jar.rename(old_path, new_path) {
            return None;
        }
        let paths = refresh_loaded_entry_indexes(loaded);
        Some((loaded.jar.name.clone(), paths))
    }

    fn rebuild_file_tree(&mut self, jar_name: &str, paths: &[String]) {
        let path_refs: Vec<&str> = paths.iter().map(String::as_str).collect();
        self.layout
            .file_panel
            .set_roots(tree::build_tree(jar_name, &path_refs));
    }

    /// 渲染 editor 面板
    fn render_editor(&mut self, ui: &mut egui::Ui, rect: egui::Rect) {
        egui_shell::components::widget::island::paint(ui, rect, &theme::ISLAND);
        if let Workspace::Loading(loading) = &self.workspace {
            let current = loading.progress.current.load(Ordering::Relaxed);
            let total = loading.progress.total.load(Ordering::Relaxed);
            paint_loading(ui, rect, &loading.name, current, total);
            ui.ctx().request_repaint();
        } else if let Workspace::ArkLoading(loading) = &self.workspace {
            let current = loading.progress.current.load(Ordering::Relaxed);
            let total = loading.progress.total.load(Ordering::Relaxed);
            paint_loading(ui, rect, &loading.name, current, total);
            ui.ctx().request_repaint();
        } else {
            let jar_modified = self
                .workspace
                .jar()
                .map(|j| j.modified_paths().map(|s| s.to_string()).collect())
                .unwrap_or_default();
            self.layout.editor.render(
                &mut ui.new_child(
                    egui::UiBuilder::new()
                        .id(egui::Id::new("editor_island"))
                        .max_rect(rect),
                ),
                &jar_modified,
            );
            if let Some(entry) = self.layout.editor.pending_disasm.take() {
                self.generate_ark_disasm(&entry);
            }
        }
        egui_shell::components::widget::island::paint_corner_mask(ui, rect, &theme::ISLAND);
    }

    /// 懒生成 ArkTS record 的反汇编文本并填充到对应 tab
    fn generate_ark_disasm(&mut self, entry_path: &str) {
        let text = self
            .workspace
            .loaded_mut()
            .and_then(|s| s.ark.as_mut())
            .and_then(|ark| ark.build_disasm_entry(entry_path));
        if let Some(text) = text {
            self.layout.editor.set_tab_disasm(entry_path, text);
        }
    }

    /// 在 explorer 和 editor 之间的 gap 区域绘制可拖拽的 resize 手柄
    fn render_resize_handle(&mut self, ui: &mut egui::Ui, rects: &LayoutRects) {
        let handle_rect = egui::Rect::from_min_max(
            egui::pos2(rects.explorer.right() - 2.0, rects.explorer.top()),
            egui::pos2(rects.editor.left() + 2.0, rects.explorer.bottom()),
        );
        let id = ui.id().with("explorer_resize");
        let sense = ui.interact(handle_rect, id, egui::Sense::drag());
        if sense.dragged() {
            let delta = sense.drag_delta().x;
            self.layout.explorer_width = (self.layout.explorer_width + delta)
                .clamp(theme::EXPLORER_MIN_WIDTH, theme::EXPLORER_MAX_WIDTH);
        }
        if sense.dragged() || sense.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeColumn);
        }
    }

    /// 渲染 status bar
    fn render_status_bar(&mut self, ui: &mut egui::Ui, rect: egui::Rect) {
        self.sync_status_bar(ui.ctx());
        self.layout.status_bar.render(
            &mut ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(rect)
                    .id(egui::Id::new("status_bar")),
            ),
        );
        if let Some(v) = self.layout.status_bar.take_view_change() {
            self.layout.editor.set_focused_view(v);
        }
        if let Some(path) = self.layout.status_bar.take_clicked_file()
            && !self.layout.editor.focus_tab(&path)
        {
            self.layout.file_panel.pending_open = Some(path);
        }
    }

    /// 获取动画插值，同一帧内缓存结果避免多 pass 间抖动
    fn cached_anim_t(&self, ui: &egui::Ui, anim_id: egui::Id, visible: bool) -> f32 {
        let ctx = ui.ctx();
        let frame = ctx.cumulative_frame_nr();
        let cache_id = anim_id.with("frame_cache");
        let cached: Option<(u64, f32)> = ctx.data(|d| d.get_temp(cache_id));
        if let Some((f, t)) = cached
            && f == frame
        {
            return t;
        }
        let t = ctx.animate_bool_with_time(anim_id, visible, theme::EXPLORER_ANIM_DURATION);
        ctx.data_mut(|d| d.insert_temp::<(u64, f32)>(cache_id, (frame, t)));
        t
    }
}

/// 条目路径变化后刷新依赖路径列表的索引。
fn refresh_loaded_entry_indexes(loaded: &mut LoadedState) -> Vec<String> {
    let paths: Vec<String> = loaded.jar.paths().into_iter().map(str::to_string).collect();
    loaded.search_index = None;
    paths
}

/// 将用户输入规范化为归档条目路径。
fn normalize_entry_path(input: &str) -> Option<String> {
    let path = input.trim().replace('\\', "/");
    let path = path.trim_start_matches('/').to_string();
    if path.is_empty() || path.ends_with('/') {
        return None;
    }
    if path
        .split('/')
        .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return None;
    }
    Some(path)
}

/// 从总 rect 计算各区域的 rect
fn compute_rects(total: egui::Rect, explorer_width: f32, explorer_visible: bool) -> LayoutRects {
    let mh = theme::ISLAND_MARGIN_H;
    let mv = theme::ISLAND_MARGIN_V;
    let status_top = total.bottom() - theme::STATUS_BAR_HEIGHT - mv;
    let island_top = total.top() + mv * 0.5;
    let island_bottom = status_top - mv;
    let island_left = total.left() + mh;
    let island_right = total.right() - mh;
    let (explorer, editor_left) = if explorer_visible {
        let explorer = egui::Rect::from_min_max(
            egui::pos2(island_left, island_top),
            egui::pos2(island_left + explorer_width, island_bottom),
        );
        (explorer, explorer.right() + theme::ISLAND_GAP)
    } else {
        (egui::Rect::NOTHING, island_left)
    };
    let editor = egui::Rect::from_min_max(
        egui::pos2(editor_left, island_top),
        egui::pos2(island_right, island_bottom),
    );
    let status = egui::Rect::from_min_size(
        egui::pos2(total.left(), status_top),
        egui::vec2(total.width(), theme::STATUS_BAR_HEIGHT),
    );
    LayoutRects {
        explorer,
        editor,
        status,
    }
}

/// 绘制归档加载进度
fn paint_loading(ui: &egui::Ui, rect: egui::Rect, name: &str, current: u32, total: u32) {
    let frac = if total > 0 {
        current as f32 / total as f32
    } else {
        0.0
    };
    let painter = ui.painter();
    let cx = rect.center().x;
    let cy = rect.center().y;
    // 标题
    painter.text(
        egui::pos2(cx, cy - 20.0),
        egui::Align2::CENTER_CENTER,
        t!("layout.opening", name = name),
        egui::FontId::proportional(13.0),
        theme::TEXT_SECONDARY,
    );
    // 进度条
    let bar_w = 240.0;
    let bar_h = 4.0;
    let bar_rect = egui::Rect::from_center_size(egui::pos2(cx, cy), egui::vec2(bar_w, bar_h));
    painter.rect_filled(bar_rect, 2.0, theme::BG_MEDIUM);
    if frac > 0.0 {
        let fill = egui::Rect::from_min_size(bar_rect.left_top(), egui::vec2(bar_w * frac, bar_h));
        painter.rect_filled(fill, 2.0, theme::VERDIGRIS);
    }
    // 计数
    if total > 0 {
        painter.text(
            egui::pos2(cx, cy + 16.0),
            egui::Align2::CENTER_CENTER,
            format!("{current} / {total}"),
            egui::FontId::proportional(11.0),
            theme::TEXT_MUTED,
        );
    }
}

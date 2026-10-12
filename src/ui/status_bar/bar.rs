//! 状态栏服务：自管理 items 注册、渲染、与编辑器的状态同步
//!
//! @author sky
//! @author ljlVink

use super::export_progress::ExportProgressItem;
use super::modified_count::ModifiedCountItem;
use super::view_toggle::ViewToggleItem;
use crate::appearance::theme;
use crate::ui::editor::view_toggle::ActiveView;
use eframe::egui;
use egui_shell::components::panel::status_bar::{Alignment, StatusBarWidget, StatusItem, TextItem};
use rust_i18n::t;

tabookit::class! {
    /// 状态栏服务
    ///
    /// 内置默认 items（应用版本、视图切换、修改计数与导出进度），
    /// 外部只需调用 `render` 和 `sync` 即可。
    pub struct StatusBar {
        widget: StatusBarWidget,
    }

    /// 获取指定类型的 item 可变引用
    fn item_mut<T: StatusItem>(&mut self) -> Option<&mut T> {
        self.widget.item_mut::<T>()
    }

    /// 同步编辑器状态（ViewToggle + context_only items 可见性）
    pub fn sync_view(&mut self, active_view: Option<ActiveView>) {
        let has_tab = active_view.is_some();
        for item in self.widget.items_mut() {
            if let Some(text) = item.downcast_mut::<TextItem>()
                && text.is_context_only() {
                    text.set_visible(has_tab);
                }
            // 视图切换：ArkTS 源码|反汇编
            if let Some(vt) = item.downcast_mut::<ViewToggleItem>() {
                vt.set_active(active_view);
            }
        }
    }

    /// 取出用户通过 ViewToggle 切换的新视图（渲染后调用）
    pub fn take_view_change(&mut self) -> Option<ActiveView> {
        self.item_mut::<ViewToggleItem>()?.take_changed()
    }

    /// 同步已修改文件路径
    pub fn sync_modified_count(&mut self, saved: Vec<String>, unsaved: Vec<String>) {
        if let Some(item) = self.item_mut::<ModifiedCountItem>() {
            item.set_paths(saved, unsaved);
        }
    }

    /// 同步归档导出进度，None 表示无任务
    pub fn sync_export(&mut self, info: Option<(u32, u32)>) {
        if let Some(item) = self.item_mut::<ExportProgressItem>() {
            item.set_progress(info);
        }
    }

    /// 取出弹出列表中用户点击的文件路径
    pub fn take_clicked_file(&mut self) -> Option<String> {
        self.item_mut::<ModifiedCountItem>()?.take_clicked()
    }

    /// 渲染状态栏
    pub fn render(&mut self, ui: &mut egui::Ui) {
        self.widget.render(ui);
    }
}

impl Default for StatusBar {
    fn default() -> Self {
        let mut widget = StatusBarWidget::new(theme::status_bar_theme());
        widget.add(TextItem::new(
            t!("status.version"),
            theme::TEXT_MUTED,
            Alignment::Left,
        ));
        widget.add(ModifiedCountItem::new());
        widget.add(ViewToggleItem::new());
        widget.add(ExportProgressItem::new());
        Self { widget }
    }
}

//! 状态栏服务：自管理 items 注册、渲染、与编辑器的状态同步
//!
//! @author sky
//! @author ljlVink

use super::export_progress::ExportProgressItem;
use super::modified_count::ModifiedCountItem;
use super::view_toggle;
use crate::appearance::theme;
use crate::ui::editor::view_toggle::ActiveView;
use eframe::egui;
use egui_shell::components::panel::status_bar::{StatusBarWidget, StatusItem};
use rust_i18n::t;

tabookit::class! {
    /// 状态栏服务
    ///
    /// 内置默认 items（应用版本、修改计数与导出进度），
    /// 外部只需调用 `render` 和 `sync` 即可。
    pub struct StatusBar {
        widget: StatusBarWidget,
    }

    /// 获取指定类型的 item 可变引用
    fn item_mut<T: StatusItem>(&mut self) -> Option<&mut T> {
        self.widget.item_mut::<T>()
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

    /// 按编辑区左边缘放置切换按钮，版本号和其余状态信息避开按钮区域。
    pub fn render(
        &mut self,
        ui: &mut egui::Ui,
        active_view: Option<&mut ActiveView>,
        editor_left: f32,
    ) {
        self.widget.set_theme(theme::status_bar_theme());
        let rect = ui.max_rect();
        ui.painter().rect_filled(rect, 0.0, theme::palette().bg_dark);
        let version = ui.painter().layout_no_wrap(
            t!("status.version").to_string(),
            egui::FontId::proportional(11.0),
            theme::palette().text_muted,
        );
        let mut version_x = rect.left() + 12.0;
        let mut controls_right = version_x;
        if let Some(view) = active_view {
            let controls_x = (editor_left + 8.0).max(version_x);
            controls_right = controls_x + view_toggle::show(ui, view, controls_x);
            // 资源管理器收起或正在收起时，将版本号顺延到按钮右侧。
            if version_x + version.size().x + 16.0 > controls_x {
                version_x = controls_right + 16.0;
            }
        }
        let info_left = (version_x + version.size().x).max(controls_right) + 4.0;
        ui.painter().galley(
            egui::pos2(version_x, rect.center().y - version.size().y / 2.0),
            version,
            theme::palette().text_muted,
        );
        let mut info_rect = rect;
        info_rect.min.x = info_left.min(rect.right());
        self.widget.render(&mut ui.new_child(
            egui::UiBuilder::new().id_salt("status_info").max_rect(info_rect),
        ));
    }
}

impl Default for StatusBar {
    fn default() -> Self {
        let mut widget = StatusBarWidget::new(theme::status_bar_theme());
        widget.add(ModifiedCountItem::new());
        widget.add(ExportProgressItem::new());
        Self { widget }
    }
}

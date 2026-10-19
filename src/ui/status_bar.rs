//! 状态栏模块
//!
//! @author sky
//! @author ljlVink

mod bar;
mod export_progress;
mod modified_count;
mod view_toggle;

use crate::app::App;
pub use bar::StatusBar;
use eframe::egui;

impl App {
    /// 将 App 业务状态同步到 StatusBar 显示状态
    pub(crate) fn sync_status_bar(&mut self, ctx: &egui::Context) {
        let saved_paths: Vec<String> = self
            .workspace
            .jar()
            .map(|j| j.modified_paths().map(|s| s.to_string()).collect())
            .unwrap_or_default();
        let unsaved_paths = self.layout.editor.unsaved_paths();
        self.layout
            .status_bar
            .sync_modified_count(saved_paths, unsaved_paths);
        // 导出进度
        let export_info = self.exporting.as_ref().map(|state| {
            (
                state
                    .progress
                    .current
                    .load(std::sync::atomic::Ordering::Relaxed),
                state
                    .progress
                    .total
                    .load(std::sync::atomic::Ordering::Relaxed),
            )
        });
        self.layout.status_bar.sync_export(export_info);
        // 有后台任务运行时持续刷新
        if self.exporting.is_some() {
            ctx.request_repaint();
        }
    }
}

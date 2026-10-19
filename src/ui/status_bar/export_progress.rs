//! JAR 导出进度状态栏 item
//!
//! @author sky
//! @author ljlVink

use crate::appearance::theme;
use egui_shell::components::panel::status_bar::{Alignment, ProgressItem};
use rust_i18n::t;

egui_shell::define_progress_item! {
    /// JAR 导出进度 item（左侧显示，导出期间可见）
    pub struct ExportProgressItem;
}

impl ExportProgressItem {
    /// 创建导出进度 item
    pub fn new() -> Self {
        Self::from_progress(ProgressItem::new(
            theme::palette().text_muted,
            Alignment::Left,
        ))
    }

    /// 更新导出进度，None 表示无任务
    pub fn set_progress(&mut self, info: Option<(u32, u32)>) {
        self.set_color(theme::palette().text_muted);
        match info {
            Some((current, total)) => {
                self.set_text(
                    t!(
                        "status.exporting_progress",
                        current = current,
                        total = total
                    )
                    .to_string(),
                );
                self.set_visible(true);
            }
            None => {
                self.set_visible(false);
            }
        }
    }
}

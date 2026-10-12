//! 已修改文件数量状态栏 item
//!
//! @author sky

use crate::appearance::theme;
use eframe::egui;
use egui_shell::components::panel::status_bar::{Alignment, StatusItem};
use rust_i18n::t;

/// 文件修改条目
struct FileEntry {
    /// JAR 内条目路径
    path: String,
    /// 显示用短名（最后一段路径）
    short_name: String,
    /// 条目颜色
    color: egui::Color32,
}

tabookit::class! {
    /// 右侧显示已修改文件数量（无修改时自动隐藏）
    ///
    /// - 蓝色 (ACCENT_CYAN)：已保存到 JAR 的修改
    /// - 橙色 (ACCENT_ORANGE)：tab 有编辑但未保存
    ///
    /// 点击状态栏文字弹出文件列表，点击可聚焦对应 tab。
    pub struct ModifiedCountItem {
        /// 已保存到 JAR 的文件路径
        saved_paths: Vec<String>,
        /// tab 有编辑但未保存的文件路径
        unsaved_paths: Vec<String>,
        /// 用户点击的文件路径（由 StatusBar 消费）
        clicked: Option<String>,
        /// 弹出列表是否打开
        popup_open: bool,
        /// 上次 toggle 的帧号（多 pass 去重）
        last_toggle_pass: u64,
    }

    pub fn new() -> Self {
        Self {
            saved_paths: Vec::new(),
            unsaved_paths: Vec::new(),
            clicked: None,
            popup_open: false,
            last_toggle_pass: 0,
        }
    }

    pub fn set_paths(&mut self, saved: Vec<String>, unsaved: Vec<String>) {
        self.saved_paths = saved;
        self.unsaved_paths = unsaved;
    }

    /// 取出用户点击的文件路径（取后清空）
    pub fn take_clicked(&mut self) -> Option<String> {
        self.clicked.take()
    }

    /// 构建弹出列表的条目
    fn build_entries(&self) -> Vec<FileEntry> {
        let mut entries = Vec::new();
        for path in &self.unsaved_paths {
            let short_name = path.rsplit('/').next().unwrap_or(path).to_string();
            entries.push(FileEntry {
                path: path.clone(),
                short_name,
                color: theme::ACCENT_ORANGE,
            });
        }
        for path in &self.saved_paths {
            let short_name = path.rsplit('/').next().unwrap_or(path).to_string();
            entries.push(FileEntry {
                path: path.clone(),
                short_name,
                color: theme::ACCENT_CYAN,
            });
        }
        entries
    }

    /// 渲染弹出列表中的条目
    fn render_entries(&mut self, ui: &mut egui::Ui, entries: &[FileEntry]) {
        let font = egui::FontId::proportional(11.5);
        for entry in entries.iter() {
            let (rect, resp) = ui.allocate_exact_size(
                egui::vec2(ui.available_width(), theme::MODIFIED_POPUP_ITEM_HEIGHT),
                egui::Sense::click(),
            );
            if resp.hovered() {
                ui.painter().rect_filled(rect, 2.0, theme::BG_HOVER);
            }
            if resp.clicked() {
                self.clicked = Some(entry.path.clone());
                self.popup_open = false;
            }
            // 圆点指示器
            let dot_x = rect.left() + 8.0;
            let dot_y = rect.center().y;
            ui.painter()
                .circle_filled(egui::pos2(dot_x, dot_y), 3.0, entry.color);
            // 文件名
            let text_x = dot_x + 10.0;
            ui.painter().text(
                egui::pos2(text_x, dot_y),
                egui::Align2::LEFT_CENTER,
                &entry.short_name,
                font.clone(),
                theme::TEXT_PRIMARY,
            );
            // 路径（右侧灰色）
            let path_galley = ui.painter().layout_no_wrap(
                entry.path.clone(),
                egui::FontId::proportional(10.0),
                theme::TEXT_MUTED,
            );
            let path_w = path_galley.size().x;
            let max_path_x = rect.right() - 6.0;
            let path_x = (max_path_x - path_w).max(text_x + 60.0);
            if path_x + path_w <= max_path_x + 1.0 {
                ui.painter().galley(
                    egui::pos2(path_x, dot_y - path_galley.size().y / 2.0),
                    path_galley,
                    theme::TEXT_MUTED,
                );
            }
        }
    }
}

impl StatusItem for ModifiedCountItem {
    fn alignment(&self) -> Alignment {
        Alignment::Right
    }

    fn visible(&self) -> bool {
        !self.saved_paths.is_empty() || !self.unsaved_paths.is_empty()
    }

    fn render(&mut self, ui: &mut egui::Ui, x: f32, center_y: f32) -> f32 {
        let font = egui::FontId::proportional(11.0);
        let painter = ui.painter();
        let saved_count = self.saved_paths.len();
        let unsaved_count = self.unsaved_paths.len();
        let mut parts: Vec<(String, egui::Color32)> = Vec::new();
        if saved_count > 0 {
            parts.push((
                t!("status.modified", count = saved_count).to_string(),
                theme::ACCENT_CYAN,
            ));
        }
        if unsaved_count > 0 {
            parts.push((
                t!("status.changed", count = unsaved_count).to_string(),
                theme::ACCENT_ORANGE,
            ));
        }
        let gap = if parts.len() > 1 { 8.0 } else { 0.0 };
        // 预计算总宽度
        let galleys: Vec<_> = parts
            .iter()
            .map(|(text, color)| painter.layout_no_wrap(text.clone(), font.clone(), *color))
            .collect();
        let total_w: f32 =
            galleys.iter().map(|g| g.size().x).sum::<f32>() + gap * (galleys.len() as f32 - 1.0);
        // 交互区域
        let bar_h = theme::STATUS_BAR_HEIGHT;
        let interact_rect = egui::Rect::from_min_size(
            egui::pos2(x - total_w, center_y - bar_h / 2.0),
            egui::vec2(total_w, bar_h),
        );
        let resp = ui.interact(
            interact_rect,
            ui.id().with("modified_count_hover"),
            egui::Sense::click(),
        );
        // 从右向左绘制文字
        let mut cx = x;
        for (i, galley) in galleys.into_iter().enumerate().rev() {
            let w = galley.size().x;
            cx -= w;
            painter.galley(
                egui::pos2(cx, center_y - galley.size().y / 2.0),
                galley,
                parts[i].1,
            );
            if i > 0 {
                cx -= gap;
            }
        }
        // 点击切换弹出列表（帧级去重防止多 pass 反复 toggle）
        if resp.clicked() {
            let pass = ui.ctx().cumulative_pass_nr();
            if self.last_toggle_pass != pass {
                self.last_toggle_pass = pass;
                self.popup_open = !self.popup_open;
            }
        }
        if self.popup_open {
            let entries = self.build_entries();
            let visible_count = entries.len().min(theme::MODIFIED_POPUP_MAX_VISIBLE);
            let content_h = visible_count as f32 * theme::MODIFIED_POPUP_ITEM_HEIGHT
                + theme::MODIFIED_POPUP_PAD * 2.0;
            let popup_rect = egui::Rect::from_min_size(
                egui::pos2(
                    interact_rect.right() - theme::MODIFIED_POPUP_WIDTH,
                    interact_rect.top() - content_h,
                ),
                egui::vec2(theme::MODIFIED_POPUP_WIDTH, content_h),
            );
            let popup_id = ui.id().with("modified_popup");
            let area_resp = egui::Area::new(popup_id)
                .fixed_pos(popup_rect.min)
                .order(egui::Order::Foreground)
                .show(ui.ctx(), |ui| {
                    let frame = egui::Frame::NONE
                        .fill(theme::BG_LIGHT)
                        .stroke(egui::Stroke::new(1.0_f32, theme::BORDER))
                        .corner_radius(4.0)
                        .shadow(egui::Shadow {
                            spread: 1,
                            blur: 12,
                            offset: [0, -4],
                            color: egui::Color32::from_black_alpha(80),
                        })
                        .inner_margin(egui::Margin::same(theme::MODIFIED_POPUP_PAD as i8));
                    frame.show(ui, |ui| {
                        ui.set_width(theme::MODIFIED_POPUP_WIDTH - theme::MODIFIED_POPUP_PAD * 2.0);
                        egui::ScrollArea::vertical()
                            .max_height(
                                theme::MODIFIED_POPUP_MAX_VISIBLE as f32
                                    * theme::MODIFIED_POPUP_ITEM_HEIGHT,
                            )
                            .show(ui, |ui| {
                                self.render_entries(ui, &entries);
                            });
                    });
                });
            // 点击弹出区域外关闭
            if ui.ctx().input(|i| i.pointer.any_pressed())
                && let Some(pos) = ui.ctx().input(|i| i.pointer.press_origin())
            {
                let area_rect = area_resp.response.rect;
                if !area_rect.contains(pos) && !interact_rect.contains(pos) {
                    self.popup_open = false;
                }
            }
        }
        total_w
    }
}

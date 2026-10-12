//! 视图切换状态栏 item（ArkTS 源码 | 反汇编）
//!
//! @author sky
//! @author ljlVink

use crate::appearance::theme;
use crate::ui::editor::view_toggle::ActiveView;
use eframe::egui;
use egui_animation::Anim;
use egui_shell::components::panel::status_bar::{Alignment, StatusItem};
use rust_i18n::t;

/// ArkTS tab：源码 | 反汇编
const ARK_VIEWS: [ActiveView; 2] = [ActiveView::Decompiled, ActiveView::Disasm];

fn view_label(v: ActiveView) -> String {
    match v {
        ActiveView::Decompiled => t!("editor.view_source").to_string(),
        ActiveView::Hex => t!("status.hex").to_string(),
        ActiveView::Disasm => t!("editor.view_disasm").to_string(),
    }
}

tabookit::class! {
    /// 视图切换 item（ArkTS 源码|反汇编两项）
    pub struct ViewToggleItem {
        /// 当前活跃视图（None 表示无 ArkTS tab 聚焦，不显示）
        active: Option<ActiveView>,
        /// 用户点击后的新视图
        changed: Option<ActiveView>,
    }

    pub fn new() -> Self {
        Self {
            active: None,
            changed: None,
        }
    }

    /// 外部设置当前视图
    pub fn set_active(&mut self, view: Option<ActiveView>) {
        self.active = view;
    }

    /// 取出用户选择的新视图（取后清空）
    pub fn take_changed(&mut self) -> Option<ActiveView> {
        self.changed.take()
    }
}

impl StatusItem for ViewToggleItem {
    fn alignment(&self) -> Alignment {
        Alignment::Right
    }

    fn visible(&self) -> bool {
        self.active.is_some()
    }

    fn render(&mut self, ui: &mut egui::Ui, x: f32, center_y: f32) -> f32 {
        let active = tabookit::or!(self.active, return 0.0);
        let painter = ui.painter();
        let font = egui::FontId::proportional(11.0);
        let pad = 6.0;
        let item_gap = 1.0;
        let container_pad = 2.0;
        let views: &[ActiveView] = &ARK_VIEWS;
        let labels: Vec<String> = views.iter().map(|v| view_label(*v)).collect();
        let item_widths: Vec<f32> = labels
            .iter()
            .map(|label| {
                painter
                    .layout_no_wrap(label.clone(), font.clone(), theme::TEXT_PRIMARY)
                    .size()
                    .x
                    + pad * 2.0
            })
            .collect();
        let inner_w: f32 = item_widths.iter().sum::<f32>() + item_gap * (views.len() as f32 - 1.0);
        let container_w = inner_w + container_pad * 2.0;
        let bar_height = theme::STATUS_BAR_HEIGHT;
        let container_h = bar_height - 4.0;
        let container_rect = egui::Rect::from_min_size(
            egui::pos2(x - container_w, center_y - container_h / 2.0),
            egui::vec2(container_w, container_h),
        );
        painter.rect_filled(container_rect, 3.0, theme::BG_DARKEST);
        let item_h = container_h - container_pad * 2.0;
        let base_x = container_rect.left() + container_pad;
        let iy = container_rect.top() + container_pad;
        // 高亮滑块动画（用相对偏移量，避免窗口缩放时跳变）
        let active_idx = views.iter().position(|v| *v == active).unwrap_or(0);
        let target_offset: f32 = (0..active_idx).map(|i| item_widths[i] + item_gap).sum();
        let target_w = item_widths[active_idx];
        let anim = Anim::new(ui, theme::VIEW_TOGGLE_ANIM_DURATION);
        let anim_offset = anim.f32("offset", target_offset);
        let anim_w = anim.f32("width", target_w);
        let highlight_rect = egui::Rect::from_min_size(
            egui::pos2(base_x + anim_offset, iy),
            egui::vec2(anim_w, item_h),
        );
        painter.rect_filled(highlight_rect, 2.0, theme::verdigris_alpha(40));
        // 各选项文字 + 点击区域
        let mut ix = base_x;
        for (i, view) in views.iter().enumerate() {
            let iw = item_widths[i];
            let item_rect = egui::Rect::from_min_size(egui::pos2(ix, iy), egui::vec2(iw, item_h));
            let response = ui.interact(
                item_rect,
                ui.id().with(format!("vt_{i}")),
                egui::Sense::click(),
            );
            if response.clicked() {
                self.changed = Some(*view);
            }
            // 文字颜色动画
            let color = if response.hovered() && active != *view {
                theme::TEXT_PRIMARY
            } else {
                let target = if active == *view {
                    theme::VERDIGRIS
                } else {
                    theme::TEXT_MUTED
                };
                anim.color(format!("c{i}"), target)
            };
            painter.text(
                item_rect.center(),
                egui::Align2::CENTER_CENTER,
                &labels[i],
                font.clone(),
                color,
            );
            ix += iw + item_gap;
        }
        container_w
    }
}

//! 对齐编辑区左边缘的源码 / 反汇编切换按钮
//!
//! @author sky
//! @author ljlVink

use crate::appearance::theme;
use crate::ui::editor::view_toggle::ActiveView;
use eframe::egui;
use egui_animation::Anim;
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

/// 在底栏的指定横坐标绘制按钮，返回占用宽度，不占用编辑区高度。
pub(super) fn show(ui: &mut egui::Ui, active_view: &mut ActiveView, x: f32) -> f32 {
    let active = *active_view;
    let center_y = ui.max_rect().center().y;
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
                .layout_no_wrap(label.clone(), font.clone(), theme::palette().text_primary)
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
        egui::pos2(x, center_y - container_h / 2.0),
        egui::vec2(container_w, container_h),
    );
    painter.rect_filled(container_rect, 3.0, theme::palette().bg_darkest);
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
            *active_view = *view;
            ui.ctx().memory_mut(|memory| memory.stop_text_input());
            ui.ctx().request_repaint();
        }
        // 文字颜色动画
        let color = if response.hovered() && active != *view {
            theme::palette().text_primary
        } else {
            let target = if active == *view {
                theme::palette().verdigris
            } else {
                theme::palette().text_muted
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

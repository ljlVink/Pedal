//! Header 渲染：图标 + 标题 + pin 按钮 + 自定义右侧区域
//!
//! @author sky

use super::FloatingWindow;
use crate::app::ShellTheme;
use eframe::egui;

/// 将窗口内 Ui 的 visuals 设置为匹配 ShellTheme 的风格
pub(super) fn apply_style(ui: &mut egui::Ui, theme: &ShellTheme) {
    let style = ui.style_mut();
    style.visuals.widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0_f32, theme.text_primary);
    style.visuals.widgets.inactive.weak_bg_fill = egui::Color32::TRANSPARENT;
    style.visuals.widgets.hovered.bg_fill = theme.bg_hover;
    style.visuals.widgets.active.bg_fill = theme.bg_active;
    style.visuals.widgets.hovered.bg_stroke = egui::Stroke::NONE;
    style.visuals.widgets.active.bg_stroke = egui::Stroke::NONE;
    ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
}

impl FloatingWindow {
    /// 渲染 header：左侧图标 + 标题，右侧 pin 按钮 + 自定义内容
    pub(super) fn render_header(
        &mut self,
        ui: &mut egui::Ui,
        theme: &ShellTheme,
        header_right: impl FnOnce(&mut egui::Ui),
    ) {
        ui.horizontal(|ui| {
            ui.set_height(theme.window.header_height);
            ui.add_space(10.0);
            if let Some(icon) = self.icon {
                ui.label(
                    egui::RichText::new(icon)
                        .font(egui::FontId::new(14.0, theme.icon_font.clone()))
                        .color(theme.accent),
                );
                ui.add_space(6.0);
            }
            ui.label(
                egui::RichText::new(&self.title)
                    .font(egui::FontId::proportional(13.0))
                    .color(theme.text_primary),
            );
            // 右侧区域：pin 按钮 + 调用方自定义内容
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_space(6.0);
                self.render_pin_button(ui, theme);
                ui.add_space(8.0);
                header_right(ui);
                // 记录右侧按钮区域左边界，供 handle_move 排除此区域
                self.header_right_x = ui.min_rect().left();
            });
        });
    }

    /// Pin/Unpin 切换按钮
    fn render_pin_button(&mut self, ui: &mut egui::Ui, theme: &ShellTheme) {
        let is_pinned = self.pinned;
        let color = if is_pinned {
            theme.accent
        } else {
            theme.text_muted
        };
        let base_fill = if is_pinned {
            theme.bg_active
        } else {
            egui::Color32::TRANSPARENT
        };
        let resp = ui
            .scope(|ui| {
                let wv = &mut ui.style_mut().visuals.widgets;
                wv.inactive.weak_bg_fill = base_fill;
                wv.hovered.weak_bg_fill = theme.bg_hover;
                wv.active.weak_bg_fill = theme.bg_active;
                ui.add(
                    egui::Button::new(
                        egui::RichText::new(theme.window.pin_icon)
                            .font(egui::FontId::new(14.0, theme.icon_font.clone()))
                            .color(color),
                    )
                    .corner_radius(3)
                    .min_size(egui::vec2(26.0, 24.0)),
                )
            })
            .inner;
        if resp
            .on_hover_text(if is_pinned {
                &theme.window.unpin_tooltip
            } else {
                &theme.window.pin_tooltip
            })
            .clicked()
        {
            self.pinned = !self.pinned;
        }
    }
}

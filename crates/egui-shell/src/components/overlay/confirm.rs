//! 模态确认弹窗：遮罩 + 居中面板 + 标题/描述/按钮
//!
//! @author sky

use crate::components::widget::flat_button::FlatButton;
use eframe::egui;

/// 确认弹窗配色
#[derive(Clone)]
pub struct ConfirmTheme {
    /// 弹窗面板样式（fill / stroke / corner_radius / shadow）
    pub frame: egui::Frame,
    /// 标题文字颜色
    pub title_color: egui::Color32,
    /// 描述文字颜色
    pub message_color: egui::Color32,
    /// 分隔线颜色
    pub separator: egui::Color32,
    /// 遮罩层颜色
    pub backdrop: egui::Color32,
    /// 按钮配色
    pub button: crate::components::widget::FlatButtonTheme,
}

/// 确认弹窗用户操作
pub enum ConfirmResult {
    /// 弹窗仍打开，用户未操作
    None,
    /// 用户点击确认按钮
    Confirmed,
    /// 用户取消（Escape / 点击外部 / 取消按钮）
    Dismissed,
}

tabookit::class! {
    /// 模态确认弹窗
    ///
    /// ```ignore
    /// let result = ConfirmDialog::new("标题", "描述")
    ///     .confirm_label("删除")
    ///     .cancel_label("取消")
    ///     .confirm_color(RED)
    ///     .show(ctx, &theme);
    /// ```
    pub struct ConfirmDialog<'a> {
        /// 标题文字
        title: &'a str,
        /// 描述文字
        message: &'a str,
        /// 确认按钮文字
        confirm_label: &'a str,
        /// 取消按钮文字
        cancel_label: &'a str,
        /// 确认按钮文字颜色（破坏性操作可设为红色）
        confirm_color: Option<egui::Color32>,
        /// 点击弹窗外部是否关闭
        close_on_click_outside: bool,
    }

    pub fn new(title: &'a str, message: &'a str) -> Self {
        Self {
            title,
            message,
            confirm_label: "OK",
            cancel_label: "Cancel",
            confirm_color: None,
            close_on_click_outside: true,
        }
    }

    /// 点击弹窗外部是否关闭弹窗，默认 true
    pub fn close_on_click_outside(mut self, enabled: bool) -> Self {
        self.close_on_click_outside = enabled;
        self
    }

    pub fn confirm_label(mut self, label: &'a str) -> Self {
        self.confirm_label = label;
        self
    }

    pub fn cancel_label(mut self, label: &'a str) -> Self {
        self.cancel_label = label;
        self
    }

    /// 确认按钮文字颜色（用于标识破坏性操作）
    pub fn confirm_color(mut self, color: egui::Color32) -> Self {
        self.confirm_color = Some(color);
        self
    }

    /// 渲染弹窗并返回用户操作结果
    pub fn show(&self, ctx: &egui::Context, theme: &ConfirmTheme) -> ConfirmResult {
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            return ConfirmResult::Dismissed;
        }
        Self::paint_backdrop(ctx, theme);
        let mut result = ConfirmResult::None;
        let dialog_rect = egui::Area::new(egui::Id::new("confirm_dialog"))
            .order(egui::Order::Foreground)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                theme.frame.show(ui, |ui| {
                    ui.set_width(340.0);
                    self.paint_content(ui, theme);
                    result = self.paint_buttons(ui, theme);
                });
            })
            .response
            .rect;
        if matches!(result, ConfirmResult::None) && self.close_on_click_outside {
            result = Self::check_click_outside(ctx, dialog_rect);
        }
        result
    }

    fn paint_backdrop(ctx: &egui::Context, theme: &ConfirmTheme) {
        let layer = egui::LayerId::new(egui::Order::Foreground, egui::Id::new("confirm_backdrop"));
        ctx.layer_painter(layer)
            .rect_filled(ctx.content_rect(), 0.0, theme.backdrop);
    }

    fn paint_content(&self, ui: &mut egui::Ui, theme: &ConfirmTheme) {
        egui::Frame::NONE
            .inner_margin(egui::Margin {
                left: 20,
                right: 20,
                top: 20,
                bottom: 16,
            })
            .show(ui, |ui| {
                ui.label(
                    egui::RichText::new(self.title)
                        .size(13.0)
                        .color(theme.title_color)
                        .strong(),
                );
                ui.add_space(6.0);
                ui.label(
                    egui::RichText::new(self.message)
                        .size(12.0)
                        .color(theme.message_color),
                );
            });
    }

    fn paint_buttons(&self, ui: &mut egui::Ui, theme: &ConfirmTheme) -> ConfirmResult {
        let mut result = ConfirmResult::None;
        egui::Frame::NONE
            .inner_margin(egui::Margin {
                left: 20,
                right: 20,
                top: 12,
                bottom: 12,
            })
            .show(ui, |ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let cancel = FlatButton::new(self.cancel_label, &theme.button)
                        .min_size(egui::vec2(72.0, 28.0));
                    if ui.add(cancel).clicked() {
                        result = ConfirmResult::Dismissed;
                    }
                    ui.add_space(6.0);
                    let mut confirm = FlatButton::new(self.confirm_label, &theme.button)
                        .min_size(egui::vec2(88.0, 28.0));
                    if let Some(color) = self.confirm_color {
                        confirm = confirm.inactive_color(color);
                    }
                    if ui.add(confirm).clicked() {
                        result = ConfirmResult::Confirmed;
                    }
                });
            });
        result
    }

    fn check_click_outside(ctx: &egui::Context, dialog_rect: egui::Rect) -> ConfirmResult {
        if ctx.input(|i| i.pointer.any_pressed())
            && let Some(pos) = ctx.input(|i| i.pointer.interact_pos())
                && !dialog_rect.contains(pos) {
                    return ConfirmResult::Dismissed;
                }
        ConfirmResult::None
    }
}

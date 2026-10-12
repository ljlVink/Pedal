//! 编辑器内查找栏：右上角浮动 island
//!
//! @author sky

use crate::search::{self, FindMatch};
use crate::theme::FindBarTheme;
use eframe::egui;
use egui_shell::components::FlatButton;
use std::hash::{Hash, Hasher};

/// island 到编辑区边缘的间距
const MARGIN: f32 = 8.0;
/// island 高度
const BAR_H: f32 = 30.0;

tabookit::class! {
    /// 编辑器内查找栏
    #[derive(Default)]
    pub struct FindBar {
        /// 是否打开
        pub open: bool,
        /// 搜索查询
        query: String,
        /// 是否区分大小写
        case_sensitive: bool,
        /// 是否仅匹配完整单词
        whole_word: bool,
        /// 是否使用正则表达式
        regex: bool,
        /// 搜索匹配项列表
        matches: Vec<FindMatch>,
        /// 当前选中项索引
        current: usize,
        /// 是否需要聚焦输入框
        focus_input: bool,
        /// 导航匹配项时请求视图滚动
        scroll_requested: bool,
        /// 缓存指纹，避免重复搜索
        last_key: u64,
    }

    pub fn new() -> Self {
        Self {
            open: false,
            query: String::new(),
            case_sensitive: false,
            whole_word: false,
            regex: false,
            matches: Vec::new(),
            current: 0,
            focus_input: false,
            scroll_requested: false,
            last_key: 0,
        }
    }

    pub fn toggle(&mut self) {
        if self.open {
            self.close();
        } else {
            self.open();
        }
    }

    pub fn open(&mut self) {
        self.open = true;
        self.focus_input = true;
    }

    pub fn close(&mut self) {
        self.open = false;
        self.matches.clear();
        self.current = 0;
        self.scroll_requested = false;
        self.last_key = 0;
    }

    /// 更新文本搜索结果
    pub fn update_text(&mut self, text: &str) {
        self.update_search(text);
    }

    /// 更新字节搜索结果
    pub fn update_bytes(&mut self, data: &[u8]) {
        self.update_hex(data);
    }

    /// 消费滚动请求（调用后自动清除）
    pub fn take_scroll_request(&mut self) -> bool {
        std::mem::take(&mut self.scroll_requested)
    }

    /// 返回当前匹配列表（克隆，避免借用冲突）
    pub fn highlight_info(&self) -> (Vec<FindMatch>, Option<usize>) {
        if self.matches.is_empty() {
            return (Vec::new(), None);
        }
        (self.matches.clone(), Some(self.current))
    }

    /// 渲染浮动 island（在内容渲染之后调用，固定 rect overlay）
    pub fn render_overlay(
        &mut self,
        ui: &mut egui::Ui,
        content_rect: egui::Rect,
        theme: &FindBarTheme,
    ) {
        let bar_rect = egui::Rect::from_min_size(
            egui::pos2(content_rect.left() + MARGIN, content_rect.top() + MARGIN),
            egui::vec2(content_rect.width() - MARGIN * 2.0, BAR_H),
        );
        // 阻断下方内容的点击
        ui.interact(
            bar_rect,
            ui.id().with("find_blocker"),
            egui::Sense::click_and_drag(),
        );
        // Island 背景 + 边框
        let painter = ui.painter();
        painter.rect_filled(bar_rect, 6.0, theme.bg);
        painter.rect_stroke(
            bar_rect,
            6.0,
            egui::Stroke::new(1.0_f32, theme.border),
            egui::StrokeKind::Inside,
        );
        // 内容
        let inner = bar_rect.shrink2(egui::vec2(8.0, 0.0));
        let mut child = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(inner)
                .id_salt("find_bar_overlay"),
        );
        child.set_clip_rect(bar_rect);
        self.render_contents(&mut child, theme);
    }

    fn render_contents(&mut self, ui: &mut egui::Ui, theme: &FindBarTheme) {
        // 右到左布局：按钮固定居右，输入框占剩余空间
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            // 关闭
            if ui
                .add(icon_btn(theme.icons.close, theme))
                .on_hover_text(&theme.labels.close)
                .clicked()
            {
                self.close();
            }
            ui.add_space(2.0);
            // 下一个
            if ui
                .add(icon_btn(theme.icons.next, theme))
                .on_hover_text(&theme.labels.next)
                .clicked()
            {
                self.next_match();
                self.focus_input = true;
            }
            // 上一个
            if ui
                .add(icon_btn(theme.icons.prev, theme))
                .on_hover_text(&theme.labels.prev)
                .clicked()
            {
                self.prev_match();
                self.focus_input = true;
            }
            ui.add_space(2.0);
            // 结果计数（始终渲染以保持 widget 数量稳定，避免布局偏移触发 egui ID 冲突警告）
            let (count_text, count_color) = if self.query.is_empty() {
                (String::new(), theme.text_muted)
            } else if self.matches.is_empty() {
                (theme.labels.no_results.clone(), theme.error_color)
            } else {
                (
                    (theme.labels.result_fmt)(self.current + 1, self.matches.len()),
                    theme.text_muted,
                )
            };
            ui.label(
                egui::RichText::new(count_text)
                    .size(12.0)
                    .color(count_color),
            );
            ui.add_space(4.0);
            // 正则
            if ui
                .add(icon_toggle(theme.icons.regex, self.regex, theme))
                .on_hover_text(&theme.labels.use_regex)
                .clicked()
            {
                self.regex = !self.regex;
                self.invalidate();
                self.focus_input = true;
            }
            // 全词匹配
            if ui
                .add(icon_toggle(theme.icons.whole_word, self.whole_word, theme))
                .on_hover_text(&theme.labels.match_word)
                .clicked()
            {
                self.whole_word = !self.whole_word;
                self.invalidate();
                self.focus_input = true;
            }
            // 大小写敏感
            if ui
                .add(icon_toggle(
                    theme.icons.case_sensitive,
                    self.case_sensitive,
                    theme,
                ))
                .on_hover_text(&theme.labels.match_case)
                .clicked()
            {
                self.case_sensitive = !self.case_sensitive;
                self.invalidate();
                self.focus_input = true;
            }
            ui.add_space(4.0);
            // 搜索图标 + 输入框（占剩余全部宽度）
            let remaining = ui.available_width();
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                ui.label(
                    egui::RichText::new(theme.icons.search)
                        .font(egui::FontId::new(14.0, theme.icons.font.clone()))
                        .color(theme.text_muted),
                );
                let resp = ui.add(
                    egui::TextEdit::singleline(&mut self.query)
                        .id(ui.id().with("find_input"))
                        .hint_text(&theme.labels.hint)
                        .frame(egui::Frame::NONE)
                        .text_color(theme.text_primary)
                        .font(egui::FontId::proportional(13.0))
                        .desired_width(remaining - 22.0),
                );
                if self.focus_input {
                    resp.request_focus();
                    self.focus_input = false;
                }
                if resp.lost_focus() {
                    if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                        self.close();
                        return;
                    }
                    if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        if ui.input(|i| i.modifiers.shift) {
                            self.prev_match();
                        } else {
                            self.next_match();
                        }
                        self.focus_input = true;
                    }
                }
                if ui.input(|i| i.key_pressed(egui::Key::F3)) {
                    if ui.input(|i| i.modifiers.shift) {
                        self.prev_match();
                    } else {
                        self.next_match();
                    }
                }
            });
        });
    }

    fn next_match(&mut self) {
        if !self.matches.is_empty() {
            self.current = (self.current + 1) % self.matches.len();
            self.scroll_requested = true;
        }
    }

    fn prev_match(&mut self) {
        if !self.matches.is_empty() {
            self.current = if self.current == 0 {
                self.matches.len() - 1
            } else {
                self.current - 1
            };
            self.scroll_requested = true;
        }
    }

    fn invalidate(&mut self) {
        self.last_key = 0;
    }

    fn update_search(&mut self, text: &str) {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        self.query.hash(&mut h);
        self.case_sensitive.hash(&mut h);
        self.whole_word.hash(&mut h);
        self.regex.hash(&mut h);
        (text.as_ptr() as usize).hash(&mut h);
        text.len().hash(&mut h);
        let key = h.finish();
        if key == self.last_key {
            return;
        }
        self.last_key = key;
        self.matches = search::find_all(text, &self.query, self.case_sensitive, self.whole_word);
        if self.current >= self.matches.len() {
            self.current = 0;
        }
    }

    fn update_hex(&mut self, data: &[u8]) {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        self.query.hash(&mut h);
        self.case_sensitive.hash(&mut h);
        (data.as_ptr() as usize).hash(&mut h);
        data.len().hash(&mut h);
        0xBEEFu32.hash(&mut h);
        let key = h.finish();
        if key == self.last_key {
            return;
        }
        self.last_key = key;
        self.matches = search::find_bytes(data, &self.query, self.case_sensitive);
        if self.current >= self.matches.len() {
            self.current = 0;
        }
    }
}

/// Codicon 图标 toggle 按钮（带 active 状态）
fn icon_toggle<'a>(icon: &'a str, active: bool, theme: &'a FindBarTheme) -> FlatButton<'a> {
    FlatButton::new(icon, &theme.button)
        .font_family(theme.icons.font.clone())
        .font_size(15.0)
        .active(active)
        .inactive_color(theme.text_muted)
        .min_size(egui::vec2(24.0, 22.0))
}

/// Codicon 图标普通按钮
fn icon_btn<'a>(icon: &'a str, theme: &'a FindBarTheme) -> FlatButton<'a> {
    FlatButton::new(icon, &theme.button)
        .font_family(theme.icons.font.clone())
        .font_size(14.0)
        .inactive_color(theme.text_muted)
        .min_size(egui::vec2(22.0, 22.0))
}

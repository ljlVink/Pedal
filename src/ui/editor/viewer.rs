//! TabViewer 实现：定义每个 Tab 的标题和内容渲染
//!
//! @author sky
//! @author ljlVink

use super::render;
use super::tab::EditorTab;
use super::view_toggle::ActiveView;
use crate::appearance::{codicon, theme};
use crate::ui::keybindings;
use eframe::egui;
use egui_dock::{NodePath, TabViewer, tab_viewer::OnCloseResponse};
use egui_editor::find_bar::FindBar;
use egui_shell::components::menu_item;
use rust_i18n::t;

use std::collections::HashSet;

/// Tab 右键菜单触发的动作
pub enum TabAction {
    /// 关闭当前 tab
    Close(Option<String>),
    CloseAll,
    /// 关闭除指定 tab 外的所有 tab（按 entry_path 匹配）
    CloseOthers(Option<String>),
    /// 关闭指定 tab 右侧的所有 tab
    CloseToRight(Option<String>),
    /// ArkTS 反汇编懒生成请求
    LoadDisasm(String),
}

pub struct EditorTabViewer<'a> {
    /// 右键菜单触发的动作（每帧最多一个，由 EditorArea 消费）
    pub action: Option<TabAction>,
    /// 编辑器内查找栏状态
    pub find_bar: &'a mut FindBar,
    /// 归档中已保存但未落盘的条目路径
    pub jar_modified: &'a HashSet<String>,
    /// on_close 被 is_modified 拦截时记录要关闭的 tab 路径
    pub pending_close: Option<Option<String>>,
}

impl TabViewer for EditorTabViewer<'_> {
    type Tab = EditorTab;

    fn title(&mut self, tab: &mut Self::Tab) -> egui::WidgetText {
        let mut job = egui::text::LayoutJob::default();
        job.append("", 4.0, egui::TextFormat::default());
        job.append(
            if tab.is_ark {
                codicon::SYMBOL_CLASS
            } else {
                codicon::FILE
            },
            0.0,
            egui::TextFormat {
                font_id: egui::FontId::new(11.0, codicon::family()),
                color: theme::VERDIGRIS,
                ..Default::default()
            },
        );
        job.append(" ", 0.0, egui::TextFormat::default());
        job.append(
            &tab.title,
            0.0,
            egui::TextFormat {
                font_id: egui::FontId::proportional(12.0),
                color: egui::Color32::PLACEHOLDER,
                ..Default::default()
            },
        );
        job.append("", 4.0, egui::TextFormat::default());
        job.into()
    }

    fn ui(&mut self, ui: &mut egui::Ui, tab: &mut Self::Tab) {
        ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
        // 更新搜索（先于内容渲染，提供高亮数据）
        if self.find_bar.open {
            match tab.active_view {
                ActiveView::Hex => self.find_bar.update_bytes(&tab.raw_bytes),
                ActiveView::Decompiled => self.find_bar.update_text(&tab.decompiled),
                ActiveView::Disasm => self.find_bar.update_text(&tab.disasm_text),
            }
        }
        let (matches, current) = if self.find_bar.open {
            self.find_bar.highlight_info()
        } else {
            (vec![], None)
        };
        // 滚动到当前匹配项
        if self.find_bar.take_scroll_request()
            && let Some(idx) = current
            && let Some(m) = matches.get(idx)
        {
            match tab.active_view {
                ActiveView::Hex => {
                    tab.hex_state.scroll_to_byte = Some(m.start);
                }
                ActiveView::Decompiled => {
                    let line = tab.decompiled.as_bytes()[..m.start.min(tab.decompiled.len())]
                        .iter()
                        .filter(|&&b| b == b'\n')
                        .count();
                    tab.pending_scroll_to_line = Some(line);
                }
                ActiveView::Disasm => {
                    let line = tab.disasm_text.as_bytes()[..m.start.min(tab.disasm_text.len())]
                        .iter()
                        .filter(|&&b| b == b'\n')
                        .count();
                    tab.pending_scroll_to_line = Some(line);
                }
            }
        }
        // ArkTS tab：反汇编视图懒生成
        if tab.active_view == ActiveView::Disasm
            && tab.disasm_text.is_empty()
            && let Some(path) = tab.entry_path.clone()
        {
            self.action = Some(TabAction::LoadDisasm(path));
        }
        // ArkTS 子标签栏（源码 | 反汇编），jadx 风格置于内容上方
        if tab.is_ark {
            egui::Panel::top(ui.id().with("ark_subtabs"))
                .frame(egui::Frame::new().inner_margin(egui::Margin::symmetric(8, 3)))
                .show_inside(ui, |ui| {
                    ui.horizontal(|ui| {
                        let bt = crate::appearance::theme::flat_button_theme(theme::TEXT_SECONDARY);
                        let src_sel = tab.active_view == ActiveView::Decompiled;
                        let src_label = t!("editor.view_source");
                        let src_btn = egui_shell::components::FlatButton::new(&src_label, &bt)
                            .min_size(egui::vec2(64.0, 24.0))
                            .active(src_sel);
                        if ui.add(src_btn).clicked() {
                            tab.active_view = ActiveView::Decompiled;
                        }
                        let asm_sel = tab.active_view == ActiveView::Disasm;
                        let asm_label = t!("editor.view_disasm");
                        let asm_btn = egui_shell::components::FlatButton::new(&asm_label, &bt)
                            .min_size(egui::vec2(64.0, 24.0))
                            .active(asm_sel);
                        if ui.add(asm_btn).clicked() {
                            tab.active_view = ActiveView::Disasm;
                        }
                    });
                });
        }
        // 渲染内容
        match tab.active_view {
            ActiveView::Decompiled if tab.is_text => {
                render::render_editable(ui, tab, &matches, current);
            }
            ActiveView::Decompiled => {
                render::render_decompiled(ui, tab, &matches, current);
            }
            ActiveView::Hex => render::render_hex(ui, tab, &matches, current),
            ActiveView::Disasm => render::render_disasm(ui, tab, &matches, current),
        }
        // 浮动查找栏（overlay，用 clip_rect 定位到可见区域，不随 ScrollArea 滚动）
        if self.find_bar.open {
            let fbt = theme::find_bar_theme();
            self.find_bar.render_overlay(ui, ui.clip_rect(), &fbt);
        }
    }

    fn context_menu(&mut self, ui: &mut egui::Ui, tab: &mut Self::Tab, _path: NodePath) {
        let mt = &theme::menu_theme();
        if menu_item(
            ui,
            mt,
            &t!("editor.close"),
            Some(&keybindings::DEFAULT_CLOSE_TAB),
        ) {
            self.action = Some(TabAction::Close(tab.entry_path.clone()));
            ui.close();
        }
        if menu_item(ui, mt, &t!("editor.close_others"), None) {
            self.action = Some(TabAction::CloseOthers(tab.entry_path.clone()));
            ui.close();
        }
        if menu_item(ui, mt, &t!("editor.close_to_right"), None) {
            self.action = Some(TabAction::CloseToRight(tab.entry_path.clone()));
            ui.close();
        }
        if menu_item(
            ui,
            mt,
            &t!("editor.close_all"),
            Some(&keybindings::DEFAULT_CLOSE_ALL_TABS),
        ) {
            self.action = Some(TabAction::CloseAll);
            ui.close();
        }
    }

    fn id(&mut self, tab: &mut Self::Tab) -> egui::Id {
        match &tab.entry_path {
            Some(path) => egui::Id::new(path),
            None => egui::Id::new(&tab.title),
        }
    }

    fn on_close(&mut self, tab: &mut Self::Tab) -> OnCloseResponse {
        if tab.is_modified {
            self.pending_close = Some(tab.entry_path.clone());
            return OnCloseResponse::Focus;
        }
        OnCloseResponse::Close
    }

    fn allowed_in_windows(&self, _tab: &mut Self::Tab) -> bool {
        false
    }

    fn modification_color(&self, tab: &Self::Tab) -> Option<egui::Color32> {
        if tab.is_modified {
            return Some(theme::ACCENT_ORANGE);
        }
        if let Some(path) = &tab.entry_path
            && self.jar_modified.contains(path)
        {
            return Some(theme::ACCENT_GREEN);
        }
        None
    }
}

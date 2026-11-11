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
                color: theme::palette().verdigris,
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
        tab.poll_auto_format(ui.ctx());
        ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
        if tab.format_task.is_some() && tab.active_view == ActiveView::Decompiled {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(t!("editor.formatting"));
            });
            return;
        }
        if let Some(tree) = &mut tab.resource_tree {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(8.0, 4.0);
                ui.selectable_value(
                    &mut tab.active_view,
                    ActiveView::Resources,
                    t!("resource_tree.view"),
                );
                ui.selectable_value(&mut tab.active_view, ActiveView::Hex, t!("status.hex"));
            });
            ui.separator();
            if tab.active_view == ActiveView::Resources {
                tree.show(ui);
                return;
            }
        }
        if let Some(preview) = &mut tab.image_preview {
            preview.poll(ui.ctx());
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(8.0, 4.0);
                ui.selectable_value(
                    &mut tab.active_view,
                    ActiveView::Image,
                    t!("editor.view_image"),
                );
                ui.selectable_value(&mut tab.active_view, ActiveView::Hex, t!("status.hex"));
                if tab.active_view == ActiveView::Image {
                    preview.controls(ui);
                }
            });
            ui.separator();
        }
        // ArkTS tab：反汇编视图懒生成
        if tab.active_view == ActiveView::Disasm
            && tab.disasm_text.is_empty()
            && let Some(path) = tab.entry_path.clone()
        {
            self.action = Some(TabAction::LoadDisasm(path));
        }
        // 代码内容独立滚动。
        if tab.active_view == ActiveView::Image {
            if let Some(preview) = &mut tab.image_preview {
                preview.show(ui);
            }
            return;
        }
        egui::ScrollArea::both()
            .id_salt(("editor_content", tab.active_view))
            .auto_shrink([false, false])
            .show(ui, |ui| match tab.active_view {
                ActiveView::Decompiled if tab.is_text => {
                    render::render_editable(ui, tab);
                }
                ActiveView::Decompiled => {
                    render::render_decompiled(ui, tab);
                }
                ActiveView::Hex => render::render_hex(ui, tab),
                ActiveView::Disasm => render::render_disasm(ui, tab),
                ActiveView::Image | ActiveView::Resources => {}
            });
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

    /// 由内容区管理滚动。
    fn scroll_bars(&self, _tab: &Self::Tab) -> [bool; 2] {
        [false, false]
    }

    fn modification_color(&self, tab: &Self::Tab) -> Option<egui::Color32> {
        if tab.is_modified {
            return Some(theme::palette().accent_orange);
        }
        if let Some(path) = &tab.entry_path
            && self.jar_modified.contains(path)
        {
            return Some(theme::palette().accent_green);
        }
        None
    }
}

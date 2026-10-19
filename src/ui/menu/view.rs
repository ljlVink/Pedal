//! View 子菜单
//!
//! @author sky
//! @author ljlVink

use super::MenuState;
use crate::app::App;
use crate::appearance::theme;
use crate::ui::editor::view_toggle::ActiveView;
use eframe::egui;
use egui_shell::components::{menu_item, menu_item_if};
use rust_i18n::t;

pub fn render(ui: &mut egui::Ui, app: &mut App) {
    let mt = &theme::menu_theme();
    let state = MenuState::from_app(app);
    if menu_item_if(ui, mt, &t!("editor.view_source"), None, state.has_tab) {
        ui.close();
        app.layout.editor.set_focused_view(ActiveView::Decompiled);
    }
    if menu_item_if(ui, mt, &t!("menu.hex"), None, state.has_tab) {
        ui.close();
        app.layout.editor.set_focused_view(ActiveView::Hex);
    }
    ui.separator();
    if menu_item_if(
        ui,
        mt,
        &t!("menu.toggle_viewport"),
        Some(&app.settings.keymap.toggle_viewport),
        state.has_tab,
    ) {
        ui.close();
        app.layout.editor.toggle_viewport();
    }
    if menu_item(
        ui,
        mt,
        &t!("menu.toggle_explorer"),
        Some(&app.settings.keymap.toggle_explorer),
    ) {
        ui.close();
        app.layout.explorer_visible = !app.layout.explorer_visible;
    }
}

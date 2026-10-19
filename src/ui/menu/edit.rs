//! Edit 子菜单
//!
//! @author sky
//! @author ljlVink

use super::MenuState;
use crate::app::App;
use crate::appearance::theme;
use eframe::egui;
use egui_shell::components::menu_item_if;
use rust_i18n::t;

pub fn render(ui: &mut egui::Ui, app: &mut App) {
    let mt = &theme::menu_theme();
    let state = MenuState::from_app(app);
    if menu_item_if(
        ui,
        mt,
        &t!("menu.save"),
        Some(&app.settings.keymap.save),
        state.has_tab,
    ) {
        ui.close();
        app.save_active_tab();
    }
    ui.separator();
    if menu_item_if(
        ui,
        mt,
        &t!("menu.find"),
        Some(&app.settings.keymap.find),
        state.has_tab,
    ) {
        ui.close();
        app.layout.editor.open_find();
    }
    if menu_item_if(
        ui,
        mt,
        &t!("menu.find_in_files"),
        Some(&app.settings.keymap.find_in_files),
        state.has_jar,
    ) {
        ui.close();
        app.layout.search.open();
    }
}

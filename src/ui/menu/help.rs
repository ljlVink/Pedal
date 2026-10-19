//! Help 子菜单
//!
//! @author sky
//! @author ljlVink

use crate::app::App;
use crate::appearance::theme;
use eframe::egui;
use egui_shell::components::menu_item;
use rust_i18n::t;

pub fn render(ui: &mut egui::Ui, app: &mut App) {
    let mt = &theme::menu_theme();
    if menu_item(ui, mt, &t!("menu.about"), None) {
        ui.close();
        app.layout.about_open = true;
    }
}

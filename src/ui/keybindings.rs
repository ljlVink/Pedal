//! 应用快捷键定义：默认值 + KeyMap 构建
//!
//! @author sky

use crate::app::App;
use crate::settings::KeymapSettings;
use eframe::egui;
use egui_keybind::{KeyBind, KeyMap};

pub const DEFAULT_TOGGLE_EXPLORER: KeyBind = KeyBind::alt(egui::Key::Num1);
pub const DEFAULT_OPEN_JAR: KeyBind = KeyBind::ctrl(egui::Key::O);
pub const DEFAULT_EXPORT_DECOMPILED: KeyBind = KeyBind::ctrl_shift(egui::Key::E);
pub const DEFAULT_EXPORT_JAR: KeyBind = KeyBind::ctrl_shift(egui::Key::S);
pub const DEFAULT_SAVE: KeyBind = KeyBind::ctrl(egui::Key::S);
pub const DEFAULT_CLOSE_TAB: KeyBind = KeyBind::ctrl(egui::Key::W);
pub const DEFAULT_CLOSE_ALL_TABS: KeyBind = KeyBind::ctrl_alt(egui::Key::W);
pub const DEFAULT_CYCLE_VIEW: KeyBind = KeyBind::key(egui::Key::Tab);
pub const DEFAULT_OPEN_SETTINGS: KeyBind = KeyBind::ctrl(egui::Key::Comma);
pub const DEFAULT_TOGGLE_VIEWPORT: KeyBind = KeyBind::ctrl(egui::Key::M);

/// 根据用户配置构建 KeyMap
pub fn build_keymap(km: &KeymapSettings) -> KeyMap<App> {
    KeyMap::new()
        .bind(km.toggle_explorer, |a: &mut App| {
            a.layout.explorer_visible = !a.layout.explorer_visible
        })
        .bind(km.open_jar, |a: &mut App| a.request_open_jar_dialog())
        .bind(km.close_tab, |a: &mut App| {
            a.layout.editor.close_active_tab()
        })
        .bind(km.close_all_tabs, |a: &mut App| {
            a.layout.editor.close_all_tabs()
        })
        .bind(km.save, |a: &mut App| a.save_active_tab())
        .bind(km.cycle_view, |a: &mut App| a.layout.editor.cycle_view())
        .bind(km.open_settings, |a: &mut App| a.open_settings())
        .bind(km.toggle_viewport, |a: &mut App| {
            a.layout.editor.toggle_viewport()
        })
        .bind(km.export_decompiled, |a: &mut App| a.export_decompiled())
        .bind(km.export_jar, |a: &mut App| a.export_jar())
}

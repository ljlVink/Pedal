//! 菜单栏：File / Edit / View / Help
//!
//! @author sky
//! @author ljlVink

mod edit;
mod file;
mod help;
mod view;

use crate::app::App;
use crate::appearance::theme;
use crate::appearance::theme::flat_button_theme;
use eframe::egui;
use egui_shell::components::FlatButton;
use rust_i18n::t;

tabookit::class! {
    /// 菜单各项的可用状态（从 App 快照，供子菜单判断 enabled）
    struct MenuState {
        /// 有 JAR 打开
        has_jar: bool,
        /// 有活跃的编辑器 tab
        has_tab: bool,
        /// ArkTS 工作区（虚拟归档，只读；加载即全量反编译）
        is_ark: bool,
        /// 没有正在进行的后台任务（导出）
        is_idle: bool,
        /// JAR 内存中存在已修改条目
        has_jar_changes: bool,
    }

    fn from_app(app: &mut App) -> Self {
        let has_jar = app.workspace.is_loaded();
        let has_tab = app.layout.editor.focused_tab_mut().is_some();
        let is_ark = app
            .workspace
            .loaded()
            .is_some_and(|s| s.ark.is_some());
        let is_idle = app.exporting.is_none();
        let has_jar_changes = app
            .workspace
            .jar()
            .is_some_and(|jar| jar.has_modified_entries());
        Self {
            has_jar,
            has_tab,
            is_ark,
            is_idle,
            has_jar_changes,
        }
    }
}

type MenuRenderer = fn(&mut egui::Ui, &mut App);

/// 渲染菜单栏（注入到标题栏）
pub fn menu_bar(ui: &mut egui::Ui, app: &mut App) {
    let fbt = flat_button_theme(theme::TEXT_SECONDARY);
    let menus: &[(&str, MenuRenderer)] = &[
        (&t!("menu.file"), file::render),
        (&t!("menu.edit"), edit::render),
        (&t!("menu.view"), view::render),
        (&t!("menu.help"), help::render),
    ];
    for (name, render) in menus {
        let btn = ui.add(FlatButton::new(name, &fbt).min_size(egui::vec2(40.0, 24.0)));
        egui::Popup::menu(&btn)
            .close_behavior(egui::PopupCloseBehavior::CloseOnClick)
            .show(|ui| {
                ui.style_mut().visuals.widgets.hovered.bg_fill = theme::BG_HOVER;
                render(ui, app);
            });
    }
}

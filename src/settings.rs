//! 用户配置：数据定义 + 设置面板 UI
//!
//! TOML 持久化由 [`egui_shell::components::SettingsFile`] trait 提供，
//! 所有 section 均标注 `#[serde(default)]`，新增字段不会破坏旧配置文件。
//!
//! @author sky
//! @author ljlVink

use crate::appearance::{codicon, theme};
use eframe::egui;
use egui_keybind::KeyBind;
use egui_shell::components::{
    SectionDef, SettingsFile, SettingsPanel, SettingsTheme, section_header,
};
use egui_shell::keybind_rows;
use rust_i18n::t;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

/// 最近打开列表上限
const MAX_RECENT: usize = 10;

/// 界面语言
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Language {
    #[default]
    #[serde(rename = "zh")]
    Zh,
    #[serde(rename = "en")]
    En,
}

impl Language {
    pub const ALL: &[Self] = &[Self::Zh, Self::En];

    /// 返回 rust-i18n 使用的 locale code
    pub fn code(self) -> &'static str {
        match self {
            Self::Zh => "zh",
            Self::En => "en",
        }
    }

    /// 返回语言显示名称
    pub fn label(self) -> String {
        match self {
            Self::Zh => t!("lang.zh").to_string(),
            Self::En => t!("lang.en").to_string(),
        }
    }
}

/// 打开第二个项目时的窗口策略
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum OpenBehavior {
    /// 每次弹窗询问用户
    #[default]
    #[serde(rename = "ask")]
    Ask,
    /// 启动新进程实例打开
    #[serde(rename = "new_window")]
    NewWindow,
    /// 在当前窗口中替换现有项目
    #[serde(rename = "current_window")]
    CurrentWindow,
}

impl OpenBehavior {
    pub const ALL: &[Self] = &[Self::CurrentWindow, Self::NewWindow, Self::Ask];

    /// 返回显示名称
    pub fn label(self) -> String {
        match self {
            Self::CurrentWindow => t!("settings.open_behavior_current").to_string(),
            Self::NewWindow => t!("settings.open_behavior_new").to_string(),
            Self::Ask => t!("settings.open_behavior_ask").to_string(),
        }
    }
}

/// 最近打开的文件条目
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RecentEntry {
    /// 文件完整路径
    pub path: String,
    /// 文件名（显示用）
    pub name: String,
    /// 打开时间（unix epoch 秒）
    pub timestamp: u64,
}

/// 快捷键配置
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct KeymapSettings {
    /// 切换资源管理器
    pub toggle_explorer: KeyBind,
    /// 打开 JAR
    pub open_jar: KeyBind,
    /// 打开查找
    pub find: KeyBind,
    /// 打开文件中查找
    pub find_in_files: KeyBind,
    /// 保存当前标签页
    pub save: KeyBind,
    /// 关闭当前标签页
    pub close_tab: KeyBind,
    /// 关闭全部标签页
    pub close_all_tabs: KeyBind,
    /// 导出反编译结果
    pub export_decompiled: KeyBind,
    /// 导出 JAR
    pub export_jar: KeyBind,
    /// 循环切换视图
    pub cycle_view: KeyBind,
    /// 打开设置
    pub open_settings: KeyBind,
    /// 切换视窗模式
    pub toggle_viewport: KeyBind,
}

impl Default for KeymapSettings {
    fn default() -> Self {
        use crate::ui::keybindings;
        Self {
            toggle_explorer: keybindings::DEFAULT_TOGGLE_EXPLORER,
            open_jar: keybindings::DEFAULT_OPEN_JAR,
            find: keybindings::DEFAULT_FIND,
            find_in_files: keybindings::DEFAULT_FIND_IN_FILES,
            save: keybindings::DEFAULT_SAVE,
            close_tab: keybindings::DEFAULT_CLOSE_TAB,
            close_all_tabs: keybindings::DEFAULT_CLOSE_ALL_TABS,
            export_decompiled: keybindings::DEFAULT_EXPORT_DECOMPILED,
            export_jar: keybindings::DEFAULT_EXPORT_JAR,
            cycle_view: keybindings::DEFAULT_CYCLE_VIEW,
            open_settings: keybindings::DEFAULT_OPEN_SETTINGS,
            toggle_viewport: keybindings::DEFAULT_TOGGLE_VIEWPORT,
        }
    }
}

impl SettingsFile for Settings {
    fn app_name() -> &'static str {
        "pedal"
    }
}

tabookit::class! {
    /// 顶层配置
    #[derive(Clone, Debug, Default, Serialize, Deserialize)]
    #[serde(default)]
    pub struct Settings {
        /// 界面语言
        pub language: Language,
        /// 打开第二个项目时的窗口策略
        pub open_behavior: OpenBehavior,
        /// 快捷键设置
        pub keymap: KeymapSettings,
        /// 最近打开列表
        pub recent: Vec<RecentEntry>,
    }

    /// 仅读取语言配置（用于启动时在 UI 初始化前设置 locale）
    pub fn load_for_locale() -> Self {
        Self::load()
    }

    /// 将文件加入最近打开列表头部（去重 + 截断）
    pub fn add_recent(&mut self, path: &Path, name: &str) {
        let path_str = path.to_string_lossy().into_owned();
        self.recent.retain(|e| e.path != path_str);
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        self.recent.insert(
            0,
            RecentEntry {
                path: path_str,
                name: name.to_owned(),
                timestamp,
            },
        );
        self.recent.truncate(MAX_RECENT);
    }

    /// 从最近打开列表中移除指定路径
    pub fn remove_recent(&mut self, path: &Path) {
        let path_str = path.to_string_lossy();
        self.recent.retain(|e| e.path != *path_str);
    }

    /// 清空最近打开列表
    pub fn clear_recent(&mut self) {
        self.recent.clear();
    }
}

/// 设置面板输出
pub struct SettingsOutput {
    /// 配置变更
    pub settings: Option<Settings>,
}

/// 构造预配置的设置面板实例
pub fn new_panel() -> SettingsPanel<Settings> {
    SettingsPanel::new("settings_window", t!("settings.title").to_string())
        .icon(codicon::SETTINGS_GEAR)
        .default_size([700.0, 500.0])
        .min_size([520.0, 380.0])
}

/// 渲染设置面板
pub fn show(
    panel: &mut SettingsPanel<Settings>,
    ctx: &egui::Context,
    shell_theme: &egui_shell::ShellTheme,
) -> SettingsOutput {
    let st = theme::settings_theme();
    let sections = [
        SectionDef {
            icon: codicon::SETTINGS_GEAR,
            label: t!("settings.general").to_string(),
        },
        SectionDef {
            icon: codicon::KEYBOARD,
            label: t!("settings.keymap").to_string(),
        },
    ];
    let settings = panel.show(ctx, shell_theme, &st, &sections, |draft, active, ui, st| {
        render_section(draft, active, ui, st)
    });
    SettingsOutput { settings }
}

/// 渲染指定 section 的内容
fn render_section(
    draft: &mut Settings,
    active: usize,
    ui: &mut egui::Ui,
    st: &SettingsTheme,
) -> bool {
    match active {
        0 => render_general(draft, ui, st),
        _ => render_keymap(&mut draft.keymap, ui, st),
    }
}

fn render_general(draft: &mut Settings, ui: &mut egui::Ui, st: &SettingsTheme) -> bool {
    let mut changed = false;
    section_header(ui, st, &t!("settings.section_general"));
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.add_space(16.0);
        ui.label(
            egui::RichText::new(t!("settings.language").to_string())
                .size(13.0)
                .color(st.text_primary),
        );
        ui.add_space(8.0);
        let current = draft.language;
        egui::ComboBox::from_id_salt("language_combo")
            .selected_text(current.label())
            .width(120.0)
            .show_ui(ui, |ui| {
                for &lang in Language::ALL {
                    if ui.selectable_label(current == lang, lang.label()).clicked() {
                        draft.language = lang;
                        rust_i18n::set_locale(lang.code());
                        changed = true;
                    }
                }
            });
    });
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.add_space(16.0);
        ui.label(
            egui::RichText::new(t!("settings.open_behavior").to_string())
                .size(13.0)
                .color(st.text_primary),
        );
        ui.add_space(8.0);
        let current = draft.open_behavior;
        egui::ComboBox::from_id_salt("open_behavior_combo")
            .selected_text(current.label())
            .width(140.0)
            .show_ui(ui, |ui| {
                for &behavior in OpenBehavior::ALL {
                    if ui
                        .selectable_label(current == behavior, behavior.label())
                        .clicked()
                    {
                        draft.open_behavior = behavior;
                        changed = true;
                    }
                }
            });
    });
    ui.add_space(4.0);
    changed
}

fn render_keymap(km: &mut KeymapSettings, ui: &mut egui::Ui, st: &SettingsTheme) -> bool {
    let defaults = KeymapSettings::default();
    let hint = t!("settings.press_key");
    let mut changed = false;
    section_header(ui, st, &t!("settings.section_general"));
    changed |= keybind_rows!(ui, st, hint, km, defaults,
        t!("settings.open_jar") => open_jar,
        t!("settings.toggle_explorer") => toggle_explorer,
        t!("settings.open_settings") => open_settings,
    );
    section_header(ui, st, &t!("settings.section_editor"));
    changed |= keybind_rows!(ui, st, hint, km, defaults,
        t!("settings.save") => save,
        t!("settings.find") => find,
        t!("settings.find_in_files") => find_in_files,
        t!("settings.close_tab") => close_tab,
        t!("settings.close_all_tabs") => close_all_tabs,
        t!("settings.cycle_view") => cycle_view,
        t!("settings.toggle_viewport") => toggle_viewport,
    );
    section_header(ui, st, &t!("settings.section_export"));
    changed |= keybind_rows!(ui, st, hint, km, defaults,
        t!("settings.export_jar") => export_jar,
        t!("settings.export_decompiled") => export_decompiled,
    );
    changed
}

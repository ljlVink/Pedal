//! Codicon 图标 codepoint 常量
//!
//! 所有 codepoint 来自 VS Code Codicon 字体，按用途分组。
//! 参考：https://microsoft.github.io/vscode-codicons/dist/codicon.html
//!
//! @author sky
//! @author ljlVink

use eframe::egui;

/// 文件图标
pub const FILE: &str = "\u{EA7B}";
/// 关闭的文件夹
pub const FOLDER: &str = "\u{EA83}";
/// 展开的文件夹
pub const FOLDER_OPENED: &str = "\u{EAF7}";
/// 符号类（class 图标）
pub const SYMBOL_CLASS: &str = "\u{EB5B}";
/// JAR / 归档包图标
pub const PACKAGE: &str = "\u{EB29}";

/// 向右箭头（折叠状态）
pub const CHEVRON_RIGHT: &str = "\u{EAB6}";
/// 向下箭头（展开状态）
pub const CHEVRON_DOWN: &str = "\u{EAB4}";
/// 全部展开
pub const EXPAND_ALL: &str = "\u{EB95}";
/// 全部折叠
pub const COLLAPSE_ALL: &str = "\u{EAC5}";

/// 搜索
pub const SEARCH: &str = "\u{EA6D}";
/// 设置齿轮
pub const SETTINGS_GEAR: &str = "\u{EB51}";
/// 图钉（大头针，竖直）
pub const PIN: &str = "\u{EBA0}";
/// 键盘
pub const KEYBOARD: &str = "\u{EB44}";

/// Codicon 字体族（来自 egui-shell）
pub fn family() -> egui::FontFamily {
    egui_shell::codicon_family()
}

//! 浅色 / 深色主题及组件色板
//!
//! 应用统一维护语义色板，组件通过主题结构体接收配色。
//!
//! @author sky
//! @author ljlVink

use eframe::egui;
use rust_i18n::t;

use super::codicon;

/// 界面主题，旧配置未指定时使用浅色。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThemeMode {
    #[default]
    Light,
    Dark,
}

impl ThemeMode {
    pub const ALL: &[Self] = &[Self::Light, Self::Dark];

    /// 本地化显示名称。
    pub fn label(self) -> String {
        match self {
            Self::Light => t!("settings.theme_light").to_string(),
            Self::Dark => t!("settings.theme_dark").to_string(),
        }
    }
}

// 应用每个进程仅有一个窗口；后台读取色板时无需加锁。
static DARK_MODE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

tabookit::class! {
    /// 窗口、编辑器及工具面板共享的语义色板。
    #[derive(Clone, Copy, Debug)]
    pub struct Palette {
        /// 最深背景（Island 底色）
        pub bg_darkest: egui::Color32,
        /// 行号栏背景（介于 island 底色与面板底色之间）
        pub bg_gutter: egui::Color32,
        /// 主背景（窗口底色、Header、StatusBar）
        pub bg_dark: egui::Color32,
        /// 中层背景（输入框、编辑区、ViewToggle 容器）
        pub bg_medium: egui::Color32,
        /// 浅层背景（关闭按钮 hover 等）
        pub bg_light: egui::Color32,
        /// 悬停背景
        pub bg_hover: egui::Color32,
        /// 主边框
        pub border: egui::Color32,
        /// 浅边框
        pub border_light: egui::Color32,
        /// 主要文字
        pub text_primary: egui::Color32,
        /// 次要文字
        pub text_secondary: egui::Color32,
        /// 暗淡文字
        pub text_muted: egui::Color32,
        /// 主色调铜绿
        pub verdigris: egui::Color32,
        /// 辅助绿
        pub accent_green: egui::Color32,
        /// 辅助橙
        pub accent_orange: egui::Color32,
        /// 辅助红
        pub accent_red: egui::Color32,
        /// 辅助青
        pub accent_cyan: egui::Color32,
        /// 代码默认文字
        pub syn_text: egui::Color32,
        /// 关键字
        pub syn_keyword: egui::Color32,
        /// JVM 操作码（区别于关键字，偏蓝灰，JetBrains 风格）
        pub syn_opcode: egui::Color32,
        /// 字符串
        pub syn_string: egui::Color32,
        /// 类型 / 类名（白色）
        pub syn_type: egui::Color32,
        /// 常量 / 字段
        pub syn_constant: egui::Color32,
        /// 数字
        pub syn_number: egui::Color32,
        /// 注释
        pub syn_comment: egui::Color32,
        /// 注解
        pub syn_annotation: egui::Color32,
        /// 方法调用
        pub syn_method: egui::Color32,
        /// 方法声明
        pub syn_method_decl: egui::Color32,
        /// 标题栏按钮 hover
        pub caption_hover: egui::Color32,
        /// 关闭按钮 hover（Win11 风格红）
        pub close_hover: egui::Color32,
        /// Hex 视图 null 字节色（premultiplied）
        pub hex_null: egui::Color32,
        /// Hex 视图行 hover 底色（premultiplied，极淡）
        pub hex_row_hover: egui::Color32,
        /// Hex 视图字节 hover 底色（premultiplied，铜绿色调）
        pub hex_byte_hover: egui::Color32,
        /// Hex 视图选区底色（premultiplied，铜绿色调）
        pub hex_selection: egui::Color32,
        /// Hex 视图光标底色（premultiplied，铜绿色调）
        pub hex_cursor: egui::Color32,
        /// Hex 视图列分隔线（premultiplied，近透明）
        pub hex_separator: egui::Color32,
    }
}

const DARK: Palette = Palette {
    bg_darkest: egui::Color32::from_rgb(17, 17, 18),
    bg_gutter: egui::Color32::from_rgb(19, 19, 20),
    bg_dark: egui::Color32::from_rgb(21, 21, 22),
    bg_medium: egui::Color32::from_rgb(28, 28, 30),
    bg_light: egui::Color32::from_rgb(37, 37, 39),
    bg_hover: egui::Color32::from_rgb(46, 46, 49),
    border: egui::Color32::from_rgb(46, 46, 48),
    border_light: egui::Color32::from_rgb(58, 58, 61),
    text_primary: egui::Color32::from_rgb(236, 236, 239),
    text_secondary: egui::Color32::from_rgb(160, 160, 171),
    text_muted: egui::Color32::from_rgb(92, 92, 106),
    verdigris: egui::Color32::from_rgb(67, 179, 174),
    accent_green: egui::Color32::from_rgb(110, 231, 183),
    accent_orange: egui::Color32::from_rgb(224, 164, 88),
    accent_red: egui::Color32::from_rgb(224, 108, 117),
    accent_cyan: egui::Color32::from_rgb(126, 200, 200),
    syn_text: egui::Color32::from_rgb(188, 190, 196),
    syn_keyword: egui::Color32::from_rgb(207, 142, 109),
    syn_opcode: egui::Color32::from_rgb(104, 151, 187),
    syn_string: egui::Color32::from_rgb(106, 171, 115),
    syn_type: egui::Color32::from_rgb(236, 236, 239),
    syn_constant: egui::Color32::from_rgb(199, 125, 187),
    syn_number: egui::Color32::from_rgb(42, 172, 184),
    syn_comment: egui::Color32::from_rgb(122, 126, 133),
    syn_annotation: egui::Color32::from_rgb(179, 174, 96),
    syn_method: egui::Color32::from_rgb(124, 180, 255),
    syn_method_decl: egui::Color32::from_rgb(86, 168, 245),
    caption_hover: egui::Color32::from_rgb(42, 42, 47),
    close_hover: egui::Color32::from_rgb(196, 43, 28),
    hex_null: egui::Color32::from_rgba_premultiplied(60, 60, 70, 128),
    hex_row_hover: egui::Color32::from_rgba_premultiplied(15, 15, 16, 15),
    hex_byte_hover: egui::Color32::from_rgba_premultiplied(17, 45, 44, 64),
    hex_selection: egui::Color32::from_rgba_premultiplied(20, 54, 52, 77),
    hex_cursor: egui::Color32::from_rgba_premultiplied(34, 90, 87, 128),
    hex_separator: egui::Color32::from_rgba_premultiplied(13, 13, 13, 13),
};

const LIGHT: Palette = Palette {
    bg_darkest: egui::Color32::from_rgb(255, 255, 255),
    bg_gutter: egui::Color32::from_rgb(248, 249, 250),
    bg_dark: egui::Color32::from_rgb(243, 245, 247),
    bg_medium: egui::Color32::from_rgb(234, 238, 242),
    bg_light: egui::Color32::from_rgb(225, 231, 236),
    bg_hover: egui::Color32::from_rgb(220, 229, 232),
    border: egui::Color32::from_rgb(209, 216, 224),
    border_light: egui::Color32::from_rgb(190, 201, 207),
    text_primary: egui::Color32::from_rgb(28, 37, 47),
    text_secondary: egui::Color32::from_rgb(77, 91, 106),
    text_muted: egui::Color32::from_rgb(103, 116, 129),
    verdigris: egui::Color32::from_rgb(0, 120, 115),
    accent_green: egui::Color32::from_rgb(28, 122, 68),
    accent_orange: egui::Color32::from_rgb(151, 87, 24),
    accent_red: egui::Color32::from_rgb(180, 50, 66),
    accent_cyan: egui::Color32::from_rgb(14, 110, 130),
    syn_text: egui::Color32::from_rgb(41, 50, 61),
    syn_keyword: egui::Color32::from_rgb(152, 63, 26),
    syn_opcode: egui::Color32::from_rgb(40, 100, 150),
    syn_string: egui::Color32::from_rgb(45, 119, 59),
    syn_type: egui::Color32::from_rgb(28, 37, 47),
    syn_constant: egui::Color32::from_rgb(143, 56, 139),
    syn_number: egui::Color32::from_rgb(0, 113, 130),
    syn_comment: egui::Color32::from_rgb(106, 115, 124),
    syn_annotation: egui::Color32::from_rgb(120, 101, 25),
    syn_method: egui::Color32::from_rgb(34, 96, 172),
    syn_method_decl: egui::Color32::from_rgb(15, 92, 159),
    caption_hover: egui::Color32::from_rgb(221, 228, 234),
    close_hover: egui::Color32::from_rgb(196, 43, 28),
    hex_null: egui::Color32::from_rgb(150, 160, 169),
    hex_row_hover: egui::Color32::from_rgba_premultiplied(0, 0, 0, 8),
    hex_byte_hover: egui::Color32::from_rgba_premultiplied(0, 12, 11, 25),
    hex_selection: egui::Color32::from_rgba_premultiplied(0, 28, 27, 60),
    hex_cursor: egui::Color32::from_rgba_premultiplied(0, 42, 40, 90),
    hex_separator: egui::Color32::from_rgba_premultiplied(0, 0, 0, 24),
};

/// 设置应用色板，启动时在创建组件之前调用。
pub fn set_mode(mode: ThemeMode) {
    DARK_MODE.store(
        mode == ThemeMode::Dark,
        std::sync::atomic::Ordering::Relaxed,
    );
}

/// 获取当前色板。
pub fn palette() -> &'static Palette {
    if DARK_MODE.load(std::sync::atomic::Ordering::Relaxed) {
        &DARK
    } else {
        &LIGHT
    }
}

/// 同时切换应用色板与 egui 原生控件样式。
pub fn apply(ctx: &egui::Context, mode: ThemeMode) {
    set_mode(mode);
    let mut visuals = match mode {
        ThemeMode::Light => {
            ctx.set_theme(egui::Theme::Light);
            egui::Visuals::light()
        }
        ThemeMode::Dark => {
            ctx.set_theme(egui::Theme::Dark);
            egui::Visuals::dark()
        }
    };
    let p = palette();
    visuals.override_text_color = Some(p.text_primary);
    visuals.panel_fill = p.bg_dark;
    visuals.window_fill = p.bg_gutter;
    visuals.extreme_bg_color = p.bg_darkest;
    visuals.faint_bg_color = p.bg_medium;
    visuals.window_stroke = egui::Stroke::new(1.0_f32, p.border);
    visuals.selection.bg_fill = verdigris_alpha(60);
    visuals.selection.stroke = egui::Stroke::new(1.0_f32, p.text_primary);
    visuals.hyperlink_color = p.verdigris;
    ctx.set_visuals(visuals);
    ctx.request_repaint();
}

/// 标题栏高度
pub const TITLE_BAR_HEIGHT: f32 = 36.0;
/// 文件面板宽度
pub const FILE_PANEL_WIDTH: f32 = 260.0;
/// 状态栏高度
pub const STATUS_BAR_HEIGHT: f32 = 24.0;
/// Island 圆角半径
pub const ISLAND_RADIUS: u8 = 8;
/// Island 之间的间距
pub const ISLAND_GAP: f32 = 6.0;
/// Island 到窗口左右边缘的水平边距
pub const ISLAND_MARGIN_H: f32 = 6.0;
/// Island 到窗口上下边缘的垂直边距
pub const ISLAND_MARGIN_V: f32 = 4.0;
/// 修改文件弹出列表单行高度
pub const MODIFIED_POPUP_ITEM_HEIGHT: f32 = 22.0;
/// 修改文件弹出列表内边距
pub const MODIFIED_POPUP_PAD: f32 = 4.0;
/// 修改文件弹出列表最大可见行数
pub const MODIFIED_POPUP_MAX_VISIBLE: usize = 12;
/// 修改文件弹出列表宽度
pub const MODIFIED_POPUP_WIDTH: f32 = 260.0;
/// 视图切换动画时长（秒）
pub const VIEW_TOGGLE_ANIM_DURATION: f32 = 0.15;
/// Explorer 面板最小宽度
pub const EXPLORER_MIN_WIDTH: f32 = 160.0;
/// Explorer 面板最大宽度
pub const EXPLORER_MAX_WIDTH: f32 = 600.0;
/// Explorer 折叠/展开动画时长（秒）
pub const EXPLORER_ANIM_DURATION: f32 = 0.08;
/// 搜索结果列表最小高度
pub const SEARCH_MIN_RESULTS_H: f32 = 80.0;
/// 搜索预览面板最小高度
pub const SEARCH_MIN_PREVIEW_H: f32 = 80.0;
/// 搜索面板分割线拖拽手柄高度
pub const SEARCH_RESIZE_HANDLE_H: f32 = 6.0;
/// 文件树行高
pub const TREE_ROW_HEIGHT: f32 = 22.0;
/// 文件树展开/折叠动画时长（秒）
pub const TREE_EXPAND_DURATION: f32 = 0.12;
/// 搜索结果行高
pub const SEARCH_ROW_HEIGHT: f32 = 24.0;
/// 搜索分组 header 行高
pub const SEARCH_GROUP_HEADER_HEIGHT: f32 = 28.0;

/// Explorer / Editor 面板共用的 island 样式
pub fn island_style() -> egui_shell::components::IslandStyle {
    egui_shell::components::IslandStyle {
        radius: ISLAND_RADIUS as f32,
        fill: palette().bg_darkest,
        mask: palette().bg_dark,
    }
}

/// 铜绿色带透明度（用于选中高亮等）
pub fn verdigris_alpha(alpha: u8) -> egui::Color32 {
    egui::Color32::from_rgba_premultiplied(
        (palette().verdigris.r() as u16 * alpha as u16 / 255) as u8,
        (palette().verdigris.g() as u16 * alpha as u16 / 255) as u8,
        (palette().verdigris.b() as u16 * alpha as u16 / 255) as u8,
        alpha,
    )
}

/// 扁平按钮主题
pub fn flat_button_theme(inactive_color: egui::Color32) -> egui_shell::components::FlatButtonTheme {
    egui_shell::components::FlatButtonTheme {
        text_primary: palette().text_primary,
        text_active: palette().verdigris,
        text_inactive: inactive_color,
        bg_hover: palette().bg_hover,
        bg_pressed: palette().bg_light,
    }
}

/// 浮动窗口（Search / Settings 等）的 WindowConfig
pub fn window_config() -> egui_shell::WindowConfig {
    egui_shell::WindowConfig {
        frame: egui::Frame {
            fill: palette().bg_gutter,
            corner_radius: egui::CornerRadius::same(8),
            stroke: egui::Stroke::new(1.0_f32, palette().border),
            inner_margin: egui::Margin::same(0),
            shadow: egui::Shadow {
                spread: 2,
                blur: 20,
                offset: [0, 4],
                color: egui::Color32::from_black_alpha(80),
            },
            ..Default::default()
        },
        header_height: 32.0,
        pin_icon: codicon::PIN,
        pin_tooltip: t!("window.pin").to_string(),
        unpin_tooltip: t!("window.unpin").to_string(),
    }
}

/// 主窗口外壳主题。
pub fn shell_theme() -> egui_shell::ShellTheme {
    let p = palette();
    egui_shell::ShellTheme {
        bg: p.bg_dark,
        bg_hover: p.bg_hover,
        bg_active: p.bg_light,
        text_primary: p.text_primary,
        text_secondary: p.text_secondary,
        text_muted: p.text_muted,
        accent: p.verdigris,
        separator: p.border,
        icon_font: codicon::family(),
        caption_hover: p.caption_hover,
        close_hover: p.close_hover,
        title_bar_height: TITLE_BAR_HEIGHT,
        window: window_config(),
    }
}

/// 设置面板 widget 主题
pub fn settings_theme() -> egui_shell::components::SettingsTheme {
    egui_shell::components::SettingsTheme {
        text_primary: palette().text_primary,
        text_secondary: palette().text_secondary,
        text_muted: palette().text_muted,
        bg_hover: palette().bg_hover,
        bg_light: palette().bg_light,
        bg_medium: palette().bg_medium,
        bg_sidebar: palette().bg_darkest,
        border: palette().border,
        accent: palette().verdigris,
        icon_font: egui::FontFamily::Name("codicon".into()),
        chevron_icon: codicon::CHEVRON_DOWN,
    }
}

/// 菜单项主题
pub fn menu_theme() -> egui_shell::components::MenuTheme {
    egui_shell::components::MenuTheme {
        text_primary: palette().text_primary,
        text_muted: palette().text_muted,
        bg_hover: palette().bg_hover,
    }
}

/// 状态栏主题
pub fn status_bar_theme() -> egui_shell::components::StatusBarTheme {
    egui_shell::components::StatusBarTheme {
        bg: palette().bg_dark,
        separator: palette().border,
    }
}

/// 确认弹窗主题
pub fn confirm_theme() -> egui_shell::components::ConfirmTheme {
    egui_shell::components::ConfirmTheme {
        frame: egui::Frame {
            fill: palette().bg_light,
            stroke: egui::Stroke::new(1.0_f32, palette().border_light),
            corner_radius: egui::CornerRadius::same(8),
            shadow: egui::Shadow {
                spread: 2,
                blur: 24,
                offset: [0, 8],
                color: egui::Color32::from_black_alpha(100),
            },
            ..Default::default()
        },
        title_color: palette().text_primary,
        message_color: palette().text_secondary,
        separator: palette().border,
        backdrop: egui::Color32::from_black_alpha(80),
        button: flat_button_theme(palette().text_secondary),
    }
}

/// 编辑器代码视图主题
pub fn editor_theme() -> egui_editor::CodeViewTheme {
    egui_editor::CodeViewTheme {
        syntax: egui_editor::SyntaxTheme {
            text: palette().syn_text,
            keyword: palette().syn_keyword,
            opcode: palette().syn_opcode,
            string: palette().syn_string,
            type_name: palette().syn_type,
            number: palette().syn_number,
            comment: palette().syn_comment,
            annotation: palette().syn_annotation,
            muted: palette().text_muted,
            constant: palette().syn_constant,
            method_call: palette().syn_method,
            method_declaration: palette().syn_method_decl,
        },
        bg: palette().bg_darkest,
        gutter_bg: palette().bg_gutter,
        line_number_color: palette().text_muted,
        search_bg: verdigris_alpha(40),
        search_current_bg: verdigris_alpha(100),
        code_font_size: 13.0,
        line_highlight: verdigris_alpha(50),
        word_highlight_bg: verdigris_alpha(80),
    }
}

/// 编辑器查找栏主题
pub fn find_bar_theme() -> egui_editor::FindBarTheme {
    egui_editor::FindBarTheme {
        bg: palette().bg_dark,
        border: palette().border,
        text_primary: palette().text_primary,
        text_muted: palette().text_muted,
        error_color: palette().accent_red,
        icons: egui_editor::theme::FindBarIcons {
            close: codicon::CLOSE,
            next: codicon::CHEVRON_DOWN,
            prev: codicon::CHEVRON_UP,
            regex: codicon::REGEX,
            whole_word: codicon::WHOLE_WORD,
            case_sensitive: codicon::CASE_SENSITIVE,
            search: codicon::SEARCH,
            font: codicon::family(),
        },
        labels: egui_editor::theme::FindBarLabels {
            close: t!("find.close").to_string(),
            next: t!("find.next").to_string(),
            prev: t!("find.prev").to_string(),
            use_regex: t!("find.use_regex").to_string(),
            match_word: t!("find.match_word").to_string(),
            match_case: t!("find.match_case").to_string(),
            no_results: t!("find.no_results").to_string(),
            result_fmt: |current, total| {
                t!("find.result_count", current = current, total = total).to_string()
            },
            hint: t!("find.hint").to_string(),
        },
        button: flat_button_theme(palette().text_muted),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn luminance(color: egui::Color32) -> f64 {
        let linear = |byte: u8| {
            let value = f64::from(byte) / 255.0;
            if value <= 0.04045 {
                value / 12.92
            } else {
                ((value + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * linear(color.r()) + 0.7152 * linear(color.g()) + 0.0722 * linear(color.b())
    }

    #[test]
    fn switching_updates_native_widgets_and_keeps_text_readable() {
        let ctx = egui::Context::default();
        for mode in [ThemeMode::Light, ThemeMode::Dark, ThemeMode::Light] {
            apply(&ctx, mode);
            let p = palette();
            assert_eq!(
                ctx.global_style().visuals.dark_mode,
                mode == ThemeMode::Dark
            );
            assert_eq!(ctx.global_style().visuals.window_fill, p.bg_gutter);
            assert_eq!(shell_theme().bg, p.bg_dark);
            assert_eq!(editor_theme().syntax.text, p.syn_text);
            for (foreground, background) in [
                (p.text_primary, p.bg_dark),
                (p.text_secondary, p.bg_gutter),
                (p.syn_text, p.bg_darkest),
                (p.syn_keyword, p.bg_darkest),
                (p.syn_string, p.bg_darkest),
                (p.syn_number, p.bg_darkest),
            ] {
                let a = luminance(foreground);
                let b = luminance(background);
                let ratio = (a.max(b) + 0.05) / (a.min(b) + 0.05);
                assert!(ratio >= 4.5, "{mode:?} 主题文字对比度过低：{ratio:.2}");
            }
        }
    }
}

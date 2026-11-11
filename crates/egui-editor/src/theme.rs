//! 编辑器主题配色
//!
//! @author sky

use eframe::egui;

/// 语法高亮配色（12 种 token 类型）
#[derive(Clone)]
pub struct SyntaxTheme {
    /// 普通文本
    pub text: egui::Color32,
    /// 关键字
    pub keyword: egui::Color32,
    /// JVM 操作码（字节码指令，区别于语言关键字）
    pub opcode: egui::Color32,
    /// 字符串
    pub string: egui::Color32,
    /// 类型名
    pub type_name: egui::Color32,
    /// 数字
    pub number: egui::Color32,
    /// 注释
    pub comment: egui::Color32,
    /// 注解
    pub annotation: egui::Color32,
    /// 低对比度文本（标点、分隔符）
    pub muted: egui::Color32,
    /// 常量（enum 值、static final）
    pub constant: egui::Color32,
    /// 方法调用
    pub method_call: egui::Color32,
    /// 方法声明
    pub method_declaration: egui::Color32,
}

/// 代码视图配色（gutter + 编辑区 + 搜索匹配）
#[derive(Clone)]
pub struct CodeViewTheme {
    /// 语法高亮
    pub syntax: SyntaxTheme,
    /// 编辑器背景
    pub bg: egui::Color32,
    /// 行号栏背景
    pub gutter_bg: egui::Color32,
    /// 行号文字色
    pub line_number_color: egui::Color32,
    /// 搜索匹配背景（普通匹配）
    pub search_bg: egui::Color32,
    /// 搜索匹配背景（当前匹配）
    pub search_current_bg: egui::Color32,
    /// 代码字体大小
    pub code_font_size: f32,
    /// 行高亮闪烁底色（scroll_to_line 触发，alpha 由动画控制）
    pub line_highlight: egui::Color32,
    /// 选中同名字段高亮背景
    pub word_highlight_bg: egui::Color32,
}

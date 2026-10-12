//! 编辑器视图渲染：代码视图 + Hex 视图
//!
//! 代码视图委托给 egui-editor，此处提供 EditorTab 解包的薄包装。
//!
//! @author sky
//! @author ljlVink

use super::tab::EditorTab;
use crate::appearance::theme;
use eframe::egui;
use egui_editor::code_view::CodeViewConfig;
use egui_editor::search::FindMatch;

pub use egui_editor::code_view::line_number_width;

/// 在 ScrollArea 外绘制全高背景（左侧 gutter + 右侧编辑区）
pub fn paint_editor_bg(ui: &egui::Ui, full_rect: egui::Rect, gutter_w: f32) {
    let t = theme::editor_theme();
    egui_editor::code_view::paint_editor_bg(ui, full_rect, gutter_w, &t);
}

/// 只读文本视图（非文本 tab 的回退显示）
pub fn render_decompiled(
    ui: &mut egui::Ui,
    tab: &mut EditorTab,
    matches: &[FindMatch],
    current: Option<usize>,
) {
    let t = theme::editor_theme();
    let no_line_mapping: [Option<u32>; 0] = [];
    egui_editor::code_view::code_view(
        ui,
        &tab.decompiled,
        &tab.decompiled_data.spans,
        &no_line_mapping,
        CodeViewConfig {
            matches,
            current,
            known_classes: None,
            ..CodeViewConfig::new(egui::Id::new(&tab.entry_path).with("decompiled"), &t)
        },
        &mut tab.layout_cache,
        &mut tab.pending_scroll_to_line,
    );
}

/// ArkTS 反汇编视图（只读，pandasm 文本）
pub fn render_disasm(
    ui: &mut egui::Ui,
    tab: &mut EditorTab,
    matches: &[FindMatch],
    current: Option<usize>,
) {
    let t = theme::editor_theme();
    let no_line_mapping: [Option<u32>; 0] = [];
    egui_editor::code_view::code_view(
        ui,
        &tab.disasm_text,
        &tab.disasm_data.spans,
        &no_line_mapping,
        CodeViewConfig {
            matches,
            current,
            known_classes: None,
            ..CodeViewConfig::new(egui::Id::new(&tab.entry_path).with("disasm"), &t)
        },
        &mut tab.disasm_layout_cache,
        &mut tab.pending_scroll_to_line,
    );
}

/// 可编辑文本视图
///
/// 文本变更后自动刷新语法高亮数据并标记 tab 为已修改。
pub fn render_editable(
    ui: &mut egui::Ui,
    tab: &mut EditorTab,
    matches: &[FindMatch],
    current: Option<usize>,
) {
    let t = theme::editor_theme();
    let output = egui_editor::code_view::code_view_editable(
        ui,
        &mut tab.decompiled,
        CodeViewConfig {
            lang: tab.language,
            matches,
            current,
            viewport_override: tab.viewport_override,
            ..CodeViewConfig::new(egui::Id::new(&tab.entry_path).with("editable"), &t)
        },
        &mut tab.editable_layout_cache,
        &mut tab.pending_scroll_to_line,
    );
    if output.value {
        tab.refresh_decompiled_data();
        tab.is_modified = true;
    }
}

/// Hex 视图
pub fn render_hex(
    ui: &mut egui::Ui,
    tab: &mut EditorTab,
    matches: &[FindMatch],
    current: Option<usize>,
) {
    let t = super::style::hex::hex_theme();
    let highlights: Vec<(usize, usize)> = matches.iter().map(|m| (m.start, m.end)).collect();
    egui_hex_view::show(
        ui,
        &tab.raw_bytes,
        &mut tab.hex_state,
        &t,
        &highlights,
        current,
    );
}

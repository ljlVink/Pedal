//! 代码视图渲染：行号栏 + 语法高亮 + 搜索匹配
//!
//! 提供只读和可编辑两种 TextEdit 视图，
//! 通过 layouter 注入语法高亮和搜索匹配高亮。
//! 行号栏通过 painter overlay 绘制在视口左侧固定位置。
//!
//! @author sky

mod frame;
mod gutter;
mod layout;
mod navigation;
mod scroll;
mod text_edit;

use crate::highlight::{Language, Span};
use crate::search::FindMatch;
use crate::theme::CodeViewTheme;
use crate::viewport;
use eframe::egui;
use std::collections::HashSet;

pub use crate::viewport::VIEWPORT_TEXT_LEN;
pub use gutter::{line_number_width, paint_editor_bg};
pub use layout::{EditableLayoutCache, LayoutCache};
pub use navigation::NavigationHit;

/// 代码视图配置：身份、主题、语言与搜索/导航选项。
#[derive(Clone, Copy)]
pub struct CodeViewConfig<'a> {
    /// 视图的稳定标识。
    pub id: egui::Id,
    /// 调用方提供的主题。
    pub theme: &'a CodeViewTheme,
    /// 搜索匹配范围。
    pub matches: &'a [FindMatch],
    /// 当前搜索匹配索引。
    pub current: Option<usize>,
    /// 导航可用的类名集合。
    pub known_classes: Option<&'a HashSet<String>>,
    /// 可编辑/结构视图的语法语言。
    pub lang: Language,
    /// 手动覆盖大文本视窗模式；为空时自动判断。
    pub viewport_override: Option<bool>,
}

impl<'a> CodeViewConfig<'a> {
    /// 创建无搜索匹配、自动视窗模式的纯文本视图配置。
    pub fn new(id: egui::Id, theme: &'a CodeViewTheme) -> Self {
        Self {
            id,
            theme,
            matches: &[],
            current: None,
            known_classes: None,
            lang: Language::Plain,
            viewport_override: None,
        }
    }
}

/// 代码视图渲染输出
pub struct CodeViewOutput<T> {
    pub value: T,
    pub response: Option<egui::Response>,
}

use frame::{
    CodeFrameConfig, finish_code_view_frame, line_highlight_ids, remember_line_highlight,
    show_code_view_frame,
};

pub(crate) use gutter::{GUTTER_PAD, TEXT_PAD_LEFT, paint_line_numbers};
pub(crate) use layout::{
    GalleyConfig, LayoutHighlights, byte_offset_at_char, extract_highlight_word, hash_text,
    paint_word_highlight_overlay, rebuild_galley,
};
pub(crate) use scroll::{apply_scroll_delta, detect_edge_scroll};
pub(crate) use text_edit::code_text_edit;
/// 只读代码视图（TextEdit + 语法高亮 + 搜索高亮 + 行号）
///
/// `line_mapping` 为空时显示顺序行号（1-indexed），
/// 非空时显示原始源码行号，无映射行不显示行号。
pub fn code_view(
    ui: &mut egui::Ui,
    text: &str,
    spans: &[Span],
    line_mapping: &[Option<u32>],
    config: CodeViewConfig<'_>,
    cache: &mut Option<LayoutCache>,
    scroll_to_line: &mut Option<usize>,
) -> CodeViewOutput<Option<NavigationHit>> {
    let CodeViewConfig {
        id,
        matches,
        current,
        theme,
        known_classes,
        ..
    } = config;
    let line_count = text.split('\n').count().max(1);
    let max_number = if line_mapping.is_empty() {
        line_count
    } else {
        line_mapping
            .iter()
            .filter_map(|n| n.map(|v| v as usize))
            .max()
            .unwrap_or(line_count)
    };
    let gutter_w = line_number_width(max_number);
    let code_font = egui::FontId::monospace(theme.code_font_size);
    let match_ranges = LayoutCache::collect_match_ranges(matches);
    let match_ref = match_ranges.as_slice();
    let (prev_word_ranges, word_gen) = LayoutCache::word_state(cache);
    let word_ref = prev_word_ranges.as_slice();
    let mut nav_hit: Option<NavigationHit> = None;
    let (hl_time_id, hl_line_id) = line_highlight_ids(id);
    remember_line_highlight(ui, hl_time_id, hl_line_id, *scroll_to_line);
    let frame = {
        let mut layouter = LayoutCache::build_layouter(
            spans,
            LayoutHighlights {
                match_ranges: match_ref,
                current_match: current,
                word_ranges: word_ref,
                word_gen,
            },
            theme,
            cache,
        );
        let mut buf: &str = text;
        show_code_view_frame(
            ui,
            &mut buf,
            CodeFrameConfig {
                id,
                code_font: &code_font,
                theme,
                gutter_w,
                line_count,
                line_mapping: &[],
            },
            scroll_to_line,
            &mut layouter,
            |ui, output| {
                nav_hit =
                    navigation::detect_navigation(ui, output, text, spans, theme, known_classes);
            },
        )
    };
    let new_word = finish_code_view_frame(
        ui,
        text,
        &frame,
        &prev_word_ranges,
        CodeFrameConfig {
            id,
            line_count,
            line_mapping,
            gutter_w,
            code_font: &code_font,
            theme,
        },
    );
    LayoutCache::update_highlight_word(cache, text, new_word);
    CodeViewOutput {
        value: nav_hit,
        response: frame.response,
    }
}

/// 结构代码视图（TextEdit + 语法高亮 + 搜索高亮 + 行号）
///
/// `editable` 为 `false` 时仍可正常显示和选择文本，但不会写入修改。
fn code_view_structured(
    ui: &mut egui::Ui,
    text: &mut String,
    config: CodeViewConfig<'_>,
    cache: &mut Option<EditableLayoutCache>,
    scroll_to_line: &mut Option<usize>,
    editable: bool,
) -> CodeViewOutput<bool> {
    let CodeViewConfig {
        id,
        lang,
        matches,
        current,
        theme,
        viewport_override,
        ..
    } = config;
    // 视窗模式判断：优先用手动覆盖，否则自动检测
    let is_viewport = match viewport_override {
        Some(v) => v,
        None => match cache.as_ref() {
            Some(c) => c.is_viewport,
            None => text.len() > viewport::VIEWPORT_TEXT_LEN,
        },
    };
    // 过渡帧：从视窗切换到普通模式时，先渲染一帧 loading overlay 再实际切换，
    // 避免用户感受到 galley 全量布局的卡顿。
    if !is_viewport
        && let Some(c) = cache.as_ref()
        && c.is_viewport
    {
        viewport::code_view_editable_viewport(ui, text, config, cache, scroll_to_line, editable);
        // viewport 函数内部会把 is_viewport 写回 true，必须在之后强制覆盖
        if let Some(c) = cache.as_mut() {
            c.is_viewport = false;
        }
        paint_transition_overlay(ui, theme);
        ui.ctx().request_repaint();
        return CodeViewOutput {
            value: false,
            response: None,
        };
    }
    if is_viewport {
        let changed = viewport::code_view_editable_viewport(
            ui,
            text,
            config,
            cache,
            scroll_to_line,
            editable,
        );
        return CodeViewOutput {
            value: changed,
            response: None,
        };
    }
    let line_count = text.split('\n').count().max(1);
    let gutter_w = line_number_width(line_count);
    let code_font = egui::FontId::monospace(theme.code_font_size);
    let match_ranges = LayoutCache::collect_match_ranges(matches);
    let match_ref = match_ranges.as_slice();
    let (prev_word_ranges, word_gen) = EditableLayoutCache::word_state(cache);
    let word_ref = prev_word_ranges.as_slice();
    let mut changed = false;
    let (hl_time_id, hl_line_id) = line_highlight_ids(id);
    remember_line_highlight(ui, hl_time_id, hl_line_id, *scroll_to_line);
    let frame = {
        let mut layouter = EditableLayoutCache::build_layouter(
            lang,
            LayoutHighlights {
                match_ranges: match_ref,
                current_match: current,
                word_ranges: word_ref,
                word_gen,
            },
            theme,
            &code_font,
            cache,
        );
        if editable {
            show_code_view_frame(
                ui,
                text,
                CodeFrameConfig {
                    id,
                    code_font: &code_font,
                    theme,
                    gutter_w,
                    line_count,
                    line_mapping: &[],
                },
                scroll_to_line,
                &mut layouter,
                |_ui, output| {
                    changed = output.response.changed();
                },
            )
        } else {
            let mut buf = text.as_str();
            show_code_view_frame(
                ui,
                &mut buf,
                CodeFrameConfig {
                    id,
                    code_font: &code_font,
                    theme,
                    gutter_w,
                    line_count,
                    line_mapping: &[],
                },
                scroll_to_line,
                &mut layouter,
                |_ui, _output| {
                    changed = false;
                },
            )
        }
    };
    let line_count_now = text.split('\n').count().max(1);
    let new_word = finish_code_view_frame(
        ui,
        text,
        &frame,
        &prev_word_ranges,
        CodeFrameConfig {
            id,
            line_count: line_count_now,
            line_mapping: &[],
            gutter_w,
            code_font: &code_font,
            theme,
        },
    );
    EditableLayoutCache::update_highlight_word(cache, text, new_word);
    CodeViewOutput {
        value: changed,
        response: frame.response,
    }
}

/// 可编辑代码视图（TextEdit + 语法高亮 + 搜索高亮 + 行号）
///
/// 返回 `true` 表示文本已被修改，调用方负责刷新高亮数据和标记 tab 状态。
pub fn code_view_editable(
    ui: &mut egui::Ui,
    text: &mut String,
    config: CodeViewConfig<'_>,
    cache: &mut Option<EditableLayoutCache>,
    scroll_to_line: &mut Option<usize>,
) -> CodeViewOutput<bool> {
    code_view_structured(ui, text, config, cache, scroll_to_line, true)
}

/// 只读代码视图（TextEdit + 语法高亮 + 搜索高亮 + 行号）
///
/// 用于需要保留代码视图显示/选择能力、但禁止编辑写入的场景。
pub fn code_view_readonly_editable_style(
    ui: &mut egui::Ui,
    text: &mut String,
    config: CodeViewConfig<'_>,
    cache: &mut Option<EditableLayoutCache>,
    scroll_to_line: &mut Option<usize>,
) -> CodeViewOutput<bool> {
    code_view_structured(ui, text, config, cache, scroll_to_line, false)
}

/// 视窗→普通模式过渡帧 overlay
fn paint_transition_overlay(ui: &egui::Ui, theme: &CodeViewTheme) {
    let clip = ui.clip_rect();
    let painter = ui.painter();
    painter.rect_filled(clip, 0.0, egui::Color32::from_black_alpha(120));
    let font = egui::FontId::proportional(14.0);
    painter.text(
        clip.center(),
        egui::Align2::CENTER_CENTER,
        "Switching...",
        font,
        theme.line_number_color,
    );
}

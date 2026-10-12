//! 文本布局缓存与选区高亮辅助
//!
//! @author sky

use crate::highlight::{self, Language, Span};
use crate::theme::CodeViewTheme;
use eframe::egui;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

/// 大文本 debounce 阈值：超过此长度的文本在编辑时延迟重排版
const DEBOUNCE_TEXT_LEN: usize = 100_000;
/// debounce 等待时间（秒）
const DEBOUNCE_SECS: f64 = 0.3;

/// 搜索匹配与选中同名词的布局高亮状态。
#[derive(Clone, Copy)]
pub(crate) struct LayoutHighlights<'a> {
    pub(crate) match_ranges: &'a [(usize, usize)],
    pub(crate) current_match: Option<usize>,
    pub(crate) word_ranges: &'a [(usize, usize)],
    pub(crate) word_gen: u64,
}

/// 语法高亮重建的语言、主题与视窗状态。
pub(crate) struct GalleyConfig<'a> {
    pub(crate) lang: Language,
    pub(crate) highlights: LayoutHighlights<'a>,
    pub(crate) theme: &'a CodeViewTheme,
    pub(crate) is_viewport: bool,
    pub(crate) viewport_cursor_char: usize,
    pub(crate) viewport_window_start: usize,
}

/// 只读视图布局缓存
///
/// 文本和 span 在只读模式下不变，仅搜索状态或选中高亮变化触发重建。
pub struct LayoutCache {
    /// 搜索匹配数
    pub(crate) match_count: usize,
    /// 当前高亮的匹配索引
    pub(crate) current_match: Option<usize>,
    /// 上一帧选中的高亮词（用于缓存命中判断）
    pub(crate) highlight_word: Option<String>,
    /// 选中同名字段高亮范围（字节偏移）
    pub(crate) word_highlight_ranges: Vec<(usize, usize)>,
    /// 选中同名高亮版本号（word extraction 每次变更时递增）
    pub(crate) word_gen: u64,
    /// galley 构建时的高亮版本号（用于缓存命中判断）
    pub(crate) galley_word_gen: u64,
    /// 缓存的 galley
    pub(crate) galley: Arc<egui::Galley>,
}

impl LayoutCache {
    pub(crate) fn collect_match_ranges(
        matches: &[crate::search::FindMatch],
    ) -> Vec<(usize, usize)> {
        matches.iter().map(|m| (m.start, m.end)).collect()
    }

    pub(crate) fn word_state(cache: &Option<Self>) -> (Vec<(usize, usize)>, u64) {
        let prev_word_ranges = cache
            .as_ref()
            .map(|c| c.word_highlight_ranges.clone())
            .unwrap_or_default();
        let word_gen = cache.as_ref().map_or(0, |c| c.word_gen);
        (prev_word_ranges, word_gen)
    }

    pub(crate) fn update_highlight_word(
        cache: &mut Option<Self>,
        text: &str,
        new_word: Option<String>,
    ) {
        if let Some(c) = cache.as_mut() {
            update_highlight_word_cache(
                &mut c.highlight_word,
                &mut c.word_highlight_ranges,
                &mut c.word_gen,
                text,
                new_word,
            );
        }
    }

    pub(crate) fn build_layouter<'a>(
        spans: &'a [Span],
        highlights: LayoutHighlights<'a>,
        theme: &'a CodeViewTheme,
        cache: &'a mut Option<Self>,
    ) -> impl FnMut(&egui::Ui, &dyn egui::TextBuffer, f32) -> Arc<egui::Galley> + 'a {
        let LayoutHighlights {
            match_ranges: match_ref,
            current_match: current,
            word_ranges: word_ref,
            word_gen,
        } = highlights;
        move |ui, text_buf, _wrap_width| {
            let mc = match_ref.len();
            let cm = current;
            if let Some(c) = cache.as_ref()
                && c.match_count == mc
                && c.current_match == cm
                && c.galley_word_gen == word_gen
            {
                return c.galley.clone();
            }
            let s = text_buf.as_str();
            let job =
                highlight::build_layout_job_with_matches(s, spans, match_ref, cm, word_ref, theme);
            let galley = ui.fonts_mut(|f| f.layout_job(job));
            let old_hw = cache.as_ref().and_then(|c| c.highlight_word.clone());
            let old_wr = cache
                .as_ref()
                .map(|c| c.word_highlight_ranges.clone())
                .unwrap_or_default();
            let old_wg = cache.as_ref().map_or(0, |c| c.word_gen);
            *cache = Some(LayoutCache {
                match_count: mc,
                current_match: cm,
                highlight_word: old_hw,
                word_highlight_ranges: old_wr,
                word_gen: old_wg,
                galley_word_gen: word_gen,
                galley: galley.clone(),
            });
            galley
        }
    }
}

/// 可编辑视图布局缓存
///
/// 通过文本哈希检测内容变更，缓存 tree-sitter span 和 galley。
pub struct EditableLayoutCache {
    /// 文本内容哈希
    pub(crate) text_hash: u64,
    /// 缓存的语法高亮 span
    pub(crate) spans: Vec<Span>,
    /// 搜索匹配数
    pub(crate) match_count: usize,
    /// 当前高亮的匹配索引
    pub(crate) current_match: Option<usize>,
    /// 缓存的 galley
    pub(crate) galley: Arc<egui::Galley>,
    /// 大文本编辑 debounce：galley 过期但尚未重排版
    pub(crate) stale: bool,
    /// 文本最后一次变更时间
    pub(crate) last_change_time: f64,
    /// 是否启用视窗模式（大文本）
    pub(crate) is_viewport: bool,
    /// 视窗模式：光标在全文中的字符偏移
    pub(crate) viewport_cursor_char: usize,
    /// 视窗模式：当前窗口起始字符偏移
    pub(crate) viewport_window_start: usize,
    /// 全文字符数缓存（视窗模式用，避免每帧 O(n)）
    pub(crate) full_text_chars: usize,
    /// 全文行数缓存（视窗模式用，避免每帧 O(n)）
    pub(crate) full_text_lines: usize,
    /// 全文字节数缓存（用于检测文本变更后刷新 chars/lines）
    pub(crate) full_text_len: usize,
    /// 上一帧选中的高亮词
    pub(crate) highlight_word: Option<String>,
    /// 选中同名字段高亮范围（字节偏移）
    pub(crate) word_highlight_ranges: Vec<(usize, usize)>,
    /// 选中同名高亮版本号（word extraction 每次变更时递增）
    pub(crate) word_gen: u64,
    /// galley 构建时的高亮版本号（用于缓存命中判断）
    pub(crate) galley_word_gen: u64,
}

impl EditableLayoutCache {
    /// 当前是否处于视窗模式
    pub fn is_viewport(&self) -> bool {
        self.is_viewport
    }

    pub(crate) fn word_state(cache: &Option<Self>) -> (Vec<(usize, usize)>, u64) {
        let prev_word_ranges = cache
            .as_ref()
            .map(|c| c.word_highlight_ranges.clone())
            .unwrap_or_default();
        let word_gen = cache.as_ref().map_or(0, |c| c.word_gen);
        (prev_word_ranges, word_gen)
    }

    pub(crate) fn update_highlight_word(
        cache: &mut Option<Self>,
        text: &str,
        new_word: Option<String>,
    ) {
        if let Some(c) = cache.as_mut() {
            update_highlight_word_cache(
                &mut c.highlight_word,
                &mut c.word_highlight_ranges,
                &mut c.word_gen,
                text,
                new_word,
            );
        }
    }

    pub(crate) fn build_layouter<'a>(
        lang: Language,
        highlights: LayoutHighlights<'a>,
        theme: &'a CodeViewTheme,
        code_font: &'a egui::FontId,
        cache: &'a mut Option<Self>,
    ) -> impl FnMut(&egui::Ui, &dyn egui::TextBuffer, f32) -> Arc<egui::Galley> + 'a {
        move |ui, text_buf, _wrap_width| {
            let s = text_buf.as_str();
            let th = hash_text(s);
            // 大文本 debounce：编辑时用纯文本 galley（跟手），停止输入后再做完整高亮
            if s.len() > DEBOUNCE_TEXT_LEN
                && let Some(c) = cache.as_mut()
            {
                let now = ui.input(|i| i.time);
                if c.text_hash != th {
                    // 文本变更：立即用纯文本 LayoutJob 重排版（跳过 tree-sitter）
                    c.text_hash = th;
                    c.stale = true;
                    c.last_change_time = now;
                    let plain_color = theme.syntax.text;
                    let job = egui::text::LayoutJob::simple(
                        s.to_owned(),
                        code_font.clone(),
                        plain_color,
                        f32::INFINITY,
                    );
                    let galley = ui.fonts_mut(|f| f.layout_job(job));
                    c.galley = galley.clone();
                    ui.ctx().request_repaint_after_secs(DEBOUNCE_SECS as f32);
                    return galley;
                }
                if c.stale && now - c.last_change_time < DEBOUNCE_SECS {
                    // debounce 窗口内，复用上次的纯文本 galley
                    let remaining = DEBOUNCE_SECS - (now - c.last_change_time) + 0.016;
                    ui.ctx().request_repaint_after_secs(remaining as f32);
                    return c.galley.clone();
                }
                c.stale = false;
            }
            rebuild_galley(
                ui,
                s,
                th,
                GalleyConfig {
                    lang,
                    highlights,
                    theme,
                    is_viewport: false,
                    viewport_cursor_char: 0,
                    viewport_window_start: 0,
                },
                cache,
            )
            .unwrap()
        }
    }
}

/// 计算文本内容哈希（SipHash，660KB ~0.3ms）
pub(crate) fn hash_text(text: &str) -> u64 {
    let mut hasher = std::hash::DefaultHasher::new();
    text.hash(&mut hasher);
    hasher.finish()
}

/// 字符偏移转字节偏移
pub(crate) fn byte_offset_at_char(s: &str, char_offset: usize) -> usize {
    s.char_indices()
        .nth(char_offset)
        .map_or(s.len(), |(i, _)| i)
}

/// 从选区提取高亮词
///
/// `primary` 和 `secondary` 为选区两端的字符偏移（非字节偏移）。
/// 选中文本须满足：2~100 字节、不含空白。
pub(crate) fn extract_highlight_word(
    text: &str,
    primary: usize,
    secondary: usize,
) -> Option<String> {
    if primary == secondary {
        return None;
    }
    let (start_char, end_char) = if primary < secondary {
        (primary, secondary)
    } else {
        (secondary, primary)
    };
    let start_byte = byte_offset_at_char(text, start_char);
    let end_byte = byte_offset_at_char(text, end_char);
    if start_byte >= end_byte || end_byte > text.len() {
        return None;
    }
    let selected = &text[start_byte..end_byte];
    if selected.len() < 2 || selected.len() > 100 || selected.contains(char::is_whitespace) {
        return None;
    }
    Some(selected.to_string())
}

/// 查找文本中所有匹配的字节偏移范围
pub(crate) fn find_word_occurrences(text: &str, word: &str) -> Vec<(usize, usize)> {
    let mut ranges = Vec::new();
    let word_len = word.len();
    let mut start = 0;
    while let Some(pos) = text[start..].find(word) {
        let abs_pos = start + pos;
        ranges.push((abs_pos, abs_pos + word_len));
        start = abs_pos + word_len;
    }
    ranges
}

/// 根据当前选区文本更新同名高亮缓存。
pub(crate) fn update_highlight_word_cache(
    highlight_word: &mut Option<String>,
    word_highlight_ranges: &mut Vec<(usize, usize)>,
    word_gen: &mut u64,
    text: &str,
    new_word: Option<String>,
) {
    if new_word == *highlight_word {
        return;
    }
    *highlight_word = new_word;
    *word_highlight_ranges = highlight_word
        .as_deref()
        .map(|word| find_word_occurrences(text, word))
        .unwrap_or_default();
    *word_gen = word_gen.wrapping_add(1);
}

/// 根据字符偏移计算当前选区的字节范围。
pub(crate) fn selection_byte_range(text: &str, primary: usize, secondary: usize) -> (usize, usize) {
    if primary == secondary {
        return (0, 0);
    }
    let (start, end) = if primary < secondary {
        (primary, secondary)
    } else {
        (secondary, primary)
    };
    (
        byte_offset_at_char(text, start),
        byte_offset_at_char(text, end),
    )
}

/// 绘制当前选区对应的同名高亮 overlay。
pub(crate) fn paint_word_highlight_overlay(
    ui: &egui::Ui,
    galley: &egui::Galley,
    galley_pos: egui::Pos2,
    text: &str,
    word_ranges: &[(usize, usize)],
    selection: (usize, usize),
    color: egui::Color32,
) {
    paint_word_highlights(
        ui,
        galley,
        galley_pos,
        text,
        word_ranges,
        selection_byte_range(text, selection.0, selection.1),
        color,
    );
}

/// 选中同名字段高亮绘制（painter overlay，带垂直内缩避免行间重叠）
///
/// `selection_byte_range` 为当前选区的字节范围，该范围内的匹配不绘制。
pub(crate) fn paint_word_highlights(
    ui: &egui::Ui,
    galley: &egui::Galley,
    galley_pos: egui::Pos2,
    text: &str,
    word_ranges: &[(usize, usize)],
    selection_byte_range: (usize, usize),
    color: egui::Color32,
) {
    if word_ranges.is_empty() {
        return;
    }
    let clip = ui.clip_rect();
    let painter = ui.painter();
    for &(sb, eb) in word_ranges {
        // 跳过选区自身
        if sb == selection_byte_range.0 && eb == selection_byte_range.1 {
            continue;
        }
        let sc = text[..sb.min(text.len())].chars().count();
        let ec = text[..eb.min(text.len())].chars().count();
        let sr = galley.pos_from_cursor(egui::text::CCursor::new(sc));
        let er = galley.pos_from_cursor(egui::text::CCursor::new(ec));
        let rect = egui::Rect::from_min_max(
            egui::pos2(galley_pos.x + sr.min.x, galley_pos.y + sr.min.y + 1.0),
            egui::pos2(galley_pos.x + er.min.x, galley_pos.y + sr.max.y - 1.0),
        );
        if rect.intersects(clip) {
            painter.rect_filled(rect, 0.0, color);
        }
    }
}

/// 重建语法高亮 galley（提取公共逻辑，被普通模式和视窗模式共用）
///
/// 检查缓存命中后执行完整重建：tree-sitter 解析 → LayoutJob → 字体布局。
pub(crate) fn rebuild_galley(
    ui: &egui::Ui,
    text: &str,
    text_hash: u64,
    config: GalleyConfig<'_>,
    cache: &mut Option<EditableLayoutCache>,
) -> Option<Arc<egui::Galley>> {
    let GalleyConfig {
        lang,
        highlights,
        theme,
        is_viewport,
        viewport_cursor_char,
        viewport_window_start,
    } = config;
    let LayoutHighlights {
        match_ranges,
        current_match,
        word_ranges,
        word_gen,
    } = highlights;
    let mc = match_ranges.len();
    // 缓存命中
    if let Some(c) = cache.as_ref()
        && c.text_hash == text_hash
        && !c.stale
        && c.match_count == mc
        && c.current_match == current_match
        && c.galley_word_gen == word_gen
    {
        return Some(c.galley.clone());
    }
    // 保留旧缓存的 word 状态（word extraction 在 show() 之后更新，不能丢失）
    let old_highlight_word = cache.as_ref().and_then(|c| c.highlight_word.clone());
    let old_word_ranges = cache
        .as_ref()
        .map(|c| c.word_highlight_ranges.clone())
        .unwrap_or_default();
    let old_word_gen = cache.as_ref().map_or(0, |c| c.word_gen);
    // 全量重建
    let old = cache.take();
    let text_changed = old.as_ref().is_none_or(|c| c.text_hash != text_hash);
    let spans = if text_changed {
        highlight::compute_spans(text, lang)
    } else {
        old.unwrap().spans
    };
    let job = highlight::build_layout_job_with_matches(
        text,
        &spans,
        match_ranges,
        current_match,
        word_ranges,
        theme,
    );
    let galley = ui.fonts_mut(|f| f.layout_job(job));
    *cache = Some(EditableLayoutCache {
        text_hash,
        spans,
        match_count: mc,
        current_match,
        galley: galley.clone(),
        stale: false,
        last_change_time: 0.0,
        is_viewport,
        viewport_cursor_char,
        viewport_window_start,
        full_text_chars: 0,
        full_text_lines: 0,
        full_text_len: 0,
        highlight_word: old_highlight_word,
        word_highlight_ranges: old_word_ranges,
        word_gen: old_word_gen,
        galley_word_gen: word_gen,
    });
    Some(galley)
}

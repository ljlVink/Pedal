//! 搜索面板的小型 widget 和渲染辅助函数
//!
//! @author sky
//! @author ljlVink

use super::result::SearchResultGroup;
use crate::appearance::{codicon, theme};
use eframe::egui;
use egui_editor::search::FindMatch;
use rust_i18n::t;

/// 渲染分组标题行（类名 + 包路径 + 匹配数），返回是否被点击
pub fn render_group_header(ui: &mut egui::Ui, group: &SearchResultGroup) -> bool {
    let avail_w = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(
        egui::vec2(avail_w, theme::SEARCH_GROUP_HEADER_HEIGHT),
        egui::Sense::click(),
    );
    let painter = ui.painter();
    if resp.hovered() {
        painter.rect_filled(rect, 0.0, theme::palette().bg_hover);
    }
    let mid_y = rect.center().y;
    let chevron = if group.expanded {
        codicon::CHEVRON_DOWN
    } else {
        codicon::CHEVRON_RIGHT
    };
    painter.text(
        egui::pos2(rect.left() + 8.0, mid_y),
        egui::Align2::LEFT_CENTER,
        chevron,
        egui::FontId::new(12.0, codicon::family()),
        theme::palette().text_muted,
    );
    painter.text(
        egui::pos2(rect.left() + 24.0, mid_y),
        egui::Align2::LEFT_CENTER,
        codicon::SYMBOL_CLASS,
        egui::FontId::new(12.0, codicon::family()),
        theme::palette().verdigris,
    );
    painter.text(
        egui::pos2(rect.left() + 40.0, mid_y),
        egui::Align2::LEFT_CENTER,
        &group.class_name,
        egui::FontId::proportional(12.0),
        theme::palette().text_primary,
    );
    let info = format!(
        "{}  ({})",
        group.package,
        t!("search.matches", count = group.matches.len())
    );
    painter.text(
        egui::pos2(rect.right() - 8.0, mid_y),
        egui::Align2::RIGHT_CENTER,
        &info,
        egui::FontId::proportional(11.0),
        theme::palette().text_muted,
    );
    resp.clicked()
}

/// 结果列表行内匹配区间高亮（VERDIGRIS 前景）
pub fn highlight_preview(text: &str, ranges: &[(usize, usize)]) -> egui::text::LayoutJob {
    let fmt = |color| egui::TextFormat {
        font_id: egui::FontId::monospace(11.0),
        color,
        ..Default::default()
    };
    let mut job = egui::text::LayoutJob::default();
    let len = text.len();
    let mut pos = 0;
    for &(start, end) in ranges {
        let start = start.min(len);
        let end = end.min(len);
        if start > pos {
            job.append(&text[pos..start], 0.0, fmt(theme::palette().text_secondary));
        }
        if end > start {
            job.append(&text[start..end], 0.0, fmt(theme::palette().verdigris));
        }
        pos = end;
    }
    if pos < len {
        job.append(&text[pos..], 0.0, fmt(theme::palette().text_secondary));
    }
    job
}

/// 水平分隔线
pub fn separator(ui: &mut egui::Ui) {
    let avail = ui.available_rect_before_wrap();
    ui.painter().line_segment(
        [
            egui::pos2(avail.left(), avail.top()),
            egui::pos2(avail.right(), avail.top()),
        ],
        egui::Stroke::new(1.0_f32, theme::palette().border),
    );
    ui.allocate_space(egui::vec2(avail.width(), 1.0));
}

/// 将匹配行内的高亮区间转换为源码全文字节偏移的 FindMatch 列表
///
/// `highlights` 是 preview 文本（去前导空白）内的字节区间，
/// 需要加上行首偏移和前导空白长度还原为源码全文偏移。
pub fn compute_find_matches(
    source: &str,
    match_line: usize,
    highlights: &[(usize, usize)],
) -> Vec<FindMatch> {
    let line_start: usize = source
        .split('\n')
        .take(match_line)
        .map(|l| l.len() + 1)
        .sum();
    let src_line = source.split('\n').nth(match_line).unwrap_or("");
    let trim_offset = src_line.len() - src_line.trim_start().len();
    highlights
        .iter()
        .map(|&(s, e)| FindMatch {
            start: line_start + s + trim_offset,
            end: line_start + e + trim_offset,
        })
        .collect()
}

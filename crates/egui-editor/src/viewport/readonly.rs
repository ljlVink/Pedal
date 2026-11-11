use crate::code_view::{
    CodeViewConfig, EditableLayoutCache, GUTTER_PAD, GalleyConfig, LayoutHighlights, TEXT_PAD_LEFT,
    apply_scroll_delta, code_text_edit, detect_edge_scroll, extract_highlight_word, hash_text,
    line_number_width, paint_line_numbers, paint_word_highlight_overlay, rebuild_galley,
};
use eframe::egui;
use std::ops::Range;
use std::sync::Arc;

const OVERSCAN_LINES: usize = 8;

struct LineIndex {
    bytes: Vec<usize>,
    chars: Vec<usize>,
    total_chars: usize,
    text_len: usize,
    longest_line: usize,
}

impl LineIndex {
    fn new(text: &str) -> Self {
        let mut bytes = vec![0];
        let mut chars = vec![0];
        let mut total_chars = 0;
        for (byte, character) in text.char_indices() {
            total_chars += 1;
            if character == '\n' {
                bytes.push(byte + 1);
                chars.push(total_chars);
            }
        }
        let longest_line = bytes
            .iter()
            .copied()
            .zip(
                bytes
                    .iter()
                    .copied()
                    .skip(1)
                    .chain(std::iter::once(text.len())),
            )
            .map(|(start, end)| end - start)
            .max()
            .unwrap_or(0);
        Self {
            bytes,
            chars,
            total_chars,
            text_len: text.len(),
            longest_line,
        }
    }

    fn visible_lines(&self, top: f32, bottom: f32, row_height: f32) -> Range<usize> {
        let first = ((top / row_height).max(0.0) as usize).min(self.bytes.len() - 1);
        let last = ((bottom / row_height).max(0.0).ceil() as usize).max(first + 1);
        first.saturating_sub(OVERSCAN_LINES)
            ..last.saturating_add(OVERSCAN_LINES).min(self.bytes.len())
    }
}

#[derive(Clone)]
struct ReadonlyState {
    index: Arc<LineIndex>,
    selection: Option<egui::text::CCursorRange>,
    window_start: usize,
    max_width: f32,
}

pub(crate) fn code_view_readonly_viewport(
    ui: &mut egui::Ui,
    text: &mut String,
    config: CodeViewConfig<'_>,
    cache: &mut Option<EditableLayoutCache>,
    scroll_to_line: &mut Option<usize>,
) {
    let CodeViewConfig {
        id,
        lang,
        matches,
        current,
        theme,
        ..
    } = config;
    let state_id = id.with("readonly_viewport");
    let stored = ui
        .ctx()
        .data(|data| data.get_temp::<ReadonlyState>(state_id));
    let mut state = stored
        .filter(|state| state.index.text_len == text.len() && cache.is_some())
        .unwrap_or_else(|| ReadonlyState {
            index: Arc::new(LineIndex::new(text)),
            selection: None,
            window_start: 0,
            max_width: 0.0,
        });
    if state.index.longest_line > 16 * 1024 {
        ui.ctx().data_mut(|data| data.insert_temp(state_id, state));
        super::code_view_editable_viewport(ui, text, config, cache, scroll_to_line, false);
        return;
    }
    let font = egui::FontId::monospace(theme.code_font_size);
    let row_height = ui.fonts_mut(|fonts| fonts.row_height(&font));
    let origin = ui.cursor().min;
    let clip = ui.clip_rect();
    let line_count = state.index.bytes.len();
    let gutter_width = line_number_width(line_count);
    let requested = scroll_to_line.take().map(|line| line.min(line_count - 1));
    if let Some(line) = requested {
        let line_start = state.index.bytes[line];
        let line_end = state
            .index
            .bytes
            .get(line + 1)
            .copied()
            .unwrap_or(text.len());
        let target_x = current
            .and_then(|index| matches.get(index))
            .filter(|found| found.start >= line_start && found.start < line_end)
            .and_then(|found| text.get(line_start..found.start))
            .map(|prefix| {
                let prefix_width = ui.fonts_mut(|fonts| {
                    fonts
                        .layout_no_wrap(prefix.to_owned(), font.clone(), theme.syntax.text)
                        .size()
                        .x
                });
                gutter_width + GUTTER_PAD + TEXT_PAD_LEFT + prefix_width
            })
            .unwrap_or(0.0);
        state.max_width = state.max_width.max(target_x + clip.width() * 0.5);
        let target = egui::Rect::from_min_size(
            egui::pos2(origin.x + target_x, origin.y + line as f32 * row_height),
            egui::vec2(1.0, row_height),
        );
        ui.scroll_to_rect_animation(
            target,
            Some(egui::Align::Center),
            egui::style::ScrollAnimation::none(),
        );
    }
    let lines =
        state
            .index
            .visible_lines(clip.top() - origin.y, clip.bottom() - origin.y, row_height);
    let start_byte = state.index.bytes[lines.start];
    let end_byte = state
        .index
        .bytes
        .get(lines.end)
        .copied()
        .unwrap_or(text.len());
    let start_char = state.index.chars[lines.start];
    let end_char = state
        .index
        .chars
        .get(lines.end)
        .copied()
        .unwrap_or(state.index.total_chars);
    let window = &text[start_byte..end_byte];
    if ui.memory(|memory| memory.has_focus(id)) {
        let copy = ui.input_mut(|input| {
            let copy = input
                .events
                .iter()
                .any(|event| matches!(event, egui::Event::Copy));
            input
                .events
                .retain(|event| !matches!(event, egui::Event::Copy));
            copy
        });
        if copy && let Some(selection) = state.selection {
            let start = selection.primary.index.min(selection.secondary.index);
            let end = selection.primary.index.max(selection.secondary.index);
            let start = super::byte_offset_at_char(text, start);
            let end = super::byte_offset_at_char(text, end);
            ui.ctx().copy_text(text[start..end].to_owned());
        }
    }
    let local_matches: Vec<_> = matches
        .iter()
        .filter(|found| found.end > start_byte && found.start < end_byte)
        .map(|found| {
            (
                found.start.saturating_sub(start_byte),
                found.end.min(end_byte) - start_byte,
            )
        })
        .collect();
    let local_current = current.and_then(|current| {
        let found = matches.get(current)?;
        if found.end <= start_byte || found.start >= end_byte {
            return None;
        }
        local_matches.iter().position(|range| {
            *range
                == (
                    found.start.saturating_sub(start_byte),
                    found.end.min(end_byte) - start_byte,
                )
        })
    });
    let local_words: Vec<_> = cache
        .as_ref()
        .into_iter()
        .flat_map(|cache| &cache.word_highlight_ranges)
        .filter(|&&(start, end)| end > start_byte && start < end_byte)
        .map(|&(start, end)| {
            (
                start.saturating_sub(start_byte),
                end.min(end_byte) - start_byte,
            )
        })
        .collect();
    let word_gen = cache.as_ref().map_or(0, |cache| cache.word_gen);
    if state.window_start != start_char {
        if let Some(mut edit_state) = egui::TextEdit::load_state(ui.ctx(), id) {
            edit_state
                .cursor
                .set_char_range(state.selection.map(|mut range| {
                    range.primary.index = range
                        .primary
                        .index
                        .saturating_sub(start_char)
                        .min(end_char - start_char);
                    range.secondary.index = range
                        .secondary
                        .index
                        .saturating_sub(start_char)
                        .min(end_char - start_char);
                    range
                }));
            edit_state.store(ui.ctx(), id);
        }
        state.window_start = start_char;
    }
    let width = ui.available_width().max(clip.width()).max(state.max_width);
    let full_size = egui::vec2(width, line_count as f32 * row_height);
    ui.allocate_exact_size(full_size, egui::Sense::hover());
    let window_rect = egui::Rect::from_min_size(
        egui::pos2(origin.x, origin.y + lines.start as f32 * row_height),
        egui::vec2(width, lines.len() as f32 * row_height),
    );
    let mut content_ui = ui.new_child(
        egui::UiBuilder::new()
            .id_salt("readonly_lines")
            .max_rect(window_rect)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    let cursor_char = state.selection.map_or(0, |range| range.primary.index);
    let previous_selection = state.selection;
    let mut layouter = |ui: &egui::Ui, buffer: &dyn egui::TextBuffer, _wrap_width: f32| {
        rebuild_galley(
            ui,
            buffer.as_str(),
            hash_text(buffer.as_str()),
            GalleyConfig {
                lang,
                highlights: LayoutHighlights {
                    match_ranges: &local_matches,
                    current_match: local_current,
                    word_ranges: &local_words,
                    word_gen,
                },
                theme,
                is_viewport: true,
                viewport_cursor_char: cursor_char,
                viewport_window_start: start_char,
            },
            cache,
        )
        .unwrap()
    };
    let mut edge_delta = egui::Vec2::ZERO;
    let mut wheel_delta = egui::Vec2::ZERO;
    content_ui.horizontal_top(|ui| {
        ui.add_space(gutter_width + GUTTER_PAD + TEXT_PAD_LEFT);
        let mut buffer = window;
        let output = code_text_edit(&mut buffer, id, font.clone(), &mut layouter).show(ui);
        state.max_width = state
            .max_width
            .max(output.galley.size().x + gutter_width + GUTTER_PAD + TEXT_PAD_LEFT);
        if output.response.has_focus()
            && (output.response.clicked()
                || output.response.dragged()
                || ui.input(|input| {
                    input
                        .events
                        .iter()
                        .any(|event| matches!(event, egui::Event::Key { pressed: true, .. }))
                }))
            && let Some(mut range) = output.cursor_range
        {
            range.primary.index += start_char;
            range.secondary.index += start_char;
            if (ui.input(|input| input.modifiers.shift) || output.response.dragged())
                && let Some(previous) = state.selection
            {
                range.secondary = previous.secondary;
            }
            if ui.input(|input| input.modifiers.command && input.key_pressed(egui::Key::A)) {
                range = egui::text::CCursorRange::two(
                    egui::text::CCursor::new(0),
                    egui::text::CCursor::new(state.index.total_chars),
                );
            }
            state.selection = Some(range);
        }
        if output.response.dragged() {
            (edge_delta, wheel_delta) = detect_edge_scroll(&output.response, ui);
        }
        let selection = state.selection.map_or((0, 0), |range| {
            (
                range.primary.index.saturating_sub(start_char),
                range.secondary.index.saturating_sub(start_char),
            )
        });
        paint_word_highlight_overlay(
            ui,
            &output.galley,
            output.galley_pos,
            window,
            &local_words,
            selection,
            theme.word_highlight_bg,
        );
    });
    if state.selection != previous_selection {
        let word = state.selection.and_then(|range| {
            extract_highlight_word(text, range.primary.index, range.secondary.index)
        });
        EditableLayoutCache::update_highlight_word(cache, text, word);
    }
    apply_scroll_delta(ui, edge_delta, wheel_delta);
    ui.expand_to_include_rect(egui::Rect::from_min_size(
        origin,
        egui::vec2(state.max_width, full_size.y),
    ));
    paint_line_numbers(ui, origin.y, line_count, &[], gutter_width, &font, theme);
    ui.ctx().data_mut(|data| data.insert_temp(state_id, state));
}

#[cfg(test)]
mod tests;

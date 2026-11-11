use super::tab::EditorTab;
use super::view_toggle::ActiveView;
use crate::appearance::{codicon, theme};
use crate::task::{CancellableTask, Poll, Pollable};
use eframe::egui;
use egui_editor::FindMatch;
use egui_editor::search::{SearchOptions, SearchPattern};
use egui_shell::components::FlatButton;
use rust_i18n::t;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

const MATCH_LIMIT: usize = 20_000;
const SEARCH_DELAY: Duration = Duration::from_millis(150);

struct FindResults {
    matches: Vec<FindMatch>,
    lines: Vec<usize>,
    limited: bool,
}

tabookit::class! {
    pub(super) struct FindState {
        pub open: bool,
        pub query: String,
        pub options: SearchOptions,
        pub matches: Vec<FindMatch>,
        pub current: Option<usize>,
        pub scroll_to_line: Option<usize>,
        lines: Vec<usize>,
        limited: bool,
        focus_requested: bool,
        dirty: bool,
        changed_at: Instant,
        searched_view: Option<ActiveView>,
        target: Option<usize>,
        task: Option<CancellableTask<Result<FindResults, String>>>,
        error: Option<String>,
    }

    pub fn new() -> Self {
        Self {
            open: false,
            query: String::new(),
            options: SearchOptions::default(),
            matches: Vec::new(),
            current: None,
            scroll_to_line: None,
            lines: Vec::new(),
            limited: false,
            focus_requested: false,
            dirty: false,
            changed_at: Instant::now(),
            searched_view: None,
            target: None,
            task: None,
            error: None,
        }
    }

    pub fn invalidate(&mut self) {
        self.task = None;
        self.dirty = true;
        self.changed_at = Instant::now();
        self.matches.clear();
        self.lines.clear();
        self.current = None;
        self.scroll_to_line = None;
        self.limited = false;
        self.error = None;
    }

    fn close(&mut self) {
        self.open = false;
        self.task = None;
        self.matches.clear();
        self.lines.clear();
        self.current = None;
        self.scroll_to_line = None;
        self.target = None;
        self.dirty = false;
    }

    fn navigate(&mut self, previous: bool) {
        if self.matches.is_empty() {
            return;
        }
        let count = self.matches.len();
        let current = self.current.unwrap_or(if previous { 0 } else { count - 1 });
        let next = if previous {
            (current + count - 1) % count
        } else {
            (current + 1) % count
        };
        self.current = Some(next);
        self.scroll_to_line = self.lines.get(next).copied();
    }
}

impl EditorTab {
    pub(crate) fn can_find(&self) -> bool {
        (self.is_ark
            || self
                .entry_path
                .as_deref()
                .is_some_and(crate::app::search::is_code_path))
            && !self.is_image()
            && !self.is_resource()
            && matches!(
                self.active_view,
                ActiveView::Decompiled | ActiveView::Disasm
            )
    }

    pub(crate) fn open_find(&mut self) {
        if !self.can_find() {
            return;
        }
        if !self.find.open {
            self.find.invalidate();
        }
        self.find.open = true;
        self.find.focus_requested = true;
    }

    pub(crate) fn reveal_search_match(
        &mut self,
        query: String,
        options: SearchOptions,
        start: usize,
    ) {
        self.active_view = ActiveView::Decompiled;
        self.find.query = query;
        self.find.options = options;
        self.find.open = true;
        self.find.invalidate();
        self.find.target = Some(start);
        self.find.changed_at = Instant::now() - SEARCH_DELAY;
    }

    pub(super) fn render_find_bar(&mut self, ui: &mut egui::Ui) {
        if !self.can_find() || !self.find.open {
            return;
        }
        if self.find.searched_view != Some(self.active_view) {
            self.find.invalidate();
            self.find.searched_view = Some(self.active_view);
        }
        self.poll_find(ui.ctx());
        let id = egui::Id::new(&self.entry_path).with("find_query");
        let mut changed = false;
        let mut previous = false;
        let mut next = false;
        let mut close = false;
        let editor_focused = ui.memory(|memory| {
            ["decompiled", "editable", "disasm"]
                .into_iter()
                .any(|view| memory.has_focus(egui::Id::new(&self.entry_path).with(view)))
        });
        if editor_focused || ui.memory(|memory| memory.has_focus(id)) {
            ui.input_mut(|input| {
                previous |= input.consume_key(egui::Modifiers::SHIFT, egui::Key::F3);
                next |= input.consume_key(egui::Modifiers::NONE, egui::Key::F3);
                close |= input.consume_key(egui::Modifiers::NONE, egui::Key::Escape);
            });
        }
        let p = theme::palette();
        let bar = egui::Frame::new()
            .fill(p.bg_gutter)
            .inner_margin(egui::Margin::symmetric(8, 4))
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing = egui::vec2(6.0, 5.0);
                ui.spacing_mut().button_padding = egui::vec2(4.0, 3.0);
                ui.set_min_width(ui.available_width());
                let narrow = ui.available_width() < 570.0;
                let response = ui
                    .horizontal(|ui| {
                        ui.set_min_height(28.0);
                        let input_width = if narrow {
                            (ui.available_width() - 34.0).max(80.0)
                        } else {
                            let controls_width =
                                76.0 + 6.0 * 28.0 + 1.0 + 8.0 * ui.spacing().item_spacing.x;
                            (ui.available_width() - controls_width).max(180.0)
                        };
                        let response = query_input(ui, &mut self.find, id, input_width);
                        if !narrow {
                            let actions = find_tools(ui, &mut self.find);
                            changed |= actions.0;
                            previous |= actions.1;
                            next |= actions.2;
                        }
                        close |= close_button(ui);
                        response
                    })
                    .inner;
                if narrow {
                    let actions = ui
                        .horizontal_wrapped(|ui| find_tools(ui, &mut self.find))
                        .inner;
                    changed |= actions.0;
                    previous |= actions.1;
                    next |= actions.2;
                }
                if let Some(error) = &self.find.error {
                    ui.label(
                        egui::RichText::new(t!("search.invalid_pattern", error = error))
                            .size(11.0)
                            .color(p.accent_red),
                    );
                } else if self.find.limited {
                    ui.label(
                        egui::RichText::new(t!("search.match_limit", limit = MATCH_LIMIT))
                            .size(11.0)
                            .color(p.text_muted),
                    );
                }
                response
            });
        ui.painter().hline(
            bar.response.rect.x_range(),
            bar.response.rect.bottom(),
            egui::Stroke::new(1.0_f32, p.border),
        );
        let response = bar.inner;
        changed |= response.changed();
        if response.has_focus() || response.lost_focus() {
            ui.input_mut(|input| {
                previous |= input.consume_key(egui::Modifiers::SHIFT, egui::Key::Enter);
                next |= input.consume_key(egui::Modifiers::NONE, egui::Key::Enter);
                close |= input.consume_key(egui::Modifiers::NONE, egui::Key::Escape);
            });
            if previous || next {
                response.request_focus();
            }
        }
        if close {
            self.find.close();
            return;
        }
        if changed {
            self.find.target = None;
            self.find.invalidate();
        }
        if previous || next {
            self.find.navigate(previous);
        }
        self.poll_find(ui.ctx());
    }

    fn poll_find(&mut self, ctx: &egui::Context) {
        if self.find.dirty {
            if self.find.query.is_empty() {
                self.find.dirty = false;
                return;
            }
            let elapsed = self.find.changed_at.elapsed();
            if elapsed < SEARCH_DELAY {
                ctx.request_repaint_after(SEARCH_DELAY - elapsed);
                return;
            }
            let text = match self.active_view {
                ActiveView::Disasm => self.disasm_text.clone(),
                _ => self.decompiled.clone(),
            };
            let query = self.find.query.clone();
            let options = self.find.options;
            let ctx = ctx.clone();
            self.find.task = Some(CancellableTask::spawn(move |cancel| {
                let result = collect_matches(&text, &query, options, cancel);
                ctx.request_repaint();
                result
            }));
            self.find.dirty = false;
        }
        let Some(task) = &self.find.task else {
            return;
        };
        match task.poll() {
            Poll::Pending => ctx.request_repaint_after(Duration::from_millis(50)),
            Poll::Ready(result) => {
                self.find.task = None;
                match result {
                    Ok(result) => {
                        self.find.matches = result.matches;
                        self.find.lines = result.lines;
                        self.find.limited = result.limited;
                        self.find.current = if self.find.matches.is_empty() {
                            None
                        } else {
                            Some(self.find.target.take().map_or(0, |target| {
                                self.find
                                    .matches
                                    .partition_point(|found| found.start < target)
                                    .min(self.find.matches.len() - 1)
                            }))
                        };
                        self.find.scroll_to_line = self
                            .find
                            .current
                            .and_then(|index| self.find.lines.get(index).copied());
                    }
                    Err(error) => self.find.error = Some(error),
                }
            }
            Poll::Lost => {
                self.find.task = None;
                self.find.error = Some(t!("search.failed").to_string());
            }
        }
    }
}

fn query_input(
    ui: &mut egui::Ui,
    find: &mut FindState,
    id: egui::Id,
    width: f32,
) -> egui::Response {
    let p = theme::palette();
    let focused = find.focus_requested || ui.memory(|memory| memory.has_focus(id));
    let response = egui::Frame::new()
        .fill(p.bg_darkest)
        .stroke(egui::Stroke::new(
            1.0_f32,
            if focused { p.verdigris } else { p.border_light },
        ))
        .corner_radius(5)
        .inner_margin(egui::Margin::symmetric(8, 4))
        .show(ui, |ui| {
            ui.set_width((width - 18.0).max(1.0));
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                ui.label(
                    egui::RichText::new(codicon::SEARCH)
                        .font(egui::FontId::new(14.0, codicon::family()))
                        .color(p.text_muted),
                );
                ui.add(
                    egui::TextEdit::singleline(&mut find.query)
                        .id(id)
                        .frame(egui::Frame::NONE)
                        .font(egui::FontId::proportional(13.0))
                        .margin(egui::Margin::ZERO)
                        .desired_width(ui.available_width())
                        .hint_text(t!("search.placeholder")),
                )
            })
            .inner
        })
        .inner;
    if find.focus_requested {
        response.request_focus();
        if let Some(mut state) = egui::TextEdit::load_state(ui.ctx(), id) {
            state
                .cursor
                .set_char_range(Some(egui::text::CCursorRange::two(
                    egui::text::CCursor::new(0),
                    egui::text::CCursor::new(find.query.chars().count()),
                )));
            state.store(ui.ctx(), id);
        }
        find.focus_requested = false;
    }
    response
}

fn find_tools(ui: &mut egui::Ui, find: &mut FindState) -> (bool, bool, bool) {
    let p = theme::palette();
    ui.allocate_ui_with_layout(
        egui::vec2(76.0, 28.0),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.set_min_width(76.0);
            if find.task.is_some() || find.dirty {
                ui.add(egui::Spinner::new().size(14.0))
                    .on_hover_text(t!("search.searching"));
            } else {
                ui.label(
                    egui::RichText::new(t!(
                        "search.position",
                        current = find.current.map_or(0, |index| index + 1),
                        total = find.matches.len()
                    ))
                    .size(11.0)
                    .color(p.text_muted),
                );
            }
        },
    );
    let mut changed = false;
    changed |= option_button(
        ui,
        &mut find.options.case_sensitive,
        &t!("search.case_short"),
        &t!("search.case_sensitive"),
    );
    changed |= option_button(
        ui,
        &mut find.options.whole_word,
        &t!("search.word_short"),
        &t!("search.whole_word"),
    );
    changed |= option_button(
        ui,
        &mut find.options.regex,
        &t!("search.regex_short"),
        &t!("search.regex"),
    );
    let (separator, _) = ui.allocate_exact_size(egui::vec2(1.0, 16.0), egui::Sense::hover());
    ui.painter().vline(
        separator.center().x,
        separator.y_range(),
        egui::Stroke::new(1.0_f32, p.border),
    );
    let button_theme = theme::flat_button_theme(p.text_secondary);
    let previous = ui
        .add_enabled(
            !find.matches.is_empty(),
            FlatButton::new(&t!("search.previous_short"), &button_theme)
                .font_size(16.0)
                .min_size(egui::vec2(28.0, 28.0)),
        )
        .on_hover_text(t!("search.previous"))
        .clicked();
    let next = ui
        .add_enabled(
            !find.matches.is_empty(),
            FlatButton::new(&t!("search.next_short"), &button_theme)
                .font_size(16.0)
                .min_size(egui::vec2(28.0, 28.0)),
        )
        .on_hover_text(t!("search.next"))
        .clicked();
    (changed, previous, next)
}

fn option_button(ui: &mut egui::Ui, selected: &mut bool, label: &str, tooltip: &str) -> bool {
    let mut button_theme = theme::flat_button_theme(theme::palette().text_secondary);
    if *selected {
        button_theme.bg_hover = theme::verdigris_alpha(35);
    }
    let clicked = ui
        .add(
            FlatButton::new(label, &button_theme)
                .active(*selected)
                .font_size(12.0)
                .min_size(egui::vec2(28.0, 28.0)),
        )
        .on_hover_text(tooltip)
        .clicked();
    if clicked {
        *selected = !*selected;
    }
    clicked
}

fn close_button(ui: &mut egui::Ui) -> bool {
    let button_theme = theme::flat_button_theme(theme::palette().text_secondary);
    ui.add(
        FlatButton::new("\u{EAB8}", &button_theme)
            .font_family(codicon::family())
            .font_size(12.0)
            .min_size(egui::vec2(28.0, 28.0)),
    )
    .on_hover_text(t!("search.close"))
    .clicked()
}

fn collect_matches(
    text: &str,
    query: &str,
    options: SearchOptions,
    cancel: &std::sync::atomic::AtomicBool,
) -> Result<FindResults, String> {
    let pattern = SearchPattern::new(query, options)?;
    let mut matches = Vec::new();
    let mut lines = Vec::new();
    let mut line = 0;
    let mut offset = 0;
    let mut limited = false;
    for found in pattern.find_iter(text) {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        if matches.len() == MATCH_LIMIT {
            limited = true;
            break;
        }
        line += text.as_bytes()[offset..found.start]
            .iter()
            .filter(|&&byte| byte == b'\n')
            .count();
        offset = found.start;
        matches.push(found);
        lines.push(line);
    }
    Ok(FindResults {
        matches,
        lines,
        limited,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui_editor::Language;
    use std::sync::atomic::AtomicBool;

    fn tab(path: &str, text: &str) -> EditorTab {
        EditorTab::new_text(
            path,
            path,
            text.into(),
            text.as_bytes().to_vec(),
            Language::Plain,
        )
    }

    fn finish(tab: &mut EditorTab) {
        let ctx = egui::Context::default();
        tab.find.changed_at = Instant::now() - SEARCH_DELAY;
        let deadline = Instant::now() + Duration::from_secs(5);
        while tab.find.task.is_some() || tab.find.dirty {
            assert!(Instant::now() < deadline, "search timed out");
            tab.poll_find(&ctx);
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    #[test]
    fn matches_keep_unicode_byte_offsets_and_source_lines() {
        let result = collect_matches(
            "中foo\r\nfoo\n文foo",
            "foo",
            SearchOptions::default(),
            &AtomicBool::new(false),
        )
        .unwrap();
        assert_eq!(
            result
                .matches
                .iter()
                .map(|found| found.start)
                .collect::<Vec<_>>(),
            [3, 8, 15]
        );
        assert_eq!(result.lines, [0, 1, 2]);
    }

    #[test]
    fn find_navigation_wraps_and_reveals_the_requested_match() {
        let mut tab = tab("source.ets", "foo\n中foo\nfoo");
        tab.reveal_search_match("foo".into(), SearchOptions::default(), 7);
        finish(&mut tab);
        assert_eq!(tab.find.current, Some(1));
        assert_eq!(tab.find.scroll_to_line, Some(1));
        tab.find.navigate(true);
        tab.find.navigate(true);
        assert_eq!(tab.find.current, Some(2));
        assert_eq!(tab.find.scroll_to_line, Some(2));
        tab.find.navigate(false);
        assert_eq!(tab.find.current, Some(0));
    }

    #[test]
    fn stale_query_tasks_and_edited_code_cannot_restore_old_matches() {
        let mut tab = tab("source.ts", "old\nnew");
        tab.reveal_search_match("old".into(), SearchOptions::default(), 0);
        tab.poll_find(&egui::Context::default());
        tab.find.query = "new".into();
        tab.find.invalidate();
        finish(&mut tab);
        assert_eq!(tab.find.matches, [FindMatch { start: 4, end: 7 }]);
        tab.decompiled = "new\nold".into();
        tab.refresh_decompiled_data();
        finish(&mut tab);
        assert_eq!(tab.find.matches, [FindMatch { start: 0, end: 3 }]);
    }

    #[test]
    fn source_search_includes_readonly_code_and_excludes_resources() {
        let mut source = tab("source.ets", "const code = 1;");
        source.is_text = false;
        assert!(source.can_find());
        source.active_view = ActiveView::Hex;
        assert!(!source.can_find());
        assert!(!tab("module.json", "{}").can_find());
        assert!(!tab("data.txt", "code").can_find());
    }

    #[test]
    fn matching_is_bounded_and_cancellable() {
        let text = "x ".repeat(MATCH_LIMIT + 1);
        let result = collect_matches(
            &text,
            "x",
            SearchOptions::default(),
            &AtomicBool::new(false),
        )
        .unwrap();
        assert_eq!(result.matches.len(), MATCH_LIMIT);
        assert!(result.limited);
        let cancelled =
            collect_matches(&text, "x", SearchOptions::default(), &AtomicBool::new(true)).unwrap();
        assert!(cancelled.matches.is_empty());
    }

    #[test]
    fn search_keys_affect_only_the_focused_tab() {
        let ctx = egui::Context::default();
        let mut fonts = egui::FontDefinitions::default();
        fonts.families.insert(
            codicon::family(),
            fonts.families[&egui::FontFamily::Proportional].clone(),
        );
        ctx.set_fonts(fonts);
        let mut first = tab("first.ts", "foo foo");
        let mut second = tab("second.ts", "foo foo");
        for tab in [&mut first, &mut second] {
            tab.reveal_search_match("foo".into(), SearchOptions::default(), 0);
            tab.find.searched_view = Some(ActiveView::Decompiled);
            finish(tab);
        }
        first.open_find();
        let frame = |first: &mut EditorTab, second: &mut EditorTab, key: Option<egui::Key>| {
            let events = key
                .into_iter()
                .map(|key| egui::Event::Key {
                    key,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                })
                .collect();
            let _ = ctx.run_ui(
                egui::RawInput {
                    events,
                    ..Default::default()
                },
                |ui| {
                    second.render_find_bar(ui);
                    first.render_find_bar(ui);
                },
            );
        };
        frame(&mut first, &mut second, None::<egui::Key>);
        frame(&mut first, &mut second, Some(egui::Key::Enter));
        assert_eq!(first.find.current, Some(1));
        assert_eq!(second.find.current, Some(0));
        frame(&mut first, &mut second, Some(egui::Key::Escape));
        assert!(!first.find.open);
        assert!(second.find.open);
    }
}

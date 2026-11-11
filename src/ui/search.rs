use crate::app::App;
use crate::app::search::{SearchResult, WorkspaceSearch};
use crate::appearance::{codicon, theme};
use eframe::egui;
use egui_shell::components::FlatButton;
use rust_i18n::t;
use std::time::Duration;

enum ResultRow {
    File { first: usize, count: usize },
    Match(usize),
}

fn workspace_search_viewport_id() -> egui::ViewportId {
    egui::ViewportId::from_hash_of("workspace_search")
}

impl App {
    pub(crate) fn render_workspace_search(&mut self, ctx: &egui::Context) {
        if !self.workspace_search.open {
            ctx.data_mut(|data| data.remove::<f64>(egui::Id::new("workspace_search_debounce")));
            return;
        }
        ctx.show_viewport_immediate(
            workspace_search_viewport_id(),
            egui::ViewportBuilder::default()
                .with_title(t!("search.workspace").to_string())
                .with_inner_size([920.0, 540.0])
                .with_min_inner_size([560.0, 360.0])
                .with_decorations(true)
                .with_resizable(true)
                .with_active(true),
            |ui, class| self.render_workspace_search_content(ui, class),
        );
    }

    fn render_workspace_search_content(&mut self, ui: &mut egui::Ui, class: egui::ViewportClass) {
        let ctx = ui.ctx().clone();
        let debounce_id = egui::Id::new("workspace_search_debounce");
        let native = class != egui::ViewportClass::EmbeddedWindow;
        if (native && ctx.input(|input| input.viewport().close_requested()))
            || ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
        {
            self.workspace_search.open = false;
            self.workspace_search.cancel();
            ctx.data_mut(|data| data.remove::<f64>(debounce_id));
            ctx.request_repaint_of(egui::ViewportId::ROOT);
            return;
        }
        if native {
            let native_theme = match self.settings.theme {
                theme::ThemeMode::Light => egui::viewport::SystemTheme::Light,
                theme::ThemeMode::Dark => egui::viewport::SystemTheme::Dark,
            };
            let theme_id = workspace_search_viewport_id().0.with("theme");
            if self.workspace_search.focus_requested
                || ctx.data(|data| data.get_temp::<egui::viewport::SystemTheme>(theme_id))
                    != Some(native_theme)
            {
                ctx.send_viewport_cmd(egui::ViewportCommand::SetTheme(native_theme));
                ctx.data_mut(|data| data.insert_temp(theme_id, native_theme));
            }
            if self.workspace_search.focus_requested {
                ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
            }
        }
        let mut changed = false;
        let mut start = false;
        let mut jump = None;
        let mut close = false;
        let state = &mut self.workspace_search;
        let p = theme::palette();
        egui::CentralPanel::default()
            .frame(
                egui::Frame::NONE
                    .fill(p.bg_gutter)
                    .inner_margin(egui::Margin::same(16)),
            )
            .show_inside(ui, |ui| {
                ui.spacing_mut().item_spacing = egui::vec2(8.0, 10.0);
                ui.style_mut().interaction.selectable_labels = false;
                search_input(ui, state, &mut changed, &mut start);
                if changed {
                    state.clear_results();
                }
                let rows = result_rows(&state.results);
                let files = rows.len() - state.results.len();
                status(ui, state, files);
                let results_height = (ui.available_height() - 52.0).max(170.0);
                egui::Frame::NONE
                    .fill(p.bg_darkest)
                    .stroke(egui::Stroke::new(1.0_f32, p.border))
                    .corner_radius(6)
                    .inner_margin(4)
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 0.0;
                        if rows.is_empty() {
                            empty_results(ui, state, results_height);
                        } else {
                            egui::ScrollArea::vertical()
                                .id_salt("workspace_search_results")
                                .auto_shrink([false, false])
                                .max_height(results_height)
                                .min_scrolled_height(results_height)
                                .show_rows(ui, 30.0, rows.len(), |ui, visible| {
                                    ui.spacing_mut().item_spacing.y = 0.0;
                                    for row in visible {
                                        match rows[row] {
                                            ResultRow::File { first, count } => {
                                                file_row(ui, &state.results[first].path, count)
                                            }
                                            ResultRow::Match(index) => {
                                                if result_row(ui, state, index) {
                                                    jump = Some(index);
                                                }
                                            }
                                        }
                                    }
                                });
                        }
                    });
                ui.horizontal(|ui| {
                    changed |= ui
                        .checkbox(&mut state.auto_search, t!("search.auto"))
                        .changed();
                    ui.checkbox(&mut state.keep_open, t!("search.keep_open"));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .add_enabled(
                                state.selected.is_some(),
                                action_button(&t!("search.go_to")),
                            )
                            .clicked()
                        {
                            jump = state.selected;
                        }
                        close = ui
                            .add(
                                FlatButton::new(
                                    &t!("search.close"),
                                    &theme::flat_button_theme(p.text_secondary),
                                )
                                .min_size(egui::vec2(64.0, 30.0)),
                            )
                            .clicked();
                    });
                });
            });
        if close {
            state.open = false;
            state.cancel();
            ctx.data_mut(|data| data.remove::<f64>(debounce_id));
            ctx.request_repaint_of(egui::ViewportId::ROOT);
            return;
        }
        let now = ctx.input(|input| input.time);
        if changed {
            state.clear_results();
            ctx.data_mut(|data| {
                data.remove::<f64>(debounce_id);
                if state.auto_search && !state.query.is_empty() {
                    data.insert_temp(debounce_id, now);
                }
            });
        }
        if let Some(changed_at) = ctx.data(|data| data.get_temp::<f64>(debounce_id)) {
            if now - changed_at >= 0.25 {
                start = true;
            } else {
                ctx.request_repaint_after(Duration::from_millis(40));
            }
        }
        if start {
            ctx.data_mut(|data| data.remove::<f64>(debounce_id));
            self.start_workspace_search();
            ctx.request_repaint_after(Duration::from_millis(40));
        } else if let Some(index) = jump {
            self.jump_to_search_result(index);
            ctx.request_repaint_of(egui::ViewportId::ROOT);
            if native && !self.workspace_search.open {
                ctx.send_viewport_cmd_to(egui::ViewportId::ROOT, egui::ViewportCommand::Focus);
            }
        }
    }
}

fn icon(ui: &mut egui::Ui, text: &str, color: egui::Color32, size: f32) {
    ui.label(
        egui::RichText::new(text)
            .font(egui::FontId::new(size, codicon::family()))
            .color(color),
    );
}

fn action_button(text: &str) -> egui::Button<'_> {
    egui::Button::new(
        egui::RichText::new(text)
            .size(12.0)
            .color(theme::palette().verdigris),
    )
    .fill(theme::verdigris_alpha(26))
    .stroke(egui::Stroke::new(1.0_f32, theme::verdigris_alpha(70)))
    .corner_radius(5)
    .min_size(egui::vec2(76.0, 32.0))
}

fn search_input(
    ui: &mut egui::Ui,
    state: &mut WorkspaceSearch,
    changed: &mut bool,
    start: &mut bool,
) {
    let p = theme::palette();
    let id = egui::Id::new("workspace_search_query");
    let focused = state.focus_requested || ui.memory(|memory| memory.has_focus(id));
    let width = ui.available_width() - 88.0;
    ui.horizontal(|ui| {
        egui::Frame::NONE
            .fill(p.bg_medium)
            .stroke(egui::Stroke::new(
                1.0_f32,
                if focused { p.verdigris } else { p.border_light },
            ))
            .corner_radius(6)
            .inner_margin(egui::Margin::symmetric(10, 4))
            .show(ui, |ui| {
                ui.set_width(width - 20.0);
                ui.spacing_mut().item_spacing.x = 7.0;
                ui.allocate_ui_with_layout(
                    egui::vec2(ui.available_width(), 28.0),
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| {
                        icon(ui, codicon::SEARCH, p.text_muted, 16.0);
                        let buttons_width = 3.0 * (28.0 + ui.spacing().item_spacing.x);
                        let input = ui.add(
                            egui::TextEdit::singleline(&mut state.query)
                                .id(id)
                                .frame(egui::Frame::NONE)
                                .font(egui::FontId::proportional(14.0))
                                .margin(egui::Margin::ZERO)
                                .desired_width((ui.available_width() - buttons_width).max(1.0))
                                .hint_text(egui::RichText::new(t!("search.code_hint")).size(14.0)),
                        );
                        if state.focus_requested {
                            input.request_focus();
                            state.focus_requested = false;
                        }
                        *changed |= input.changed();
                        *start |=
                            input.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                        let fbt = theme::flat_button_theme(p.text_muted);
                        for (value, label, hint) in [
                            (
                                &mut state.options.case_sensitive,
                                t!("search.case_short"),
                                t!("search.case_sensitive"),
                            ),
                            (
                                &mut state.options.whole_word,
                                t!("search.word_short"),
                                t!("search.whole_word"),
                            ),
                            (
                                &mut state.options.regex,
                                t!("search.regex_short"),
                                t!("search.regex"),
                            ),
                        ] {
                            if ui
                                .add(
                                    FlatButton::new(&label, &fbt)
                                        .active(*value)
                                        .min_size(egui::vec2(28.0, 28.0)),
                                )
                                .on_hover_text(hint)
                                .clicked()
                            {
                                *value = !*value;
                                *changed = true;
                            }
                        }
                    },
                );
            });
        if state.running() {
            if ui.add(action_button(&t!("search.stop"))).clicked() {
                state.cancel();
            }
        } else {
            *start |= ui.add(action_button(&t!("search.run"))).clicked();
        }
    });
}

fn status(ui: &mut egui::Ui, state: &WorkspaceSearch, files: usize) {
    if !state.running() && state.error.is_none() && !state.cancelled && !state.searched {
        return;
    }
    ui.horizontal(|ui| {
        ui.set_min_height(20.0);
        if state.running() {
            ui.add(egui::Spinner::new().size(14.0));
            ui.weak(t!("search.searching"));
        } else if let Some(error) = &state.error {
            ui.add(
                egui::Label::new(
                    egui::RichText::new(error.lines().next().unwrap_or(error))
                        .color(theme::palette().accent_red),
                )
                .truncate(),
            )
            .on_hover_text(error);
        } else if state.cancelled {
            ui.weak(t!("search.cancelled"));
        } else if state.searched {
            ui.label(
                egui::RichText::new(t!(
                    "search.results_in_files",
                    count = state.results.len(),
                    files = files
                ))
                .size(12.0)
                .color(theme::palette().text_secondary),
            );
            if state.truncated {
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(t!("search.limit", count = state.results.len()))
                            .small()
                            .color(theme::palette().text_muted),
                    )
                    .truncate(),
                );
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(
                    egui::RichText::new(t!("search.double_click"))
                        .size(11.0)
                        .color(theme::palette().text_muted),
                );
            });
        }
    });
}

fn empty_results(ui: &mut egui::Ui, state: &WorkspaceSearch, height: f32) {
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), height),
        egui::Sense::hover(),
    );
    let center = rect.center();
    ui.painter().text(
        center - egui::vec2(0.0, 22.0),
        egui::Align2::CENTER_CENTER,
        codicon::SEARCH,
        egui::FontId::new(30.0, codicon::family()),
        theme::palette().text_muted,
    );
    let text = if state.running() {
        t!("search.searching")
    } else if state.error.is_some() {
        t!("search.check_expression")
    } else if state.cancelled {
        t!("search.cancelled")
    } else if state.searched {
        t!("search.no_results")
    } else {
        t!("search.empty_hint")
    };
    ui.painter().text(
        center + egui::vec2(0.0, 20.0),
        egui::Align2::CENTER_CENTER,
        text,
        egui::FontId::proportional(13.0),
        theme::palette().text_muted,
    );
}

fn result_rows(results: &[SearchResult]) -> Vec<ResultRow> {
    let mut rows = Vec::with_capacity(results.len());
    let mut first = 0;
    while first < results.len() {
        let count = results[first..]
            .iter()
            .take_while(|result| result.path == results[first].path)
            .count();
        rows.push(ResultRow::File { first, count });
        rows.extend((first..first + count).map(ResultRow::Match));
        first += count;
    }
    rows
}

fn file_row(ui: &mut egui::Ui, path: &str, count: usize) {
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 30.0), egui::Sense::hover());
    ui.painter()
        .rect_filled(rect, 4.0, theme::palette().bg_medium);
    let mut row = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect.shrink2(egui::vec2(10.0, 0.0)))
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    let (directory, name) = path.rsplit_once('/').unwrap_or(("", path));
    icon(&mut row, codicon::FILE, theme::palette().verdigris, 14.0);
    row.add(egui::Label::new(egui::RichText::new(name).size(12.0).strong()).truncate())
        .on_hover_text(path);
    row.label(
        egui::RichText::new(count.to_string())
            .size(11.0)
            .color(theme::palette().text_muted),
    );
    row.add(
        egui::Label::new(
            egui::RichText::new(directory)
                .size(11.0)
                .color(theme::palette().text_muted),
        )
        .truncate(),
    );
}

fn result_row(ui: &mut egui::Ui, state: &mut WorkspaceSearch, index: usize) -> bool {
    let result = &state.results[index];
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 30.0), egui::Sense::hover());
    let response = ui.interact(
        rect,
        egui::Id::new("workspace_search_result").with(index),
        egui::Sense::click(),
    );
    if response.clicked() {
        state.selected = Some(index);
    }
    if state.selected == Some(index) || response.hovered() {
        let fill = if state.selected == Some(index) {
            theme::verdigris_alpha(26)
        } else {
            theme::palette().bg_hover
        };
        ui.painter().rect_filled(rect, 3.0, fill);
        if state.selected == Some(index) {
            ui.painter().rect_filled(
                egui::Rect::from_min_size(rect.min + egui::vec2(0.0, 6.0), egui::vec2(2.0, 18.0)),
                1.0,
                theme::palette().verdigris,
            );
        }
    }
    let mut row = ui.new_child(
        egui::UiBuilder::new()
            .id_salt(index)
            .max_rect(rect.shrink2(egui::vec2(12.0, 0.0)))
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    row.style_mut().interaction.selectable_labels = false;
    row.add_sized(
        [66.0, 30.0],
        egui::Label::new(
            egui::RichText::new(format!("{}:{}", result.line, result.column))
                .monospace()
                .size(11.0)
                .color(theme::palette().text_muted),
        )
        .halign(egui::Align::Min),
    );
    let mut job = egui::text::LayoutJob::default();
    for (text, matched) in [
        (&result.snippet[..result.snippet_match.start], false),
        (&result.snippet[result.snippet_match.clone()], true),
        (&result.snippet[result.snippet_match.end..], false),
    ] {
        job.append(
            text,
            0.0,
            egui::TextFormat {
                font_id: egui::FontId::monospace(12.0),
                color: if matched {
                    theme::palette().verdigris
                } else {
                    theme::palette().text_secondary
                },
                background: if matched {
                    theme::verdigris_alpha(24)
                } else {
                    egui::Color32::TRANSPARENT
                },
                ..Default::default()
            },
        );
    }
    row.add(egui::Label::new(job).truncate());
    let double_clicked = response.double_clicked();
    response.on_hover_text(format!(
        "{}:{}:{}\n{}",
        result.path, result.line, result.column, result.snippet
    ));
    double_clicked
}

#[cfg(test)]
mod tests;

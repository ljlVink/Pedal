use super::tab::{CodeData, EditorTab};
use super::view_toggle::ActiveView;
use crate::task::{Poll, Pollable, Task};
use eframe::egui;
use egui_editor::highlight::{self, Language};
use std::time::Duration;

pub(super) struct FormattedText {
    original: String,
    text: String,
    language: Language,
    data: CodeData,
}

impl EditorTab {
    pub(crate) fn new_text_file(
        title: impl Into<String>,
        entry_path: impl Into<String>,
        text: String,
        raw_bytes: Vec<u8>,
        language: Language,
        auto_format_json: bool,
    ) -> Self {
        let entry_path = entry_path.into();
        if !auto_format_json || !crate::formatting::is_json_file(&entry_path) {
            return Self::new_text(title, entry_path, text, raw_bytes, language);
        }
        let original = text.clone();
        let mut tab = Self::new_binary(title, entry_path, raw_bytes);
        tab.decompiled = text;
        tab.language = language;
        tab.active_view = ActiveView::Decompiled;
        tab.is_text = true;
        tab.format_task = Some(Task::spawn(move || {
            let formatted = crate::formatting::format_json(&original);
            let language = if formatted.is_some() {
                Language::Json
            } else {
                language
            };
            let text = formatted.unwrap_or_else(|| original.clone());
            let data = CodeData::new(&text, highlight::compute_spans(&text, language));
            FormattedText {
                original,
                text,
                language,
                data,
            }
        }));
        tab
    }

    pub(super) fn poll_auto_format(&mut self, ctx: &egui::Context) {
        let Some(task) = &self.format_task else {
            return;
        };
        match task.poll() {
            Poll::Ready(result) => {
                self.format_task = None;
                if !self.is_modified && self.decompiled == result.original {
                    self.decompiled = result.text;
                    self.language = result.language;
                    self.decompiled_data = result.data;
                    self.layout_cache = None;
                    self.editable_layout_cache = None;
                }
            }
            Poll::Pending => ctx.request_repaint_after(Duration::from_millis(16)),
            Poll::Lost => {
                self.format_task = None;
                self.decompiled_data = CodeData::new(&self.decompiled, vec![]);
                log::warn!("Automatic JSON formatting task disconnected");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    fn open(path: &str, text: &str, enabled: bool) -> EditorTab {
        EditorTab::new_text_file(
            path,
            path,
            text.to_owned(),
            text.as_bytes().to_vec(),
            Language::from_filename(path),
            enabled,
        )
    }

    fn finish(tab: &mut EditorTab) {
        let ctx = egui::Context::default();
        let deadline = Instant::now() + Duration::from_secs(5);
        while tab.format_task.is_some() {
            assert!(Instant::now() < deadline, "formatting timed out");
            tab.poll_auto_format(&ctx);
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    #[test]
    fn automatic_formatting_changes_display_and_keeps_original_bytes() {
        let text = r#"{"pages":["pages/Index"],"name":"微信"}"#;
        for path in ["module.json", ".pages.info", "PACK.INFO"] {
            let mut tab = open(path, text, true);
            assert!(tab.format_task.is_some());
            finish(&mut tab);
            assert_eq!(
                tab.decompiled,
                crate::formatting::format_json(text).unwrap()
            );
            assert!(tab.language == Language::Json);
            assert!(tab.active_view == ActiveView::Decompiled);
            assert!(!tab.is_modified);
            assert_eq!(tab.raw_bytes, text.as_bytes());
            assert_eq!(
                tab.decompiled_data.line_count(),
                tab.decompiled.lines().count()
            );
        }
    }

    #[test]
    fn disabled_unsupported_and_invalid_files_keep_their_text() {
        let text = r#"{"a":1}"#;
        for (path, enabled) in [
            ("module.json", false),
            ("pack.info", false),
            ("data.txt", true),
        ] {
            let tab = open(path, text, enabled);
            assert!(tab.format_task.is_none());
            assert_eq!(tab.decompiled, text);
        }
        let invalid = "name = hello\ninvalid JSON\n";
        let mut tab = open("pack.info", invalid, true);
        finish(&mut tab);
        assert_eq!(tab.decompiled, invalid);
        assert!(tab.language == Language::Plain);
        assert_eq!(tab.raw_bytes, invalid.as_bytes());
    }

    #[test]
    fn formatting_does_not_overwrite_changes_made_after_open() {
        let mut tab = open("module.json", r#"{"a":1}"#, true);
        tab.decompiled = r#"{"a":2}"#.into();
        tab.is_modified = true;
        finish(&mut tab);
        assert_eq!(tab.decompiled, r#"{"a":2}"#);
        assert!(tab.is_modified);
    }

    #[test]
    fn readonly_tabs_keep_original_serialization_after_formatting() {
        let text = r#"{"b":2,"a":1}"#;
        let mut tab = open("module.json", text, true);
        tab.is_text = false;
        finish(&mut tab);
        assert!(!tab.is_text);
        assert_eq!(tab.serialize_bytes().unwrap(), text.as_bytes());
    }
}

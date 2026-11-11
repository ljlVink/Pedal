use super::App;
use super::workspace::LoadedState;
use crate::task::{CancellableTask, Poll, Pollable};
use crate::ui::editor::EditorArea;
use eframe::egui;
use egui_editor::search::{SearchOptions, SearchPattern};
use rust_i18n::t;
use std::collections::BTreeMap;
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

pub(crate) const MAX_SEARCH_RESULTS: usize = 10_000;
const PREVIEW_CHARS: usize = 180;

#[derive(Clone, Debug)]
pub(crate) struct SearchResult {
    pub path: String,
    pub start: usize,
    pub end: usize,
    pub line: usize,
    pub column: usize,
    pub snippet: String,
    pub snippet_match: std::ops::Range<usize>,
    document_hash: u64,
}

#[derive(Default)]
struct SearchBatch {
    results: Vec<SearchResult>,
    truncated: bool,
}

enum SearchContent {
    Bytes(Vec<u8>),
    Text(String),
}

type SearchDocuments = BTreeMap<String, SearchContent>;

tabookit::class! {
    pub(crate) struct WorkspaceSearch {
        pub open: bool,
        pub focus_requested: bool,
        pub query: String,
        pub options: SearchOptions,
        pub results: Vec<SearchResult>,
        pub selected: Option<usize>,
        pub error: Option<String>,
        pub truncated: bool,
        pub cancelled: bool,
        pub searched: bool,
        pub auto_search: bool,
        pub keep_open: bool,
        submitted: Option<(String, SearchOptions)>,
        task: Option<CancellableTask<Result<SearchBatch, String>>>,
    }

    pub fn running(&self) -> bool {
        self.task.is_some()
    }

    pub fn cancel(&mut self) {
        if self.task.take().is_some() {
            self.cancelled = true;
        }
    }

    pub fn clear_results(&mut self) {
        self.task = None;
        self.results.clear();
        self.selected = None;
        self.error = None;
        self.truncated = false;
        self.cancelled = false;
        self.searched = false;
        self.submitted = None;
    }

    pub fn reset(&mut self) {
        self.clear_results();
        self.open = false;
        self.focus_requested = false;
    }
}

impl Default for WorkspaceSearch {
    fn default() -> Self {
        Self {
            open: false,
            focus_requested: false,
            query: String::new(),
            options: SearchOptions::default(),
            results: Vec::new(),
            selected: None,
            error: None,
            truncated: false,
            cancelled: false,
            searched: false,
            auto_search: true,
            keep_open: true,
            submitted: None,
            task: None,
        }
    }
}

impl App {
    pub fn open_workspace_search(&mut self) {
        if !self.workspace.is_loaded() {
            return;
        }
        self.workspace_search.open = true;
        self.workspace_search.focus_requested = true;
    }

    pub fn start_workspace_search(&mut self) {
        self.workspace_search.clear_results();
        let Some(loaded) = self.workspace.loaded() else {
            return;
        };
        if self.workspace_search.query.is_empty() {
            return;
        }
        let documents = snapshot_documents(loaded, &self.layout.editor);
        let query = self.workspace_search.query.clone();
        let options = self.workspace_search.options;
        self.workspace_search.submitted = Some((query.clone(), options));
        self.workspace_search.searched = true;
        self.workspace_search.task = Some(CancellableTask::spawn(move |cancel| {
            search_documents(documents, &query, options, cancel, MAX_SEARCH_RESULTS)
        }));
    }

    pub fn poll_workspace_search(&mut self, ctx: &egui::Context) {
        if !self.workspace.is_loaded() {
            self.workspace_search.reset();
            return;
        }
        let state = &mut self.workspace_search;
        if state
            .submitted
            .as_ref()
            .is_some_and(|(query, options)| query != &state.query || options != &state.options)
        {
            state.clear_results();
            return;
        }
        let Some(task) = &state.task else {
            return;
        };
        match task.poll() {
            Poll::Ready(Ok(batch)) => {
                state.task = None;
                state.results = batch.results;
                state.truncated = batch.truncated;
                state.selected = (!state.results.is_empty()).then_some(0);
            }
            Poll::Ready(Err(error)) => {
                state.task = None;
                state.error = Some(error);
            }
            Poll::Lost => {
                state.task = None;
                state.error = Some(t!("search.failed").to_string());
            }
            Poll::Pending => ctx.request_repaint_after(Duration::from_millis(40)),
        }
    }

    pub fn jump_to_search_result(&mut self, index: usize) {
        let Some((query, options)) = &self.workspace_search.submitted else {
            return;
        };
        if query != &self.workspace_search.query || options != &self.workspace_search.options {
            self.workspace_search.clear_results();
            return;
        }
        let Some(result) = self.workspace_search.results.get(index).cloned() else {
            return;
        };
        let query = query.clone();
        let options = *options;
        if self
            .workspace
            .jar()
            .and_then(|jar| jar.get(&result.path))
            .is_none()
        {
            self.workspace_search.error =
                Some(t!("layout.entry_not_found", path = result.path).to_string());
            return;
        }
        self.layout.file_panel.pending_open = Some(result.path.clone());
        self.handle_pending_open();
        let Some((_, tab)) = self
            .layout
            .editor
            .dock_state
            .iter_all_tabs_mut()
            .find(|(_, tab)| {
                tab.entry_path.as_deref() == Some(result.path.as_str())
                    && tab.standalone_path.is_none()
            })
        else {
            return;
        };
        if result.start > result.end
            || tab.decompiled.get(result.start..result.end).is_none()
            || document_hash(&tab.decompiled) != result.document_hash
        {
            self.workspace_search.error = Some(t!("search.result_changed").to_string());
            return;
        }
        tab.reveal_search_match(query, options, result.start);
        self.workspace_search.selected = Some(index);
        self.layout.file_panel.selected = Some(result.path);
        if !self.workspace_search.keep_open {
            self.workspace_search.open = false;
        }
    }
}

pub(crate) fn is_code_path(path: &str) -> bool {
    let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
    let Some((_, extension)) = name.rsplit_once('.') else {
        return false;
    };
    matches!(
        extension.to_ascii_lowercase().as_str(),
        "ets"
            | "ts"
            | "tsx"
            | "js"
            | "jsx"
            | "mjs"
            | "cjs"
            | "mts"
            | "cts"
            | "java"
            | "kt"
            | "kts"
            | "c"
            | "h"
            | "cpp"
            | "hpp"
            | "cc"
            | "cxx"
            | "hh"
            | "hxx"
            | "cs"
            | "rs"
            | "py"
            | "go"
            | "swift"
            | "lua"
            | "rb"
            | "php"
            | "scala"
            | "sc"
            | "dart"
            | "m"
            | "mm"
            | "pandasm"
    )
}

fn snapshot_documents(loaded: &LoadedState, editor: &EditorArea) -> SearchDocuments {
    let mut documents = SearchDocuments::new();
    let tabs: BTreeMap<_, _> = editor
        .dock_state
        .iter_all_tabs()
        .map(|(_, tab)| tab)
        .filter(|tab| {
            tab.standalone_path.is_none()
                && !tab.is_image()
                && !tab.is_resource()
                && (tab.is_text || tab.is_ark || !tab.decompiled.is_empty())
        })
        .filter_map(|tab| tab.entry_path.as_deref().map(|path| (path, tab)))
        .collect();
    for path in loaded
        .jar
        .paths()
        .into_iter()
        .filter(|path| is_code_path(path))
    {
        if let Some(tab) = tabs.get(path) {
            documents.insert(path.to_owned(), SearchContent::Text(tab.decompiled.clone()));
        } else if let Some(text) = loaded.ark.as_ref().and_then(|ark| ark.sources.get(path)) {
            documents.insert(path.to_owned(), SearchContent::Text(text.clone()));
        } else if let Some(bytes) = loaded.jar.get(path) {
            documents.insert(path.to_owned(), SearchContent::Bytes(bytes.to_vec()));
        }
    }
    documents
}

fn search_documents(
    documents: SearchDocuments,
    query: &str,
    options: SearchOptions,
    cancel: &AtomicBool,
    limit: usize,
) -> Result<SearchBatch, String> {
    let mut batch = SearchBatch::default();
    if cancel.load(Ordering::Relaxed) || query.is_empty() {
        return Ok(batch);
    }
    let pattern = SearchPattern::new(query, options)
        .map_err(|error| t!("search.invalid_pattern", error = error).to_string())?;
    for (path, content) in documents {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        let text = match content {
            SearchContent::Bytes(bytes) => {
                if App::is_binary(&bytes) {
                    continue;
                }
                App::decode_text(&bytes)
            }
            SearchContent::Text(text) => text,
        };
        let mut line = 1;
        let mut column = 1;
        let mut line_start = 0;
        let mut scanned = 0;
        let document_hash = document_hash(&text);
        for found in pattern.find_iter(&text) {
            if cancel.load(Ordering::Relaxed) {
                return Ok(batch);
            }
            if batch.results.len() == limit {
                batch.truncated = true;
                return Ok(batch);
            }
            for (offset, character) in text[scanned..found.start].char_indices() {
                if character == '\n' {
                    line += 1;
                    line_start = scanned + offset + 1;
                    column = 1;
                } else {
                    column += 1;
                }
            }
            scanned = found.start;
            let (snippet, snippet_match) = preview(&text, line_start, found.start, found.end);
            batch.results.push(SearchResult {
                path: path.clone(),
                start: found.start,
                end: found.end,
                line,
                column,
                snippet,
                snippet_match,
                document_hash,
            });
        }
    }
    Ok(batch)
}

fn preview(
    text: &str,
    line_start: usize,
    matched_start: usize,
    matched_end: usize,
) -> (String, std::ops::Range<usize>) {
    let prefix = &text[line_start..matched_start];
    let start = prefix
        .char_indices()
        .rev()
        .nth(40)
        .map_or(line_start, |(offset, _)| line_start + offset);
    let mut chars = text[start..]
        .chars()
        .take_while(|character| !matches!(character, '\r' | '\n'));
    let mut snippet: String = chars.by_ref().take(PREVIEW_CHARS).collect();
    let mut snippet_match =
        (matched_start - start).min(snippet.len())..(matched_end - start).min(snippet.len());
    if start > line_start {
        snippet.insert(0, '…');
        snippet_match.start += '…'.len_utf8();
        snippet_match.end += '…'.len_utf8();
    }
    if chars.next().is_some() {
        snippet.push('…');
    }
    (snippet.replace('\t', " "), snippet_match)
}

fn document_hash(text: &str) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    text.hash(&mut hasher);
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::workspace::Workspace;
    use crate::settings::Settings;
    use crate::ui::editor::EditorTab;
    use crate::ui::layout::Layout;
    use egui_editor::Language;
    use pedal_archive::jar::JarArchive;
    use std::collections::HashMap;
    use std::path::PathBuf;

    fn loaded(entries: &[(&str, &[u8])]) -> LoadedState {
        LoadedState::new(JarArchive::from_entries(
            "test.zip".into(),
            PathBuf::from("test.zip"),
            "hash".into(),
            0,
            entries
                .iter()
                .map(|(path, bytes)| (path.to_string(), bytes.to_vec()))
                .collect::<HashMap<_, _>>(),
        ))
    }

    fn search(documents: SearchDocuments, query: &str, limit: usize) -> SearchBatch {
        search_documents(
            documents,
            query,
            SearchOptions::default(),
            &AtomicBool::new(false),
            limit,
        )
        .unwrap()
    }

    #[test]
    fn workspace_search_includes_unopened_code_and_excludes_resources_and_binary() {
        let loaded = loaded(&[
            ("src/main.ets", b"needle()"),
            ("src/lib.JS", b"needle()"),
            ("raw/data.json", b"needle"),
            ("raw/icon.png", b"needle"),
            ("src/binary.ts", b"needle\0binary"),
        ]);
        let documents = snapshot_documents(&loaded, &EditorArea::new());
        let batch = search(documents, "needle", 100);
        assert_eq!(
            batch
                .results
                .iter()
                .map(|result| result.path.as_str())
                .collect::<Vec<_>>(),
            ["src/lib.JS", "src/main.ets"]
        );
    }

    #[test]
    fn workspace_search_uses_unsaved_code_and_excludes_standalone_tabs() {
        let loaded = loaded(&[("main.ts", b"old value"), ("other.ts", b"archive value")]);
        let mut editor = EditorArea::new();
        let mut edited = EditorTab::new_text(
            "main.ts",
            "main.ts",
            "new value".into(),
            b"old value".to_vec(),
            Language::ArkTs,
        );
        edited.is_modified = true;
        editor.open_tab(edited);
        let mut standalone = EditorTab::new_text(
            "other.ts",
            "other.ts",
            "new value".into(),
            b"new value".to_vec(),
            Language::ArkTs,
        );
        standalone.standalone_path = Some(PathBuf::from("other.ts"));
        editor.open_tab(standalone);
        let batch = search(snapshot_documents(&loaded, &editor), "new", 100);
        assert_eq!(batch.results.len(), 1);
        assert_eq!(batch.results[0].path, "main.ts");
        assert!(
            search(snapshot_documents(&loaded, &editor), "old", 100)
                .results
                .is_empty()
        );
    }

    #[test]
    fn workspace_search_reports_unicode_byte_ranges_and_character_columns() {
        let text = "开头\r\n  中文目标(); 目标();\n目标();";
        let documents =
            SearchDocuments::from([("main.ets".into(), SearchContent::Text(text.into()))]);
        let batch = search(documents, "目标", 100);
        assert_eq!(
            batch
                .results
                .iter()
                .map(|result| (result.line, result.column))
                .collect::<Vec<_>>(),
            [(2, 5), (2, 11), (3, 1)]
        );
        for result in batch.results {
            assert_eq!(&text[result.start..result.end], "目标");
            assert_eq!(&result.snippet[result.snippet_match], "目标");
        }
    }

    #[test]
    fn workspace_search_limits_results_and_long_previews() {
        let text = format!("{}needle needle needle", "中".repeat(500));
        let documents = SearchDocuments::from([("main.ets".into(), SearchContent::Text(text))]);
        let batch = search(documents, "needle", 2);
        assert_eq!(batch.results.len(), 2);
        assert!(batch.truncated);
        assert!(batch.results[0].snippet.starts_with('…'));
        assert!(batch.results[0].snippet.contains("needle"));
        assert_eq!(
            &batch.results[0].snippet[batch.results[0].snippet_match.clone()],
            "needle"
        );
        assert!(batch.results[0].snippet.chars().count() <= PREVIEW_CHARS + 2);
    }

    #[test]
    fn cancelled_search_does_no_work_and_clearing_discards_state() {
        let documents =
            SearchDocuments::from([("main.ets".into(), SearchContent::Text("needle".into()))]);
        let batch = search_documents(
            documents,
            "[",
            SearchOptions {
                regex: true,
                ..Default::default()
            },
            &AtomicBool::new(true),
            100,
        )
        .unwrap();
        assert!(batch.results.is_empty());
        let mut state = WorkspaceSearch {
            searched: true,
            cancelled: true,
            truncated: true,
            error: Some("error".into()),
            ..Default::default()
        };
        state.clear_results();
        assert!(!state.searched && !state.cancelled && !state.truncated);
        assert!(state.error.is_none());
    }

    fn app(loaded: LoadedState) -> App {
        let settings = Settings::default();
        App {
            layout: Layout::new(&settings),
            settings,
            toasts: Default::default(),
            pending_confirm: None,
            workspace: Workspace::Loaded(Box::new(loaded)),
            exporting: None,
            workspace_search: WorkspaceSearch::default(),
        }
    }

    fn finish_search(app: &mut App, query: &str) {
        let documents = snapshot_documents(app.workspace.loaded().unwrap(), &app.layout.editor);
        app.workspace_search.query = query.into();
        app.workspace_search.results = search(documents, query, MAX_SEARCH_RESULTS).results;
        app.workspace_search.submitted = Some((query.into(), SearchOptions::default()));
        app.workspace_search.searched = true;
    }

    #[test]
    fn result_navigation_opens_unopened_code_and_rejects_same_length_edits() {
        let mut app = app(loaded(&[
            ("main.ts", b"needle()"),
            ("other.ts", b"needle()"),
        ]));
        finish_search(&mut app, "needle");
        app.jump_to_search_result(0);
        assert!(app.workspace_search.error.is_none());
        assert_eq!(
            app.layout.editor.focused_entry_path().as_deref(),
            Some("main.ts")
        );
        let tab = app.layout.editor.focused_tab_mut().unwrap();
        tab.decompiled = "edited()".into();
        tab.is_modified = true;
        app.jump_to_search_result(0);
        assert_eq!(
            app.workspace_search.error.as_deref(),
            Some(t!("search.result_changed").as_ref())
        );
        assert_eq!(
            app.layout.editor.focused_tab_mut().unwrap().decompiled,
            "edited()"
        );
    }

    #[test]
    fn changing_a_query_discards_pending_results_and_reset_closes_the_dialog() {
        let mut app = app(loaded(&[("main.ts", b"needle()")]));
        app.workspace_search.query = "needle".into();
        app.open_workspace_search();
        app.start_workspace_search();
        assert!(app.workspace_search.running());
        app.workspace_search.query = "different".into();
        app.poll_workspace_search(&egui::Context::default());
        assert!(!app.workspace_search.running());
        assert!(!app.workspace_search.searched);
        assert!(app.workspace_search.results.is_empty());
        app.workspace_search.reset();
        assert!(!app.workspace_search.open);
        assert!(!app.workspace_search.focus_requested);
    }

    #[test]
    fn reaching_the_exact_limit_does_not_report_truncation() {
        let documents = SearchDocuments::from([(
            "main.ets".into(),
            SearchContent::Text("needle needle".into()),
        )]);
        let batch = search(documents, "needle", 2);
        assert_eq!(batch.results.len(), 2);
        assert!(!batch.truncated);
    }
}

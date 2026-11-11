use super::*;
use crate::app::workspace::{LoadedState, Workspace};
use crate::settings::Settings;
use crate::ui::layout::Layout;
use pedal_archive::jar::JarArchive;
use std::cell::RefCell;
use std::rc::Rc;
use std::time::Instant;

fn application() -> App {
    let settings = Settings::default();
    App {
        layout: Layout::new(&settings),
        settings,
        toasts: Default::default(),
        pending_confirm: None,
        workspace: Workspace::Loaded(Box::new(LoadedState::new(JarArchive::from_entries(
            "test.zip".into(),
            "test.zip".into(),
            "hash".into(),
            0,
            [("main.ts".into(), b"needle();\nneedle();".to_vec())].into(),
        )))),
        exporting: None,
        workspace_search: WorkspaceSearch::default(),
    }
}

fn context() -> egui::Context {
    let ctx = egui::Context::default();
    let mut fonts = egui::FontDefinitions::default();
    fonts.families.insert(
        codicon::family(),
        fonts.families[&egui::FontFamily::Proportional].clone(),
    );
    ctx.set_fonts(fonts);
    ctx
}

#[test]
fn result_text_click_selects_and_double_click_navigates() {
    let mut app = application();
    let ctx = context();
    app.workspace_search.query = "needle".into();
    app.open_workspace_search();
    app.start_workspace_search();
    let deadline = Instant::now() + Duration::from_secs(5);
    while app.workspace_search.running() {
        assert!(Instant::now() < deadline);
        app.poll_workspace_search(&ctx);
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(app.workspace_search.results.len(), 2);
    let mut time = 0.0;
    let mut frame = |app: &mut App, events| {
        time += 0.02;
        let _ = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1160.0, 760.0),
                )),
                time: Some(time),
                events,
                ..Default::default()
            },
            |ui| app.render_workspace_search(ui.ctx()),
        );
    };
    for _ in 0..3 {
        frame(&mut app, vec![]);
    }
    let rect = ctx
        .read_response(egui::Id::new("workspace_search_result").with(1usize))
        .unwrap()
        .rect;
    let pos = egui::pos2(rect.left() + 110.0, rect.center().y);
    for click in 0..2 {
        for pressed in [true, false] {
            frame(
                &mut app,
                vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            );
        }
        assert_eq!(app.workspace_search.selected, Some(1));
        if click == 0 {
            assert!(app.layout.editor.focused_entry_path().is_none());
        }
    }
    assert_eq!(
        app.layout.editor.focused_entry_path().as_deref(),
        Some("main.ts")
    );
}

#[derive(Default)]
struct NativeFrames {
    input: Option<egui::RawInput>,
    windows: Vec<(egui::ViewportIdPair, egui::ViewportBuilder)>,
    rendered_ids: Vec<egui::ViewportIdPair>,
    outputs: Vec<egui::FullOutput>,
}

tabookit::class! {
    struct NativeHarness {
        ctx: egui::Context,
        frames: Rc<RefCell<NativeFrames>>,
        time: f64,
    }

    fn new() -> Self {
        let ctx = context();
        ctx.set_embed_viewports(false);
        let frames = Rc::new(RefCell::new(NativeFrames::default()));
        let captured = Rc::clone(&frames);
        egui::Context::set_immediate_viewport_renderer(move |ctx, mut viewport| {
            let mut input = captured.borrow_mut().input.take().unwrap_or_default();
            input.viewport_id = viewport.ids.this;
            input.screen_rect = Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                viewport.builder.inner_size.unwrap_or(egui::vec2(920.0, 540.0)),
            ));
            input.viewports.entry(viewport.ids.parent).or_default();
            let child = input.viewports.entry(viewport.ids.this).or_default();
            child.parent = Some(viewport.ids.parent);
            child.focused = Some(true);
            child.outer_rect = Some(egui::Rect::from_min_size(
                egui::pos2(1400.0, 100.0),
                egui::vec2(920.0, 540.0),
            ));
            captured.borrow_mut().windows.push((viewport.ids, viewport.builder));
            let output = ctx.run_ui(input, |ui| {
                captured.borrow_mut().rendered_ids.push(egui::ViewportIdPair {
                    this: ui.ctx().viewport_id(),
                    parent: ui.ctx().parent_viewport_id(),
                });
                (viewport.viewport_ui_cb)(ui);
            });
            captured.borrow_mut().outputs.push(output);
        });
        Self { ctx, frames, time: 0.0 }
    }

    fn frame(&mut self, app: &mut App, events: Vec<egui::Event>, native_close: bool) -> egui::FullOutput {
        self.time += 0.02;
        let mut input = egui::RawInput {
            time: Some(self.time),
            events,
            ..Default::default()
        };
        if native_close {
            input.viewports.insert(workspace_search_viewport_id(), egui::ViewportInfo {
                events: vec![egui::ViewportEvent::Close],
                ..Default::default()
            });
        }
        self.frames.borrow_mut().input = Some(input);
        self.ctx.run_ui(egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0))),
            time: Some(self.time),
            focused: false,
            ..Default::default()
        }, |ui| app.render_workspace_search(ui.ctx()))
    }

    fn assert_root_remains_open(&self, output: &egui::FullOutput) {
        for output in std::iter::once(output).chain(self.frames.borrow().outputs.iter()) {
            if let Some(root) = output.viewport_output.get(&egui::ViewportId::ROOT) {
                assert!(!root.commands.contains(&egui::ViewportCommand::Close));
            }
        }
    }
}

#[test]
fn native_search_window_closes_and_reopens_without_closing_the_app() {
    let mut app = application();
    let mut harness = NativeHarness::new();
    app.workspace_search.query = "needle".into();
    app.open_workspace_search();
    let output = harness.frame(&mut app, vec![], false);
    harness.assert_root_remains_open(&output);
    {
        let frames = harness.frames.borrow();
        let (ids, builder) = frames.windows.last().unwrap();
        assert_eq!(ids.this, workspace_search_viewport_id());
        assert_ne!(ids.this, egui::ViewportId::ROOT);
        assert_eq!(ids.parent, egui::ViewportId::ROOT);
        assert_eq!(builder.decorations, Some(true));
        assert_eq!(
            builder.title.as_deref(),
            Some(t!("search.workspace").as_ref())
        );
        let output = frames.outputs.last().unwrap();
        assert_eq!(frames.rendered_ids.last().unwrap(), ids);
        assert_eq!(
            output.viewport_output[&ids.this].parent,
            egui::ViewportId::ROOT
        );
        assert!(output.viewport_output[&ids.this].viewport_ui_cb.is_none());
    }
    let output = harness.frame(&mut app, vec![], true);
    harness.assert_root_remains_open(&output);
    assert!(!app.workspace_search.open);
    assert!(app.workspace.is_loaded());
    assert_eq!(app.workspace_search.query, "needle");
    let closed_frames = harness.frames.borrow().windows.len();
    harness.frame(&mut app, vec![], false);
    assert_eq!(harness.frames.borrow().windows.len(), closed_frames);
    app.open_workspace_search();
    harness.frame(&mut app, vec![], false);
    assert!(app.workspace_search.open);
    assert_eq!(app.workspace_search.query, "needle");
    assert!(harness.frames.borrow().windows.len() > closed_frames);
}

#[test]
fn escape_in_native_search_window_cancels_search_without_closing_the_app() {
    let mut app = application();
    let mut harness = NativeHarness::new();
    app.workspace_search.query = "needle".into();
    app.open_workspace_search();
    harness.frame(&mut app, vec![], false);
    app.start_workspace_search();
    assert!(app.workspace_search.running());
    let output = harness.frame(
        &mut app,
        vec![egui::Event::Key {
            key: egui::Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }],
        false,
    );
    harness.assert_root_remains_open(&output);
    assert!(!app.workspace_search.open);
    assert!(!app.workspace_search.running());
    assert!(app.workspace.is_loaded());
    assert_eq!(app.workspace_search.query, "needle");
}

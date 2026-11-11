use super::*;
use crate::FindMatch;
use crate::code_view::code_view_readonly_editable_style;
use crate::theme::{CodeViewTheme, SyntaxTheme};

fn theme() -> CodeViewTheme {
    let color = egui::Color32::WHITE;
    CodeViewTheme {
        syntax: SyntaxTheme {
            text: color,
            keyword: color,
            opcode: color,
            string: color,
            type_name: color,
            number: color,
            comment: color,
            annotation: color,
            muted: color,
            constant: color,
            method_call: color,
            method_declaration: color,
        },
        bg: color,
        gutter_bg: color,
        line_number_color: color,
        search_bg: color,
        search_current_bg: color,
        code_font_size: 14.0,
        line_highlight: color,
        word_highlight_bg: color,
    }
}

struct Harness {
    ctx: egui::Context,
    text: String,
    cache: Option<EditableLayoutCache>,
    requested: Option<usize>,
    matches: Vec<FindMatch>,
    time: f64,
    offset: f32,
    horizontal_offset: f32,
    content_height: f32,
}

impl Harness {
    fn new() -> Self {
        Self {
            ctx: egui::Context::default(),
            text: (0..10_000)
                .map(|i| format!("{{\"名称\":\"resource_{i:05}\"}},\n"))
                .collect(),
            cache: None,
            requested: None,
            matches: Vec::new(),
            time: 0.0,
            offset: 0.0,
            horizontal_offset: 0.0,
            content_height: 0.0,
        }
    }

    fn id() -> egui::Id {
        egui::Id::new("readonly_test")
    }

    fn state(&self) -> ReadonlyState {
        self.ctx
            .data(|data| data.get_temp(Self::id().with("readonly_viewport")))
            .unwrap()
    }

    fn frame(&mut self, events: Vec<egui::Event>) -> egui::FullOutput {
        self.time += 1.0 / 60.0;
        let modifiers = events
            .iter()
            .find_map(|event| match event {
                egui::Event::Key { modifiers, .. } => Some(*modifiers),
                _ => None,
            })
            .unwrap_or_default();
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(800.0, 600.0),
            )),
            time: Some(self.time),
            modifiers,
            events,
            ..Default::default()
        };
        self.ctx.run_ui(input, |ui| {
            egui::CentralPanel::default().show_inside(ui, |ui| {
                ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
                let output = egui::ScrollArea::both()
                    .id_salt("content")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        code_view_readonly_editable_style(
                            ui,
                            &mut self.text,
                            CodeViewConfig {
                                matches: &self.matches,
                                current: (!self.matches.is_empty()).then_some(0),
                                ..CodeViewConfig::new(Self::id(), &theme())
                            },
                            &mut self.cache,
                            &mut self.requested,
                        );
                    });
                self.offset = output.state.offset.y;
                self.horizontal_offset = output.state.offset.x;
                self.content_height = output.content_size.y;
            });
        })
    }
}

fn assert_match_visible_horizontally(harness: &Harness) {
    let galley = &harness.cache.as_ref().unwrap().galley;
    let byte_offset = galley.job.text.find("needle").unwrap();
    let cursor = egui::text::CCursor::new(galley.job.text[..byte_offset].chars().count());
    let line_count = harness.text.bytes().filter(|&byte| byte == b'\n').count() + 1;
    let x = galley.pos_from_cursor(cursor).min.x
        + line_number_width(line_count)
        + GUTTER_PAD
        + TEXT_PAD_LEFT
        - harness.horizontal_offset;
    assert!(harness.horizontal_offset > 2_000.0);
    assert!(x > 0.0 && x < 700.0, "match x={x}");
}

#[test]
fn ordinary_search_request_reveals_match_beyond_horizontal_view() {
    let mut harness = Harness::new();
    harness.text = format!("中文{}needle", "x".repeat(400));
    let start = harness.text.find("needle").unwrap();
    harness.matches = vec![FindMatch {
        start,
        end: start + "needle".len(),
    }];
    harness.frame(vec![]);
    harness.requested = Some(0);
    for _ in 0..40 {
        harness.frame(vec![]);
    }
    assert_match_visible_horizontally(&harness);
}

#[test]
fn distant_search_request_reveals_unlaid_match_horizontally() {
    let mut harness = Harness::new();
    harness.text = (0..10_000)
        .map(|line| {
            if line == 9_000 {
                format!("中文{}needle\n", "x".repeat(400))
            } else {
                format!("resource_{line:05}\n")
            }
        })
        .collect();
    let start = harness.text.find("needle").unwrap();
    harness.matches = vec![FindMatch {
        start,
        end: start + "needle".len(),
    }];
    harness.frame(vec![]);
    assert_eq!(harness.horizontal_offset, 0.0);
    harness.requested = Some(9_000);
    for _ in 0..6 {
        harness.frame(vec![]);
    }
    assert!(harness.requested.is_none());
    assert_match_visible_horizontally(&harness);
}

#[test]
fn wheel_scroll_keeps_full_document_height_and_stays_near_input() {
    let mut harness = Harness::new();
    harness.frame(vec![]);
    let index = harness.state().index;
    let full_height = harness.content_height;
    assert!(full_height > 100_000.0);
    for _ in 0..20 {
        harness.frame(vec![
            egui::Event::PointerMoved(egui::pos2(400.0, 300.0)),
            egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: egui::vec2(0.0, -32.0),
                phase: egui::TouchPhase::Move,
                modifiers: egui::Modifiers::NONE,
            },
        ]);
        assert!((harness.content_height - full_height).abs() < 1.0);
        assert!(harness.offset < 1_000.0);
        assert!(Arc::ptr_eq(&index, &harness.state().index));
    }
    for _ in 0..60 {
        harness.frame(vec![]);
    }
    assert!(harness.offset > 500.0 && harness.offset < 800.0);
    let stable_start = harness.state().window_start;
    let stable_offset = harness.offset;
    for _ in 0..10 {
        harness.frame(vec![]);
        assert_eq!(harness.state().window_start, stable_start);
        assert_eq!(harness.offset, stable_offset);
    }
    assert!(harness.cache.as_ref().unwrap().galley.rows.len() < 100);
}

#[test]
fn distant_line_request_renders_target_and_copy_preserves_unicode() {
    let mut harness = Harness::new();
    let original = harness.text.clone();
    harness.frame(vec![]);
    harness.requested = Some(9_000);
    harness.frame(vec![]);
    harness.frame(vec![]);
    harness.frame(vec![]);
    assert!(harness.requested.is_none());
    let galley = &harness.cache.as_ref().unwrap().galley;
    assert!(
        galley.job.text.contains("resource_09000"),
        "offset={} start={} text={}",
        harness.offset,
        harness.state().window_start,
        &galley.job.text[..100]
    );
    assert!(galley.rows.len() < 100);
    harness
        .ctx
        .memory_mut(|memory| memory.request_focus(Harness::id()));
    harness.frame(vec![egui::Event::Key {
        key: egui::Key::A,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::COMMAND,
    }]);
    let copied = harness.frame(vec![egui::Event::Copy]);
    assert!(copied.platform_output.commands.iter().any(|command| {
        matches!(command, egui::OutputCommand::CopyText(text) if text == &original)
    }));
    assert_eq!(harness.text, original);
}

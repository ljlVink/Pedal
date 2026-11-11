use crate::appearance::{codicon, theme};
use crate::task::{Poll, Pollable, Task};
use eframe::egui;
use pedal_abc_bridge::resource::{ResourceEntry, ResourceIndex, ResourceValue};
use rust_i18n::t;
use std::collections::{BTreeMap, HashSet};
use std::ops::Range;
use std::sync::Arc;
use std::time::Duration;

const ROW_HEIGHT: f32 = 24.0;
const VALUE_ROW_HEIGHT: f32 = 19.0;
const VALUE_COLUMNS: usize = 160;

#[derive(Clone, Copy, PartialEq, Eq)]
enum NodeKind {
    Type,
    Resource,
    Config,
    Value,
    Parent,
    Item,
}

struct Node {
    parent: Option<usize>,
    children: Vec<usize>,
    depth: usize,
    kind: NodeKind,
    label: String,
    summary: String,
    search: String,
    value: Option<ValueText>,
}

tabookit::class! {
    struct ValueText {
        text: String,
        rows: Vec<Range<usize>>,
        columns: usize,
    }

    fn new(text: String) -> Self {
        let mut rows = Vec::new();
        let mut start = 0;
        let mut columns = 0;
        let mut current_columns = 0;
        for (offset, character) in text.char_indices() {
            if character == '\n' {
                rows.push(start..offset);
                start = offset + 1;
                columns = columns.max(current_columns);
                current_columns = 0;
            } else {
                if current_columns >= VALUE_COLUMNS {
                    rows.push(start..offset);
                    start = offset;
                    columns = columns.max(current_columns);
                    current_columns = 0;
                }
                current_columns += if character == '\t' { 4 } else if character.is_ascii() { 1 } else { 2 };
            }
        }
        rows.push(start..text.len());
        columns = columns.max(current_columns);
        Self { text, rows, columns }
    }
}

tabookit::class! {
    pub(crate) struct ResourceTreeData {
        nodes: Vec<Node>,
        roots: Vec<usize>,
        resource_count: usize,
        variant_count: usize,
    }

    pub fn new(index: ResourceIndex) -> Self {
        let mut grouped = BTreeMap::<u32, BTreeMap<u32, Vec<ResourceEntry>>>::new();
        let variant_count = index.entries.len();
        for entry in index.entries {
            grouped.entry(entry.resource_type).or_default().entry(entry.id).or_default().push(entry);
        }
        let mut data = Self { nodes: Vec::new(), roots: Vec::new(), resource_count: 0, variant_count };
        for (kind, resources) in grouped {
            data.resource_count += resources.len();
            let root = data.push(None, NodeKind::Type, crate::resources::type_name(kind), resources.len().to_string(), None);
            for (id, entries) in resources {
                let name = &entries[0].name;
                let resource = data.push(Some(root), NodeKind::Resource,
                    format!("{name}  ·  0x{id:08X}"), entries.len().to_string(), None);
                data.nodes[resource].search.push_str(&format!(" {id}"));
                for entry in entries {
                    let summary = match &entry.value {
                        ResourceValue::String(value) => preview(value),
                        ResourceValue::Array(values) => format!("[{}]", values.len()),
                        ResourceValue::Pairs { values, .. } => format!("{{{}}}", values.len()),
                    };
                    let variant = data.push(Some(resource), NodeKind::Config,
                        crate::resources::config_name(&entry.qualifiers), summary, None);
                    match entry.value {
                        ResourceValue::String(value) => {
                            data.push(Some(variant), NodeKind::Value, String::new(), String::new(), Some(value));
                        }
                        ResourceValue::Array(values) => {
                            for (index, value) in values.into_iter().enumerate() {
                                data.push(Some(variant), NodeKind::Item, format!("[{index}]"), String::new(), Some(value));
                            }
                        }
                        ResourceValue::Pairs { parent, values } => {
                            if let Some(parent) = parent {
                                data.push(Some(variant), NodeKind::Parent, String::new(), String::new(), Some(parent));
                            }
                            for (key, value) in values {
                                data.push(Some(variant), NodeKind::Item, key, String::new(), Some(value));
                            }
                        }
                    }
                }
            }
        }
        data
    }

    fn push(&mut self, parent: Option<usize>, kind: NodeKind, label: String, mut summary: String, value: Option<String>) -> usize {
        let mut search = label.to_lowercase();
        let value = value.map(|text| {
            summary = preview(&text);
            search.push(' ');
            search.push_str(&text.to_lowercase());
            ValueText::new(text)
        });
        let depth = parent.map_or(0, |parent| self.nodes[parent].depth + 1);
        let id = self.nodes.len();
        self.nodes.push(Node { parent, children: Vec::new(), depth, kind, label, summary, search, value });
        if let Some(parent) = parent {
            self.nodes[parent].children.push(id);
        } else {
            self.roots.push(id);
        }
        id
    }

    fn filter(&self, query: &str) -> FilterResult {
        let query = query.trim().to_lowercase();
        let mut included = vec![false; self.nodes.len()];
        let mut expanded = HashSet::new();
        let mut matches = 0;
        for (id, node) in self.nodes.iter().enumerate() {
            if !node.search.contains(&query) {
                continue;
            }
            matches += 1;
            included[id] = true;
            let mut parent = node.parent;
            while let Some(id) = parent {
                included[id] = true;
                expanded.insert(id);
                parent = self.nodes[id].parent;
            }
            if !node.children.is_empty() {
                expanded.insert(id);
                let mut pending = node.children.clone();
                while let Some(child) = pending.pop() {
                    included[child] = true;
                    pending.extend(&self.nodes[child].children);
                }
            }
        }
        FilterResult { included, expanded, matches }
    }
}

struct FilterResult {
    included: Vec<bool>,
    expanded: HashSet<usize>,
    matches: usize,
}

tabookit::class! {
    pub(super) struct ResourceTree {
        data: Arc<ResourceTreeData>,
        expanded: HashSet<usize>,
        visible: Vec<usize>,
        selected: Option<usize>,
        query: String,
        filter: Option<FilterResult>,
        filter_task: Option<Task<FilterResult>>,
        search_focus: bool,
        scroll_offset: Option<f32>,
        reset_value_scroll: bool,
        search_failed: bool,
    }

    pub fn new(data: Arc<ResourceTreeData>) -> Self {
        let visible = data.roots.clone();
        Self { data, expanded: HashSet::new(), visible, selected: None, query: String::new(),
            filter: None, filter_task: None, search_focus: false, scroll_offset: None,
            reset_value_scroll: false, search_failed: false }
    }

    pub fn request_search_focus(&mut self) {
        self.search_focus = true;
    }

    pub fn show(&mut self, ui: &mut egui::Ui) {
        self.poll_filter(ui.ctx());
        ui.spacing_mut().item_spacing = egui::vec2(6.0, 4.0);
        ui.horizontal_wrapped(|ui| {
            let search = ui.add(egui::TextEdit::singleline(&mut self.query)
                .id_salt("resource_filter")
                .hint_text(t!("resource_tree.search"))
                .desired_width(260.0));
            if self.search_focus {
                search.request_focus();
                self.search_focus = false;
            }
            let mut changed = search.changed();
            if !self.query.is_empty() && ui.button(t!("resource_tree.clear")).clicked() {
                self.query.clear();
                changed = true;
            }
            if changed {
                self.start_filter();
            }
            if ui.button(t!("resource_tree.collapse_all")).clicked() {
                if let Some(filter) = &mut self.filter {
                    filter.expanded.clear();
                } else {
                    self.expanded.clear();
                }
                self.rebuild_visible();
                self.scroll_offset = Some(0.0);
            }
            if self.filter_task.is_some() {
                ui.spinner();
            }
        });
        ui.horizontal_wrapped(|ui| {
            ui.label(egui::RichText::new(t!("resource_tree.counts", resources = self.data.resource_count, variants = self.data.variant_count))
                .color(theme::palette().text_muted));
            if let Some(filter) = &self.filter {
                ui.label(t!("resource_tree.matches", count = filter.matches));
            }
        });
        if self.search_failed {
            ui.colored_label(theme::palette().accent_red, t!("resource_tree.search_failed"));
        }
        ui.separator();
        let height = (ui.available_height() - 160.0).max(60.0);
        if self.visible.is_empty() {
            ui.label(t!("resource_tree.empty"));
        }
        self.show_tree(ui, height);
        ui.separator();
        self.show_value(ui);
    }

    fn start_filter(&mut self) {
        self.search_failed = false;
        self.filter_task = None;
        if self.query.trim().is_empty() {
            self.filter = None;
            self.rebuild_visible();
            self.scroll_offset = Some(0.0);
        } else {
            let data = Arc::clone(&self.data);
            let query = self.query.clone();
            self.filter_task = Some(Task::spawn(move || data.filter(&query)));
        }
    }

    fn poll_filter(&mut self, ctx: &egui::Context) {
        let Some(task) = &self.filter_task else { return; };
        match task.poll() {
            Poll::Ready(filter) => {
                self.filter_task = None;
                self.filter = Some(filter);
                self.rebuild_visible();
                self.scroll_offset = Some(0.0);
            }
            Poll::Pending => ctx.request_repaint_after(Duration::from_millis(30)),
            Poll::Lost => {
                self.filter_task = None;
                self.search_failed = true;
            }
        }
    }

    fn rebuild_visible(&mut self) {
        self.visible.clear();
        let expanded = self.filter.as_ref().map_or(&self.expanded, |filter| &filter.expanded);
        let mut pending: Vec<_> = self.data.roots.iter().rev().copied().collect();
        while let Some(id) = pending.pop() {
            if self.filter.as_ref().is_some_and(|filter| !filter.included[id]) {
                continue;
            }
            self.visible.push(id);
            if expanded.contains(&id) {
                pending.extend(self.data.nodes[id].children.iter().rev().copied());
            }
        }
    }

    fn toggle(&mut self, id: usize) {
        let expanded = self.filter.as_mut().map_or(&mut self.expanded, |filter| &mut filter.expanded);
        if !expanded.remove(&id) {
            expanded.insert(id);
        }
        self.rebuild_visible();
    }

    fn show_tree(&mut self, ui: &mut egui::Ui, height: f32) -> egui::scroll_area::ScrollAreaOutput<Range<usize>> {
        let mut area = egui::ScrollArea::vertical().id_salt("resource_tree_rows")
            .auto_shrink([false, false]).max_height(height);
        if let Some(offset) = self.scroll_offset.take() {
            area = area.vertical_scroll_offset(offset);
        }
        let mut clicked = None;
        let palette = theme::palette();
        let output = ui.scope(|ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            area.show_rows(ui, ROW_HEIGHT, self.visible.len(), |ui, range| {
                for row in range.clone() {
                    let id = self.visible[row];
                    let node = &self.data.nodes[id];
                    let (rect, response) = ui.allocate_exact_size(egui::vec2(ui.available_width(), ROW_HEIGHT), egui::Sense::click());
                    if self.selected == Some(id) {
                        ui.painter().rect_filled(rect, 3.0, theme::verdigris_alpha(35));
                    } else if response.hovered() {
                        ui.painter().rect_filled(rect, 3.0, palette.bg_hover);
                    }
                    let left = rect.left() + 6.0 + node.depth as f32 * 18.0;
                    if !node.children.is_empty() {
                        let open = self.filter.as_ref().map_or_else(|| self.expanded.contains(&id), |filter| filter.expanded.contains(&id));
                        ui.painter().text(egui::pos2(left, rect.center().y), egui::Align2::LEFT_CENTER,
                            if open { codicon::CHEVRON_DOWN } else { codicon::CHEVRON_RIGHT },
                            egui::FontId::new(12.0, codicon::family()), palette.text_secondary);
                    }
                    let label = node_label(node);
                    let label = preview(&label);
                    let text = if node.summary.is_empty() { label } else { format!("{label}    {}", node.summary) };
                    let color = if node.kind == NodeKind::Type { palette.verdigris } else { palette.text_primary };
                    ui.painter().with_clip_rect(rect.intersect(ui.clip_rect())).text(
                        egui::pos2(left + 18.0, rect.center().y), egui::Align2::LEFT_CENTER,
                        text, egui::FontId::proportional(13.0), color);
                    if response.clicked() {
                        clicked = Some(id);
                    }
                    if let Some(value) = &node.value {
                        response.context_menu(|ui| {
                            if ui.button(t!("resource_tree.copy_value")).clicked() {
                                ui.ctx().copy_text(value.text.clone());
                                ui.close();
                            }
                        });
                    }
                }
                range
            })
        }).inner;
        if let Some(id) = clicked {
            self.selected = Some(id);
            self.reset_value_scroll = true;
            if !self.data.nodes[id].children.is_empty() {
                self.toggle(id);
            }
        }
        output
    }

    fn show_value(&mut self, ui: &mut egui::Ui) {
        let Some(node) = self.selected.map(|id| &self.data.nodes[id]) else {
            ui.label(t!("resource_tree.select_value"));
            return;
        };
        let Some(value) = &node.value else {
            ui.label(node_label(node));
            ui.label(t!("resource_tree.select_value"));
            return;
        };
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(t!("resource_tree.full_value")).strong());
            if ui.button(t!("resource_tree.copy_value")).clicked() {
                ui.ctx().copy_text(value.text.clone());
            }
            ui.label(egui::RichText::new(t!("resource_tree.value_bytes", bytes = value.text.len())).color(theme::palette().text_muted));
        });
        let mut area = egui::ScrollArea::both().id_salt("resource_value_rows").auto_shrink([false, false]);
        if self.reset_value_scroll {
            area = area.scroll_offset(egui::Vec2::ZERO);
            self.reset_value_scroll = false;
        }
        ui.scope(|ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            area.show_rows(ui, VALUE_ROW_HEIGHT, value.rows.len(), |ui, rows| {
                let width = ui.available_width().max(value.columns as f32 * 8.0);
                for row in rows {
                    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, VALUE_ROW_HEIGHT), egui::Sense::hover());
                    ui.painter().text(rect.left_center(), egui::Align2::LEFT_CENTER,
                        &value.text[value.rows[row].clone()], egui::FontId::monospace(13.0), theme::palette().text_primary);
                }
            });
        });
    }
}

fn node_label(node: &Node) -> String {
    match node.kind {
        NodeKind::Value => t!("resource_tree.value").into_owned(),
        NodeKind::Parent => t!("resource_tree.parent").into_owned(),
        _ => node.label.clone(),
    }
}

fn preview(value: &str) -> String {
    let mut characters = value.chars();
    let mut result: String = characters
        .by_ref()
        .take(120)
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .collect();
    if characters.next().is_some() {
        result.push('…');
    }
    result
}

#[cfg(test)]
mod tests;

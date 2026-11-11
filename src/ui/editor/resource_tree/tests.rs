use super::*;
use pedal_abc_bridge::resource::Qualifier;

fn entry(id: u32, name: &str, value: ResourceValue) -> ResourceEntry {
    ResourceEntry {
        id,
        name: name.into(),
        resource_type: 9,
        qualifiers: Vec::new(),
        value,
    }
}

fn data(entries: Vec<ResourceEntry>) -> ResourceTreeData {
    ResourceTreeData::new(ResourceIndex {
        version: "RestoolV2 test".into(),
        entries,
    })
}

#[test]
fn model_groups_variants_and_preserves_array_pairs_and_unicode_values() {
    let mut english = entry(1, "app_name", ResourceValue::String("WeChat".into()));
    english.qualifiers.push(Qualifier {
        kind: 0,
        value: u32::from_be_bytes([0, 0, b'e', b'n']),
    });
    let mut array = entry(
        2,
        "names",
        ResourceValue::Array(vec!["微信".into(), "游戏".into()]),
    );
    array.resource_type = 10;
    let mut pairs = entry(
        3,
        "theme",
        ResourceValue::Pairs {
            parent: Some("base_theme".into()),
            values: vec![("color".into(), "#abcdef".into())],
        },
    );
    pairs.resource_type = 16;
    let tree = data(vec![
        english,
        entry(1, "app_name", ResourceValue::String("微信".into())),
        array,
        pairs,
    ]);
    assert_eq!(tree.resource_count, 3);
    assert_eq!(tree.variant_count, 4);
    assert_eq!(tree.roots.len(), 3);
    let app = tree
        .nodes
        .iter()
        .find(|node| node.kind == NodeKind::Resource && node.label.contains("app_name"))
        .unwrap();
    assert_eq!(app.children.len(), 2);
    assert!(tree.nodes.iter().any(|node| {
        node.kind == NodeKind::Parent
            && node
                .value
                .as_ref()
                .is_some_and(|value| value.text == "base_theme")
    }));
    assert!(tree.nodes.iter().any(|node| {
        node.label == "[1]"
            && node
                .value
                .as_ref()
                .is_some_and(|value| value.text == "游戏")
    }));
}

#[test]
fn search_finds_full_values_and_decimal_or_hex_ids_with_ancestors() {
    let tree = data(vec![
        entry(
            1241513984,
            "app_name",
            ResourceValue::String("微信完整资源".into()),
        ),
        entry(2, "other", ResourceValue::String("unrelated".into())),
    ]);
    for query in ["完整资源", "1241513984", "0x4A000000", "APP_NAME"] {
        let filter = tree.filter(query);
        assert!(filter.matches > 0, "{query}");
        let value = tree
            .nodes
            .iter()
            .position(|node| {
                node.value
                    .as_ref()
                    .is_some_and(|value| value.text == "微信完整资源")
            })
            .unwrap();
        assert!(filter.included[value], "{query}");
        let mut parent = tree.nodes[value].parent;
        while let Some(id) = parent {
            assert!(filter.included[id]);
            parent = tree.nodes[id].parent;
        }
        let unrelated = tree
            .nodes
            .iter()
            .position(|node| node.label.contains("other"))
            .unwrap();
        assert!(!filter.included[unrelated]);
    }
}

#[test]
fn large_values_have_bounded_render_rows_and_keep_original_copy_content() {
    let original = format!("{}\n\n\tend", "中文🙂".repeat(20_000));
    let value = ValueText::new(original.clone());
    assert_eq!(value.text, original);
    assert!(value.rows.len() > 100);
    assert!(
        value
            .rows
            .iter()
            .all(|row| value.text.is_char_boundary(row.start)
                && value.text.is_char_boundary(row.end)
                && value.text[row.clone()].chars().count() <= VALUE_COLUMNS)
    );
    assert_eq!(&value.text[value.rows.last().unwrap().clone()], "\tend");
    assert!(preview(&value.text).chars().count() <= 121);
}

fn frame(tree: &mut ResourceTree, ctx: &egui::Context) -> (Range<usize>, f32) {
    let mut result = None;
    let _ = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(900.0, 600.0),
            )),
            ..Default::default()
        },
        |ui| {
            egui::CentralPanel::default().show_inside(ui, |ui| {
                let output = tree.show_tree(ui, 400.0);
                result = Some((output.inner, output.state.offset.y));
            });
        },
    );
    result.unwrap()
}

#[test]
fn expanded_large_tree_renders_only_visible_rows_and_keeps_scroll_offset() {
    let entries = (0..20_000)
        .map(|id| {
            entry(
                id,
                &format!("resource_{id}"),
                ResourceValue::String(format!("value_{id}")),
            )
        })
        .collect();
    let mut tree = ResourceTree::new(Arc::new(data(entries)));
    assert_eq!(tree.visible.len(), 1);
    tree.toggle(tree.data.roots[0]);
    assert_eq!(tree.visible.len(), 20_001);
    let ctx = egui::Context::default();
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "codicon".into(),
        egui::FontData::from_static(include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/crates/egui-shell/fonts/codicon.ttf"
        )))
        .into(),
    );
    fonts
        .families
        .insert(codicon::family(), vec!["codicon".into()]);
    ctx.set_fonts(fonts);
    frame(&mut tree, &ctx);
    tree.scroll_offset = Some(10_000.0 * ROW_HEIGHT);
    let (rows, offset) = frame(&mut tree, &ctx);
    assert!(rows.len() < 25);
    assert!(rows.start.abs_diff(10_000) <= 1);
    assert_eq!(offset, 10_000.0 * ROW_HEIGHT);
    let resource = tree.visible[rows.start + 5];
    tree.toggle(resource);
    let (after, after_offset) = frame(&mut tree, &ctx);
    assert_eq!(after.start, rows.start);
    assert_eq!(after_offset, offset);
    tree.toggle(resource);
    let (after, after_offset) = frame(&mut tree, &ctx);
    assert_eq!(after.start, rows.start);
    assert_eq!(after_offset, offset);
}

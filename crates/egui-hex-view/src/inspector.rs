//! 数据检查面板：用 egui Grid 布局展示光标/选区处的字节解读
//!
//! @author sky

use crate::{HexTheme, HexViewState, INSPECTOR_FONT_SIZE, PAD_LEFT};
use eframe::egui;

/// 渲染数据检查面板
pub(crate) fn show(
    ui: &mut egui::Ui,
    data: &[u8],
    state: &HexViewState,
    hover_idx: Option<usize>,
    theme: &HexTheme,
) {
    // 确定检查的字节范围
    let (start_idx, end_idx) = tabookit::or!(
        state
            .selection
            .or_else(|| state.cursor.map(|c| (c, c + 1)))
            .or_else(|| hover_idx.map(|h| (h, h + 1))),
        return
    );
    let end_idx = end_idx.min(data.len());
    let selected = &data[start_idx..end_idx];
    if selected.is_empty() {
        return;
    }
    // 顶部分隔线
    let avail = ui.available_rect_before_wrap();
    ui.painter().line_segment(
        [avail.left_top(), egui::pos2(avail.right(), avail.top())],
        egui::Stroke::new(1.0_f32, theme.border),
    );
    // 面板
    egui::Frame::NONE
        .fill(theme.inspector_bg)
        .inner_margin(egui::Margin {
            left: PAD_LEFT as i8,
            right: PAD_LEFT as i8,
            top: 6,
            bottom: 6,
        })
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.style_mut().spacing.item_spacing = egui::vec2(0.0, 2.0);
            // 标题行
            let source = if state.selection.is_some() {
                &theme.labels.selection
            } else if state.cursor.is_some() {
                &theme.labels.cursor
            } else {
                &theme.labels.hover
            };
            ui.horizontal(|ui| {
                ui.label(mono(&format!("{source}  0x{start_idx:08X}"), theme.accent));
                if end_idx - start_idx > 1 {
                    ui.add_space(16.0);
                    ui.label(mono(
                        &format!("{} {}", end_idx - start_idx, theme.labels.bytes),
                        theme.text_secondary,
                    ));
                }
            });
            ui.add_space(2.0);
            // 数据 Grid（4 列：label value | label value）
            // 每行始终渲染满 4 列，数据不足时填空，避免 widget ID 跨帧漂移
            let len = selected.len();
            egui::Grid::new("hex_inspector")
                .num_columns(4)
                .spacing([20.0, 2.0])
                .show(ui, |ui| {
                    let b = selected[0];
                    // Row 1: Int8 + Hex
                    kv(ui, "Int8", &format!("{b} / {}", b as i8), theme);
                    kv(ui, "Hex", &format!("0x{b:02X}"), theme);
                    ui.end_row();
                    // Row 2: Binary + Int16
                    kv(ui, "Binary", &format!("{b:08b}"), theme);
                    if len >= 2 {
                        let le = u16::from_le_bytes([selected[0], selected[1]]);
                        let be = u16::from_be_bytes([selected[0], selected[1]]);
                        kv(ui, "Int16", &format!("LE:{le}  BE:{be}"), theme);
                    } else {
                        empty(ui, 2);
                    }
                    ui.end_row();
                    // Row 3: Int32 + Int64
                    if len >= 4 {
                        let buf: [u8; 4] = [selected[0], selected[1], selected[2], selected[3]];
                        let le = u32::from_le_bytes(buf);
                        let be = u32::from_be_bytes(buf);
                        kv(ui, "Int32", &format!("LE:{le}  BE:{be}"), theme);
                    } else {
                        empty(ui, 2);
                    }
                    if len >= 8 {
                        let mut buf = [0u8; 8];
                        buf.copy_from_slice(&selected[..8]);
                        let le = i64::from_le_bytes(buf);
                        let be = i64::from_be_bytes(buf);
                        kv(ui, "Int64", &format!("LE:{le}  BE:{be}"), theme);
                    } else {
                        empty(ui, 2);
                    }
                    ui.end_row();
                    // Row 4: Float32 + Float64
                    if len >= 4 {
                        let buf: [u8; 4] = [selected[0], selected[1], selected[2], selected[3]];
                        let le = f32::from_le_bytes(buf);
                        let be = f32::from_be_bytes(buf);
                        kv(ui, "Float32", &format!("LE:{le:.6}  BE:{be:.6}"), theme);
                    } else {
                        empty(ui, 2);
                    }
                    if len >= 8 {
                        let mut buf = [0u8; 8];
                        buf.copy_from_slice(&selected[..8]);
                        let le = f64::from_le_bytes(buf);
                        let be = f64::from_be_bytes(buf);
                        kv(ui, "Float64", &format!("LE:{le:.6}  BE:{be:.6}"), theme);
                    } else {
                        empty(ui, 2);
                    }
                    ui.end_row();
                    // Row 5: UTF-8（始终占位）
                    if let Ok(s) = std::str::from_utf8(selected) {
                        let preview: String = s.chars().take(48).collect();
                        let display = if s.len() > 48 {
                            format!("{preview}...")
                        } else {
                            preview
                        };
                        kv(ui, "UTF-8", &format!("\"{display}\""), theme);
                    } else {
                        empty(ui, 2);
                    }
                    empty(ui, 2);
                    ui.end_row();
                });
        });
}

/// monospace 样式文本
fn mono(text: &str, color: egui::Color32) -> egui::RichText {
    egui::RichText::new(text)
        .monospace()
        .size(INSPECTOR_FONT_SIZE)
        .color(color)
}

/// Grid 内的 label: value 键值对（占 2 列）
fn kv(ui: &mut egui::Ui, label: &str, value: &str, theme: &HexTheme) {
    ui.label(mono(label, theme.text_muted));
    ui.label(mono(value, theme.text_primary));
}

/// 占位空列（保持 Grid 列数稳定）
fn empty(ui: &mut egui::Ui, n: usize) {
    for _ in 0..n {
        ui.label("");
    }
}

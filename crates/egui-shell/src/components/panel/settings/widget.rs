//! 设置项 widget 原语（全自绘）
//!
//! 每个函数返回 `bool` 表示值是否被修改，调用方据此触发 auto-save。
//! 所有控件通过 painter 手动绘制，不使用 egui 内置 Slider/ComboBox。
//!
//! @author sky

use super::SettingsTheme;
use eframe::egui;
use egui_keybind::KeyBind;

/// 设置行高度
const ROW_H: f32 = 32.0;
/// label 起始缩进
const PAD_LEFT: f32 = 16.0;
/// 控件距右边缘留白
const PAD_RIGHT: f32 = 16.0;
/// 侧栏项高度
const SIDEBAR_ITEM_H: f32 = 30.0;
/// 活跃项左侧 accent bar 宽度
const ACCENT_BAR_W: f32 = 2.0;

/// 侧栏项：图标 + 文字，active 状态高亮 + accent bar，返回是否被点击
pub fn sidebar_item(
    ui: &mut egui::Ui,
    theme: &SettingsTheme,
    icon: &str,
    label: &str,
    active: bool,
) -> bool {
    let avail_w = ui.available_width();
    let (rect, resp) =
        ui.allocate_exact_size(egui::vec2(avail_w, SIDEBAR_ITEM_H), egui::Sense::click());
    let painter = ui.painter();
    // 选中 / hover 背景动画
    let anim = egui_animation::Anim::new(ui, 0.1).with(label);
    let bg = anim.select_bg(
        active,
        resp.hovered(),
        resp.clicked(),
        theme.bg_hover,
        theme.bg_light,
    );
    if bg.a() > 0 {
        painter.rect_filled(rect, 0.0, bg);
    }
    if active {
        let bar =
            egui::Rect::from_min_size(rect.left_top(), egui::vec2(ACCENT_BAR_W, rect.height()));
        painter.rect_filled(bar, 0.0, theme.accent);
    }
    let mid_y = rect.center().y;
    let icon_color = if active {
        theme.accent
    } else {
        theme.text_muted
    };
    let text_color = if active {
        theme.text_primary
    } else {
        theme.text_secondary
    };
    painter.text(
        egui::pos2(rect.left() + 14.0, mid_y),
        egui::Align2::LEFT_CENTER,
        icon,
        egui::FontId::new(13.0, theme.icon_font.clone()),
        icon_color,
    );
    painter.text(
        egui::pos2(rect.left() + 34.0, mid_y),
        egui::Align2::LEFT_CENTER,
        label,
        egui::FontId::proportional(12.0),
        text_color,
    );
    resp.clicked()
}

/// 分类标题（带底部细线）
pub fn section_header(ui: &mut egui::Ui, theme: &SettingsTheme, title: &str) {
    let avail_w = ui.available_width();
    ui.add_space(8.0);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(avail_w, 24.0), egui::Sense::hover());
    let painter = ui.painter();
    painter.text(
        egui::pos2(rect.left() + PAD_LEFT, rect.center().y),
        egui::Align2::LEFT_CENTER,
        title,
        egui::FontId::proportional(11.0),
        theme.text_muted,
    );
    painter.line_segment(
        [
            egui::pos2(rect.left() + PAD_LEFT, rect.bottom()),
            egui::pos2(rect.right() - PAD_RIGHT, rect.bottom()),
        ],
        egui::Stroke::new(1.0_f32, theme.border),
    );
}

/// 绘制设置行背景（hover 高亮），返回行 rect 和 response
fn row(
    ui: &mut egui::Ui,
    theme: &SettingsTheme,
    sense: egui::Sense,
) -> (egui::Rect, egui::Response) {
    let avail_w = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(avail_w, ROW_H), sense);
    if resp.hovered() {
        ui.painter().rect_filled(rect, 0.0, theme.bg_hover);
    }
    (rect, resp)
}

/// 绘制行左侧 label
fn paint_label(painter: &egui::Painter, rect: egui::Rect, label: &str, color: egui::Color32) {
    painter.text(
        egui::pos2(rect.left() + PAD_LEFT, rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        egui::FontId::proportional(12.0),
        color,
    );
}

/// 开关设置行
pub fn toggle(ui: &mut egui::Ui, theme: &SettingsTheme, label: &str, value: &mut bool) -> bool {
    let old = *value;
    let (rect, resp) = row(ui, theme, egui::Sense::click());
    let painter = ui.painter();
    paint_label(painter, rect, label, theme.text_primary);
    let w = 32.0;
    let h = 16.0;
    let mid_y = rect.center().y;
    let x = rect.right() - PAD_RIGHT - w;
    let track = egui::Rect::from_min_size(egui::pos2(x, mid_y - h / 2.0), egui::vec2(w, h));
    let r = h / 2.0;
    let (bg, knob_x) = if *value {
        (theme.accent, track.right() - r)
    } else {
        (theme.bg_light, track.left() + r)
    };
    painter.rect_filled(track, r, bg);
    painter.circle_filled(egui::pos2(knob_x, mid_y), r - 2.0, egui::Color32::WHITE);
    if resp.clicked() {
        *value = !*value;
    }
    *value != old
}

/// 自绘滑条设置行
pub fn slider(
    ui: &mut egui::Ui,
    theme: &SettingsTheme,
    label: &str,
    value: &mut f32,
    range: std::ops::RangeInclusive<f32>,
    step: f32,
) -> bool {
    let old = *value;
    let min = *range.start();
    let max = *range.end();
    let avail_w = ui.available_width();
    let (rect, resp) =
        ui.allocate_exact_size(egui::vec2(avail_w, ROW_H), egui::Sense::click_and_drag());
    let painter = ui.painter();
    if resp.hovered() || resp.dragged() {
        painter.rect_filled(rect, 0.0, theme.bg_hover);
    }
    paint_label(painter, rect, label, theme.text_primary);
    let mid_y = rect.center().y;
    // 数值文字（最右）
    let value_text = if step >= 1.0 {
        format!("{}", *value as i32)
    } else {
        format!("{:.1}", *value)
    };
    let value_x = rect.right() - PAD_RIGHT;
    painter.text(
        egui::pos2(value_x, mid_y),
        egui::Align2::RIGHT_CENTER,
        &value_text,
        egui::FontId::monospace(11.0),
        theme.text_secondary,
    );
    // 滑条轨道
    let track_w = 100.0;
    let track_h = 4.0;
    let track_right = value_x - 32.0;
    let track_left = track_right - track_w;
    let track_rect = egui::Rect::from_min_size(
        egui::pos2(track_left, mid_y - track_h / 2.0),
        egui::vec2(track_w, track_h),
    );
    painter.rect_filled(track_rect, 2.0, theme.bg_light);
    // 填充部分
    let t = ((*value - min) / (max - min)).clamp(0.0, 1.0);
    let fill_w = track_w * t;
    if fill_w > 0.5 {
        let fill_rect =
            egui::Rect::from_min_size(track_rect.left_top(), egui::vec2(fill_w, track_h));
        painter.rect_filled(fill_rect, 2.0, theme.accent);
    }
    // 手柄
    let handle_x = track_left + fill_w;
    let handle_r = 6.0;
    let handle_color = if resp.dragged() {
        egui::Color32::WHITE
    } else if resp.hovered() {
        theme.text_primary
    } else {
        theme.text_secondary
    };
    painter.circle_filled(egui::pos2(handle_x, mid_y), handle_r, handle_color);
    // 拖拽交互
    if resp.dragged()
        && let Some(pos) = resp.interact_pointer_pos()
    {
        let raw = ((pos.x - track_left) / track_w).clamp(0.0, 1.0);
        let mut v = min + raw * (max - min);
        if step > 0.0 {
            v = (v / step).round() * step;
        }
        *value = v.clamp(min, max);
    }
    // 点击跳转
    if resp.clicked()
        && let Some(pos) = resp.interact_pointer_pos()
        && pos.x >= track_left
        && pos.x <= track_right
    {
        let raw = ((pos.x - track_left) / track_w).clamp(0.0, 1.0);
        let mut v = min + raw * (max - min);
        if step > 0.0 {
            v = (v / step).round() * step;
        }
        *value = v.clamp(min, max);
    }
    (*value - old).abs() > f32::EPSILON
}

/// 自绘下拉选择设置行
pub fn dropdown<T: PartialEq + Copy + std::fmt::Display>(
    ui: &mut egui::Ui,
    theme: &SettingsTheme,
    label: &str,
    value: &mut T,
    options: &[T],
) -> bool {
    let old = *value;
    let (rect, resp) = row(ui, theme, egui::Sense::click());
    let painter = ui.painter();
    paint_label(painter, rect, label, theme.text_primary);
    let mid_y = rect.center().y;
    // 当前值 + 下拉箭头
    let arrow_x = rect.right() - PAD_RIGHT;
    painter.text(
        egui::pos2(arrow_x, mid_y),
        egui::Align2::RIGHT_CENTER,
        theme.chevron_icon,
        egui::FontId::new(10.0, theme.icon_font.clone()),
        theme.text_muted,
    );
    let text = format!("{value}");
    painter.text(
        egui::pos2(arrow_x - 16.0, mid_y),
        egui::Align2::RIGHT_CENTER,
        &text,
        egui::FontId::proportional(12.0),
        theme.text_secondary,
    );
    // 下拉弹窗
    egui::Popup::from_toggle_button_response(&resp)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClick)
        .show(|ui| {
            ui.style_mut().visuals.widgets.hovered.bg_fill = theme.bg_hover;
            ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
            for &opt in options {
                let opt_text = format!("{opt}");
                let selected = opt == *value;
                let (opt_rect, opt_resp) =
                    ui.allocate_exact_size(egui::vec2(100.0, 26.0), egui::Sense::click());
                let p = ui.painter();
                if selected {
                    p.rect_filled(opt_rect, 0.0, theme.bg_hover);
                } else if opt_resp.hovered() {
                    p.rect_filled(opt_rect, 0.0, theme.bg_light);
                }
                let color = if selected {
                    theme.accent
                } else {
                    theme.text_primary
                };
                p.text(
                    egui::pos2(opt_rect.left() + 12.0, opt_rect.center().y),
                    egui::Align2::LEFT_CENTER,
                    &opt_text,
                    egui::FontId::proportional(12.0),
                    color,
                );
                if opt_resp.clicked() {
                    *value = opt;
                }
            }
        });
    *value != old
}

/// 路径选择设置行
///
/// 上行 label，下行文本输入框 + Browse 按钮，双行布局。
/// `on_browse` 在按钮点击时调用，返回 `Some(path)` 则更新值。
pub fn path_picker(
    ui: &mut egui::Ui,
    theme: &SettingsTheme,
    label: &str,
    value: &mut String,
    hint: &str,
    on_browse: impl FnOnce() -> Option<String>,
) -> bool {
    path_picker_with(ui, theme, label, value, hint, "Browse", on_browse)
}

/// 路径选择设置行（可自定义 Browse 按钮文字）
pub fn path_picker_with(
    ui: &mut egui::Ui,
    theme: &SettingsTheme,
    label: &str,
    value: &mut String,
    hint: &str,
    browse_label: &str,
    on_browse: impl FnOnce() -> Option<String>,
) -> bool {
    let old = value.clone();
    let avail_w = ui.available_width();
    let total_h = 56.0;
    let (rect, _) = ui.allocate_exact_size(egui::vec2(avail_w, total_h), egui::Sense::hover());
    let painter = ui.painter();
    // label（上半部分）
    painter.text(
        egui::pos2(rect.left() + PAD_LEFT, rect.top() + 16.0),
        egui::Align2::LEFT_CENTER,
        label,
        egui::FontId::proportional(12.0),
        theme.text_primary,
    );
    // Browse 按钮（右下角）
    let btn_w = 60.0;
    let btn_h = 22.0;
    let btn_x = rect.right() - PAD_RIGHT - btn_w;
    let btn_y = rect.bottom() - 8.0 - btn_h;
    let btn_rect = egui::Rect::from_min_size(egui::pos2(btn_x, btn_y), egui::vec2(btn_w, btn_h));
    let btn_id = ui.id().with(label).with("browse");
    let btn_resp = ui.interact(btn_rect, btn_id, egui::Sense::click());
    let btn_bg = if btn_resp.hovered() {
        theme.bg_light
    } else {
        theme.bg_medium
    };
    painter.rect_filled(btn_rect, 3.0, btn_bg);
    painter.rect_stroke(
        btn_rect,
        3.0,
        egui::Stroke::new(1.0_f32, theme.border),
        egui::StrokeKind::Outside,
    );
    painter.text(
        btn_rect.center(),
        egui::Align2::CENTER_CENTER,
        browse_label,
        egui::FontId::proportional(11.0),
        theme.text_secondary,
    );
    if btn_resp.clicked()
        && let Some(path) = on_browse()
    {
        *value = path;
    }
    // 文本输入框（左下角）
    let input_left = rect.left() + PAD_LEFT;
    let input_right = btn_x - 8.0;
    let input_w = (input_right - input_left).max(60.0);
    let input_rect =
        egui::Rect::from_min_size(egui::pos2(input_left, btn_y), egui::vec2(input_w, btn_h));
    // 输入框底部细线
    painter.line_segment(
        [
            egui::pos2(input_rect.left(), input_rect.bottom()),
            egui::pos2(input_rect.right(), input_rect.bottom()),
        ],
        egui::Stroke::new(1.0_f32, theme.border),
    );
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(input_rect));
    child.add(
        egui::TextEdit::singleline(value)
            .hint_text(hint)
            .frame(egui::Frame::NONE)
            .text_color(theme.text_primary)
            .font(egui::FontId::proportional(11.0))
            .desired_width(input_w - 4.0),
    );
    *value != old
}

/// 录制状态存储 key
fn rec_key() -> egui::Id {
    egui::Id::new("egui_ws_keybind_rec")
}

/// 检查是否有 keybind widget 正在录制（用于外部抑制快捷键分发）
pub fn is_recording_keybind(ctx: &egui::Context) -> bool {
    ctx.data(|d| d.get_temp::<u64>(rec_key()).unwrap_or(0) != 0)
}

/// 快捷键配置行
///
/// 左侧 label，右侧显示当前快捷键（pill 样式），点击进入录制模式。
/// Escape 取消录制，按任意非修饰键完成录制。
/// 返回 `true` 表示绑定被修改。
pub fn keybind_row(
    ui: &mut egui::Ui,
    theme: &SettingsTheme,
    label: &str,
    bind: &mut KeyBind,
    default: KeyBind,
) -> bool {
    keybind_row_with(ui, theme, label, bind, default, "Press a key...")
}

/// 快捷键配置行（可自定义录制提示文字）
pub fn keybind_row_with(
    ui: &mut egui::Ui,
    theme: &SettingsTheme,
    label: &str,
    bind: &mut KeyBind,
    default: KeyBind,
    recording_hint: &str,
) -> bool {
    let mut changed = false;
    let avail_w = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(avail_w, ROW_H), egui::Sense::hover());
    let painter = ui.painter();
    let mid_y = rect.center().y;
    let widget_id = ui.id().with(label);
    let ctx = ui.ctx().clone();
    let is_recording = ctx.data(|d| d.get_temp::<u64>(rec_key())) == Some(widget_id.value());
    // hover 高亮
    let hover_rect = ui.interact(rect, widget_id.with("hover"), egui::Sense::hover());
    if hover_rect.hovered() && !is_recording {
        painter.rect_filled(rect, 0.0, theme.bg_hover);
    }
    paint_label(painter, rect, label, theme.text_primary);
    if is_recording {
        // 录制中：背景高亮 + 提示文字
        painter.rect_filled(rect, 0.0, theme.bg_hover);
        let result = ctx.input(|i| {
            for event in &i.events {
                if let egui::Event::Key {
                    key,
                    modifiers,
                    pressed: true,
                    ..
                } = event
                {
                    if *key == egui::Key::Escape {
                        return Some(None);
                    }
                    return Some(Some(KeyBind::new(*modifiers, *key)));
                }
            }
            None
        });
        match result {
            Some(Some(new_bind)) => {
                *bind = new_bind;
                changed = true;
                ctx.data_mut(|d| d.insert_temp::<u64>(rec_key(), 0));
            }
            Some(None) => {
                ctx.data_mut(|d| d.insert_temp::<u64>(rec_key(), 0));
            }
            None => {}
        }
        painter.text(
            egui::pos2(rect.right() - PAD_RIGHT, mid_y),
            egui::Align2::RIGHT_CENTER,
            recording_hint,
            egui::FontId::proportional(11.0),
            theme.accent,
        );
    } else {
        let mut right_edge = rect.right() - PAD_RIGHT;
        // 已修改时显示重置按钮
        if *bind != default {
            let reset_size = 16.0;
            let reset_rect = egui::Rect::from_center_size(
                egui::pos2(right_edge - reset_size / 2.0, mid_y),
                egui::vec2(reset_size, reset_size),
            );
            let reset_resp = ui.interact(reset_rect, widget_id.with("reset"), egui::Sense::click());
            let reset_color = if reset_resp.hovered() {
                theme.text_primary
            } else {
                theme.text_muted
            };
            painter.text(
                reset_rect.center(),
                egui::Align2::CENTER_CENTER,
                "\u{EB37}",
                egui::FontId::new(11.0, theme.icon_font.clone()),
                reset_color,
            );
            if reset_resp.clicked() {
                *bind = default;
                changed = true;
            }
            right_edge -= reset_size + 6.0;
        }
        // 快捷键 pill
        let bind_text = bind.label();
        let text_galley = painter.layout_no_wrap(
            bind_text.clone(),
            egui::FontId::proportional(11.0),
            theme.text_secondary,
        );
        let btn_w = text_galley.size().x + 16.0;
        let btn_h = 20.0;
        let btn_rect = egui::Rect::from_min_size(
            egui::pos2(right_edge - btn_w, mid_y - btn_h / 2.0),
            egui::vec2(btn_w, btn_h),
        );
        let btn_resp = ui.interact(btn_rect, widget_id.with("pill"), egui::Sense::click());
        let btn_bg = if btn_resp.hovered() {
            theme.bg_light
        } else {
            theme.bg_medium
        };
        painter.rect_filled(btn_rect, 3.0, btn_bg);
        painter.rect_stroke(
            btn_rect,
            3.0,
            egui::Stroke::new(1.0_f32, theme.border),
            egui::StrokeKind::Outside,
        );
        let text_color = if *bind != default {
            theme.accent
        } else {
            theme.text_secondary
        };
        painter.text(
            btn_rect.center(),
            egui::Align2::CENTER_CENTER,
            &bind_text,
            egui::FontId::proportional(11.0),
            text_color,
        );
        if btn_resp.clicked() {
            ctx.data_mut(|d| d.insert_temp::<u64>(rec_key(), widget_id.value()));
        }
    }
    changed
}

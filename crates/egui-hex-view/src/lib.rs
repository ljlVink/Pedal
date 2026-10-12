//! egui-hex-view: 交互式 hex 查看器 widget
//!
//! 自绘 HexGrid，支持列头、hover 即时反馈、字节级点击、双向联动高亮（hex + ASCII）、
//! 拖选范围、键盘导航（方向键/PgUp/PgDn/Home/End）、Shift 扩展选区、Ctrl+C 复制、
//! Ctrl+A 全选、右键菜单、字节分类着色、固定底部数据检查面板。
//! 所有颜色通过 `HexTheme` 外部配置。
//!
//! @author sky

mod input;
mod inspector;
mod layout;
mod paint;

use eframe::egui;
use layout::Cols;

/// 创建 premultiplied alpha 颜色（简写）
pub const fn color(r: u8, g: u8, b: u8, a: u8) -> egui::Color32 {
    egui::Color32::from_rgba_premultiplied(r, g, b, a)
}
/// 每行字节数
pub(crate) const BYTES_PER_ROW: usize = 16;
/// 代码字体大小
pub(crate) const FONT_SIZE: f32 = 13.0;
/// Inspector 字体大小
pub(crate) const INSPECTOR_FONT_SIZE: f32 = 11.0;
/// 行高
pub(crate) const ROW_H: f32 = 20.0;
/// 区域间分隔宽度
pub(crate) const SECTION_GAP: f32 = 16.0;
/// 地址列字符数 "XXXXXXXX"
pub(crate) const ADDR_CHARS: usize = 8;
/// Hex 区每个字节占 3 字符 "XX "，加上 8 字节处额外 1 字符空格
pub(crate) const HEX_CHARS: usize = BYTES_PER_ROW * 3 + 1;
/// 左侧内边距
pub(crate) const PAD_LEFT: f32 = 12.0;
/// 顶部内边距
pub(crate) const PAD_TOP: f32 = 4.0;
/// 列头高度
pub(crate) const HEADER_H: f32 = 24.0;
/// 主题配色（所有颜色由调用方提供）
#[derive(Clone)]
pub struct HexTheme {
    /// 地址列文字颜色（暗淡）
    pub addr_color: egui::Color32,
    /// 地址列文字颜色（hover 行时变亮）
    pub addr_hover_color: egui::Color32,
    /// null 字节 (0x00) hex 颜色
    pub hex_null_color: egui::Color32,
    /// 可打印 ASCII 字节 (0x20-0x7E) hex 颜色
    pub hex_printable_color: egui::Color32,
    /// 控制字符 (0x01-0x1F, 0x7F) hex 颜色
    pub hex_control_color: egui::Color32,
    /// 高位字节 (>0x7F) hex 颜色
    pub hex_high_color: egui::Color32,
    /// ASCII 可打印字符颜色
    pub ascii_color: egui::Color32,
    /// ASCII 不可打印字符（'.'）颜色
    pub ascii_dot_color: egui::Color32,
    /// 主要文字颜色（光标字节、inspector value）
    pub text_primary: egui::Color32,
    /// 次要文字颜色（inspector 辅助信息）
    pub text_secondary: egui::Color32,
    /// 暗淡文字颜色（inspector label）
    pub text_muted: egui::Color32,
    /// 强调色（inspector 标题）
    pub accent: egui::Color32,
    /// hover 行整行背景
    pub hover_row_bg: egui::Color32,
    /// hover 字节高亮背景
    pub hover_byte_bg: egui::Color32,
    /// 选中范围高亮背景
    pub selection_bg: egui::Color32,
    /// 光标字节高亮背景
    pub cursor_bg: egui::Color32,
    /// 列分隔线颜色
    pub separator: egui::Color32,
    /// 边框/分隔线颜色（inspector 顶部分隔线）
    pub border: egui::Color32,
    /// inspector 面板背景
    pub inspector_bg: egui::Color32,
    /// 列头文字颜色
    pub header_color: egui::Color32,
    /// 列头背景色
    pub header_bg: egui::Color32,
    /// 搜索匹配背景色
    pub search_bg: egui::Color32,
    /// 搜索当前匹配背景色
    pub search_current_bg: egui::Color32,
    /// UI 文字标签
    pub labels: HexLabels,
}

/// Hex 视图中的可翻译文字标签
#[derive(Clone)]
pub struct HexLabels {
    pub empty: String,
    pub copy_hex: String,
    pub copy_ascii: String,
    pub copy_offset: String,
    pub select_all: String,
    pub selection: String,
    pub cursor: String,
    pub hover: String,
    pub bytes: String,
}

impl Default for HexLabels {
    fn default() -> Self {
        Self {
            empty: "(empty)".into(),
            copy_hex: "Copy as Hex".into(),
            copy_ascii: "Copy as ASCII".into(),
            copy_offset: "Copy Offset".into(),
            select_all: "Select All".into(),
            selection: "Selection".into(),
            cursor: "Cursor".into(),
            hover: "Hover".into(),
            bytes: "bytes".into(),
        }
    }
}
/// 用户点击的活跃区域
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Region {
    Hex,
    Ascii,
}

/// HexGrid 交互状态（持久化在调用方）
pub struct HexViewState {
    /// 光标所在字节索引（点击选中）
    pub cursor: Option<usize>,
    /// 选择范围 [start, end)（拖选）
    pub selection: Option<(usize, usize)>,
    /// 拖选起点（按下时记录）
    pub(crate) drag_anchor: Option<usize>,
    /// 活跃区域
    pub(crate) active_region: Region,
    /// 滚动到指定字节（由外部 find bar 设置，消费后自动清空）
    pub scroll_to_byte: Option<usize>,
}

impl Default for HexViewState {
    fn default() -> Self {
        Self {
            cursor: None,
            selection: None,
            drag_anchor: None,
            active_region: Region::Hex,
            scroll_to_byte: None,
        }
    }
}
/// 渲染 HexGrid（列头 + ScrollArea 网格 + Inspector 浮层）
///
/// `highlights` 为搜索匹配的字节范围 `(start, end)`，
/// `current_highlight` 为当前选中的匹配索引。
pub fn show(
    ui: &mut egui::Ui,
    data: &[u8],
    state: &mut HexViewState,
    theme: &HexTheme,
    highlights: &[(usize, usize)],
    current_highlight: Option<usize>,
) {
    if data.is_empty() {
        ui.centered_and_justified(|ui| {
            ui.label(egui::RichText::new(&theme.labels.empty).color(theme.text_muted));
        });
        return;
    }
    let char_w = layout::measure_char_width(ui);
    let cols = Cols::compute(char_w);
    let total_rows = data.len().div_ceil(BYTES_PER_ROW);
    let content_w = cols.total_w.max(ui.available_width());
    // 列头
    paint::header(ui, &cols, theme, content_w);
    // Inspector 上帧高度（用于 ScrollArea 内底部 padding）
    let show_inspector = state.cursor.is_some();
    let insp_h_id = ui.id().with("__insp_h");
    let insp_h: f32 = if show_inspector {
        ui.ctx().data(|d| d.get_temp(insp_h_id)).unwrap_or(120.0)
    } else {
        0.0
    };
    // 记录 grid 区域用于 inspector 定位
    let grid_rect = ui.available_rect_before_wrap();
    // ScrollArea + 网格（占满全高，不为 inspector 预留空间）
    let mut hover_idx_out: Option<usize> = None;
    egui::ScrollArea::vertical()
        .id_salt("hex_scroll")
        .show(ui, |ui| {
            // 底部 padding 让最后几行能滚过 inspector 浮层
            let content_h = total_rows as f32 * ROW_H + PAD_TOP * 2.0 + insp_h;
            let (response, painter) = ui.allocate_painter(
                egui::vec2(content_w, content_h),
                egui::Sense::click_and_drag(),
            );
            if response.hovered() {
                ui.ctx().request_repaint();
            }
            let origin = response.rect.min;
            let clip = ui.clip_rect();
            // 滚动到指定字节
            if let Some(byte_offset) = state.scroll_to_byte.take() {
                let row = byte_offset / BYTES_PER_ROW;
                let y = origin.y + PAD_TOP + row as f32 * ROW_H;
                let rect =
                    egui::Rect::from_min_size(egui::pos2(origin.x, y), egui::vec2(10.0, ROW_H));
                ui.scroll_to_rect(rect, Some(egui::Align::Center));
            }
            // 鼠标输入
            input::handle_mouse(&response, &cols, origin, data, state, total_rows);
            let is_dragging = state.drag_anchor.is_some() || response.dragged();
            // hover hit test（拖选中禁用 hover 避免闪烁）
            let hover_idx = if is_dragging {
                None
            } else {
                response.hover_pos().and_then(|pos| {
                    input::hit_test(pos, &cols, origin, data.len(), total_rows).map(|(idx, _)| idx)
                })
            };
            hover_idx_out = hover_idx;
            let hover_row = hover_idx.map(|idx| idx / BYTES_PER_ROW);
            // 可见行范围（虚拟滚动）
            let visible_top = (clip.min.y - origin.y - PAD_TOP).max(0.0);
            let visible_bottom = clip.max.y - origin.y - PAD_TOP;
            let first_row = (visible_top / ROW_H).floor() as usize;
            let last_row = ((visible_bottom / ROW_H).ceil() as usize).min(total_rows);
            let font = egui::FontId::monospace(FONT_SIZE);
            // 列分隔线
            let vis_top_y = origin.y + PAD_TOP + first_row as f32 * ROW_H;
            let vis_bot_y = origin.y + PAD_TOP + last_row as f32 * ROW_H;
            let sep_stroke = egui::Stroke::new(1.0_f32, theme.separator);
            painter.line_segment(
                [
                    egui::pos2(origin.x + cols.sep1_x, vis_top_y),
                    egui::pos2(origin.x + cols.sep1_x, vis_bot_y),
                ],
                sep_stroke,
            );
            painter.line_segment(
                [
                    egui::pos2(origin.x + cols.sep2_x, vis_top_y),
                    egui::pos2(origin.x + cols.sep2_x, vis_bot_y),
                ],
                sep_stroke,
            );
            let row_config = paint::HexRowConfig {
                cols: &cols,
                font: &font,
                origin,
                state,
                hover_idx,
                hover_row,
                theme,
                content_w,
                highlights,
                current_highlight,
            };
            // 绘制可见行
            for row in first_row..last_row {
                paint::row(&painter, row, data, &row_config);
            }
            // 右键菜单
            response.context_menu(|ui| {
                input::context_menu(ui, data, state, theme);
            });
        });
    // Inspector 浮层：从底部往上展开，覆盖在 ScrollArea 上方
    if show_inspector {
        let insp_rect = egui::Rect::from_min_max(
            egui::pos2(grid_rect.left(), grid_rect.bottom() - insp_h),
            grid_rect.right_bottom(),
        );
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(insp_rect));
        child.set_clip_rect(insp_rect);
        inspector::show(&mut child, data, state, hover_idx_out, theme);
        let actual_h = child.min_rect().height();
        ui.ctx().data_mut(|d| d.insert_temp(insp_h_id, actual_h));
    }
}

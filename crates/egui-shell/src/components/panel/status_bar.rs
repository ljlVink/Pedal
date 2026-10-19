//! 状态栏通用组件：item trait、布局引擎、分隔符绘制
//!
//! 颜色通过 `StatusBarTheme` 注入，不硬编码任何色板。
//!
//! @author ljlVink

use eframe::egui;
use std::any::Any;

/// 两侧内边距
const PAD: f32 = 12.0;
/// 相邻 item 之间的间距（含分隔符）
const SEP_GAP: f32 = 16.0;
/// 分隔符半高
const SEP_HALF_H: f32 = 7.0;

/// 状态栏项目的位置
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Alignment {
    Left,
    Right,
}

/// 状态栏可注册项目
///
/// 每个 item 负责在给定的 x 坐标处渲染自身，返回占用宽度。
/// 左侧 item 从左向右排列，右侧 item 从右向左排列。
pub trait StatusItem: Any {
    fn alignment(&self) -> Alignment;
    /// 当前是否可见（不可见时跳过渲染且不占空间）
    fn visible(&self) -> bool {
        true
    }
    /// 渲染 item 并返回占用宽度
    ///
    /// - `ui`: 状态栏 UI（用于交互和绘制）
    /// - `x`: 起始 x 坐标（左侧 item 为左边缘，右侧 item 为右边缘）
    /// - `center_y`: 状态栏垂直中心
    fn render(&mut self, ui: &mut egui::Ui, x: f32, center_y: f32) -> f32;
}

/// blanket impl：所有 StatusItem 实现者自动获得 downcast 能力
impl dyn StatusItem {
    pub fn downcast_mut<T: StatusItem>(&mut self) -> Option<&mut T> {
        (self as &mut dyn Any).downcast_mut::<T>()
    }
}

/// 状态栏配色
#[derive(Clone)]
pub struct StatusBarTheme {
    /// 背景填充色
    pub bg: egui::Color32,
    /// 分隔符颜色
    pub separator: egui::Color32,
}

tabookit::class! {
    /// 通用状态栏容器
    ///
    /// 管理一组 `StatusItem`，按 Alignment 分组渲染，自动插入分隔符。
    pub struct StatusBarWidget {
        items: Vec<Box<dyn StatusItem>>,
        theme: StatusBarTheme,
    }

    pub fn new(theme: StatusBarTheme) -> Self {
        Self {
            items: Vec::new(),
            theme,
        }
    }

    /// 更新配色，保留已注册项目及其状态。
    pub fn set_theme(&mut self, theme: StatusBarTheme) {
        self.theme = theme;
    }

    /// 注册一个 item
    pub fn add(&mut self, item: impl StatusItem + 'static) {
        self.items.push(Box::new(item));
    }

    /// 获取所有 items 的可变引用切片（供业务层遍历 downcast）
    pub fn items_mut(&mut self) -> &mut [Box<dyn StatusItem>] {
        &mut self.items
    }

    /// 获取指定类型的 item 可变引用
    pub fn item_mut<T: StatusItem>(&mut self) -> Option<&mut T> {
        self.items
            .iter_mut()
            .find_map(|item| item.downcast_mut::<T>())
    }

    /// 渲染状态栏
    pub fn render(&mut self, ui: &mut egui::Ui) {
        let rect = ui.max_rect();
        ui.painter().rect_filled(rect, 0.0, self.theme.bg);
        let center_y = rect.center().y;
        let sep_color = self.theme.separator;
        self.render_side(
            ui,
            Alignment::Left,
            rect.left() + PAD,
            center_y,
            sep_color,
            false,
        );
        self.render_side(
            ui,
            Alignment::Right,
            rect.right() - PAD,
            center_y,
            sep_color,
            true,
        );
    }

    /// 渲染某一侧的所有 items（`reverse` 控制遍历方向，右侧从右向左）
    fn render_side(
        &mut self,
        ui: &mut egui::Ui,
        side: Alignment,
        start_x: f32,
        center_y: f32,
        sep_color: egui::Color32,
        reverse: bool,
    ) {
        let mut x = start_x;
        let mut first = true;
        let indices: Vec<usize> = if reverse {
            (0..self.items.len()).rev().collect()
        } else {
            (0..self.items.len()).collect()
        };
        for i in indices {
            if self.items[i].alignment() != side || !self.items[i].visible() {
                continue;
            }
            if !first {
                let sep_x = if reverse {
                    x - SEP_GAP / 2.0
                } else {
                    x + SEP_GAP / 2.0
                };
                Self::paint_separator(ui, sep_x, center_y, sep_color);
                if reverse {
                    x -= SEP_GAP
                } else {
                    x += SEP_GAP
                };
            }
            let w = self.items[i].render(ui, x, center_y);
            if reverse {
                x -= w
            } else {
                x += w
            };
            first = false;
        }
    }

    fn paint_separator(ui: &egui::Ui, x: f32, y: f32, color: egui::Color32) {
        ui.painter().line_segment(
            [egui::pos2(x, y - SEP_HALF_H), egui::pos2(x, y + SEP_HALF_H)],
            egui::Stroke::new(1.0_f32, color),
        );
    }
}

tabookit::class! {
    /// 纯文本 item（无交互）
    pub struct TextItem {
        /// 显示文本
        text: String,
        /// 文本颜色
        color: egui::Color32,
        /// 対齐方向（左 / 右）
        alignment: Alignment,
        /// 仅在有活跃 tab 時显示（由 StatusBar 统一控制）
        context_only: bool,
        /// 是否可見
        visible: bool,
    }

    pub fn new(text: impl Into<String>, color: egui::Color32, alignment: Alignment) -> Self {
        Self {
            text: text.into(),
            color,
            alignment,
            context_only: false,
            visible: true,
        }
    }

    pub fn is_context_only(&self) -> bool {
        self.context_only
    }

    pub fn set_visible(&mut self, visible: bool) {
        self.visible = visible;
    }
}

impl StatusItem for TextItem {
    fn alignment(&self) -> Alignment {
        self.alignment
    }

    fn visible(&self) -> bool {
        self.visible
    }

    fn render(&mut self, ui: &mut egui::Ui, x: f32, center_y: f32) -> f32 {
        let painter = ui.painter();
        let galley = painter.layout_no_wrap(
            self.text.clone(),
            egui::FontId::proportional(11.0),
            self.color,
        );
        let w = galley.size().x;
        let draw_x = match self.alignment {
            Alignment::Left => x,
            Alignment::Right => x - w,
        };
        painter.galley(
            egui::pos2(draw_x, center_y - galley.size().y / 2.0),
            galley,
            self.color,
        );
        w
    }
}

/// 通用进度/状态文本 item 内核
///
/// 封装 text + color + alignment + visible + render 逻辑。
/// 业务层通过 `define_progress_item!` 宏生成 newtype wrapper，
/// wrapper 自动 Deref 到此类型并代理 `StatusItem`。
pub struct ProgressItem {
    /// 显示文本
    text: String,
    /// 文本颜色
    color: egui::Color32,
    /// 对齐方向
    alignment: Alignment,
    /// 是否可见
    visible: bool,
}

impl ProgressItem {
    pub fn new(color: egui::Color32, alignment: Alignment) -> Self {
        Self {
            text: String::new(),
            color,
            alignment,
            visible: false,
        }
    }

    /// 更新文字颜色，保留进度内容与可见状态。
    pub fn set_color(&mut self, color: egui::Color32) {
        self.color = color;
    }

    /// 设置显示文本
    pub fn set_text(&mut self, text: impl Into<String>) {
        self.text = text.into();
    }

    /// 设置可见性
    pub fn set_visible(&mut self, visible: bool) {
        self.visible = visible;
    }

    /// 获取当前文本
    pub fn text(&self) -> &str {
        &self.text
    }

    /// 获取对齐方向
    pub fn alignment(&self) -> Alignment {
        self.alignment
    }

    /// 获取可见性
    pub fn is_visible(&self) -> bool {
        self.visible
    }

    /// 渲染文本并返回占用宽度（供 wrapper 代理 `StatusItem::render`）
    pub fn render(&mut self, ui: &mut egui::Ui, x: f32, center_y: f32) -> f32 {
        let painter = ui.painter();
        let galley = painter.layout_no_wrap(
            self.text.clone(),
            egui::FontId::proportional(11.0),
            self.color,
        );
        let w = galley.size().x;
        let draw_x = match self.alignment {
            Alignment::Left => x,
            Alignment::Right => x - w,
        };
        painter.galley(
            egui::pos2(draw_x, center_y - galley.size().y / 2.0),
            galley,
            self.color,
        );
        w
    }
}

/// 生成 `ProgressItem` 的 newtype wrapper
///
/// 自动实现 `Deref` / `DerefMut`（解锁 `set_text` / `set_visible` 等方法）
/// 和 `StatusItem` trait（代理到内部 `ProgressItem::render`）。
///
/// ```ignore
/// egui_shell::define_progress_item! {
///     /// 导出进度 item
///     pub struct ExportProgressItem;
/// }
/// ```
#[macro_export]
macro_rules! define_progress_item {
    ($(#[$attr:meta])* $vis:vis struct $name:ident ;) => {
        $(#[$attr])*
        $vis struct $name($crate::components::panel::status_bar::ProgressItem);

        impl $name {
            /// 从 `ProgressItem` 构造
            $vis fn from_progress(inner: $crate::components::panel::status_bar::ProgressItem) -> Self {
                Self(inner)
            }
        }

        impl ::std::ops::Deref for $name {
            type Target = $crate::components::panel::status_bar::ProgressItem;
            fn deref(&self) -> &Self::Target {
                &self.0
            }
        }

        impl ::std::ops::DerefMut for $name {
            fn deref_mut(&mut self) -> &mut Self::Target {
                &mut self.0
            }
        }

        impl $crate::components::panel::status_bar::StatusItem for $name {
            fn alignment(&self) -> $crate::components::panel::status_bar::Alignment {
                self.0.alignment()
            }
            fn visible(&self) -> bool {
                self.0.is_visible()
            }
            fn render(
                &mut self,
                ui: &mut ::eframe::egui::Ui,
                x: f32,
                center_y: f32,
            ) -> f32 {
                self.0.render(ui, x, center_y)
            }
        }
    };
}

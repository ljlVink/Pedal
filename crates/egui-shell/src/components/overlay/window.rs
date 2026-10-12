//! FloatingWindow 核心：结构体定义、builder API、show 渲染入口、拖拽移动
//!
//! @author sky

mod header;
mod resize;

use crate::app::ShellTheme;
use eframe::egui;

/// 边缘 resize 抓取宽度
const GRAB: f32 = 5.0;

impl Default for FloatingWindow {
    fn default() -> Self {
        Self {
            id: egui::Id::NULL,
            title: String::new(),
            icon: None,
            default_size: egui::vec2(600.0, 400.0),
            min_size: egui::vec2(300.0, 200.0),
            resizable: true,
            open: false,
            pinned: false,
            size: None,
            pos: None,
            header_right_x: f32::MAX,
        }
    }
}

tabookit::class! {
    /// 带 pin 支持的主题化浮动窗口
    ///
    /// 基于 `egui::Area`（movable=false）+ `egui::Frame` 自绘。
    /// 移动通过 header 拖拽实现，resize 通过四边/四角手柄实现，互不冲突。
    pub struct FloatingWindow {
        /// 窗口唯一标识
        id: egui::Id,
        /// 标题栏文字
        title: String,
        /// 标题栏图标（codicon 字符串）
        icon: Option<&'static str>,
        /// 初始尺寸
        default_size: egui::Vec2,
        /// 最小尺寸
        min_size: egui::Vec2,
        /// 是否可拖拽缩放
        resizable: bool,
        /// 是否打开
        pub open: bool,
        /// 是否固定（pin 后点击外部不关闭、Escape 不关闭）
        pub pinned: bool,
        /// 当前窗口尺寸，None 表示使用 default_size
        size: Option<egui::Vec2>,
        /// 当前窗口位置（左上角），None 表示首次打开居中
        pos: Option<egui::Pos2>,
        /// header 右侧按钮区域左边界（屏幕坐标），drag 区域排除此右侧避免吞掉按钮点击
        header_right_x: f32,
    }

    /// 创建浮动窗口
    pub fn new(id: impl Into<egui::Id>, title: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            ..Default::default()
        }
    }

    /// 设置标题栏图标（codicon 字符串）
    pub fn icon(mut self, icon: &'static str) -> Self {
        self.icon = Some(icon);
        self
    }

    /// 设置初始尺寸
    pub fn default_size(mut self, size: impl Into<egui::Vec2>) -> Self {
        self.default_size = size.into();
        self
    }

    /// 设置最小尺寸
    pub fn min_size(mut self, size: impl Into<egui::Vec2>) -> Self {
        self.min_size = size.into();
        self
    }

    /// 设置是否可拖拽缩放
    pub fn resizable(mut self, resizable: bool) -> Self {
        self.resizable = resizable;
        self
    }

    /// 打开窗口（重置位置为居中）
    pub fn open(&mut self) {
        self.open = true;
        self.pos = None;
    }

    /// 关闭窗口并取消 pin
    pub fn close(&mut self) {
        self.open = false;
        self.pinned = false;
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    /// 渲染浮动窗口
    ///
    /// `header_right`: header 右侧自定义按钮区域
    /// `content`: 窗口主体内容
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        theme: &ShellTheme,
        header_right: impl FnOnce(&mut egui::Ui),
        content: impl FnOnce(&mut egui::Ui),
    ) {
        if !self.open {
            return;
        }
        // 未 pin 时 Escape 关闭
        if !self.pinned && ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.close();
            return;
        }
        let size = self.size.unwrap_or(self.default_size);
        let screen = ctx.content_rect();
        // 首次打开居中，之后保持位置
        let pos = self.pos.unwrap_or_else(|| {
            egui::pos2(
                screen.center().x - size.x * 0.5,
                screen.center().y - size.y * 0.5,
            )
        });
        self.pos = Some(pos);
        let area_resp = egui::Area::new(self.id)
            .movable(false)
            .sense(egui::Sense::hover())
            .current_pos(pos)
            .constrain(true)
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                let frame_resp = theme.window.frame.show(ui, |ui| {
                    let origin = ui.available_rect_before_wrap().min;
                    let rect = egui::Rect::from_min_size(origin, size);
                    let mut child = ui.new_child(
                        egui::UiBuilder::new()
                            .id(self.id.with("content"))
                            .max_rect(rect),
                    );
                    child.set_clip_rect(rect);
                    header::apply_style(&mut child, theme);
                    self.render_header(&mut child, theme, header_right);
                    crate::components::separator(&mut child, theme.separator);
                    content(&mut child);
                    ui.allocate_rect(rect, egui::Sense::hover());
                });
                let frame_rect = frame_resp.response.rect;
                // Frame 的 fill+stroke 绘制在 content 之下，
                // content 的矩形 clip_rect 在底部圆角区域会覆盖边框。
                // 此处重绘一次 stroke 使其始终在 content 之上。
                ui.painter().rect_stroke(
                    frame_rect,
                    theme.window.frame.corner_radius,
                    theme.window.frame.stroke,
                    egui::StrokeKind::Inside,
                );
                self.handle_move(ui, frame_rect, theme);
                if self.resizable {
                    self.handle_resize(ui, frame_rect, theme);
                }
            });
        // 未 pin 时点击窗口外部关闭
        if !self.pinned {
            let window_rect = area_resp.response.rect.expand(4.0);
            let clicked_outside = ctx.input(|i| {
                i.pointer.any_pressed()
                    && i.pointer
                        .interact_pos()
                        .is_some_and(|p| !window_rect.contains(p))
            });
            if clicked_outside {
                self.close();
            }
        }
    }

    /// header 拖拽移动（排除右侧按钮区域）
    fn handle_move(&mut self, ui: &mut egui::Ui, frame_rect: egui::Rect, theme: &ShellTheme) {
        let drag_width = (self.header_right_x - frame_rect.left()).max(0.0);
        let header_rect = egui::Rect::from_min_size(
            frame_rect.min,
            egui::vec2(drag_width, theme.window.header_height),
        );
        let resp = ui.interact(
            header_rect,
            self.id.with("header_drag"),
            egui::Sense::drag(),
        );
        if resp.dragged()
            && let Some(pos) = &mut self.pos {
                *pos += resp.drag_delta();
            }
    }
}

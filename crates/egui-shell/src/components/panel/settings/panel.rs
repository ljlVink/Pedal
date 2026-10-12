//! 设置面板：FloatingWindow + 侧栏/内容分栏 + 配置草稿管理
//!
//! 泛型 `T` 为配置数据类型，面板内部管理编辑副本（draft）与快照（snapshot），
//! 关闭时自动检测变更。业务层只需提供分类描述 + 活跃 section 的内容渲染回调。
//!
//! @author sky

use super::widget::sidebar_item;
use super::{SectionDef, SettingsTheme};
use crate::app::ShellTheme;
use crate::components::overlay::window::FloatingWindow;
use eframe::egui;
use serde::Serialize;

/// 侧栏宽度
const SIDEBAR_W: f32 = 130.0;

impl<T: Clone + Serialize> Default for SettingsPanel<T> {
    fn default() -> Self {
        Self {
            window: FloatingWindow::default(),
            active: 0,
            draft: None,
            snapshot: None,
        }
    }
}

tabookit::class! {
    /// 设置面板：FloatingWindow + 侧栏/内容分栏 + 配置草稿管理
    ///
    /// `T` 为配置数据类型，面板内部持有编辑副本（draft）和打开时的快照（snapshot），
    /// 关闭时通过 TOML 序列化比较检测是否有变更。
    pub struct SettingsPanel<T: Clone + Serialize> {
        window: FloatingWindow,
        /// 当前选中的 section 索引
        active: usize,
        /// 编辑中的工作副本（None = 面板未打开或数据未加载）
        draft: Option<T>,
        /// 打开时的快照（用于关闭时检测变更）
        snapshot: Option<T>,
    }

    pub fn new(id: impl Into<egui::Id>, title: impl Into<String>) -> Self {
        Self {
            window: FloatingWindow::new(id, title),
            active: 0,
            draft: None,
            snapshot: None,
        }
    }

    pub fn icon(mut self, icon: &'static str) -> Self {
        self.window = self.window.icon(icon);
        self
    }

    pub fn default_size(mut self, size: impl Into<egui::Vec2>) -> Self {
        self.window = self.window.default_size(size);
        self
    }

    pub fn min_size(mut self, size: impl Into<egui::Vec2>) -> Self {
        self.window = self.window.min_size(size);
        self
    }

    /// 打开面板，传入当前生效配置作为编辑起点
    pub fn open(&mut self, current: &T) {
        if self.window.is_open() {
            return;
        }
        self.draft = Some(current.clone());
        self.snapshot = Some(current.clone());
        self.active = 0;
        self.window.open();
    }

    pub fn close(&mut self) {
        self.window.close();
    }

    pub fn is_open(&self) -> bool {
        self.window.is_open()
    }

    /// 渲染面板，返回 `Some(T)` 表示配置有变更需要应用
    ///
    /// 变更在两种时机触发：
    /// - 内容回调返回 `true`（实时变更，如语言切换）
    /// - 面板关闭时 draft 与 snapshot 不同（累积变更）
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        theme: &ShellTheme,
        st: &SettingsTheme,
        sections: &[SectionDef],
        content: impl FnOnce(&mut T, usize, &mut egui::Ui, &SettingsTheme) -> bool,
    ) -> Option<T> {
        let Self {
            window,
            active,
            draft,
            snapshot,
        } = self;
        if draft.is_none() {
            return None;
        }
        let mut changed = false;
        {
            let d = draft.as_mut().unwrap();
            window.show(
                ctx,
                theme,
                |_ui| {},
                |ui| {
                    let total = ui.available_rect_before_wrap();
                    let sw = SIDEBAR_W.min(total.width() * 0.5);
                    // 侧栏背景
                    let sidebar_rect =
                        egui::Rect::from_min_size(total.left_top(), egui::vec2(sw, total.height()));
                    ui.painter().rect_filled(sidebar_rect, 0.0, st.bg_sidebar);
                    // 侧栏
                    let mut sidebar_ui = ui.new_child(
                        egui::UiBuilder::new()
                            .id(egui::Id::new("settings_sidebar"))
                            .max_rect(sidebar_rect),
                    );
                    sidebar_ui.set_clip_rect(sidebar_rect);
                    sidebar_ui.add_space(6.0);
                    for (i, sec) in sections.iter().enumerate() {
                        if sidebar_item(&mut sidebar_ui, st, sec.icon, &sec.label, *active == i) {
                            *active = i;
                        }
                    }
                    // 垂直分隔线
                    ui.painter().line_segment(
                        [
                            egui::pos2(sidebar_rect.right(), total.top()),
                            egui::pos2(sidebar_rect.right(), total.bottom()),
                        ],
                        egui::Stroke::new(1.0_f32, st.border),
                    );
                    // 内容区
                    let content_left = (sidebar_rect.right() + 1.0).min(total.right());
                    let content_rect = egui::Rect::from_min_max(
                        egui::pos2(content_left, total.top()),
                        total.right_bottom(),
                    );
                    if content_rect.width() > 1.0 {
                        let mut content_outer = ui.new_child(
                            egui::UiBuilder::new()
                                .id(egui::Id::new("settings_content"))
                                .max_rect(content_rect),
                        );
                        content_outer.set_clip_rect(content_rect);
                        egui::ScrollArea::vertical()
                            .id_salt("settings_scroll")
                            .show(&mut content_outer, |content_ui| {
                                changed = content(d, *active, content_ui, st);
                            });
                    }
                    ui.allocate_rect(total, egui::Sense::hover());
                },
            );
        }
        // 面板关闭时检测累积变更
        if !window.is_open() {
            let d = draft.take().unwrap();
            let s = snapshot.take();
            return s.and_then(|s| {
                let a = toml::to_string(&d).unwrap_or_default();
                let b = toml::to_string(&s).unwrap_or_default();
                if a != b {
                    Some(d)
                } else {
                    None
                }
            });
        }
        if changed {
            draft.as_ref().cloned()
        } else {
            None
        }
    }
}

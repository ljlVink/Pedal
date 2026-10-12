//! egui 动画工具库
//!
//! 通过 [`Anim`] 作用域对象消除重复的 `ctx` / `id` / `duration` 样板代码。
//!
//! ```ignore
//! let anim = Anim::new(ui, 0.15);
//! let offset = anim.f32("offset", target_offset);
//! let color = anim.color("text", target_color);
//! ```
//!
//! @author sky
//! @author ljlVink

use egui::{Color32, Context, Id, Ui, Vec2};
use std::hash::Hash;

/// 两个颜色之间的线性插值。
///
/// `t = 0.0` 返回 `a`，`t = 1.0` 返回 `b`。
pub fn lerp_color(a: Color32, b: Color32, t: f32) -> Color32 {
    let l = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * t).round() as u8;
    Color32::from_rgba_premultiplied(
        l(a.r(), b.r()),
        l(a.g(), b.g()),
        l(a.b(), b.b()),
        l(a.a(), b.a()),
    )
}

/// 动画作用域，绑定 `Context`、基础 `Id` 和 `duration`，后续调用零样板。
///
/// 内部克隆 `Context`（`Arc` 级别，几乎零开销），不持有 `&Ui` 引用，
/// 因此创建后仍可自由使用 `&mut Ui`。
pub struct Anim {
    ctx: Context,
    base_id: Id,
    duration: f32,
}

impl Anim {
    /// 从当前 `Ui` 创建动画作用域。
    pub fn new(ui: &Ui, duration: f32) -> Self {
        Self {
            ctx: ui.ctx().clone(),
            base_id: ui.id(),
            duration,
        }
    }

    /// 追加 ID salt，用于循环中区分不同元素。
    ///
    /// ```ignore
    /// for (i, item) in items.iter().enumerate() {
    ///     let anim = Anim::new(ui, 0.15).with(i);
    ///     let bg = anim.color("bg", target);
    /// }
    /// ```
    pub fn with(mut self, salt: impl Hash) -> Self {
        self.base_id = self.base_id.with(salt);
        self
    }

    /// 动画化 f32 值。
    pub fn f32(&self, salt: impl Hash, target: f32) -> f32 {
        self.ctx
            .animate_value_with_time(self.base_id.with(salt), target, self.duration)
    }

    /// 动画化布尔状态，返回 `0.0..=1.0` 的过渡值。
    pub fn bool(&self, salt: impl Hash, target: bool) -> f32 {
        self.f32(salt, if target { 1.0 } else { 0.0 })
    }

    /// 动画化颜色，RGBA 四通道独立过渡。
    pub fn color(&self, salt: impl Hash, target: Color32) -> Color32 {
        let id = self.base_id.with(salt);
        let r = self
            .ctx
            .animate_value_with_time(id.with(0u8), target.r() as f32, self.duration);
        let g = self
            .ctx
            .animate_value_with_time(id.with(1u8), target.g() as f32, self.duration);
        let b = self
            .ctx
            .animate_value_with_time(id.with(2u8), target.b() as f32, self.duration);
        let a = self
            .ctx
            .animate_value_with_time(id.with(3u8), target.a() as f32, self.duration);
        Color32::from_rgba_premultiplied(r as u8, g as u8, b as u8, a as u8)
    }

    /// 选中/悬停背景色动画。
    ///
    /// 根据 `selected`、`hovered`、`clicked` 状态确定目标颜色，通过内部 `color("bg", ...)` 做平滑过渡：
    /// - `selected` 或 `clicked` → `selected_color`
    /// - `hovered`（且未选中）→ `hover_color`
    /// - 否则 → `Color32::TRANSPARENT`
    ///
    /// `clicked` 用于点击帧立即使用选中色，避免 hover → transparent → selected 的闪烁--
    /// 因为选中状态通常在下一帧才由父级更新，中间会经过一帧透明。
    pub fn select_bg(
        &self,
        selected: bool,
        hovered: bool,
        clicked: bool,
        selected_color: Color32,
        hover_color: Color32,
    ) -> Color32 {
        let target = if selected || clicked {
            selected_color
        } else if hovered {
            hover_color
        } else {
            Color32::TRANSPARENT
        };
        self.color("bg", target)
    }

    /// 动画化 `Vec2`，x/y 分量独立过渡。
    pub fn vec2(&self, salt: impl Hash, target: Vec2) -> Vec2 {
        let id = self.base_id.with(salt);
        Vec2::new(
            self.ctx
                .animate_value_with_time(id.with(0u8), target.x, self.duration),
            self.ctx
                .animate_value_with_time(id.with(1u8), target.y, self.duration),
        )
    }
}

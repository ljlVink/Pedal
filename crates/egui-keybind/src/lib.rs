//! 声明式快捷键绑定：定义一次，检测 + 回调 + 显示共用
//!
//! ```ignore
//! use egui_keybind::{KeyBind, KeyMap};
//! use egui::Key;
//!
//! struct App { visible: bool }
//!
//! let mut keys = KeyMap::new()
//!     .bind(KeyBind::alt(Key::Num1), |app: &mut App| app.visible = !app.visible)
//!     .bind_double_shift(|app| { /* open search */ });
//!
//! // 每帧：检测按键并直接调用回调
//! let mut keys = std::mem::take(&mut self.keys);
//! keys.dispatch(ctx, self);
//! self.keys = keys;
//! ```
//!
//! @author sky

use egui::{Context, Key, Modifiers};
use std::fmt;

/// 快捷键绑定：修饰键 + 主键
///
/// 所有构造函数均为 `const`，可用于 `static` / `const` 常量定义。
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct KeyBind {
    pub modifiers: Modifiers,
    pub key: Key,
}

impl KeyBind {
    pub const fn new(modifiers: Modifiers, key: Key) -> Self {
        Self { modifiers, key }
    }

    /// 无修饰键
    pub const fn key(key: Key) -> Self {
        Self::new(Modifiers::NONE, key)
    }

    /// Ctrl（Windows/Linux）/ Cmd（macOS）
    pub const fn ctrl(key: Key) -> Self {
        Self::new(Modifiers::COMMAND, key)
    }

    /// Alt
    pub const fn alt(key: Key) -> Self {
        Self::new(Modifiers::ALT, key)
    }

    /// Shift
    pub const fn shift(key: Key) -> Self {
        Self::new(Modifiers::SHIFT, key)
    }

    /// Ctrl+Shift
    pub const fn ctrl_shift(key: Key) -> Self {
        Self::new(Modifiers::COMMAND.plus(Modifiers::SHIFT), key)
    }

    /// Ctrl+Alt
    pub const fn ctrl_alt(key: Key) -> Self {
        Self::new(Modifiers::COMMAND.plus(Modifiers::ALT), key)
    }

    /// 人类可读的标签（如 "Ctrl+O"、"Alt+1"、"Ctrl+Shift+F"）
    pub fn label(&self) -> String {
        let mut parts = Vec::with_capacity(4);
        if self.modifiers.ctrl || self.modifiers.command {
            parts.push(MOD_CTRL);
        }
        if self.modifiers.alt {
            parts.push(MOD_ALT);
        }
        if self.modifiers.shift {
            parts.push(MOD_SHIFT);
        }
        parts.push(key_name(self.key));
        parts.join("+")
    }

    /// 检测并消费匹配的按键事件，返回是否触发
    pub fn pressed(&self, ctx: &Context) -> bool {
        ctx.input_mut(|i| i.consume_key(self.modifiers, self.key))
    }

    /// 修饰键数量（用于排序，更多修饰键优先匹配）
    fn modifier_count(&self) -> u8 {
        let m = &self.modifiers;
        (m.ctrl || m.command) as u8 + m.alt as u8 + m.shift as u8 + m.mac_cmd as u8
    }

    /// 从 `"Ctrl+O"`、`"Alt+1"` 等格式的字符串解析
    pub fn parse(s: &str) -> Option<Self> {
        let parts: Vec<&str> = s.split('+').collect();
        if parts.is_empty() {
            return None;
        }
        let key = parse_key(parts.last()?.trim())?;
        let mut modifiers = Modifiers::NONE;
        for &part in &parts[..parts.len() - 1] {
            match part.trim() {
                "Ctrl" | "⌘" | "Cmd" => {
                    modifiers.command = true;
                }
                "Alt" => modifiers.alt = true,
                "Shift" => modifiers.shift = true,
                _ => return None,
            }
        }
        Some(Self { modifiers, key })
    }
}

impl fmt::Display for KeyBind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.label())
    }
}

impl fmt::Debug for KeyBind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "KeyBind({})", self.label())
    }
}

type KeyBinding<T> = (KeyBind, fn(&mut T));

/// 快捷键映射表：将 `KeyBind` 绑定到 `fn(&mut T)` 回调，自动处理每帧检测与分发
///
/// 使用时需 `std::mem::take` 临时取出，避免自引用借用：
/// ```ignore
/// let mut keys = std::mem::take(&mut self.keys);
/// keys.dispatch(ctx, self);
/// self.keys = keys;
/// ```
pub struct KeyMap<T> {
    bindings: Vec<KeyBinding<T>>,
    double_shift: Option<DoubleShiftBinding<T>>,
    /// 上次 dispatch 的帧号，防止同一帧多 pass 重复消费
    last_frame: u64,
}

struct DoubleShiftBinding<T> {
    handler: fn(&mut T),
    detector: DoubleShiftDetector,
}

/// Double Shift 检测器
///
/// 追踪 Shift 修饰键的按下/释放状态。
/// 两次纯 Shift 释放间隔 < 400ms 且中间无其他按键 → 触发。
struct DoubleShiftDetector {
    was_down: bool,
    last_release: f64,
}

impl DoubleShiftDetector {
    fn new() -> Self {
        Self {
            was_down: false,
            last_release: 0.0,
        }
    }

    fn update(&mut self, ctx: &Context) -> bool {
        let shift = ctx.input(|i| i.modifiers.shift);
        let time = ctx.input(|i| i.time);
        let has_key = ctx.input(|i| {
            i.events
                .iter()
                .any(|e| matches!(e, egui::Event::Key { pressed: true, .. }))
        });
        let mut triggered = false;
        if has_key {
            self.last_release = 0.0;
        } else if self.was_down && !shift {
            if self.last_release > 0.0 && time - self.last_release < 0.4 {
                triggered = true;
                self.last_release = 0.0;
            } else {
                self.last_release = time;
            }
        }
        self.was_down = shift;
        triggered
    }
}

impl<T> KeyMap<T> {
    pub fn new() -> Self {
        Self {
            bindings: Vec::new(),
            double_shift: None,
            last_frame: u64::MAX,
        }
    }

    /// 注册快捷键 → 回调（builder 风格）
    pub fn bind(mut self, keybind: KeyBind, handler: fn(&mut T)) -> Self {
        self.bindings.push((keybind, handler));
        // 按修饰键数量降序排列，确保更具体的快捷键优先匹配
        self.bindings
            .sort_by_key(|binding| std::cmp::Reverse(binding.0.modifier_count()));
        self
    }

    /// 注册 Double Shift 手势 → 回调
    pub fn bind_double_shift(mut self, handler: fn(&mut T)) -> Self {
        self.double_shift = Some(DoubleShiftBinding {
            handler,
            detector: DoubleShiftDetector::new(),
        });
        self
    }

    /// 检测按键并直接调用回调
    ///
    /// 同一帧内多次调用不会重复消费事件。
    pub fn dispatch(&mut self, ctx: &Context, target: &mut T) {
        let frame = ctx.cumulative_frame_nr();
        if self.last_frame == frame {
            return;
        }
        self.last_frame = frame;
        for (keybind, handler) in &self.bindings {
            if keybind.pressed(ctx) {
                handler(target);
            }
        }
        if let Some(ds) = &mut self.double_shift
            && ds.detector.update(ctx)
        {
            (ds.handler)(target);
        }
    }
}

impl<T> Default for KeyMap<T> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(target_os = "macos")]
const MOD_CTRL: &str = "⌘";
#[cfg(not(target_os = "macos"))]
const MOD_CTRL: &str = "Ctrl";

const MOD_ALT: &str = "Alt";
const MOD_SHIFT: &str = "Shift";

/// 将 egui Key 映射为简洁的显示名称
fn key_name(key: Key) -> &'static str {
    match key {
        Key::Escape => "Esc",
        Key::Tab => "Tab",
        Key::Enter => "Enter",
        Key::Space => "Space",
        Key::Backspace => "Backspace",
        Key::Delete => "Del",
        Key::Insert => "Ins",
        Key::Home => "Home",
        Key::End => "End",
        Key::PageUp => "PgUp",
        Key::PageDown => "PgDn",
        Key::ArrowUp => "↑",
        Key::ArrowDown => "↓",
        Key::ArrowLeft => "←",
        Key::ArrowRight => "→",
        Key::Slash => "/",
        Key::Comma => ",",
        _ => key.name(),
    }
}

/// 从显示名称反解析为 Key（支持 `key_name()` 的短名和 `Key::name()` 的原名）
fn parse_key(s: &str) -> Option<Key> {
    let normalized = match s {
        "Esc" => "Escape",
        "Del" => "Delete",
        "Ins" => "Insert",
        "PgUp" => "PageUp",
        "PgDn" => "PageDown",
        "↑" => "ArrowUp",
        "↓" => "ArrowDown",
        "←" => "ArrowLeft",
        "→" => "ArrowRight",
        "/" => "Slash",
        "," => "Comma",
        other => other,
    };
    ALL_KEYS.iter().find(|k| k.name() == normalized).copied()
}

/// 支持解析的所有 Key 变体
const ALL_KEYS: &[Key] = &[
    Key::ArrowDown,
    Key::ArrowLeft,
    Key::ArrowRight,
    Key::ArrowUp,
    Key::Escape,
    Key::Tab,
    Key::Backspace,
    Key::Enter,
    Key::Space,
    Key::Insert,
    Key::Delete,
    Key::Home,
    Key::End,
    Key::PageUp,
    Key::PageDown,
    Key::Slash,
    Key::Comma,
    Key::Minus,
    Key::Plus,
    Key::Num0,
    Key::Num1,
    Key::Num2,
    Key::Num3,
    Key::Num4,
    Key::Num5,
    Key::Num6,
    Key::Num7,
    Key::Num8,
    Key::Num9,
    Key::A,
    Key::B,
    Key::C,
    Key::D,
    Key::E,
    Key::F,
    Key::G,
    Key::H,
    Key::I,
    Key::J,
    Key::K,
    Key::L,
    Key::M,
    Key::N,
    Key::O,
    Key::P,
    Key::Q,
    Key::R,
    Key::S,
    Key::T,
    Key::U,
    Key::V,
    Key::W,
    Key::X,
    Key::Y,
    Key::Z,
    Key::F1,
    Key::F2,
    Key::F3,
    Key::F4,
    Key::F5,
    Key::F6,
    Key::F7,
    Key::F8,
    Key::F9,
    Key::F10,
    Key::F11,
    Key::F12,
    Key::F13,
    Key::F14,
    Key::F15,
    Key::F16,
    Key::F17,
    Key::F18,
    Key::F19,
    Key::F20,
];

#[cfg(feature = "serde")]
impl serde::Serialize for KeyBind {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.label())
    }
}

#[cfg(feature = "serde")]
impl<'de> serde::Deserialize<'de> for KeyBind {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = <String as serde::Deserialize>::deserialize(deserializer)?;
        Self::parse(&s).ok_or_else(|| serde::de::Error::custom(format!("invalid keybind: {s}")))
    }
}

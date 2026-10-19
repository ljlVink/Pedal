//! Shell 应用包装器：管理壳子生命周期，业务通过 trait 注入
//!
//! @author sky
//! @author ljlVink

use super::{components::WindowConfig, fonts, platform, titlebar};
use eframe::egui;

/// 业务层只需实现这个 trait
pub trait AppContent {
    /// 在 CentralPanel 内绘制业务 UI
    fn ui(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, theme: &ShellTheme);
    /// 标题栏菜单内容（注入到标题栏左侧）
    fn menu_bar(&mut self, _ui: &mut egui::Ui) {}
    /// 返回当前外壳主题，在每帧绘制前更新；默认沿用启动配置。
    fn shell_theme(&self) -> Option<ShellTheme> {
        None
    }
}

/// 窗口壳子主题配色
#[derive(Clone)]
pub struct ShellTheme {
    /// 窗口 / 标题栏 / 状态栏背景
    pub bg: egui::Color32,
    /// 悬停背景（菜单按钮 hover、widget hover）
    pub bg_hover: egui::Color32,
    /// 激活/按下背景（pin 激活、widget pressed）
    pub bg_active: egui::Color32,
    /// 主要文字
    pub text_primary: egui::Color32,
    /// 次要文字（caption button 图标、菜单 idle）
    pub text_secondary: egui::Color32,
    /// 暗淡文字（inactive pin、行号等）
    pub text_muted: egui::Color32,
    /// 强调色（标题文字、图标高亮）
    pub accent: egui::Color32,
    /// 分隔线颜色
    pub separator: egui::Color32,
    /// 图标字体族（codicon 等）
    pub icon_font: egui::FontFamily,
    /// 标题栏按钮 hover 背景
    pub caption_hover: egui::Color32,
    /// 关闭按钮 hover 背景
    pub close_hover: egui::Color32,
    /// 标题栏高度
    pub title_bar_height: f32,
    /// 浮动窗口专属配置
    pub window: WindowConfig,
}

/// 窗口启动配置
pub struct ShellOptions {
    pub title: String,
    pub size: [f32; 2],
    pub theme: ShellTheme,
}

impl Default for ShellOptions {
    fn default() -> Self {
        Self {
            title: "App".to_owned(),
            size: [1280.0, 800.0],
            theme: ShellTheme {
                bg: egui::Color32::from_rgb(21, 21, 22),
                bg_hover: egui::Color32::from_rgb(46, 46, 49),
                bg_active: egui::Color32::from_rgb(37, 37, 39),
                text_primary: egui::Color32::from_rgb(236, 236, 239),
                text_secondary: egui::Color32::from_rgb(160, 160, 171),
                text_muted: egui::Color32::from_rgb(92, 92, 106),
                accent: egui::Color32::from_rgb(67, 179, 174),
                separator: egui::Color32::from_rgb(46, 46, 48),
                icon_font: egui::FontFamily::Name("codicon".into()),
                caption_hover: egui::Color32::from_rgb(42, 42, 47),
                close_hover: egui::Color32::from_rgb(196, 43, 28),
                title_bar_height: 36.0,
                window: WindowConfig {
                    frame: egui::Frame::NONE,
                    header_height: 32.0,
                    pin_icon: "\u{eb76}",
                    pin_tooltip: "Pin".to_owned(),
                    unpin_tooltip: "Unpin".to_owned(),
                },
            },
        }
    }
}

/// 一行启动窗口壳子
pub fn run<F>(options: ShellOptions, create: F) -> eframe::Result
where
    F: FnOnce(&eframe::CreationContext<'_>) -> Box<dyn AppContent> + 'static,
{
    let viewport = platform::viewport(&options.title, options.size);
    let native = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };
    let title = options.title.clone();
    let theme = options.theme.clone();
    eframe::run_native(
        &options.title,
        native,
        Box::new(move |cc| {
            // 默认使用深色，业务层可在初始化和运行期间切换主题。
            cc.egui_ctx.set_theme(egui::Theme::Dark);
            fonts::setup(&cc.egui_ctx);
            let content = create(cc);
            Ok(Box::new(ShellApp {
                title,
                theme,
                content,
                #[cfg(target_os = "windows")]
                corners_set: false,
                #[cfg(target_os = "macos")]
                traffic_lights_set: false,
                visible: false,
            }))
        }),
    )
}

struct ShellApp {
    /// 窗口标题
    title: String,
    /// 壳子主题配色
    theme: ShellTheme,
    /// 业务层内容
    content: Box<dyn AppContent>,
    /// DWM 圆角是否已设置
    #[cfg(target_os = "windows")]
    corners_set: bool,
    /// macOS 交通灯位置是否已设置
    #[cfg(target_os = "macos")]
    traffic_lights_set: bool,
    /// 窗口是否已显示
    visible: bool,
}

impl eframe::App for ShellApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        if let Some(theme) = self.content.shell_theme() {
            self.theme = theme;
        }
        // 先填充整个窗口背景，避免面板间隙露出原生窗口黑色底色
        ui.painter().rect_filled(ui.max_rect(), 0.0, self.theme.bg);
        if !self.visible {
            self.visible = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
        }

        #[cfg(target_os = "windows")]
        if !self.corners_set {
            platform::enable_rounded_corners(&self.title);
            self.corners_set = true;
        }
        #[cfg(target_os = "macos")]
        if !self.traffic_lights_set {
            platform::set_traffic_lights();
            self.traffic_lights_set = true;
        }
        // 保存完整窗口 rect（titlebar Panel 会缩小 max_rect）
        #[cfg(not(target_os = "macos"))]
        let full_rect = ui.max_rect();
        titlebar::render(ui, &self.title, &self.theme, |ui| {
            self.content.menu_bar(ui);
        });
        #[cfg(not(target_os = "macos"))]
        handle_window_resize(ui, &ctx, full_rect);
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(self.theme.bg))
            .show_inside(ui, |ui| {
                self.content.ui(ui, &ctx, &self.theme);
            });
    }
}

/// 在窗口边缘检测拖拽并触发系统 resize
#[cfg(not(target_os = "macos"))]
fn handle_window_resize(ui: &mut egui::Ui, ctx: &egui::Context, rect: egui::Rect) {
    let maximized = ui.input(|i| i.viewport().maximized.unwrap_or(false));
    if maximized {
        return;
    }
    let margin = 5.0;
    let pointer = ui.input(|i| i.pointer.hover_pos());
    let pos = match pointer {
        Some(p) if rect.contains(p) => p,
        _ => return,
    };
    let left = pos.x - rect.left() < margin;
    let right = rect.right() - pos.x < margin;
    let top = pos.y - rect.top() < margin;
    let bottom = rect.bottom() - pos.y < margin;
    let dir = match (left, right, top, bottom) {
        (true, _, true, _) => Some(egui::ResizeDirection::NorthWest),
        (_, true, true, _) => Some(egui::ResizeDirection::NorthEast),
        (true, _, _, true) => Some(egui::ResizeDirection::SouthWest),
        (_, true, _, true) => Some(egui::ResizeDirection::SouthEast),
        (true, _, _, _) => Some(egui::ResizeDirection::West),
        (_, true, _, _) => Some(egui::ResizeDirection::East),
        (_, _, true, _) => Some(egui::ResizeDirection::North),
        (_, _, _, true) => Some(egui::ResizeDirection::South),
        _ => None,
    };
    if let Some(d) = dir {
        let cursor = match d {
            egui::ResizeDirection::North | egui::ResizeDirection::South => {
                egui::CursorIcon::ResizeVertical
            }
            egui::ResizeDirection::West | egui::ResizeDirection::East => {
                egui::CursorIcon::ResizeHorizontal
            }
            egui::ResizeDirection::NorthWest | egui::ResizeDirection::SouthEast => {
                egui::CursorIcon::ResizeNwSe
            }
            egui::ResizeDirection::NorthEast | egui::ResizeDirection::SouthWest => {
                egui::CursorIcon::ResizeNeSw
            }
        };
        ctx.set_cursor_icon(cursor);
        // 直接读指针状态，不通过 ui.interact()，
        // 避免后渲染的 widget（滚动条等）抢占 drag 事件
        if ui.input(|i| i.pointer.primary_pressed()) {
            ctx.send_viewport_cmd(egui::ViewportCommand::BeginResize(d));
        }
    }
}

//! 应用核心：业务状态 + 业务逻辑调度
//!
//! @author sky
//! @author ljlVink

mod ark;
mod export;
mod open;
mod platform;
mod search;
mod tab;
pub(crate) mod workspace;

pub use crate::ui::confirm::ConfirmAction;

use crate::settings::Settings;
use crate::ui::layout::Layout;
use egui_notify::Toasts;
use egui_shell::components::SettingsFile;
use workspace::Workspace;

pub(crate) use export::ExportingState;

/// 应用核心业务状态
pub struct App {
    /// UI 布局状态
    pub layout: Layout,
    /// 应用设置
    pub settings: Settings,
    /// 通知提示
    pub toasts: Toasts,
    /// 待确认的破坏性动作
    pub pending_confirm: Option<ConfirmAction>,
    /// 工作区状态（Empty / Loading / Loaded）
    pub(crate) workspace: Workspace,
    /// 后台 JAR 导出任务（快照已取，可跨 JAR 切换存活）
    pub(crate) exporting: Option<ExportingState>,
}

impl App {
    pub fn new() -> Self {
        let settings = Settings::load();
        Self {
            layout: Layout::new(&settings),
            settings,
            toasts: Toasts::default(),
            pending_confirm: None,
            workspace: Workspace::Empty,
            exporting: None,
        }
    }

    /// 打开设置对话框
    pub fn open_settings(&mut self) {
        self.layout.settings_panel.open(&self.settings);
    }
}

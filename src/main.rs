//! Pedal - ArkTS（HarmonyOS）反编译器 + 归档查看器
//!
//! @author sky
//! @author ljlVink

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

rust_i18n::i18n!("locales", fallback = "zh-CN");

mod app;
mod appearance;
mod settings;
mod task;
mod ui;

use app::App;
use eframe::egui;

fn main() -> eframe::Result {
    env_logger::Builder::from_default_env()
        .filter_level(log::LevelFilter::Debug)
        .init();
    // 命令行参数：第一个参数为初始打开的归档路径（新窗口模式使用）
    let initial_archive = std::env::args()
        .nth(1)
        .map(std::path::PathBuf::from)
        .filter(|p| p.exists());
    // 从持久化配置中读取语言并设置 locale
    let saved_settings = settings::Settings::load_for_locale();
    rust_i18n::set_locale(saved_settings.language.code());
    let options = appearance::ShellOptions {
        title: "Pedal".to_owned(),
        size: [1280.0, 800.0],
        theme: appearance::ShellTheme {
            bg: appearance::theme::BG_DARK,
            bg_hover: appearance::theme::BG_HOVER,
            bg_active: appearance::theme::BG_LIGHT,
            text_primary: appearance::theme::TEXT_PRIMARY,
            text_secondary: appearance::theme::TEXT_SECONDARY,
            text_muted: appearance::theme::TEXT_MUTED,
            accent: appearance::theme::VERDIGRIS,
            separator: appearance::theme::BORDER,
            icon_font: eframe::egui::FontFamily::Name("codicon".into()),
            caption_hover: appearance::theme::CAPTION_HOVER,
            close_hover: appearance::theme::CLOSE_HOVER,
            title_bar_height: appearance::theme::TITLE_BAR_HEIGHT,
            window: appearance::theme::window_config(),
        },
    };
    appearance::run(options, move |_cc| {
        let mut app = App::new();
        if let Some(path) = &initial_archive {
            app.open_jar(path);
        }
        Box::new(PervApp { app })
    })
}

struct PervApp {
    app: App,
}

impl appearance::AppContent for PervApp {
    fn ui(&mut self, ui: &mut egui::Ui, _ctx: &egui::Context, theme: &appearance::ShellTheme) {
        self.app.render(ui, theme);
    }
    fn menu_bar(&mut self, ui: &mut egui::Ui) {
        ui::menu::menu_bar(ui, &mut self.app);
    }
}

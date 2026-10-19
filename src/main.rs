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
mod cli;
mod settings;
mod task;
mod ui;

use app::App;
use eframe::egui;

fn main() -> eframe::Result {
    // CLI 和 GUI 共用语言配置；无头命令在任何窗口或图形上下文创建前完成。
    let saved_settings = settings::Settings::load_for_locale();
    rust_i18n::set_locale(saved_settings.language.code());
    let initial_archive = match cli::parse(std::env::args_os().skip(1)) {
        Ok(cli::Command::Gui(path)) => path.filter(|p| p.exists()),
        command => std::process::exit(cli::run(command)),
    };
    env_logger::Builder::from_default_env()
        .filter_level(log::LevelFilter::Debug)
        .init();
    appearance::theme::set_mode(saved_settings.theme);
    let options = appearance::ShellOptions {
        title: "Pedal".to_owned(),
        size: [1280.0, 800.0],
        theme: appearance::theme::shell_theme(),
    };
    appearance::run(options, move |cc| {
        let mut app = App::new();
        appearance::theme::apply(&cc.egui_ctx, app.settings.theme);
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
    fn shell_theme(&self) -> Option<appearance::ShellTheme> {
        Some(appearance::theme::shell_theme())
    }

    fn ui(&mut self, ui: &mut egui::Ui, _ctx: &egui::Context, theme: &appearance::ShellTheme) {
        self.app.render(ui, theme);
    }
    fn menu_bar(&mut self, ui: &mut egui::Ui) {
        ui::menu::menu_bar(ui, &mut self.app);
    }
}

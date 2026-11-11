//! 字体加载
//!
//! @author sky
//! @author ljlVink

use super::codicon;
use eframe::egui;

static CJK_FONT: &[u8] = include_bytes!("../fonts/HarmonyOS_Sans_SC_Subset.ttf");

pub fn setup(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "codicon".to_owned(),
        egui::FontData::from_static(include_bytes!("../fonts/codicon.ttf")).into(),
    );
    fonts
        .families
        .entry(codicon::family())
        .or_default()
        .push("codicon".to_owned());
    fonts.font_data.insert(
        "cjk".to_owned(),
        egui::FontData::from_static(CJK_FONT).into(),
    );
    for family in [
        egui::FontFamily::Proportional,
        egui::FontFamily::Monospace,
        codicon::family(),
    ] {
        fonts
            .families
            .entry(family)
            .or_default()
            .push("cjk".to_owned());
    }
    ctx.set_fonts(fonts);
}

//! 字体加载
//!
//! @author sky
//! @author ljlVink

use super::codicon;
use eframe::egui;

/// 内嵌 CJK 字体（HarmonyOS Sans SC 子集：拉丁/标点/CJK 常用区段，由仓库根
/// 目录的完整字库裁剪生成，见 scripts/README 或提交说明）
static CJK_FONT: &[u8] = include_bytes!("../fonts/HarmonyOS_Sans_SC_Subset.ttf");

/// 注册字体到 egui 字体系统（Codicon 图标 + CJK 回退）
pub fn setup(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    // Codicon 图标字体
    fonts.font_data.insert(
        "codicon".to_owned(),
        egui::FontData::from_static(include_bytes!("../fonts/codicon.ttf")).into(),
    );
    fonts
        .families
        .entry(codicon::family())
        .or_default()
        .push("codicon".to_owned());
    // CJK 字体作为 Proportional / Monospace 回退
    fonts.font_data.insert(
        "cjk".to_owned(),
        egui::FontData::from_static(CJK_FONT).into(),
    );
    for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
        fonts
            .families
            .entry(family)
            .or_default()
            .push("cjk".to_owned());
    }
    ctx.set_fonts(fonts);
}

//! Hex 视图桥接层：映射 classview 主题色到 egui-hex-view
//!
//! @author sky
//! @author ljlVink

use crate::appearance::theme;
use rust_i18n::t;

/// classview 主题色映射到 HexTheme
pub fn hex_theme() -> egui_hex_view::HexTheme {
    egui_hex_view::HexTheme {
        addr_color: theme::palette().text_muted,
        addr_hover_color: theme::palette().text_secondary,
        hex_null_color: theme::palette().hex_null,
        hex_printable_color: theme::palette().syn_number,
        hex_control_color: theme::palette().text_muted,
        hex_high_color: theme::palette().accent_orange,
        ascii_color: theme::palette().syn_string,
        ascii_dot_color: theme::palette().text_muted,
        text_primary: theme::palette().text_primary,
        text_secondary: theme::palette().text_secondary,
        text_muted: theme::palette().text_muted,
        accent: theme::palette().verdigris,
        hover_row_bg: theme::palette().hex_row_hover,
        hover_byte_bg: theme::palette().hex_byte_hover,
        selection_bg: theme::palette().hex_selection,
        cursor_bg: theme::palette().hex_cursor,
        separator: theme::palette().hex_separator,
        border: theme::palette().border,
        inspector_bg: theme::palette().bg_darkest,
        header_color: theme::palette().text_muted,
        header_bg: theme::palette().bg_darkest,
        search_bg: theme::verdigris_alpha(25),
        search_current_bg: theme::verdigris_alpha(60),
        labels: egui_hex_view::HexLabels {
            empty: t!("hex.empty").to_string(),
            copy_hex: t!("hex.copy_hex").to_string(),
            copy_ascii: t!("hex.copy_ascii").to_string(),
            copy_offset: t!("hex.copy_offset").to_string(),
            select_all: t!("hex.select_all").to_string(),
            selection: t!("hex.selection").to_string(),
            cursor: t!("hex.cursor").to_string(),
            hover: t!("hex.hover").to_string(),
            bytes: t!("hex.bytes").to_string(),
        },
    }
}

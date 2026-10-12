//! 编辑器 Tab 数据结构
//!
//! @author sky
//! @author ljlVink

use super::view_toggle::ActiveView;
use egui_editor::highlight::{self, Language, Span};
use egui_editor::{EditableLayoutCache, LayoutCache};
use egui_hex_view::HexViewState;
use std::path::PathBuf;

tabookit::class! {
    /// 预处理后的代码视图数据（虚拟滚动用）
    pub struct CodeData {
        /// 语法高亮 span（全文字节偏移，已排序）
        pub spans: Vec<Span>,
        /// 每行在源码中的起始字节偏移
        pub line_starts: Vec<usize>,
    }

    pub fn new(source: &str, spans: Vec<Span>) -> Self {
        let line_starts = highlight::compute_line_starts(source);
        Self { spans, line_starts }
    }

    pub fn line_count(&self) -> usize {
        self.line_starts.len()
    }
}

tabookit::class! {
    /// 编辑器 Tab 数据
    pub struct EditorTab {
        pub title: String,
        /// 归档内条目路径（用于 tab 去重 + widget ID）
        pub entry_path: Option<String>,
        /// 独立文件的磁盘路径（非归档条目，保存时直接写回磁盘，不参与归档 modified 管理）
        pub standalone_path: Option<PathBuf>,
        /// 文本内容（文本视图显示与编辑）
        pub decompiled: String,
        /// 原始字节（只读，用于 hex 视图）
        pub raw_bytes: Vec<u8>,
        pub language: Language,
        pub active_view: ActiveView,
        /// 纯文本文件（可直接编辑）
        pub is_text: bool,
        pub is_modified: bool,
        /// Hex 视图交互状态
        pub hex_state: HexViewState,
        /// 文本视图预处理数据
        pub(super) decompiled_data: CodeData,
        /// 只读视图布局缓存（避免每帧重建 LayoutJob）
        pub layout_cache: Option<LayoutCache>,
        /// 可编辑视图布局缓存（缓存 tree-sitter span + galley）
        pub editable_layout_cache: Option<EditableLayoutCache>,
        /// 视窗模式手动覆盖（None = 自动检测，Some(true) = 强制启用，Some(false) = 强制关闭）
        pub viewport_override: Option<bool>,
        /// 待滚动到的目标行号（0-based），设置后下一帧动画滚动到该行
        pub pending_scroll_to_line: Option<usize>,
        /// ArkTS record tab（源码/反汇编双视图）
        pub is_ark: bool,
        /// 反汇编文本（首次切换视图时懒生成）
        pub(super) disasm_text: String,
        /// 反汇编视图预处理数据
        pub(super) disasm_data: CodeData,
        /// 反汇编视图布局缓存
        pub disasm_layout_cache: Option<LayoutCache>,
    }

    /// 创建文本文件 tab（默认 Decompiled 视图显示文本内容）
    pub fn new_text(
        title: impl Into<String>,
        entry_path: impl Into<String>,
        text: String,
        raw_bytes: Vec<u8>,
        language: Language,
    ) -> Self {
        let dec_data = CodeData::new(&text, highlight::compute_spans(&text, language));
        Self {
            title: title.into(),
            entry_path: Some(entry_path.into()),
            standalone_path: None,
            decompiled: text,
            raw_bytes,
            language,
            active_view: ActiveView::Decompiled,
            is_text: true,
            is_modified: false,
            hex_state: HexViewState::default(),
            decompiled_data: dec_data,
            layout_cache: None,
            editable_layout_cache: None,
            viewport_override: None,
            pending_scroll_to_line: None,
            is_ark: false,
            disasm_text: String::new(),
            disasm_data: CodeData::new("", vec![]),
            disasm_layout_cache: None,
        }
    }

    /// 创建二进制文件 tab（直接显示 hex 视图）
    pub fn new_binary(
        title: impl Into<String>,
        entry_path: impl Into<String>,
        raw_bytes: Vec<u8>,
    ) -> Self {
        Self {
            title: title.into(),
            entry_path: Some(entry_path.into()),
            standalone_path: None,
            decompiled: String::new(),
            raw_bytes,
            language: Language::Plain,
            active_view: ActiveView::Hex,
            is_text: false,
            is_modified: false,
            hex_state: HexViewState::default(),
            decompiled_data: CodeData::new("", vec![]),
            layout_cache: None,
            editable_layout_cache: None,
            viewport_override: None,
            pending_scroll_to_line: None,
            is_ark: false,
            disasm_text: String::new(),
            disasm_data: CodeData::new("", vec![]),
            disasm_layout_cache: None,
        }
    }

    /// 设置反汇编文本（懒生成后调用）
    pub fn set_disasm(&mut self, text: String) {
        self.disasm_data =
            CodeData::new(&text, highlight::compute_spans(&text, Language::Bytecode));
        self.disasm_text = text;
    }

    /// 重建文本视图的高亮数据（文本编辑后调用）
    pub fn refresh_decompiled_data(&mut self) {
        self.decompiled_data = CodeData::new(
            &self.decompiled,
            highlight::compute_spans(&self.decompiled, self.language),
        );
        self.layout_cache = None;
        self.editable_layout_cache = None;
    }

    /// 根据 tab 类型序列化当前编辑内容为字节
    pub fn serialize_bytes(&self) -> Result<Vec<u8>, String> {
        if self.is_text {
            return Ok(self.decompiled.as_bytes().to_vec());
        }
        Ok(self.raw_bytes.clone())
    }

    /// 保存成功后更新 tab 状态：写入新字节、清除修改标记
    pub fn commit_save(&mut self, new_bytes: Vec<u8>) {
        self.raw_bytes = new_bytes;
        self.is_modified = false;
    }
}

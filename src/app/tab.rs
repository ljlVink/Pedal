//! Tab 生命周期：创建、保存、编码检测
//!
//! @author sky
//! @author ljlVink

use super::App;
use crate::ui::editor::EditorTab;
use egui_editor::highlight::Language;
use rust_i18n::t;

impl App {
    /// 保存当前聚焦 tab 的修改
    ///
    /// 独立文件直接写回磁盘；归档文本条目写入内存归档。
    pub fn save_active_tab(&mut self) {
        // ArkTS 虚拟归档只读：暂不支持写回（HAP 重打包为独立特性）
        if self.workspace.loaded().is_some_and(|s| s.ark.is_some())
            && self
                .layout
                .editor
                .focused_tab()
                .is_some_and(|t| t.standalone_path.is_none())
        {
            self.toasts.warning(t!("layout.ark_readonly"));
            return;
        }
        let Some(tab) = self.layout.editor.focused_tab_mut() else {
            return;
        };
        if !tab.is_modified {
            return;
        }
        // 独立文件：直接写回磁盘，不参与归档 modified 管理
        if let Some(disk_path) = tab.standalone_path.clone() {
            let new_bytes = match tab.serialize_bytes() {
                Ok(b) => b,
                Err(e) => {
                    log::error!("Save failed: {}: {e}", disk_path.display());
                    self.toasts.error(t!("editor.save_failed", error = e));
                    return;
                }
            };
            if let Err(e) = std::fs::write(&disk_path, &new_bytes) {
                log::error!("Write failed: {}: {e}", disk_path.display());
                self.toasts.error(t!("layout.write_file_failed", error = e));
                return;
            }
            tab.commit_save(new_bytes);
            log::info!("Saved standalone: {}", disk_path.display());
            return;
        }
        let Some(entry_path) = tab.entry_path.clone() else {
            return;
        };
        // 文本条目：序列化后写入归档内存
        if tab.is_text {
            let new_bytes = tab.serialize_bytes().unwrap();
            tab.commit_save(new_bytes.clone());
            if let Some(jar) = self.workspace.jar_mut() {
                jar.put(&entry_path, new_bytes);
            }
            log::info!("Saved text: {entry_path}");
        }
    }

    /// 从条目路径和字节创建编辑器 tab
    pub(super) fn create_tab(entry_path: &str, bytes: &[u8]) -> EditorTab {
        let file_name = entry_path.rsplit('/').next().unwrap_or(entry_path);
        if Self::is_binary(bytes) {
            EditorTab::new_binary(file_name, entry_path, bytes.to_vec())
        } else {
            let mut text = Self::decode_text(bytes);
            // 编辑器整篇构建 galley，超大文本会冻结 UI：截断显示（原始字节保留，
            // hex 视图/导出不受影响）
            const DISPLAY_CAP: usize = 512 * 1024;
            if text.len() > DISPLAY_CAP {
                let cut = Self::char_boundary(&text, DISPLAY_CAP);
                text = format!(
                    "{}\n/* ... 源码过大已截断显示（共 {} 字节，完整内容可导出查看） ... */\n",
                    &text[..cut],
                    text.len()
                );
            }
            let lang = Language::from_filename(file_name);
            EditorTab::new_text(file_name, entry_path, text, bytes.to_vec(), lang)
        }
    }

    /// 截断到字符边界
    fn char_boundary(s: &str, idx: usize) -> usize {
        let mut i = idx.min(s.len());
        while i > 0 && !s.is_char_boundary(i) {
            i -= 1;
        }
        i
    }

    /// 判断字节内容是否为二进制文件（前 8KB 内含 null 字节即视为二进制）
    fn is_binary(bytes: &[u8]) -> bool {
        bytes[..bytes.len().min(8192)].contains(&0)
    }

    /// 将字节解码为文本（自动检测编码）
    fn decode_text(bytes: &[u8]) -> String {
        let mut detector = chardetng::EncodingDetector::new(chardetng::Iso2022JpDetection::Allow);
        detector.feed(bytes, true);
        let encoding = detector.guess(None, chardetng::Utf8Detection::Allow);
        let (text, _, had_errors) = encoding.decode(bytes);
        if had_errors {
            log::warn!("Text decoding had errors, replacement characters inserted");
        }
        text.into_owned()
    }
}

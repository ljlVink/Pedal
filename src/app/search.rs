//! 搜索索引构建与生命周期管理
//!
//! @author sky
//! @author ljlVink

use super::App;
use super::workspace::Workspace;
use crate::ui::search::index;
use std::sync::Arc;

impl App {
    #[allow(dead_code)]
    /// 标记搜索索引需要重建
    pub(crate) fn invalidate_search_index(&mut self) {
        if let Workspace::Loaded(loaded) = &mut self.workspace {
            loaded.search_index = None;
        }
    }

    /// 构建搜索索引（搜索对话框打开且没有索引时调用）
    ///
    /// ArkTS 工作区直接以内存源码同步构建；普通归档无源码可索引。
    pub(crate) fn rebuild_search_index(&mut self) {
        let Workspace::Loaded(loaded) = &mut self.workspace else {
            return;
        };
        if loaded.search_index.is_some() {
            return;
        }
        let Some(ark) = &loaded.ark else {
            return;
        };
        let entries: Vec<index::IndexEntry> = ark
            .sources
            .iter()
            .map(|(path, source)| {
                let (class_name, package) = index::split_class_path(path);
                index::IndexEntry {
                    entry_path: path.clone(),
                    class_name,
                    package,
                    source: source.clone(),
                }
            })
            .collect();
        loaded.search_index = Some(Arc::new(index::SearchIndex { entries }));
        log::info!("ArkTS search index built from memory sources");
    }
}

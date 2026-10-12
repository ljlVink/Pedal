//! 工作区状态机：用类型约束合法状态组合
//!
//! @author sky
//! @author ljlVink

use super::ark::ArkMeta;
use crate::task::Task;
use crate::ui::search::index::SearchIndex;
use pedal_archive::error::ArchiveError;
use pedal_archive::jar::{JarArchive, LoadProgress};
use std::sync::Arc;

/// 归档工作区生命周期状态
pub(crate) enum Workspace {
    /// 未打开任何文件
    Empty,
    /// 归档正在后台加载
    Loading(LoadingState),
    /// 归档已加载
    Loaded(Box<LoadedState>),
    /// ArkTS（abc/HAP/HSP）正在后台加载并全量反编译
    ArkLoading(super::ark::ArkLoadingState),
}

/// 归档后台加载状态
pub(crate) struct LoadingState {
    /// 文件名（用于进度显示）
    pub name: String,
    /// 加载进度（原子量，后台线程写入）
    pub progress: Arc<LoadProgress>,
    /// 后台加载任务
    pub task: Task<Result<JarArchive, ArchiveError>>,
}

/// 归档已加载后的工作状态
pub(crate) struct LoadedState {
    /// 归档（ArkTS 工作区为虚拟归档）
    pub jar: JarArchive,
    /// 搜索索引（ArkTS 工作区加载完成后由内存源码构建）
    pub search_index: Option<Arc<SearchIndex>>,
    /// ArkTS 工作区元数据（打开的是 abc/HAP/HSP 时存在）
    pub ark: Option<ArkMeta>,
}

impl Workspace {
    /// 获取归档引用（仅 Loaded 状态可用）
    pub fn jar(&self) -> Option<&JarArchive> {
        match self {
            Self::Loaded(s) => Some(&s.jar),
            _ => None,
        }
    }

    /// 获取归档可变引用（仅 Loaded 状态可用）
    pub fn jar_mut(&mut self) -> Option<&mut JarArchive> {
        match self {
            Self::Loaded(s) => Some(&mut s.jar),
            _ => None,
        }
    }

    /// 获取已加载状态引用
    pub fn loaded(&self) -> Option<&LoadedState> {
        match self {
            Self::Loaded(s) => Some(s),
            _ => None,
        }
    }

    /// 获取已加载状态可变引用
    pub fn loaded_mut(&mut self) -> Option<&mut LoadedState> {
        match self {
            Self::Loaded(s) => Some(s),
            _ => None,
        }
    }

    #[allow(dead_code)]
    /// 获取加载中状态引用
    pub fn loading(&self) -> Option<&LoadingState> {
        match self {
            Self::Loading(s) => Some(s),
            _ => None,
        }
    }

    /// 是否有归档处于已加载状态
    pub fn is_loaded(&self) -> bool {
        matches!(self, Self::Loaded(_))
    }
}

impl LoadedState {
    /// 创建新的已加载状态
    pub(crate) fn new(jar: JarArchive) -> Self {
        Self {
            jar,
            search_index: None,
            ark: None,
        }
    }
}

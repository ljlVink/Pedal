//! 文件树模块
//!
//! @author sky

mod build;
mod node;
mod render;

use std::collections::HashSet;

pub use build::build_tree;
pub use node::TreeNode;
pub use render::{TreeActions, TreeRenderConfig, render_tree};

/// 仅替换文件显示名；打开、搜索定位及导出仍使用原始条目路径。
pub fn apply_file_labels(
    nodes: &mut [TreeNode],
    labels: &std::collections::HashMap<String, String>,
) {
    for node in nodes {
        if !node.is_folder
            && let Some(label) = labels.get(&node.path)
        {
            node.label = label.clone();
        }
        apply_file_labels(&mut node.children, labels);
    }
}

/// 展开到指定路径的节点（沿途展开所有祖先），返回是否找到
pub fn reveal(nodes: &mut [TreeNode], target: &str) -> bool {
    for node in nodes {
        if !node.is_folder && node.path == target {
            return true;
        }
        if node.has_children() && reveal(&mut node.children, target) {
            node.expanded = true;
            return true;
        }
    }
    false
}

/// 展开一层：找到最浅的折叠节点层级，展开该层
pub fn expand_one_level(nodes: &mut [TreeNode]) {
    if let Some(depth) = min_collapsed_depth(nodes, 0) {
        set_expanded_at(nodes, depth, 0, true);
    }
}

/// 折叠一层：找到最深的展开叶层级，折叠该层（保留根节点）
pub fn collapse_one_level(nodes: &mut [TreeNode]) {
    if let Some(depth) = max_expanded_leaf_depth(nodes, 0)
        && depth > 0
    {
        set_expanded_at(nodes, depth, 0, false);
    }
}

/// 沿展开路径查找最浅的折叠节点深度
fn min_collapsed_depth(nodes: &[TreeNode], depth: usize) -> Option<usize> {
    let mut result: Option<usize> = None;
    for node in nodes {
        if !node.has_children() {
            continue;
        }
        if !node.expanded {
            result = Some(result.map_or(depth, |d| d.min(depth)));
        } else if let Some(d) = min_collapsed_depth(&node.children, depth + 1) {
            result = Some(result.map_or(d, |r| r.min(d)));
        }
    }
    result
}

/// 查找最深的展开叶节点深度（自身展开但无展开子节点）
fn max_expanded_leaf_depth(nodes: &[TreeNode], depth: usize) -> Option<usize> {
    let mut result: Option<usize> = None;
    for node in nodes {
        if !node.has_children() || !node.expanded {
            continue;
        }
        let deeper = max_expanded_leaf_depth(&node.children, depth + 1);
        if let Some(d) = deeper {
            result = Some(result.map_or(d, |r| r.max(d)));
        } else {
            result = Some(result.map_or(depth, |r| r.max(depth)));
        }
    }
    result
}

/// 沿展开路径设置指定深度所有节点的 expanded 状态
fn set_expanded_at(nodes: &mut [TreeNode], target: usize, depth: usize, expanded: bool) {
    for node in nodes {
        if !node.has_children() {
            continue;
        }
        if depth == target {
            node.expanded = expanded;
        } else if node.expanded {
            set_expanded_at(&mut node.children, target, depth + 1, expanded);
        }
    }
}

/// 过滤索引条目（预构建，可跨线程共享）
pub struct FilterEntry {
    pub path: String,
    pub label_lower: String,
    pub is_folder: bool,
}

/// 后台过滤计算结果
pub struct FilterResult {
    pub visible: HashSet<String>,
    pub first_match: Option<String>,
}

/// 从树构建扁平过滤索引（JAR 加载时调用一次）
pub fn build_filter_index(nodes: &[TreeNode]) -> Vec<FilterEntry> {
    let mut entries = Vec::new();
    collect_entries(nodes, &mut entries);
    entries
}

fn collect_entries(nodes: &[TreeNode], entries: &mut Vec<FilterEntry>) {
    for node in nodes {
        entries.push(FilterEntry {
            path: node.path.clone(),
            label_lower: node.label.to_ascii_lowercase(),
            is_folder: node.is_folder,
        });
        collect_entries(&node.children, entries);
    }
}

/// 在后台线程中计算过滤可见集合
///
/// 匹配的节点及其所有祖先加入 visible 集合，
/// 返回第一个匹配的文件节点路径。
pub fn compute_filter(index: &[FilterEntry], filter: &str) -> FilterResult {
    let mut visible = HashSet::new();
    let mut first_match = None;
    for entry in index {
        if !entry.label_lower.contains(filter) {
            continue;
        }
        visible.insert(entry.path.clone());
        if first_match.is_none() && !entry.is_folder {
            first_match = Some(entry.path.clone());
        }
        // 沿路径分隔符回溯，将所有祖先目录加入可见集合
        visible.insert(String::new());
        let mut pos = 0;
        while let Some(idx) = entry.path[pos..].find('/') {
            pos += idx + 1;
            visible.insert(entry.path[..pos].to_string());
        }
    }
    FilterResult {
        visible,
        first_match,
    }
}

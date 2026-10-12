//! 文件树构建：路径插入、排序、包折叠、内部类嵌套
//!
//! @author sky

use super::node::TreeNode;
use std::collections::HashMap;

/// 从条目路径列表构建层级文件树
pub fn build_tree(root_label: &str, paths: &[&str]) -> Vec<TreeNode> {
    let mut root = TreeNode {
        label: root_label.to_string(),
        path: String::new(),
        is_folder: true,
        expanded: true,
        children: Vec::new(),
    };
    for &entry in paths {
        insert_entry(&mut root, entry);
    }
    sort_tree(&mut root);
    for child in &mut root.children {
        compact_packages(child);
    }
    nest_inner_classes(&mut root);
    vec![root]
}

/// 将一条路径插入树中，自动创建中间目录节点
fn insert_entry(root: &mut TreeNode, path: &str) {
    let parts: Vec<&str> = path.split('/').collect();
    let mut current = root;
    let mut dir_path = String::new();
    for (i, part) in parts.iter().enumerate() {
        let is_last = i == parts.len() - 1;
        if !is_last {
            dir_path.push_str(part);
            dir_path.push('/');
        }
        let existing = current.children.iter().position(|c| c.label == *part);
        if let Some(idx) = existing {
            current = &mut current.children[idx];
        } else {
            current.children.push(TreeNode {
                label: part.to_string(),
                path: if is_last {
                    path.to_string()
                } else {
                    dir_path.clone()
                },
                is_folder: !is_last,
                expanded: false,
                children: Vec::new(),
            });
            let idx = current.children.len() - 1;
            current = &mut current.children[idx];
        }
    }
}

/// 排序比较器：目录在前，同类内按字母序
fn cmp_nodes(a: &TreeNode, b: &TreeNode) -> std::cmp::Ordering {
    b.is_folder
        .cmp(&a.is_folder)
        .then_with(|| a.label.cmp(&b.label))
}

/// 递归排序
fn sort_tree(node: &mut TreeNode) {
    node.children.sort_by(cmp_nodes);
    for child in &mut node.children {
        sort_tree(child);
    }
}

/// 折叠只有单个子目录的中间包（Compact Middle Packages）
///
/// `com/` → `example/` → `server/` 折叠为 `com.example.server/`
fn compact_packages(node: &mut TreeNode) {
    for child in &mut node.children {
        compact_packages(child);
    }
    while node.is_folder && node.children.len() == 1 && node.children[0].is_folder {
        let child = node.children.remove(0);
        node.label = format!("{}.{}", node.label, child.label);
        node.path = child.path;
        node.children = child.children;
    }
}

/// 将内部类（`$`）嵌套到父类节点下
///
/// `Foo.class` / `Foo$Bar.class` / `Foo$Bar$1.class` 变为：
/// ```text
/// Foo.class
///   $Bar
///     $1
/// ```
fn nest_inner_classes(node: &mut TreeNode) {
    for child in &mut node.children {
        if child.is_folder {
            nest_inner_classes(child);
        }
    }
    // 分离 class 文件和其他节点
    let mut class_map: HashMap<String, TreeNode> = HashMap::new();
    let mut others = Vec::new();
    for child in node.children.drain(..) {
        if !child.is_folder && child.label.ends_with(".class") {
            let key = child.label[..child.label.len() - 6].to_string();
            class_map.insert(key, child);
        } else {
            others.push(child);
        }
    }
    // 按名称长度降序处理，确保最深的内部类先嵌套
    let mut keys: Vec<String> = class_map.keys().cloned().collect();
    keys.sort_by_key(|key| std::cmp::Reverse(key.len()));
    for key in keys {
        if !key.contains('$') {
            continue;
        }
        // 沿 $ 链向上查找最近的存在的祖先类
        let mut remaining = key.as_str();
        let mut parent_key = None;
        while let Some(pos) = remaining.rfind('$') {
            remaining = &remaining[..pos];
            if class_map.contains_key(remaining) {
                parent_key = Some(remaining.to_string());
                break;
            }
        }
        if let Some(parent) = parent_key {
            let mut inner = class_map.remove(&key).unwrap();
            inner.label = key[parent.len()..].to_string();
            class_map
                .get_mut(parent.as_str())
                .unwrap()
                .children
                .push(inner);
        }
    }
    // 排序内部类子节点
    for class_node in class_map.values_mut() {
        class_node.children.sort_by(|a, b| a.label.cmp(&b.label));
    }
    // 重组：其他节点 + class 文件，按目录优先 + 字母序
    node.children = others;
    node.children.extend(class_map.into_values());
    node.children.sort_by(cmp_nodes);
}

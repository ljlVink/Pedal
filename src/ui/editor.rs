//! 编辑器模块
//!
//! @author sky
//! @author ljlVink

mod area;
mod formatting;
mod image_preview;
mod render;
mod resource_tree;
pub mod style;
mod tab;
pub mod view_toggle;
mod viewer;

pub use area::EditorArea;
pub(crate) use resource_tree::ResourceTreeData;
pub use tab::EditorTab;
pub use viewer::TabAction;

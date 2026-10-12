//! 编辑器模块
//!
//! @author sky
//! @author ljlVink

mod area;
mod render;
pub mod style;
mod tab;
pub mod view_toggle;
mod viewer;

pub use area::EditorArea;
pub use tab::EditorTab;
pub use viewer::TabAction;

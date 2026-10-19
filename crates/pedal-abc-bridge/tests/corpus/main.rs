//! 大型本地语料测试入口；默认忽略，显式运行时缺失语料即失败。
//! @author ljlVink

use std::path::{Path, PathBuf};

mod containers;
mod download;
mod prototypes;

fn corpus_path(relative_path: &str) -> PathBuf {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../test_examples")
        .join(relative_path);
    assert!(
        path.is_file(),
        "optional corpus file missing: {}. Supply this file or run the default tests without --ignored; see tests/README.md",
        path.display()
    );
    path
}

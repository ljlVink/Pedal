//! 读取随仓库提交的小型测试样本；缺失样本属于测试失败。
//! @author ljlVink

use std::path::Path;

pub fn read_fixture(relative_path: &str) -> Vec<u8> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(relative_path);
    std::fs::read(&path).unwrap_or_else(|error| {
        panic!(
            "failed to read required fixture {}: {error}. Restore the tracked tests/fixtures files; see tests/README.md",
            path.display()
        )
    })
}

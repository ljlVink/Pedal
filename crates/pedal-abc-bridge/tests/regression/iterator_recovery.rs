//! 编译器 for-of 小样本的多出口异常恢复回归。
//! @author ljlVink

use pedal_abc_bridge::{decompile, file::AbcFile};

use super::common;

const FIXTURES: &[(&str, &str)] = &[
    ("debug", "iterator_recovery/debug.abc"),
    ("release", "iterator_recovery/release.abc"),
];

#[test]
fn compiler_iterator_methods_keep_code_and_result_checks() {
    for (mode, path) in FIXTURES {
        let bytes = common::read_fixture(path);
        let file = AbcFile::parse(bytes).unwrap();
        let record = file
            .classes
            .iter()
            .find(|r| r.name.contains("IteratorRecovery"))
            .unwrap();
        let source = decompile::decompile_record(&file, record).unwrap().source;
        for operation in [
            "findQueueNameByRequestId(",
            "breakAfterFirst(",
            "continueThenReturn(",
            ".next",
            ".return",
            "TypeError",
            "\"function\"",
            "\"object\"",
            "catch (",
        ] {
            assert!(
                source.contains(operation),
                "{mode}: lost {operation}\n{source}"
            );
        }
        assert!(
            !source.contains("Conservative fallback"),
            "{mode}:\n{source}"
        );
        assert!(
            !source.contains("lex0_"),
            "{mode}: module TAG in catch must resolve"
        );
    }
}

//! 编译器 for-of 与真实下载队列的多出口异常恢复回归。
//! @author ljlVink

use pedal_abc_bridge::{decompile, file::AbcFile, hap::HapArchive};

mod common;

const FIXTURES: &[(&str, &str)] = &[
    ("debug", "iterator_recovery/debug.abc"),
    ("release", "iterator_recovery/release.abc"),
];

#[test]
fn compiler_iterator_methods_keep_code_and_result_checks() {
    for (mode, path) in FIXTURES {
        let Some(bytes) = common::read_fixture(path) else {
            continue;
        };
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

#[test]
fn actual_find_queue_name_method_recovers_from_its_exception_table() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../test_examples/AppGallery/@hw-ae-business+download-install-default.hsp");
    if !path.exists() {
        return;
    }
    let archive = HapArchive::parse(std::fs::read(path).unwrap()).unwrap();
    let file = AbcFile::parse(archive.read_entry(archive.main_abc().unwrap()).unwrap()).unwrap();
    let record = file
        .classes
        .iter()
        .find(|r| r.name.contains("networkwrapper2/queue/QueueManager&"))
        .unwrap();
    let source = decompile::decompile_record(&file, record).unwrap().source;
    let begin = source.find("findQueueNameByRequestId(").unwrap();
    let rest = &source[begin..];
    let body = &rest[..rest.find("\n    }").unwrap()];
    assert!(!body.contains("Conservative fallback"), "{body}");
    for operation in [
        ".entries()",
        ".next",
        ".return",
        ".requestId",
        "TypeError",
        "Logger",
        "SafeJsonUtils",
    ] {
        assert!(body.contains(operation), "missing {operation}:\n{body}");
    }
    assert!(!body.contains("lex0_"), "catch TAG must use module binding");
}

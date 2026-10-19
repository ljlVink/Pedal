//! 真实下载包的迭代器、异常与词法绑定回归。
//! @author ljlVink

use pedal_abc_bridge::{decompile, file::AbcFile, hap::HapArchive};

#[test]
#[ignore = "requires optional test_examples corpus; see tests/README.md"]
fn actual_find_queue_name_method_recovers_from_its_exception_table() {
    let path = super::corpus_path("AppGallery/@hw-ae-business+download-install-default.hsp");
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

#[test]
#[ignore = "requires optional test_examples corpus; see tests/README.md"]
fn optional_download_corpus_recovers_shared_join_methods() {
    let root = super::corpus_path("AppGallery/@hw-ae-business+download-install-default.hsp");
    let archive = pedal_abc_bridge::hap::HapArchive::parse(std::fs::read(root).unwrap()).unwrap();
    let file = AbcFile::parse(archive.read_entry(archive.main_abc().unwrap()).unwrap()).unwrap();
    for record_name in ["IconTimestampStore", "DownloadTaskAssembler"] {
        let record = file
            .classes
            .iter()
            .find(|r| r.name.contains(record_name))
            .expect("corpus record");
        let source = decompile::decompile_record(&file, record).unwrap().source;
        for method_name in if record_name == "IconTimestampStore" {
            &["add(a0, a1)"][..]
        } else {
            &["ownerInfoToList(a0)", "addHapFileDownloadTask(a0, a1)"][..]
        } {
            let start = source
                .find(method_name)
                .unwrap_or_else(|| panic!("missing {method_name}"));
            let body = &source[start
                ..source[start..]
                    .find("\n    }")
                    .map(|end| start + end)
                    .unwrap_or(source.len())];
            assert!(body.contains("try {"), "{method_name}: {body}");
            assert!(
                !body.contains("Conservative fallback"),
                "{method_name}: {body}"
            );
        }
    }
}

#[test]
#[ignore = "requires optional test_examples corpus; see tests/README.md"]
fn module_constants_and_member_captures_share_declarations_after_frame_changes() {
    let path = super::corpus_path("AppGallery/@hw-ae-business+download-install-default.hsp");
    let archive = HapArchive::parse(std::fs::read(path).unwrap()).unwrap();
    let file = AbcFile::parse(archive.read_entry(archive.main_abc().unwrap()).unwrap()).unwrap();
    for (record_name, tag) in [
        ("AtomicServiceUtil", "DS-AtomicServiceUtil"),
        (
            "AtomicServiceRouteStep",
            "AGDownload-TB-AtomicServiceRouteStep",
        ),
        ("InstallResultBiReportUtil", "InstallResultBiReportUtil"),
    ] {
        let record = file
            .classes
            .iter()
            .find(|record| record.name.ends_with(&format!("/{record_name}&;")))
            .unwrap_or_else(|| panic!("missing corpus record {record_name}"));
        let source = decompile::decompile_record(&file, record).unwrap().source;
        let assignment = source
            .lines()
            .find(|line| line.contains(&format!("= \"{tag}\";")))
            .unwrap_or_else(|| panic!("missing module constant {tag}"));
        let name = assignment
            .trim()
            .trim_start_matches("let ")
            .split(" = ")
            .next()
            .unwrap();
        assert!(
            source.contains(&format!("let {name}")),
            "{record_name}: declaration missing"
        );
        assert!(
            source.matches(name).count() > 2,
            "{record_name}: members must use the module binding"
        );
        for reference in source
            .split(|c: char| !c.is_ascii_alphanumeric() && c != '_')
            .filter(|name| name.starts_with("captured_"))
        {
            assert!(
                source.contains(&format!("let {reference};"))
                    || source.contains(&format!("let {reference} =")),
                "{record_name}: undeclared capture {reference}"
            );
        }
    }
}

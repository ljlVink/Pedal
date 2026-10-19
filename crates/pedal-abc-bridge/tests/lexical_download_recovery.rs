//! 下载包真实语料的模块常量与闭包绑定回归；未提供语料时跳过。
//! @author ljlVink

use pedal_abc_bridge::{decompile, file::AbcFile, hap::HapArchive};

#[test]
fn module_constants_and_member_captures_share_declarations_after_frame_changes() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../test_examples/AppGallery/@hw-ae-business+download-install-default.hsp");
    if !path.exists() {
        return;
    }
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

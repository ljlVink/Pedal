//! 正式 CLI 的进程级回归：合成多 ABC 包，不依赖本地语料或图形环境。
//!
//! @author ljlVink

use serde_json::Value;
use std::collections::HashSet;
use std::fs;
use std::io::{Cursor, Write};
use std::path::Path;
use std::process::{Command, Output};
use tempfile::TempDir;
use zip::{ZipWriter, write::SimpleFileOptions};

fn pedal() -> Command {
    Command::new(env!("CARGO_BIN_EXE_pedal"))
}

fn analyze(input: &Path, output: &Path) -> Output {
    pedal()
        .arg("analyze")
        .arg(input)
        .arg("-o")
        .arg(output)
        .output()
        .unwrap()
}

fn assert_exit(output: &Output, code: i32) {
    assert_eq!(
        output.status.code(),
        Some(code),
        "stdout={}\nstderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn manifest(output: &Path) -> Value {
    serde_json::from_slice(&fs::read(output.join("manifest.json")).unwrap()).unwrap()
}

fn package(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in entries {
        zip.start_file(*name, SimpleFileOptions::default()).unwrap();
        zip.write_all(bytes).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

fn put_u32(bytes: &mut [u8], at: usize, value: usize) {
    bytes[at..at + 4].copy_from_slice(&(value as u32).to_le_bytes());
}

fn string(bytes: &mut Vec<u8>, value: &str) {
    let mut header = (value.encode_utf16().count() << 1) | usize::from(value.is_ascii());
    while header >= 128 {
        bytes.push((header as u8 & 0x7f) | 0x80);
        header >>= 7;
    }
    bytes.push(header as u8);
    bytes.extend_from_slice(value.as_bytes());
    bytes.push(0);
}

/// 每个 record 包含一个返回 7 的真实字节码函数，验证导出确实经过反编译。
fn abc(records: &[&str]) -> Vec<u8> {
    let region = 64 + records.len() * 4;
    let type_table = region + 40;
    let mut bytes = vec![0; type_table + 4];
    bytes[..8].copy_from_slice(b"PANDA\0\0\0");
    bytes[12] = 12;
    put_u32(&mut bytes, 28, records.len());
    put_u32(&mut bytes, 32, 64);
    put_u32(&mut bytes, 52, 1);
    put_u32(&mut bytes, 56, region);
    put_u32(&mut bytes, region, type_table + 4);
    put_u32(&mut bytes, region + 8, 1);
    put_u32(&mut bytes, region + 12, type_table);
    put_u32(&mut bytes, type_table, 12); // any

    for (index, record) in records.iter().enumerate() {
        let class_off = bytes.len();
        put_u32(&mut bytes, 64 + index * 4, class_off);
        string(&mut bytes, record);
        bytes.extend_from_slice(&[0, 0, 0, 0, 0, 0, 1, 0]); // reserved, flags, fields, methods, end
        bytes.extend_from_slice(&[0, 0, 0xff, 0xff]); // class index, absent proto
        let name_pointer = bytes.len();
        bytes.extend_from_slice(&[0; 4]);
        bytes.extend_from_slice(&[0x80, 0x80, 0x04, 0x01]); // Function kind, CODE tag
        let code_pointer = bytes.len();
        bytes.extend_from_slice(&[0; 4]);
        bytes.push(0); // method end
        let name_off = bytes.len();
        put_u32(&mut bytes, name_pointer, name_off);
        string(&mut bytes, "probe");
        let code_off = bytes.len();
        put_u32(&mut bytes, code_pointer, code_off);
        bytes.extend_from_slice(&[0, 3, 6, 0, 0x62, 7, 0, 0, 0, 0x64]); // ldai 7; return
    }
    let len = bytes.len();
    put_u32(&mut bytes, 16, len);
    put_u32(&mut bytes, region + 4, len);
    bytes
}

fn assert_sources(output: &Path, report: &Value, expected: usize) {
    let sources = report["sources"].as_array().unwrap();
    assert_eq!(sources.len(), expected);
    let mut unique = HashSet::new();
    for source in sources {
        let relative = source["file"].as_str().unwrap();
        assert!(
            unique.insert(relative.to_lowercase()),
            "colliding output: {relative}"
        );
        assert!(
            Path::new(relative)
                .components()
                .all(|part| matches!(part, std::path::Component::Normal(_)))
        );
        let file = output.join(relative);
        assert!(
            file.canonicalize()
                .unwrap()
                .starts_with(output.canonicalize().unwrap())
        );
        let text = fs::read_to_string(file).unwrap();
        assert!(text.contains("return 7;"), "{text}");
    }
}

#[test]
fn help_version_and_usage_errors_are_available_without_a_window() {
    for args in [vec!["--help"], vec!["analyze", "--help"]] {
        let result = pedal().args(args).output().unwrap();
        assert_exit(&result, 0);
        assert!(String::from_utf8_lossy(&result.stdout).contains("pedal analyze"));
        assert!(result.stderr.is_empty());
    }
    let result = pedal().arg("--version").output().unwrap();
    assert_exit(&result, 0);
    assert!(String::from_utf8_lossy(&result.stdout).contains(env!("CARGO_PKG_VERSION")));
    for args in [
        vec!["analyze"],
        vec!["analyze", "input.hap"],
        vec!["--unknown"],
    ] {
        let result = pedal().args(args).output().unwrap();
        assert_exit(&result, 2);
        assert!(!result.stderr.is_empty());
        assert!(result.stdout.is_empty());
    }
}

#[test]
fn exports_every_abc_and_keeps_identical_record_names_separate() {
    let temp = TempDir::new().unwrap();
    let bytes = abc(&["Lsample/Main;", "Lsample/Main;"]);
    let input = temp.path().join("app.hap");
    fs::write(
        &input,
        package(&[
            ("ets/modules.abc", &bytes),
            ("feature/modules.abc", &bytes),
            ("../outside.abc", &bytes),
            ("module.json", b"{}"),
        ]),
    )
    .unwrap();
    let out = temp.path().join("out");
    let result = analyze(&input, &out);
    assert_exit(&result, 0);
    let report = manifest(&out);
    assert_eq!(report["abc_files"], 3);
    assert_eq!(report["failures"], serde_json::json!([]));
    assert_sources(&out, &report, 6);
    assert!(!temp.path().join("outside.abc").exists());
    assert!(String::from_utf8_lossy(&result.stdout).contains("manifest.json"));
}

#[test]
fn standalone_unicode_paths_and_unsafe_record_names_export_portably() {
    let temp = TempDir::new().unwrap();
    let input = temp.path().join("中文 输入.ABC");
    let long_name = "长名称".repeat(60);
    let records = [
        "CON",
        "..",
        "../escape",
        "..\\escape",
        "C:\\escape",
        "A",
        "a",
        &long_name,
    ];
    fs::write(&input, abc(&records)).unwrap();
    let out = temp.path().join("中文 输出");
    fs::create_dir(&out).unwrap(); // 允许已存在的空目录
    assert_exit(&analyze(&input, &out), 0);
    let report = manifest(&out);
    assert_eq!(report["abc_files"], 1);
    assert_sources(&out, &report, records.len());
    assert_eq!(report["sources"][7]["record"], long_name);
}

#[test]
fn broken_abc_does_not_prevent_later_exports_and_sets_failure_exit_code() {
    let temp = TempDir::new().unwrap();
    let input = temp.path().join("partial.hsp");
    fs::write(
        &input,
        package(&[
            ("bad.abc", b"invalid"),
            ("ets/modules.abc", &abc(&["Good"])),
        ]),
    )
    .unwrap();
    let out = temp.path().join("out");
    let result = analyze(&input, &out);
    assert_exit(&result, 1);
    let report = manifest(&out);
    assert_eq!(report["abc_files"], 2);
    assert_eq!(report["failures"].as_array().unwrap().len(), 1);
    assert_eq!(report["failures"][0]["abc"], "bad.abc");
    assert!(report["failures"][0]["record"].is_null());
    assert_sources(&out, &report, 1);
}

#[test]
fn nested_packages_are_traversed_alongside_top_level_abcs() {
    let temp = TempDir::new().unwrap();
    let bytes = abc(&["Nested"]);
    let nested = package(&[("ets/MODULES.ABC", &bytes)]);
    let input = temp.path().join("bundle.APP");
    fs::write(
        &input,
        package(&[("entry.HAP", &nested), ("top.abc", &bytes)]),
    )
    .unwrap();
    let out = temp.path().join("out");
    assert_exit(&analyze(&input, &out), 0);
    let report = manifest(&out);
    assert_eq!(report["abc_files"], 2);
    assert_eq!(report["sources"][0]["abc"], "entry.HAP!/ets/MODULES.ABC");
    assert_sources(&out, &report, 2);
}

#[test]
fn empty_and_malformed_packages_produce_failure_manifests() {
    let temp = TempDir::new().unwrap();
    for (index, bytes) in [package(&[("module.json", b"{}")]), b"invalid zip".to_vec()]
        .iter()
        .enumerate()
    {
        let input = temp.path().join(format!("{index}.hap"));
        fs::write(&input, bytes).unwrap();
        let out = temp.path().join(format!("out{index}"));
        assert_exit(&analyze(&input, &out), 1);
        let report = manifest(&out);
        assert_eq!(report["abc_files"], 0);
        assert_eq!(report["failures"].as_array().unwrap().len(), 1);
    }
}

#[test]
fn existing_results_are_preserved_and_io_failures_return_nonzero() {
    let temp = TempDir::new().unwrap();
    let input = temp.path().join("input.abc");
    fs::write(&input, abc(&["Good"])).unwrap();
    let out = temp.path().join("out");
    assert_exit(&analyze(&input, &out), 0);
    let original = fs::read(out.join("manifest.json")).unwrap();
    assert_exit(&analyze(&input, &out), 1);
    assert_eq!(fs::read(out.join("manifest.json")).unwrap(), original);
    assert_exit(&analyze(&input, &input), 1); // 输出指向已有文件
    assert_exit(
        &analyze(
            &temp.path().join("missing.abc"),
            &temp.path().join("missing-out"),
        ),
        1,
    );
    assert!(!temp.path().join("missing-out").exists());
}

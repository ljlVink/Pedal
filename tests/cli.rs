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
    let records: Vec<_> = records
        .iter()
        .map(|record| (*record, &[0x62, 7, 0, 0, 0, 0x64][..]))
        .collect();
    abc_with_code(&records)
}

fn abc_with_code(records: &[(&str, &[u8])]) -> Vec<u8> {
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

    for (index, (record, code)) in records.iter().enumerate() {
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
        assert!(code.len() < 128); // fixture code length uses one-byte ULEB128
        bytes.extend_from_slice(&[0, 3, code.len() as u8, 0]);
        bytes.extend_from_slice(code);
    }
    if records.is_empty() {
        bytes.push(0); // 索引区域必须有非空范围，即使没有 record。
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

fn assert_jobs_exports(records: &[(&str, &[u8])], failed: &[usize]) {
    let temp = TempDir::new().unwrap();
    let input = temp.path().join("records.abc");
    fs::write(&input, abc_with_code(records)).unwrap();
    let serial = temp.path().join("serial");
    let parallel = temp.path().join("parallel");
    for (output, flag, jobs) in [(&serial, "-j", "1"), (&parallel, "--jobs", "4")] {
        let result = pedal()
            .arg("analyze")
            .arg(&input)
            .arg("-o")
            .arg(output)
            .args([flag, jobs])
            .output()
            .unwrap();
        assert_exit(&result, if failed.is_empty() { 0 } else { 1 });
    }

    let report = manifest(&serial);
    assert_eq!(report, manifest(&parallel));
    assert_eq!(report["abc_files"], 1);
    let sources = report["sources"].as_array().unwrap();
    let failures = report["failures"].as_array().unwrap();
    assert_eq!(sources.len(), records.len() - failed.len());
    assert_eq!(failures.len(), failed.len());
    let expected_sources: Vec<_> = records
        .iter()
        .enumerate()
        .filter(|(index, _)| !failed.contains(index))
        .collect();
    let mut files = HashSet::new();
    let mut offsets = HashSet::new();
    for (source, (index, (record, _))) in sources.iter().zip(expected_sources) {
        assert_eq!(source["record"], *record);
        assert_eq!(source["abc"], "records.abc");
        assert!(offsets.insert(source["offset"].as_u64().unwrap()));
        let relative = source["file"].as_str().unwrap();
        assert!(
            files.insert(relative.to_lowercase()),
            "duplicate export: {relative}"
        );
        assert!(
            Path::new(relative)
                .file_name()
                .unwrap()
                .to_str()
                .unwrap()
                .starts_with(&format!("{:04}-", index + 1)),
            "record index changed: {relative}"
        );
        let bytes = fs::read(serial.join(relative)).unwrap();
        assert!(
            String::from_utf8_lossy(&bytes).contains(&format!("return {};", records[index].1[1]))
        );
        assert_eq!(bytes, fs::read(parallel.join(relative)).unwrap());
    }
    assert!(
        sources.windows(2).all(|pair| {
            pair[0]["offset"].as_u64().unwrap() < pair[1]["offset"].as_u64().unwrap()
        })
    );
    for (failure, index) in failures.iter().zip(failed) {
        assert_eq!(failure["record"], records[*index].0);
        assert_eq!(failure["abc"], "records.abc");
    }

    // Every on-disk source must be represented once, including when some records fail.
    for output in [&serial, &parallel] {
        let entries: Vec<_> = fs::read_dir(output).unwrap().map(Result::unwrap).collect();
        assert_eq!(entries.len(), 2); // manifest and the single ABC directory
        let directory = entries.iter().find(|entry| entry.path().is_dir()).unwrap();
        let actual: HashSet<_> = fs::read_dir(directory.path())
            .unwrap()
            .map(|entry| {
                let path = entry.unwrap().path();
                assert!(path.is_file());
                path.strip_prefix(output)
                    .unwrap()
                    .to_str()
                    .unwrap()
                    .replace('\\', "/")
                    .to_lowercase()
            })
            .collect();
        assert_eq!(actual, files);
    }
}

#[test]
fn serial_and_parallel_jobs_export_every_record_in_original_order() {
    let records: Vec<_> = [
        "Lsample/Main;",
        "Lsample/Main;",
        "A",
        "a",
        "a/b",
        "a\\b",
        "中文记录",
        "日本語",
        "CON",
        "../escape",
        "Last",
        "最后",
    ]
    .iter()
    .enumerate()
    .map(|(index, name)| (*name, vec![0x62, index as u8, 0, 0, 0, 0x64]))
    .collect();
    let records: Vec<_> = records
        .iter()
        .map(|(name, code)| (*name, code.as_slice()))
        .collect();
    assert_jobs_exports(&records, &[]);
}

#[test]
fn serial_and_parallel_jobs_keep_record_failures_ordered_without_cancellation() {
    let failed = [0, 3, 5, 9];
    let records: Vec<_> = [
        "坏记录",
        "Same",
        "Same",
        "Bad",
        "A",
        "Bad",
        "a",
        "a/b",
        "a\\b",
        "最后失败",
        "AfterFailure",
        "最后成功",
    ]
    .iter()
    .enumerate()
    .map(|(index, name)| {
        let code = if failed.contains(&index) {
            vec![0xfe] // incomplete throw prefix: invalid opcode, valid ABC container
        } else {
            vec![0x62, index as u8, 0, 0, 0, 0x64]
        };
        (*name, code)
    })
    .collect();
    let records: Vec<_> = records
        .iter()
        .map(|(name, code)| (*name, code.as_slice()))
        .collect();
    assert_jobs_exports(&records, &failed);
}

#[test]
fn jobs_exceeding_record_count_handle_single_and_empty_abcs() {
    assert_jobs_exports(&[("Only", &[0x62, 7, 0, 0, 0, 0x64])], &[]);
    assert_jobs_exports(&[], &[]);
}

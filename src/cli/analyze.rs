//! 无头遍历全部 ABC，逐 record 导出伪 ArkTS，并记录可供脚本读取的清单。
//!
//! @author ljlVink

use pedal_abc_bridge::{decompile, file::AbcFile, hap::HapArchive};
use rust_i18n::t;
use serde::Serialize;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::thread;

/// 嵌套应用包的递归上限，避免异常容器耗尽调用栈。
const MAX_PACKAGE_DEPTH: usize = 16;

#[derive(Serialize)]
pub(super) struct Source {
    abc: String,
    record: String,
    offset: u32,
    /// 相对输出目录的路径，统一使用 / 分隔。
    file: String,
}

#[derive(Serialize)]
pub(super) struct Failure {
    abc: String,
    record: Option<String>,
    error: String,
}

#[derive(Default, Serialize)]
pub(super) struct Report {
    input: String,
    pub abc_files: usize,
    pub sources: Vec<Source>,
    pub failures: Vec<Failure>,
}

tabookit::class! {
    struct Exporter {
        output: PathBuf,
        jobs: usize,
        report: Report,
    }

    fn fail(&mut self, abc: &str, record: Option<&str>, error: String) {
        let target = record.map_or_else(|| abc.to_string(), |record| format!("{abc}: {record}"));
        eprintln!("{}", t!("cli.entry_failed", path = target, error = &error));
        self.report.failures.push(Failure {
            abc: abc.to_string(),
            record: record.map(str::to_string),
            error,
        });
    }

    fn abc(&mut self, name: &str, bytes: Result<Vec<u8>, String>) {
        self.report.abc_files += 1;
        eprintln!("{}", t!("cli.processing", count = self.report.abc_files, path = name));
        let file = match bytes.and_then(|bytes| AbcFile::parse(bytes).map_err(|e| e.to_string())) {
            Ok(file) => file,
            Err(error) => {
                self.fail(name, None, error);
                return;
            }
        };
        // 序号隔离同名、大小写差异和转义后重名；归档内路径从不直接 join 到磁盘。
        let directory = format!("{:04}-{}", self.report.abc_files, safe_name(name));
        let out_dir = self.output.join(&directory);
        if let Err(error) = fs::create_dir(&out_dir) {
            self.fail(name, None, io_error(&out_dir, error));
            return;
        }
        let workers = self.jobs.min(file.classes.len());
        if workers <= 1 {
            for (index, record) in file.classes.iter().enumerate() {
                match export_record(&file, index, name, &self.output, &directory) {
                    Ok(source) => self.report.sources.push(source),
                    Err(error) => self.fail(name, Some(&record.name), error),
                }
            }
            return;
        }

        let mut results: Vec<Option<Result<Source, String>>> =
            (0..file.classes.len()).map(|_| None).collect();
        {
            // 队列只分配互不重叠的结果槽；解编译及写盘均在锁外完成。
            let queue = Mutex::new(results.iter_mut().enumerate());
            let output = &self.output;
            let worker = || loop {
                let next = queue.lock().expect("record export queue poisoned").next();
                let Some((index, slot)) = next else {
                    break;
                };
                *slot = Some(export_record(&file, index, name, output, &directory));
            };
            thread::scope(|scope| {
                for _ in 1..workers {
                    scope.spawn(&worker);
                }
                worker();
            });
        }
        // 完成顺序不影响清单或错误输出；编号始终使用原始 record 序号。
        for (record, result) in file.classes.iter().zip(results) {
            match result.expect("record export worker left a result empty") {
                Ok(source) => self.report.sources.push(source),
                Err(error) => self.fail(name, Some(&record.name), error),
            }
        }
    }

    fn package(&mut self, name: &str, bytes: Vec<u8>, depth: usize) {
        if depth >= MAX_PACKAGE_DEPTH {
            self.fail(name, None, t!("cli.package_depth", limit = MAX_PACKAGE_DEPTH).into_owned());
            return;
        }
        let archive = match HapArchive::parse(bytes) {
            Ok(archive) => archive,
            Err(error) => {
                let target = if name.is_empty() { self.report.input.clone() } else { name.to_string() };
                self.fail(&target, None, error.to_string());
                return;
            }
        };
        for entry in &archive.entries {
            let nested = HapArchive::is_package(&entry.path.to_ascii_lowercase());
            if !entry.is_abc && !nested {
                continue;
            }
            let path = if name.is_empty() {
                entry.path.clone()
            } else {
                format!("{name}!/{}", entry.path)
            };
            let bytes = archive.read_entry(&entry.path).map_err(|e| e.to_string());
            if entry.is_abc {
                self.abc(&path, bytes);
            } else {
                match bytes {
                    Ok(bytes) => self.package(&path, bytes, depth + 1),
                    Err(error) => self.fail(&path, None, error),
                }
            }
        }
    }
}

pub(super) fn run(input: &Path, output: &Path, jobs: usize) -> Result<Report, String> {
    let extension = input
        .extension()
        .unwrap_or_default()
        .to_string_lossy()
        .to_ascii_lowercase();
    if !matches!(extension.as_str(), "abc" | "hap" | "hsp" | "har" | "app") {
        return Err(t!("cli.unsupported_input", path = input.display()).into_owned());
    }
    let bytes = fs::read(input).map_err(|e| io_error(input, e))?;
    fs::create_dir_all(output).map_err(|e| io_error(output, e))?;
    // 拒绝覆盖已有结果；也避免沿用已有子目录/符号链接把归档内容写到目录外。
    if fs::read_dir(output)
        .map_err(|e| io_error(output, e))?
        .next()
        .transpose()
        .map_err(|e| io_error(output, e))?
        .is_some()
    {
        return Err(t!("cli.output_not_empty", path = output.display()).into_owned());
    }
    let mut exporter = Exporter {
        output: output.to_path_buf(),
        jobs,
        report: Report {
            input: input.to_string_lossy().into_owned(),
            ..Report::default()
        },
    };
    if extension == "abc" {
        exporter.abc(
            &input.file_name().unwrap_or_default().to_string_lossy(),
            Ok(bytes),
        );
    } else {
        exporter.package("", bytes, 0);
    }
    if exporter.report.abc_files == 0 && exporter.report.failures.is_empty() {
        exporter.fail(
            &input.to_string_lossy(),
            None,
            t!("cli.no_abc").into_owned(),
        );
    }
    let manifest = output.join("manifest.json");
    let json = serde_json::to_vec_pretty(&exporter.report).map_err(|e| io_error(&manifest, e))?;
    write_new(&manifest, &json)?;
    Ok(exporter.report)
}

fn export_record(
    file: &AbcFile,
    index: usize,
    name: &str,
    output: &Path,
    directory: &str,
) -> Result<Source, String> {
    let record = &file.classes[index];
    let relative = format!(
        "{directory}/{:04}-{}.ets",
        index + 1,
        safe_name(&record.name)
    );
    let path = output.join(&relative);
    let result = decompile::decompile_record(file, record).map_err(|e| e.to_string())?;
    let source = if result.source.trim().is_empty() {
        // 无方法体的声明及外部类型也导出文件，清单保留原始名称。
        format!("// {}\n", t!("cli.empty_record"))
    } else {
        result.source
    };
    write_new(&path, source.as_bytes())?;
    Ok(Source {
        abc: name.to_string(),
        record: record.name.clone(),
        offset: record.offset,
        file: relative,
    })
}

/// 数字前缀由调用方提供；限制单个组件长度，替换跨平台不安全字符。
fn safe_name(name: &str) -> String {
    let mut safe = String::new();
    for c in name.chars() {
        let c = if c.is_control() || "<>:\"/\\|?*".contains(c) {
            '_'
        } else {
            c
        };
        if safe.len() + c.len_utf8() > 96 {
            break;
        }
        safe.push(c);
    }
    safe.trim_end_matches([' ', '.']).to_string()
}

fn io_error(path: &Path, error: impl std::fmt::Display) -> String {
    t!("cli.io_error", path = path.display(), error = error).into_owned()
}

fn write_new(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut file = File::create_new(path).map_err(|e| io_error(path, e))?;
    file.write_all(bytes).map_err(|e| io_error(path, e))
}

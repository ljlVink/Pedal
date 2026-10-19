//! 批量导出包内 record、方法索引和注释占位，供真实语料人工排查与回归比较。
//! @author ljlVink

use pedal_abc_bridge::{decompile, file::AbcFile, hap::HapArchive};
use std::fmt::Write;
use std::path::Path;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    if args.len() != 3 {
        return Err("usage: audit_package <input.abc|hap|hsp> <output-directory>".into());
    }
    let data = std::fs::read(&args[1])?;
    let bytes = if data.starts_with(b"PANDA") {
        data
    } else {
        let archive = HapArchive::parse(data)?;
        archive.read_entry(archive.main_abc().ok_or("missing modules.abc")?)?
    };
    let file = AbcFile::parse(bytes)?;
    let output = Path::new(&args[2]);
    std::fs::create_dir_all(output)?;
    std::fs::write(output.join("modules.abc"), file.bytes())?;
    let mut records = String::from("offset\tname\tfile\n");
    let mut methods = String::from("record\toffset\tname\tcode_size\tkind\n");
    let mut comments = String::from("file\tline\tcomment\n");
    let mut comment_count = 0;
    let mut placeholders = String::from("file\tline\tplaceholder\n");
    let mut placeholder_count = 0;
    let mut failures = Vec::new();
    for record in &file.classes {
        for method in &record.methods {
            writeln!(
                methods,
                "{:x}\t{:x}\t{}\t{}\t{:?}",
                record.offset,
                method.offset,
                method.name,
                method.code.as_ref().map_or(0, |code| code.code_size),
                method.kind
            )?;
        }
        let name = format!("{:x}.ets", record.offset);
        match decompile::decompile_record(&file, record) {
            Ok(result) => {
                writeln!(records, "{:x}\t{}\t{name}", record.offset, record.name)?;
                // 这是人工排查候选清单；字符串中的注释样式文本也会进入清单。
                let mut start = 0;
                while let Some(relative) = result.source[start..].find("/*") {
                    let at = start + relative;
                    let end = result.source[at + 2..]
                        .find("*/")
                        .map_or(result.source.len(), |end| at + 2 + end + 2);
                    let line = result.source[..at].bytes().filter(|b| *b == b'\n').count() + 1;
                    let comment = result.source[at..end].replace(['\t', '\r', '\n'], " ");
                    writeln!(comments, "{name}\t{line}\t{comment}")?;
                    comment_count += 1;
                    start = end;
                }
                for (line, text) in result.source.lines().enumerate() {
                    if text.contains("Unresolved closure @") || text.contains("unresolved class @")
                    {
                        writeln!(
                            placeholders,
                            "{name}\t{}\t{}",
                            line + 1,
                            text.trim().replace('\t', " ")
                        )?;
                        placeholder_count += 1;
                    }
                }
                std::fs::write(output.join(&name), result.source)?;
            }
            Err(error) => failures.push(format!("{}: {error}", record.name)),
        }
    }
    std::fs::write(output.join("records.tsv"), records)?;
    std::fs::write(output.join("methods.tsv"), methods)?;
    std::fs::write(output.join("comments.tsv"), comments)?;
    std::fs::write(output.join("placeholders.tsv"), placeholders)?;
    std::fs::write(output.join("failures.txt"), failures.join("\n"))?;
    println!(
        "records={} comment_candidates={comment_count} placeholder_candidates={placeholder_count} failures={} output={}",
        file.classes.len(),
        failures.len(),
        output.display()
    );
    if !failures.is_empty() {
        return Err("some records failed; see failures.txt".into());
    }
    Ok(())
}

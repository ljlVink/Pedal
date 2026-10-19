//! 命令行入口：保留 GUI 文件打开方式，提供无头分析及帮助。
//!
//! @author ljlVink

mod analyze;
mod console;

use rust_i18n::t;
use std::ffi::OsString;
use std::io::Write;
use std::path::PathBuf;

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Command {
    Gui(Option<PathBuf>),
    Analyze { input: PathBuf, output: PathBuf },
    Help,
    Version,
}

/// 使用 OsString 保留非 Unicode 文件路径；旧 GUI 调用仍使用首个位置参数。
pub(crate) fn parse(args: impl IntoIterator<Item = OsString>) -> Result<Command, String> {
    let mut args = args.into_iter();
    let Some(first) = args.next() else {
        return Ok(Command::Gui(None));
    };
    if first == "--help" || first == "-h" {
        return Ok(Command::Help);
    }
    if first == "--version" || first == "-V" {
        return Ok(Command::Version);
    }
    if first == "--" {
        return Ok(Command::Gui(args.next().map(PathBuf::from)));
    }
    if first != "analyze" {
        if first.as_encoded_bytes().starts_with(b"-") {
            return Err(t!("cli.unknown_option", option = first.to_string_lossy()).into_owned());
        }
        return Ok(Command::Gui(Some(first.into())));
    }

    let mut input = None;
    let mut output = None;
    let mut positional = false;
    while let Some(arg) = args.next() {
        if !positional && (arg == "--help" || arg == "-h") {
            return Ok(Command::Help);
        }
        if !positional && arg == "--" {
            positional = true;
        } else if !positional && (arg == "-o" || arg == "--output") {
            if output.is_some() {
                return Err(t!("cli.duplicate_output").into_owned());
            }
            let value = args
                .next()
                .filter(|value| !value.is_empty() && !value.as_encoded_bytes().starts_with(b"-"));
            output = Some(PathBuf::from(
                value.ok_or_else(|| t!("cli.missing_output").into_owned())?,
            ));
        } else if !positional && arg.as_encoded_bytes().starts_with(b"-") {
            return Err(t!("cli.unknown_option", option = arg.to_string_lossy()).into_owned());
        } else if arg.is_empty() || input.replace(PathBuf::from(&arg)).is_some() {
            return Err(
                t!("cli.unexpected_argument", argument = arg.to_string_lossy()).into_owned(),
            );
        }
    }
    Ok(Command::Analyze {
        input: input.ok_or_else(|| t!("cli.missing_input").into_owned())?,
        output: output.ok_or_else(|| t!("cli.missing_output").into_owned())?,
    })
}

/// 返回脚本可用的退出码：0 成功，1 分析或写出失败，2 参数错误。
pub(crate) fn run(command: Result<Command, String>) -> i32 {
    console::attach();
    let code = match command {
        Ok(Command::Help) => {
            println!("{}", t!("cli.help"));
            0
        }
        Ok(Command::Version) => {
            println!("{}", t!("cli.version", version = env!("CARGO_PKG_VERSION")));
            0
        }
        Ok(Command::Analyze { input, output }) => match analyze::run(&input, &output) {
            Ok(report) => {
                println!(
                    "{}",
                    t!(
                        "cli.complete",
                        abcs = report.abc_files,
                        records = report.sources.len(),
                        failures = report.failures.len(),
                        path = output.join("manifest.json").display()
                    )
                );
                i32::from(!report.failures.is_empty())
            }
            Err(error) => {
                eprintln!("{}", t!("cli.error", error = error));
                1
            }
        },
        Err(error) => {
            eprintln!("{}\n\n{}", t!("cli.error", error = error), t!("cli.help"));
            2
        }
        Ok(Command::Gui(_)) => unreachable!(),
    };
    // main 使用 process::exit；显式刷新缓冲，兼容重定向与管道。
    if std::io::stdout().flush().is_err() || std::io::stderr().flush().is_err() {
        return 1;
    }
    code
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_strings(args: &[&str]) -> Result<Command, String> {
        parse(args.iter().map(OsString::from))
    }

    #[test]
    fn preserves_gui_launch_and_file_opening() {
        assert_eq!(parse_strings(&[]).unwrap(), Command::Gui(None));
        assert_eq!(
            parse_strings(&["中文 应用.hap"]).unwrap(),
            Command::Gui(Some("中文 应用.hap".into()))
        );
        assert_eq!(
            parse_strings(&["--", "analyze"]).unwrap(),
            Command::Gui(Some("analyze".into()))
        );
    }

    #[test]
    fn parses_output_in_either_position_and_option_terminator() {
        for args in [
            vec!["analyze", "应用.hap", "-o", "out dir"],
            vec!["analyze", "--output", "out dir", "应用.hap"],
        ] {
            assert_eq!(
                parse_strings(&args).unwrap(),
                Command::Analyze {
                    input: "应用.hap".into(),
                    output: "out dir".into()
                }
            );
        }
        assert_eq!(
            parse_strings(&["analyze", "-o", "./-out", "--", "-file.abc"]).unwrap(),
            Command::Analyze {
                input: "-file.abc".into(),
                output: "./-out".into()
            }
        );
    }

    #[test]
    fn rejects_missing_duplicate_and_unknown_arguments() {
        for args in [
            vec!["analyze"],
            vec!["analyze", "x.abc"],
            vec!["analyze", "x.abc", "-o"],
            vec!["analyze", "-o", "out"],
            vec!["analyze", "x.abc", "-o", "out", "-o", "other"],
            vec!["analyze", "x.abc", "-o", "out", "other.abc"],
            vec!["analyze", "x.abc", "--unknown", "out"],
            vec!["analyze", "x.abc", "-o", "--output"],
            vec!["--unknown"],
        ] {
            assert!(parse_strings(&args).is_err(), "{args:?}");
        }
    }
}

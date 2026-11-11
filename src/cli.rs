//! 命令行入口：保留 GUI 文件打开方式，提供无头分析及帮助。
//!
//! @author ljlVink

mod analyze;
mod console;
mod restool;

use clap::{Parser, Subcommand};
use rust_i18n::t;
use std::ffi::OsString;
use std::io::Write;
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "pedal",
    display_name = "Pedal",
    version,
    about = "ArkTS decompiler and archive viewer",
    disable_help_subcommand = true,
    subcommand_precedence_over_arg = true,
    after_help = "Run without arguments to open the GUI. Only the first GUI file is opened.\nExit codes: 0 = success, 1 = analysis/export failure (including partial results), 2 = invalid arguments."
)]
struct Cli {
    #[arg(
        value_name = "FILE",
        trailing_var_arg = true,
        help = "Open a file in the GUI"
    )]
    files: Vec<OsString>,
    #[command(subcommand)]
    command: Option<Action>,
}

#[derive(Subcommand)]
enum Action {
    #[command(
        about = "Inspect compiled HarmonyOS resources",
        arg_required_else_help = true
    )]
    Restool {
        #[command(subcommand)]
        command: ResourceAction,
    },
    #[command(about = "Export all records")]
    Analyze {
        #[arg(value_name = "INPUT", help = "ABC file or package to analyze")]
        input: PathBuf,
        #[arg(
            short,
            long,
            value_name = "DIR",
            help = "New or empty output directory"
        )]
        output: PathBuf,
    },
}

#[derive(Subcommand)]
enum ResourceAction {
    #[command(
        about = "Print resources as JSON",
        after_help = "Examples:\n  pedal restool dump entry.hap\n  pedal restool dump config entry.hap\n  pedal restool dump resources.index"
    )]
    Dump {
        #[arg(
            value_name = "INPUT_OR_CONFIG",
            help = "HAP/HSP package, resources.index, or config"
        )]
        input: PathBuf,
        #[arg(
            value_name = "INPUT",
            help = "Input path when the first argument is config"
        )]
        config_input: Option<PathBuf>,
    },
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Command {
    Gui(Option<PathBuf>),
    Analyze { input: PathBuf, output: PathBuf },
    ResourceDump { input: PathBuf, config_only: bool },
}

/// 使用 OsString 保留非 Unicode 文件路径；旧 GUI 调用仍使用首个位置参数。
pub(crate) fn parse(args: impl IntoIterator<Item = OsString>) -> Result<Command, clap::Error> {
    let mut args: Vec<_> = args.into_iter().collect();
    if args.len() == 1 && args[0] == "analyze" {
        args.push("--help".into());
    }
    let cli = Cli::try_parse_from(std::iter::once(OsString::from("pedal")).chain(args))?;
    Ok(match cli.command {
        Some(Action::Analyze { input, output }) => Command::Analyze { input, output },
        Some(Action::Restool {
            command:
                ResourceAction::Dump {
                    input,
                    config_input,
                },
        }) => {
            if let Some(config_input) = config_input {
                if input != std::path::Path::new("config") {
                    return Err(clap::Error::raw(
                        clap::error::ErrorKind::InvalidValue,
                        "two dump arguments require 'config' before the input path",
                    ));
                }
                Command::ResourceDump {
                    input: config_input,
                    config_only: true,
                }
            } else {
                Command::ResourceDump {
                    input,
                    config_only: false,
                }
            }
        }
        None => Command::Gui(cli.files.into_iter().next().map(PathBuf::from)),
    })
}

/// 返回脚本可用的退出码：0 成功，1 分析或写出失败，2 参数错误。
pub(crate) fn run(command: Result<Command, clap::Error>) -> i32 {
    console::attach();
    let code = match command {
        Ok(Command::ResourceDump { input, config_only }) => match restool::run(&input, config_only)
        {
            Ok(()) => 0,
            Err(error) => {
                eprintln!("{}", t!("cli.error", error = error));
                1
            }
        },
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
            if error.print().is_err() {
                1
            } else {
                error.exit_code()
            }
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

    fn parse_strings(args: &[&str]) -> Result<Command, clap::Error> {
        parse(args.iter().map(OsString::from))
    }

    #[test]
    fn parses_restool_dump_and_config() {
        assert_eq!(
            parse_strings(&["restool", "dump", "中文.hap"]).unwrap(),
            Command::ResourceDump {
                input: "中文.hap".into(),
                config_only: false
            }
        );
        assert_eq!(
            parse_strings(&["restool", "dump", "config", "resources.index"]).unwrap(),
            Command::ResourceDump {
                input: "resources.index".into(),
                config_only: true
            }
        );
        assert!(parse_strings(&["restool", "dump", "wrong", "entry.hap"]).is_err());
        assert!(parse_strings(&["restool", "dump"]).is_err());
        assert_eq!(
            parse_strings(&["restool", "--help"])
                .unwrap_err()
                .exit_code(),
            0
        );
        assert_eq!(
            parse_strings(&["restool", "dump", "--help"])
                .unwrap_err()
                .exit_code(),
            0
        );
        assert_eq!(
            parse_strings(&["--", "restool"]).unwrap(),
            Command::Gui(Some("restool".into()))
        );
    }

    #[test]
    fn preserves_gui_launch_and_file_opening() {
        assert_eq!(parse_strings(&[]).unwrap(), Command::Gui(None));
        assert_eq!(parse_strings(&["--"]).unwrap(), Command::Gui(None));
        assert_eq!(
            parse_strings(&["中文 应用.hap"]).unwrap(),
            Command::Gui(Some("中文 应用.hap".into()))
        );
        assert_eq!(
            parse_strings(&["--", "analyze"]).unwrap(),
            Command::Gui(Some("analyze".into()))
        );
        assert_eq!(
            parse_strings(&["--", "-file.abc"]).unwrap(),
            Command::Gui(Some("-file.abc".into()))
        );
        for trailing in ["other.hap", "analyze", "--help", "--unknown", ""] {
            assert_eq!(
                parse_strings(&["first.hap", trailing]).unwrap(),
                Command::Gui(Some("first.hap".into()))
            );
        }
    }

    #[test]
    fn help_and_version_use_stdout_and_success_exit_codes() {
        for args in [
            vec!["-h"],
            vec!["--help"],
            vec!["analyze"],
            vec!["analyze", "-h"],
            vec!["analyze", "--help"],
        ] {
            let help = parse_strings(&args).unwrap_err();
            assert_eq!(help.kind(), clap::error::ErrorKind::DisplayHelp);
            assert_eq!(help.exit_code(), 0);
            assert!(!help.use_stderr());
            assert!(help.to_string().contains("Usage:"));
            if args[0] == "analyze" {
                assert!(help.to_string().contains("--output <DIR>"));
                assert!(!help.to_string().contains("--jobs"));
                assert!(!help.to_string().contains("-j"));
            }
        }
        for option in ["-V", "--version"] {
            let version = parse_strings(&[option]).unwrap_err();
            assert_eq!(version.kind(), clap::error::ErrorKind::DisplayVersion);
            assert_eq!(version.exit_code(), 0);
            assert!(!version.use_stderr());
            assert_eq!(
                version.to_string(),
                format!("Pedal {}\n", env!("CARGO_PKG_VERSION"))
            );
        }
    }

    #[test]
    #[cfg(any(unix, windows))]
    fn preserves_non_unicode_paths() {
        #[cfg(unix)]
        let path = {
            use std::os::unix::ffi::OsStringExt;
            OsString::from_vec(vec![b'x', 0xff])
        };
        #[cfg(windows)]
        let path = {
            use std::os::windows::ffi::OsStringExt;
            OsString::from_wide(&[b'x' as u16, 0xd800])
        };
        assert_eq!(
            parse([path.clone()]).unwrap(),
            Command::Gui(Some(PathBuf::from(&path)))
        );
        assert_eq!(
            parse(["analyze".into(), path.clone(), "-o".into(), path.clone()]).unwrap(),
            Command::Analyze {
                input: PathBuf::from(&path),
                output: PathBuf::from(path),
            }
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
                    output: "out dir".into(),
                }
            );
        }
        assert_eq!(
            parse_strings(&["analyze", "-o", "./-out", "--", "-file.abc"]).unwrap(),
            Command::Analyze {
                input: "-file.abc".into(),
                output: "./-out".into(),
            }
        );
    }

    #[test]
    fn parses_joined_clap_options() {
        for args in [
            vec!["analyze", "x.abc", "--output=out dir"],
            vec!["analyze", "x.abc", "-oout dir"],
        ] {
            assert_eq!(
                parse_strings(&args).unwrap(),
                Command::Analyze {
                    input: "x.abc".into(),
                    output: "out dir".into(),
                }
            );
        }
    }

    #[test]
    fn rejects_removed_worker_options() {
        for options in [
            vec!["-j", "4"],
            vec!["--jobs", "4"],
            vec!["-j4"],
            vec!["--jobs=4"],
        ] {
            let mut args = vec!["analyze", "x.abc", "-o", "out"];
            args.extend(options);
            let error = parse_strings(&args).unwrap_err();
            assert_eq!(error.kind(), clap::error::ErrorKind::UnknownArgument);
            assert_eq!(error.exit_code(), 2);
        }
    }

    #[test]
    fn rejects_missing_duplicate_and_unknown_arguments() {
        for args in [
            vec!["analyze", "x.abc"],
            vec!["analyze", "x.abc", "-o"],
            vec!["analyze", "-o", "out"],
            vec!["analyze", "x.abc", "-o", "out", "-o", "other"],
            vec!["analyze", "x.abc", "-o", "out", "other.abc"],
            vec!["analyze", "x.abc", "--unknown", "out"],
            vec!["analyze", "x.abc", "-o", "--output"],
            vec!["analyze", "x.abc", "-o", ""],
            vec!["analyze", "", "-o", "out"],
            vec!["--unknown"],
        ] {
            let error = parse_strings(&args).unwrap_err();
            assert_eq!(error.exit_code(), 2, "{args:?}");
            assert!(error.use_stderr(), "{args:?}");
        }
    }
}

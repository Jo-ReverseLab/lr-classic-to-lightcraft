//! Command-line tool that converts Lightroom Classic catalogs into LightCraft libraries.

use std::path::PathBuf;
use std::process::ExitCode;

const USAGE: &str = "\
Usage: lr2lc [OPTIONS] <CATALOG>

Converts a Lightroom Classic catalog (.lrcat) into a LightCraft library.
Without --apply, lr2lc only performs a dry run and reports what it would do.

Options:
  --apply      Actually write the LightCraft library (default: dry run)
  -h, --help   Print this help
  -V, --version  Print the version";

/// Parsed command-line options.
#[derive(Debug, PartialEq)]
enum Command {
    Help,
    Version,
    Run { catalog: PathBuf, apply: bool },
}

/// Parses the arguments (without the program name).
fn parse_args<I: IntoIterator<Item = String>>(args: I) -> Result<Command, String> {
    let mut apply = false;
    let mut catalog: Option<PathBuf> = None;
    for arg in args {
        match arg.as_str() {
            "-h" | "--help" => return Ok(Command::Help),
            "-V" | "--version" => return Ok(Command::Version),
            "--apply" => apply = true,
            s if s.starts_with('-') => return Err(format!("unknown option: {s}")),
            s => {
                if catalog.is_some() {
                    return Err(format!("unexpected argument: {s}"));
                }
                catalog = Some(PathBuf::from(s));
            }
        }
    }
    match catalog {
        Some(catalog) => Ok(Command::Run { catalog, apply }),
        None => Err("missing catalog path".to_string()),
    }
}

fn main() -> ExitCode {
    match parse_args(std::env::args().skip(1)) {
        Ok(Command::Help) => {
            println!("{USAGE}");
            ExitCode::SUCCESS
        }
        Ok(Command::Version) => {
            println!("lr2lc {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        Ok(Command::Run { catalog, apply }) => {
            let mode = if apply { "apply" } else { "dry run" };
            println!(
                "{} ({mode}): conversion is not implemented yet",
                catalog.display()
            );
            ExitCode::SUCCESS
        }
        Err(msg) => {
            eprintln!("error: {msg}\n\n{USAGE}");
            ExitCode::from(2)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<Command, String> {
        parse_args(args.iter().map(|s| s.to_string()))
    }

    #[test]
    fn defaults_to_dry_run() {
        assert_eq!(
            parse(&["cat.lrcat"]),
            Ok(Command::Run {
                catalog: PathBuf::from("cat.lrcat"),
                apply: false
            })
        );
    }

    #[test]
    fn apply_flag_is_recognised() {
        assert_eq!(
            parse(&["--apply", "cat.lrcat"]),
            Ok(Command::Run {
                catalog: PathBuf::from("cat.lrcat"),
                apply: true
            })
        );
    }

    #[test]
    fn missing_catalog_and_unknown_flag_are_errors() {
        assert!(parse(&[]).is_err());
        assert!(parse(&["--bogus"]).is_err());
    }
}

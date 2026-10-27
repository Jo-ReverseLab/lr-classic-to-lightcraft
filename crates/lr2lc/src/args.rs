//! Command-line argument parsing.

use crate::labels::LabelChoice;
use model::{PathMapping, PathMappings};
use std::ffi::OsString;
use std::path::PathBuf;

pub const USAGE: &str = "\
Usage: lr2lc [OPTIONS] <CATALOG>

Converts a Lightroom Classic catalog (.lrcat) into a LightCraft library.
Without --apply, lr2lc only performs a dry run and reports what it would do.
The catalog is opened read-only and original photos are never touched.

Options:
  --map OLD=NEW    Replace the path prefix OLD with NEW (repeatable),
                   e.g. when photos moved to another disk or computer
  --report FILE    Write a machine-readable report (JSON) to FILE
  --label-set SET  Names of the colour labels: en, de, auto (default) or a
                   JSON file mapping each name to red, yellow, green,
                   blue or purple
  --apply          Actually write the LightCraft library (default: dry run)
  -h, --help       Print this help
  -V, --version    Print the version";

/// What the user asked for.
#[derive(Debug, PartialEq)]
pub enum Command {
    Help,
    Version,
    Run(RunOptions),
}

/// Options of a conversion run.
#[derive(Debug, PartialEq)]
pub struct RunOptions {
    pub catalog: PathBuf,
    pub mappings: PathMappings,
    pub report: Option<PathBuf>,
    pub apply: bool,
    pub labels: LabelChoice,
}

/// Parses the arguments (without the program name).
pub fn parse_args<I: IntoIterator<Item = OsString>>(args: I) -> Result<Command, String> {
    let mut args = args.into_iter();
    let mut apply = false;
    let mut catalog: Option<PathBuf> = None;
    let mut report: Option<PathBuf> = None;
    let mut rules = Vec::new();
    let mut labels: Option<LabelChoice> = None;
    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some("-h" | "--help") => return Ok(Command::Help),
            Some("-V" | "--version") => return Ok(Command::Version),
            Some("--apply") => apply = true,
            Some("--map") => {
                let value = args.next().ok_or("--map needs a value OLD=NEW")?;
                let spec = value.to_str().ok_or("--map value is not valid UTF-8")?;
                let rule = PathMapping::parse(spec).map_err(|e| e.to_string())?;
                // The library allows an empty target (relative result), but on
                // the command line it is almost always a typo.
                if rule.to().trim().is_empty() {
                    return Err(format!("--map {spec}: the new prefix is empty"));
                }
                rules.push(rule);
            }
            Some("--report") => {
                if report.is_some() {
                    return Err("--report given more than once".to_string());
                }
                let value = args.next().ok_or("--report needs a file name")?;
                report = Some(PathBuf::from(value));
            }
            Some("--label-set") => {
                if labels.is_some() {
                    return Err("--label-set given more than once".to_string());
                }
                let value = args
                    .next()
                    .ok_or("--label-set needs a value: en, de, auto or a file")?;
                let value = value
                    .to_str()
                    .ok_or("--label-set value is not valid UTF-8")?;
                labels = Some(LabelChoice::parse(value));
            }
            Some(s) if s.starts_with('-') => return Err(format!("unknown option: {s}")),
            _ => {
                if catalog.is_some() {
                    return Err("more than one catalog given".to_string());
                }
                catalog = Some(PathBuf::from(arg));
            }
        }
    }
    let catalog = catalog.ok_or("missing catalog path")?;
    Ok(Command::Run(RunOptions {
        catalog,
        mappings: PathMappings::new(rules),
        report,
        apply,
        labels: labels.unwrap_or_default(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<Command, String> {
        parse_args(args.iter().map(OsString::from))
    }

    fn run(args: &[&str]) -> RunOptions {
        match parse(args) {
            Ok(Command::Run(o)) => o,
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn defaults_to_dry_run_without_report_or_mapping() {
        let o = run(&["cat.lrcat"]);
        assert_eq!(o.catalog, PathBuf::from("cat.lrcat"));
        assert!(!o.apply);
        assert_eq!(o.report, None);
        assert!(o.mappings.rules().is_empty());
        assert_eq!(o.labels, LabelChoice::Auto);
    }

    #[test]
    fn label_set_takes_next_argument() {
        assert_eq!(
            run(&["cat.lrcat", "--label-set", "de"]).labels,
            LabelChoice::De
        );
    }

    #[test]
    fn apply_flag_is_recognised() {
        assert!(run(&["--apply", "cat.lrcat"]).apply);
    }

    #[test]
    fn map_is_repeatable_and_kept_in_order() {
        let o = run(&[
            "--map",
            "/Users/example/Pictures=/mnt/pictures",
            "cat.lrcat",
            "--map",
            "C:\\data=D:\\data",
        ]);
        let rules = o.mappings.rules();
        assert_eq!(rules.len(), 2);
        assert_eq!(rules[0].from(), "/Users/example/Pictures");
        assert_eq!(rules[1].to(), "D:\\data");
    }

    #[test]
    fn report_takes_next_argument() {
        let o = run(&["cat.lrcat", "--report", "out.json"]);
        assert_eq!(o.report, Some(PathBuf::from("out.json")));
    }

    #[test]
    fn invalid_arguments_are_errors() {
        assert!(parse(&[]).is_err());
        assert!(parse(&["--bogus"]).is_err());
        assert!(parse(&["cat.lrcat", "--map"]).is_err());
        assert!(parse(&["cat.lrcat", "--map", "no-equals"]).is_err());
        assert!(parse(&["cat.lrcat", "--map", "=/new"]).is_err());
        assert!(parse(&["cat.lrcat", "--report"]).is_err());
        assert!(parse(&["cat.lrcat", "--report", "a", "--report", "b"]).is_err());
        assert!(parse(&["a.lrcat", "b.lrcat"]).is_err());
        assert!(parse(&["cat.lrcat", "--label-set"]).is_err());
        assert!(parse(&["cat.lrcat", "--label-set", "en", "--label-set", "de"]).is_err());
    }

    #[test]
    fn empty_or_blank_mapping_target_is_rejected() {
        for spec in ["/x=", "/x=   ", "/x=\t", "C:\\data= "] {
            let err = parse(&["cat.lrcat", "--map", spec]).unwrap_err();
            assert!(err.contains("empty"), "{spec}: {err}");
        }
        assert!(parse(&["cat.lrcat", "--map", "/a=/b", "--map", "/x="]).is_err());
    }

    #[test]
    fn mapping_target_with_content_is_kept_as_given() {
        let o = run(&["cat.lrcat", "--map", "/x= /y"]);
        assert_eq!(o.mappings.rules()[0].to(), " /y");
    }

    #[test]
    fn help_and_version_win() {
        assert_eq!(parse(&["cat.lrcat", "-h"]), Ok(Command::Help));
        assert_eq!(parse(&["-V"]), Ok(Command::Version));
    }

    #[test]
    fn usage_documents_all_options() {
        for opt in [
            "--map",
            "--report",
            "--label-set",
            "--apply",
            "--help",
            "--version",
        ] {
            assert!(USAGE.contains(opt), "{opt}");
        }
    }
}

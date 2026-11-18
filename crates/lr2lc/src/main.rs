//! Command-line tool that converts Lightroom Classic catalogs into LightCraft libraries.

#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]
#![forbid(unsafe_code)]

mod args;

use anyhow::{Context, Result};
use args::{Command, RunOptions, USAGE, parse_args};
use lr2lc_core::labels::LabelMode;
use lr2lc_core::report_file::{self, Kind, ReportTarget};
use lr2lc_core::{ExistingDecisions, Job, NoProgress, finish, prepare};
use std::process::ExitCode;

fn main() -> ExitCode {
    match parse_args(std::env::args_os().skip(1)) {
        Ok(Command::Help) => {
            println!("{USAGE}");
            ExitCode::SUCCESS
        }
        Ok(Command::Version) => {
            println!("lr2lc {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        Ok(Command::Run(options)) => {
            // The white-balance probe is not implemented yet: stop before
            // touching the catalog or any original file.
            if options.probe_white_balance {
                eprintln!(
                    "error: --probe-white-balance is not implemented yet: the white balance probe \
                     would read technical metadata from an original photo. Nothing was read or \
                     written."
                );
                return ExitCode::from(2);
            }
            // Check the output targets before anything is read, so a wrong
            // --report or --script can never overwrite the catalog or
            // another file.
            let targets = match check_targets(&options) {
                Ok(targets) => targets,
                Err(e) => {
                    eprintln!("error: {e}");
                    return ExitCode::from(2);
                }
            };
            // A bad label set is a usage error as well: nothing is read.
            let labels = match LabelMode::load(&options.labels) {
                Ok(mode) => mode,
                Err(e) => {
                    eprintln!("error: {e}");
                    return ExitCode::from(2);
                }
            };
            run_and_report(&options, &labels, &targets)
        }
        Err(msg) => {
            eprintln!("error: {msg}\n\n{USAGE}");
            ExitCode::from(2)
        }
    }
}

/// The checked `--report` and `--script` targets.
struct Targets {
    report: Option<ReportTarget>,
    script: Option<ReportTarget>,
}

fn check_targets(options: &RunOptions) -> Result<Targets, String> {
    let check = |kind, path: &Option<std::path::PathBuf>| {
        path.as_ref()
            .map(|p| report_file::check_as(kind, p, &options.catalog))
            .transpose()
            .map_err(|e| e.to_string())
    };
    let report = check(Kind::Report, &options.report)?;
    let script = check(Kind::Script, &options.script)?;
    if let (Some(r), Some(s)) = (&report, &script)
        && r.path() == s.path()
    {
        return Err("--report and --script name the same file".to_string());
    }
    Ok(Targets { report, script })
}

fn run_and_report(options: &RunOptions, labels: &LabelMode, targets: &Targets) -> ExitCode {
    match run(options, labels, targets) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e:#}");
            ExitCode::from(1)
        }
    }
}

/// Runs a conversion: reads the catalog, plans the writer commands and, with
/// `--apply`, writes them through the LightCraft control port.
fn run(options: &RunOptions, labels: &LabelMode, targets: &Targets) -> Result<()> {
    let job = Job {
        catalog: options.catalog.clone(),
        mappings: options.mappings.clone(),
        labels: labels.clone(),
        library: options.library.clone(),
        apply: options.apply,
        port: options.port,
        allow_xmp: options.allow_xmp,
        overwrite_develop: options.overwrite_develop,
        areas: options.areas,
        anonymize: options.anonymize,
        existing: ExistingDecisions::all(options.existing),
    };
    let prepared = prepare(&job, &mut NoProgress)?;
    if let (Some(target), Some(given)) = (&targets.script, &options.script) {
        report_file::write_as(Kind::Script, target, &prepared.script())
            .with_context(|| format!("cannot write script to {}", given.display()))?;
    }
    if job.writes_to().is_some() {
        prepared.check_writable()?;
    }
    // Printed before writing, which can take a while.
    print!("{}", prepared.catalog_summary(&job));
    let finished = finish(&job, prepared, &mut NoProgress)?;
    print!("{}", finished.writer_summary);

    if let (Some(target), Some(given)) = (&targets.report, &options.report) {
        report_file::write(target, &finished.report_json())
            .with_context(|| format!("cannot write report to {}", given.display()))?;
    }
    finished.result()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::args::RunOptions;
    use lr2lc_core::labels::LabelChoice;
    use model::PathMappings;
    use std::path::PathBuf;

    fn options_with(report: Option<PathBuf>, script: Option<PathBuf>) -> RunOptions {
        RunOptions {
            catalog: PathBuf::from("cat.lrcat"),
            mappings: PathMappings::default(),
            report,
            apply: false,
            labels: LabelChoice::Auto,
            library: None,
            port: crate::args::DEFAULT_PORT,
            allow_xmp: false,
            script,
            overwrite_develop: false,
            probe_white_balance: false,
            areas: lr2lc_core::Areas::ALL,
            anonymize: false,
            existing: Default::default(),
        }
    }

    #[test]
    fn report_and_script_pointing_at_the_same_file_is_rejected() {
        let same = PathBuf::from("out.json");
        let err = match check_targets(&options_with(Some(same.clone()), Some(same))) {
            Ok(_) => panic!("expected an error"),
            Err(err) => err,
        };
        assert!(
            err.contains("--report and --script name the same file"),
            "{err}"
        );
    }

    #[test]
    fn report_and_script_pointing_at_different_files_are_accepted() {
        let opts = options_with(
            Some(PathBuf::from("r.json")),
            Some(PathBuf::from("s.jsonl")),
        );
        assert!(check_targets(&opts).is_ok());
    }
}

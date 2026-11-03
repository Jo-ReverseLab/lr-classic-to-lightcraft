//! Command-line tool that converts Lightroom Classic catalogs into LightCraft libraries.

mod args;
mod dry_run;
mod labels;
mod output;
mod report_file;
mod writer;

use anyhow::{Context, Result, bail};
use args::{Command, RunOptions, USAGE, parse_args};
use labels::LabelMode;
use report_file::{Kind, ReportTarget};
use std::collections::BTreeSet;
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
    let mut outcome = dry_run::run(&options.catalog, &options.mappings, labels)?;
    let input = writer::input(std::mem::take(&mut outcome.converted));
    let state = writer::load_state(options.library.as_deref())?;
    // Without a connection the library's own keywords are unknown; the
    // plan assumes none beyond the mapping file.
    let plan = lc_writer::plan::plan(&input, &state, &BTreeSet::new());
    if let (Some(target), Some(given)) = (&targets.script, &options.script) {
        report_file::write_as(Kind::Script, target, &plan.to_script(&state))
            .with_context(|| format!("cannot write script to {}", given.display()))?;
    }

    let (text, json, stop) = match (&options.library, options.apply) {
        (Some(library), true) => {
            if !outcome.catalog_unchanged {
                bail!(
                    "the catalog changed while it was read (checksum differs); is Lightroom \
                     running? Nothing was written to LightCraft"
                );
            }
            print!("{}", output::summary_with(&outcome, false));
            let target = writer::Target {
                library,
                port: options.port,
                allow_xmp: options.allow_xmp,
            };
            // A run that stops still reports what it did (Principle 4).
            let (summary, stop) = match writer::write(&target, &input, state) {
                Ok(summary) => (summary, None),
                Err(aborted) => (*aborted.summary, Some(aborted.error)),
            };
            (
                writer::applied_summary(&summary),
                writer::applied_json(&summary, stop.as_ref()),
                stop,
            )
        }
        _ => {
            print!("{}", output::summary_with(&outcome, true));
            (
                writer::planned_summary(&plan),
                writer::planned_json(&plan),
                None,
            )
        }
    };
    print!("{text}");

    // Check again after writing: the catalog must still be unchanged.
    outcome.catalog_unchanged = outcome.catalog_unchanged
        && lr_reader::checksum(&options.catalog).ok() == Some(outcome.catalog_checksum);
    if let (Some(target), Some(given)) = (&targets.report, &options.report) {
        let report = output::report_json_with(&outcome, !options.apply, Some(&json));
        report_file::write(target, &report)
            .with_context(|| format!("cannot write report to {}", given.display()))?;
    }
    if let Some(e) = stop {
        return Err(writer::stop_error(&e));
    }
    if !outcome.catalog_unchanged {
        bail!("the catalog changed during the run (checksum differs); is Lightroom running?");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::args::RunOptions;
    use crate::labels::LabelChoice;
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

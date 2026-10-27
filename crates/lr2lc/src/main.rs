//! Command-line tool that converts Lightroom Classic catalogs into LightCraft libraries.

mod args;
mod dry_run;
mod labels;
mod output;
mod report_file;

use anyhow::{Context, Result, bail};
use args::{Command, RunOptions, USAGE, parse_args};
use labels::LabelMode;
use report_file::ReportTarget;
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
            // Check the report target before anything is read, so a wrong
            // --report can never overwrite the catalog or another file.
            let target = match &options.report {
                Some(path) => match report_file::check(path, &options.catalog) {
                    Ok(target) => Some(target),
                    Err(e) => {
                        eprintln!("error: {e}");
                        return ExitCode::from(2);
                    }
                },
                None => None,
            };
            // A bad label set is a usage error as well: nothing is read.
            let labels = match LabelMode::load(&options.labels) {
                Ok(mode) => mode,
                Err(e) => {
                    eprintln!("error: {e}");
                    return ExitCode::from(2);
                }
            };
            run_and_report(&options, &labels, target.as_ref())
        }
        Err(msg) => {
            eprintln!("error: {msg}\n\n{USAGE}");
            ExitCode::from(2)
        }
    }
}

fn run_and_report(
    options: &RunOptions,
    labels: &LabelMode,
    target: Option<&ReportTarget>,
) -> ExitCode {
    match run(options, labels, target) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e:#}");
            ExitCode::from(1)
        }
    }
}

/// Runs a conversion. Only the dry run exists so far.
fn run(options: &RunOptions, labels: &LabelMode, report: Option<&ReportTarget>) -> Result<()> {
    if options.apply {
        bail!(
            "--apply is not implemented yet: writing to LightCraft is not available; \
             nothing was read or written. Run without --apply for a dry run."
        );
    }
    let outcome = dry_run::run(&options.catalog, &options.mappings, labels)?;
    print!("{}", output::summary(&outcome));
    if let (Some(target), Some(given)) = (report, &options.report) {
        report_file::write(target, &output::report_json(&outcome))
            .with_context(|| format!("cannot write report to {}", given.display()))?;
    }
    if !outcome.catalog_unchanged {
        bail!("the catalog changed during the run (checksum differs); is Lightroom running?");
    }
    Ok(())
}

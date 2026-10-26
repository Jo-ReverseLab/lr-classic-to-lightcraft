//! Command-line tool that converts Lightroom Classic catalogs into LightCraft libraries.

mod args;
mod dry_run;
mod output;

use anyhow::{Context, Result, bail};
use args::{Command, RunOptions, USAGE, parse_args};
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
        Ok(Command::Run(options)) => match run(&options) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("error: {e:#}");
                ExitCode::from(1)
            }
        },
        Err(msg) => {
            eprintln!("error: {msg}\n\n{USAGE}");
            ExitCode::from(2)
        }
    }
}

/// Runs a conversion. Only the dry run exists so far.
fn run(options: &RunOptions) -> Result<()> {
    if options.apply {
        bail!(
            "--apply is not implemented yet: writing to LightCraft is not available; \
             nothing was read or written. Run without --apply for a dry run."
        );
    }
    let outcome = dry_run::run(&options.catalog, &options.mappings)?;
    print!("{}", output::summary(&outcome));
    if let Some(path) = &options.report {
        std::fs::write(path, output::report_json(&outcome))
            .with_context(|| format!("cannot write report to {}", path.display()))?;
    }
    if !outcome.catalog_unchanged {
        bail!("the catalog changed during the run (checksum differs); is Lightroom running?");
    }
    Ok(())
}

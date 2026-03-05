use anyhow::Result;
use clap::Parser;

use cargo_brief::cli::{BriefArgs, Cargo, CargoCommand};

fn main() -> Result<()> {
    let args = parse_args();
    let output = cargo_brief::run_pipeline(&args)?;
    print!("{output}");
    Ok(())
}

fn parse_args() -> BriefArgs {
    // Handle both `cargo brief <args>` and `cargo-brief <args>` invocations
    let raw_args: Vec<String> = std::env::args().collect();

    if raw_args.len() > 1 && raw_args[1] == "brief" {
        // Invoked as `cargo brief` — parse as cargo subcommand
        let cargo = Cargo::parse();
        let CargoCommand::Brief(args) = cargo.command;
        args
    } else {
        // Direct invocation — parse BriefArgs directly
        BriefArgs::parse()
    }
}

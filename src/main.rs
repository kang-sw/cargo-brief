use anyhow::Result;
use clap::Parser;

use cargo_brief::cli::{BriefCommand, BriefDirect, Cargo, CargoCommand};

fn main() -> Result<()> {
    let cmd = parse_command();

    match &cmd {
        BriefCommand::Api(args) => {
            if let Some(spec) = &args.remote.clean {
                cargo_brief::clean_cache(spec)?;
                return Ok(());
            }
            let output = cargo_brief::run_api_pipeline(args)?;
            print!("{output}");
        }
        BriefCommand::Search(args) => {
            if let Some(spec) = &args.remote.clean {
                cargo_brief::clean_cache(spec)?;
                return Ok(());
            }
            let output = cargo_brief::run_search_pipeline(args)?;
            print!("{output}");
        }
        BriefCommand::Examples(args) => {
            if let Some(spec) = &args.remote.clean {
                cargo_brief::clean_cache(spec)?;
                return Ok(());
            }
            let output = cargo_brief::run_examples_pipeline(args)?;
            print!("{output}");
        }
        BriefCommand::Summary(args) => {
            if let Some(spec) = &args.remote.clean {
                cargo_brief::clean_cache(spec)?;
                return Ok(());
            }
            let output = cargo_brief::run_summary_pipeline(args)?;
            print!("{output}");
        }
    }
    Ok(())
}

fn parse_command() -> BriefCommand {
    // Handle both `cargo brief <args>` and `cargo-brief <args>` invocations
    let raw_args: Vec<String> = std::env::args().collect();

    if raw_args.len() > 1 && raw_args[1] == "brief" {
        // Invoked as `cargo brief` — parse as cargo subcommand
        let cargo = Cargo::parse();
        let CargoCommand::Brief(direct) = cargo.command;
        direct.command
    } else {
        // Direct invocation — parse BriefDirect directly
        let direct = BriefDirect::parse();
        direct.command
    }
}

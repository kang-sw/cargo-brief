use anyhow::Result;
use clap::Parser;

use cargo_brief::cli::{BriefCommand, BriefDirect, Cargo, CargoCommand};

fn main() -> Result<()> {
    let direct = parse_command();
    let remote = direct.remote_opts();

    match &direct.command {
        BriefCommand::Api(args) => {
            let output = cargo_brief::run_api_pipeline(args, &remote)?;
            print!("{output}");
        }
        BriefCommand::Search(args) => {
            let output = cargo_brief::run_search_pipeline(args, &remote)?;
            print!("{output}");
        }
        BriefCommand::Examples(args) => {
            let output = cargo_brief::run_examples_pipeline(args, &remote)?;
            print!("{output}");
        }
        BriefCommand::Summary(args) => {
            let output = cargo_brief::run_summary_pipeline(args, &remote)?;
            print!("{output}");
        }
        BriefCommand::Ts(args) => {
            let output = cargo_brief::run_ts_pipeline(args, &remote)?;
            print!("{output}");
        }
        BriefCommand::Code(args) => {
            let output = cargo_brief::run_code_pipeline(args, &remote)?;
            print!("{output}");
        }
        BriefCommand::Clean(args) => {
            cargo_brief::clean_cache(args.spec.as_deref().unwrap_or(""))?;
        }
    }
    Ok(())
}

fn parse_command() -> BriefDirect {
    // Handle both `cargo brief <args>` and `cargo-brief <args>` invocations
    let raw_args: Vec<String> = std::env::args().collect();

    if raw_args.len() > 1 && raw_args[1] == "brief" {
        // Invoked as `cargo brief` — parse as cargo subcommand
        let cargo = Cargo::parse();
        let CargoCommand::Brief(direct) = cargo.command;
        direct
    } else {
        // Direct invocation — parse BriefDirect directly
        BriefDirect::parse()
    }
}

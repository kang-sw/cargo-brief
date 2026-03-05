pub mod cli;
pub mod model;
pub mod render;
pub mod rustdoc_json;

use anyhow::{Context, Result};

use cli::BriefArgs;
use model::CrateModel;

/// Run the cargo-brief pipeline and return the rendered output string.
pub fn run_pipeline(args: &BriefArgs) -> Result<String> {
    // Step 1: Generate rustdoc JSON
    let json_path = rustdoc_json::generate_rustdoc_json(
        &args.crate_name,
        &args.toolchain,
        args.manifest_path.as_deref(),
        true, // always document private items for visibility filtering
    )
    .context("Failed to generate rustdoc JSON")?;

    // Step 2: Parse JSON
    let krate =
        rustdoc_json::parse_rustdoc_json(&json_path).context("Failed to parse rustdoc JSON")?;

    // Step 3: Build model
    let model = CrateModel::from_crate(krate);

    // Step 4: Determine if observer is in the same crate
    let observer_crate = args.at_package.as_deref().unwrap_or(&args.crate_name);
    let same_crate =
        observer_crate == args.crate_name || observer_crate.replace('-', "_") == model.crate_name();

    // Step 5: Render
    let output = render::render_module_api(
        &model,
        args.module_path.as_deref(),
        args,
        args.at_mod.as_deref(),
        same_crate,
    );

    Ok(output)
}

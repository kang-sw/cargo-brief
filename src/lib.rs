pub mod cli;
pub mod model;
pub mod render;
pub mod resolve;
pub mod rustdoc_json;

use anyhow::{Context, Result};

use cli::BriefArgs;
use model::CrateModel;

/// Run the cargo-brief pipeline and return the rendered output string.
pub fn run_pipeline(args: &BriefArgs) -> Result<String> {
    // Step 0: Load cargo metadata and resolve target
    let metadata = resolve::load_cargo_metadata(args.manifest_path.as_deref())
        .context("Failed to load cargo metadata")?;

    let resolved =
        resolve::resolve_target(&args.crate_name, args.module_path.as_deref(), &metadata)
            .context("Failed to resolve target")?;

    // Step 1: Generate rustdoc JSON
    let json_path = rustdoc_json::generate_rustdoc_json(
        &resolved.package_name,
        &args.toolchain,
        args.manifest_path.as_deref(),
        true, // always document private items for visibility filtering
        &metadata.target_dir,
    )
    .with_context(|| {
        format!(
            "Failed to generate rustdoc JSON for crate '{}'",
            resolved.package_name
        )
    })?;

    // Step 2: Parse JSON
    let krate = rustdoc_json::parse_rustdoc_json(&json_path)
        .with_context(|| format!("Failed to parse rustdoc JSON at '{}'", json_path.display()))?;

    // Step 3: Build model
    let model = CrateModel::from_crate(krate);

    // Step 4: Determine if observer is in the same crate
    // If --at-package is explicit, use that. Otherwise auto-detect:
    // same_crate = true only when inspecting the current package (self).
    let same_crate = if let Some(at_pkg) = args.at_package.as_deref() {
        at_pkg == resolved.package_name || at_pkg.replace('-', "_") == model.crate_name()
    } else {
        // Auto-detect: same_crate only if target is the current package
        metadata
            .current_package
            .as_ref()
            .map(|cur| cur == &resolved.package_name || cur.replace('-', "_") == model.crate_name())
            .unwrap_or(false)
    };

    // Step 5: Render
    let output = render::render_module_api(
        &model,
        resolved.module_path.as_deref(),
        args,
        args.at_mod.as_deref(),
        same_crate,
    );

    Ok(output)
}

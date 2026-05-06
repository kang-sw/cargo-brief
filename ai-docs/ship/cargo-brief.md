# Ship: cargo-brief

## Version Strategy

Evidence-based semver proposal. Derive the recommended version before touching
version files:

1. Find the baseline: `git tag --sort=-version:refname | head -1`.
   If no tag: note "no prior tag" and skip semver-checks (step 2).
2. Run `cargo semver-checks --baseline-rev <tag>` when available.
   - Breaking-change lint on `>=1.0.0` -> recommend **major**.
   - Breaking-change lint on `0.x` -> recommend **minor** unless the change is
     intentionally unreleased/internal.
   - Tool incompatibility is inconclusive; record it, but do not treat it as a
     version signal.
3. Scan `git log <tag>..HEAD --oneline` and summarize caller-visible impact:
   - Breaking/removal/renamed CLI behavior, incompatible output contract, or
     public library API break -> recommend **major** for `>=1.0.0`, **minor**
     for `0.x`.
   - New major workflow surface, new subcommand family, or substantial new
     capability area -> recommend **minor**.
   - Additive flags/options, bug fixes, performance fixes, docs, tests, and
     hardening -> recommend **patch** for `0.x`; for `>=1.0.0`, recommend
     **patch** or **minor** based on public API significance.
4. Propose the recommended version and list any reasonable alternative. Do not
   write files until the user confirms.
5. If the user overrides the recommendation, record the rationale in the release
   commit `## AI Context`.
6. After confirmation: write the new version to `Cargo.toml` `version` field,
   then run `cargo update --workspace` to sync `Cargo.lock`.

## Pre-flight

Run before version files are changed; any failure aborts the ship:

- `cargo clippy -- -D warnings`
  - If fixes are required: apply them, commit, then **re-run both clippy and
    `cargo test` before continuing**. Do not proceed to the confirm gate until
    both pass on the same source tree.
- `cargo test`

After version confirmation:

- Verify `CHANGELOG.md` contains an entry `## [<version>]`. If missing: write
  the entry (summarise commits since baseline tag; use `### Added`,
  `### Fixed`, `### Changed` sections as appropriate).
- Run `cargo test` again after version-file edits.

## Build

- `cargo build --release`

## Tag

Format: `v<version>` (e.g. `v0.12.0`)
Push: yes

## Publish

- `cargo publish`

## Post-ship

- Commit `Cargo.toml` and `Cargo.lock` version bump as
  `chore(release): bump version to <version>` if not already committed in pre-flight.

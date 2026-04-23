# Ship: cargo-brief

## Version Strategy

Agent-determined semver bump. Derive the new version before touching any files:

1. Find the baseline: `git tag --sort=-version:refname | head -1`.
   If no tag: note "no prior tag" and skip semver-checks (step 2).
2. Run `cargo semver-checks --baseline-rev <tag>`.
   - Any breaking-change lint → bump **major** (or **minor** if current major is 0).
   - Clean → proceed to step 3.
3. Scan `git log <tag>..HEAD --oneline` (or full log if no tag):
   - Any `feat:` or `feat(...)` subject → bump **minor**.
   - Only `fix:`, `chore:`, `docs:`, `refactor:` subjects → bump **patch**.
4. Propose the derived version to the user. The user confirms or overrides at the
   confirmation gate (step 5 of the Execute flow). No files are written until confirmed.
5. After confirmation: write the new version to `Cargo.toml` `version` field, then run
   `cargo update --workspace` to sync `Cargo.lock`.

## Pre-flight

Run in order after the version bump; any failure aborts the ship:

- `cargo clippy -- -D warnings`
  - If fixes are required: apply them, commit, then **re-run both clippy and
    `cargo test` before continuing**. Do not proceed to the confirm gate until
    both pass on the same source tree.
- `cargo test`
- Verify `CHANGELOG.md` contains an entry `## [<version>]`. If missing: write the entry
  (summarise commits since baseline tag; use `### Added`, `### Fixed`, `### Changed`
  sections as appropriate), then commit as
  `chore(release): update changelog for <version>`.

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

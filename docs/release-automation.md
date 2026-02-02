# Release Automation (semantic-release)

This repo can auto‑release on commits to `main` using **semantic‑release** and then hand off to the existing `rust-release.yml` workflow via tags.

## How it works

1. `semantic-release.yml` runs on every push to `main`.
2. It computes the next version from Conventional Commits.
3. It updates `codex-rs/Cargo.toml` and commits it.
4. It creates a tag in the format `rust-vX.Y.Z`.
5. The existing `rust-release.yml` workflow is triggered by that tag to build artifacts and publish npm packages.

## Commit message rules

Use Conventional Commits so semantic-release can determine the version bump:

- `feat:` → minor
- `fix:` → patch
- `feat!:`, `fix!:` or `BREAKING CHANGE:` → major

## GitHub Actions files

- `.github/workflows/semantic-release.yml` (creates tags + bumps Cargo version)
- `.github/workflows/rust-release.yml` (builds + publishes when tag is pushed)

## Trusted publishing

Npm publishing is still handled by `rust-release.yml` using OIDC trusted publishing.
See `docs/npm-trusted-publishing.md` for setup.

## Fork toggles

These repo variables control optional CI steps for forks:

- `RUN_BAZEL=true` to enable the Bazel workflow (requires a BuildBuddy API key).
- `RUN_NPM_STAGE=true` to enable the npm staging step in `ci.yml`.

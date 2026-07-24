# Fork release validation guide

This guide is the non-destructive validation playbook for the fork release path.
It is intentionally limited to dry runs, prerelease checks, and explicit stop
conditions. It does not authorize a production publish.

## Current prepared prerelease target on July 24, 2026

This branch is prepared for the fork's first real npm prerelease:

- release version: `0.1.0-alpha.12`
- release tag: `rust-v0.1.0-alpha.12`
- release model: upstream tag-driven `rust-release.yml`

The checked-in Cargo workspace version and the three checked-in npm package
manifests are intentionally pinned to that exact prerelease version. The
release workflow now enforces that the pushed tag, `codex-rs/Cargo.toml`, and
those npm manifests all match exactly.

## Current stop conditions on July 23, 2026

Local inspection on July 23, 2026 shows that the manager integration line has
already landed the accepted fork package scope and repo-aware staging
improvements from phase `008`, the `repo-checks.yml` fork-staging guard from
task `033`, and the shipped installer release-surface fixes from task `035`.
There is no longer a known static content blocker in the audited workflow,
staging-helper, or installer surface itself.

There is also one important operator caveat to preserve during any later dry run:

- `scripts/stage_npm_packages.py` is now repo-aware and should be driven by
  `--repo`, `--workflow-url`, `GITHUB_REPOSITORY`, or the local `origin` remote.
  It still retains `openai/codex` as the final fallback default, so fork dry
  runs should pass explicit fork context rather than relying on the fallback.

Those conditions mean the current line can be statically reviewed locally and the
manual fork staging path is documented, but the built-in `repo-checks.yml` npm
staging smoke step is still not authoritative for fork prerelease validation.

## Validation matrix

| Validation surface                                        | Status on July 23, 2026                                              | Notes                                                                                                                                                                                  |
| --------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Local script and manifest sanity                          | Ready now                                                            | Safe to run repeatedly on a worker branch or worktree.                                                                                                                                 |
| PR validation or pushes to `main` via `blocking-ci.yml`   | Ready now                                                            | Use this for non-release workflow validation; a branch push by itself does not trigger `blocking-ci.yml`.                                                                              |
| Optional `repo-checks.yml` npm staging step               | Ready only when explicitly enabled with fork run context             | The fork path now requires `CODEX_REPO_CHECKS_NPM_STAGING_WORKFLOW_RUN_ID` and derives the run URL from the current repo; leave it off unless you intentionally want that smoke check. |
| Tag-triggered `rust-release.yml` beta smoke               | Ready only after normal PR/`main` CI and release-infra gating checks | `publish-npm` stays off for beta tags, but the release asset path still depends on `CODEX_ENABLE_RELEASE_INFRA` plus the fork's release runners/signing environment.                   |
| Tag-triggered `rust-release.yml` alpha prerelease publish | Ready only after final audit and GitHub-hosted Linux/Windows x64 checks | On the fork, numbered alpha tags now publish npm from GitHub-hosted Linux x64 and Windows x64 workflow artifacts directly and intentionally skip macOS release/signing, Windows signing, and ARM64 alpha artifacts. |
| R2, dev website, and winget satellites                    | Intentionally deferred                                               | Keep `CODEX_ENABLE_R2_RELEASE`, `CODEX_ENABLE_DEV_WEBSITE_DEPLOY`, and `CODEX_ENABLE_WINGET_PUBLISH` unset for the first fork dry run.                                                 |

## Prerequisites for any later dry run

1. Start from the final audited upstream-port commit, not from an older worker
   branch.
2. Confirm the audited branch includes:
   - package identity changes from phase `006`
   - workflow guards from phase `007`
   - repo-aware npm staging/publish changes from phase `008`
   - explicit npm auth fallback semantics from phase `028`
3. Use a dedicated dry-run branch name so the validation branch is obvious in
   logs and audit notes. Example:
   - `release-dry-run/2026-07-22-alpha1`
4. Keep the first fork dry run narrow:
   - enable `CODEX_ENABLE_RELEASE_INFRA=true` only when the repo actually has
     the Linux/Windows release environment needed for the path you are exercising
   - leave `CODEX_ENABLE_R2_RELEASE`, `CODEX_ENABLE_DEV_WEBSITE_DEPLOY`, and
     `CODEX_ENABLE_WINGET_PUBLISH` unset
5. Do not enable `NPM_PUBLISH_USE_TOKEN_FALLBACK` unless you are explicitly
   validating the emergency token path after the phase `028` patch is present.

## Local preflight checklist

Run these before involving external CI or registries:

1. Confirm you are on the intended branch and worktree:

   ```bash
   git status --short --branch
   ```

2. Confirm the root staging helper still parses:

   ```bash
   python scripts/stage_npm_packages.py --help
   python -m py_compile scripts/stage_npm_packages.py
   ```

3. Confirm package manifests still parse:

   ```bash
   node -e "JSON.parse(require('fs').readFileSync('codex-cli/package.json','utf8')); console.log('codex-cli package ok')"
   node -e "JSON.parse(require('fs').readFileSync('sdk/typescript/package.json','utf8')); console.log('sdk package ok')"
   node -e "JSON.parse(require('fs').readFileSync('codex-rs/responses-api-proxy/npm/package.json','utf8')); console.log('proxy package ok')"
   ```

4. Treat `codex-cli/scripts/install_native_deps.py` as a retired upstream path.
   On the July 22, 2026 upstream base it does not exist. The active replacement
   is `scripts/stage_npm_packages.py`.

5. Verify the release workflow files still parse as YAML before pushing:

   ```bash
   python - <<'PY'
   from pathlib import Path
   import yaml

   files = [
       ".github/workflows/bazel.yml",
       ".github/workflows/postmerge-ci.yml",
       ".github/workflows/repo-checks.yml",
       ".github/workflows/rust-ci.yml",
       ".github/workflows/rust-release.yml",
       ".github/workflows/rust-release-windows.yml",
       ".github/workflows/sdk.yml",
   ]

   for rel in files:
       with Path(rel).open("r", encoding="utf-8") as f:
           yaml.safe_load(f)

   print("workflow yaml parse ok")
   PY
   ```

Stop here if the audited branch still lacks the accepted phase `008` and `028` changes or if you cannot provide explicit fork repo context to the staging helper. The remaining checks depend on the fork-aware staging path and audited release config.

## CI-only validation after the local preflight passes

Use a PR, or a push to `main`, to exercise the non-release workflow surface:

1. Create and push a dedicated dry-run branch from the audited commit:

   ```bash
   git switch -c release-dry-run/2026-07-22-alpha1
   git push -u origin HEAD
   ```

2. Open a PR or push directly so `blocking-ci.yml` runs its normal jobs,
   including the reusable `repo-checks.yml` call.

3. Leave `CODEX_ENABLE_REPO_CHECKS_NPM_STAGING` unset unless you explicitly want
   the optional staging smoke check and can provide
   `CODEX_REPO_CHECKS_NPM_STAGING_WORKFLOW_RUN_ID`. The authoritative fork
   staging check remains the manual helper invocation below because it is easier
   to inspect and rerun locally.

4. Review the resulting workflow graph for:
   - `repo-checks`
   - guarded Bazel and Rust CI jobs
   - any skipped release-only or self-hosted-only jobs that are expected under
     phase `007`

## Manual npm staging dry run after phase `008` lands

Use the repo-aware staging helper directly before attempting any release tag:

```bash
python scripts/stage_npm_packages.py \
  --release-version <version> \
  --workflow-url https://github.com/ai-nd-co/codex/actions/runs/<run_id> \
  --repo ai-nd-co/codex \
  --package codex \
  --package codex-responses-api-proxy \
  --package codex-sdk \
  --artifacts-dir <downloaded-artifacts-dir> \
  --output-dir dist/npm
```

Expected outcome:

- `dist/npm/` contains tarballs for the root CLI package, platform packages, the
  responses proxy package, and the TypeScript SDK package.
- no upstream `openai/codex` release URL or `@openai` package-scope lookup is required in the shipped installer or staging path
- the command can be rerun without publishing anything

Stop if the helper cannot derive the fork repo from `--repo`, `--workflow-url`, `GITHUB_REPOSITORY`, or the local `origin` remote, or if any path still emits `@openai` package scope assumptions.

## Tag strategy for prerelease validation

Use two separate tag styles depending on what you need to validate:

| Goal                                           | Tag shape             | Why                                                                                                                     |
| ---------------------------------------------- | --------------------- | ----------------------------------------------------------------------------------------------------------------------- |
| GitHub release asset smoke without npm publish | `rust-vX.Y.Z-beta.1`  | The workflow accepts beta tags, but `publish-npm` should stay off because `should_publish_npm=false` for beta versions. |
| End-to-end npm prerelease validation           | `rust-vX.Y.Z-alpha.1` | On the fork, numbered alpha tags publish npm from GitHub-hosted Linux x64 and unsigned Windows x64 artifacts only; stable/full release expectations remain unchanged. |

Important contract:

- `rust-release.yml` validates the tag against the checked-in Cargo version and
  the checked-in npm manifest versions.
- That means the current prep commit can only be tagged as
  `rust-v0.1.0-alpha.12`.
- If you want an optional beta smoke first, do it from a separate
  beta-versioned prep commit or worktree where the checked-in versions are
  bumped to the matching beta value before tagging.

Recommended sequence:

1. Run a beta tag first if you only need to validate tag parsing, artifact
   assembly, and GitHub Release creation.
2. Run an alpha tag only after:
   - phase `008` is merged
   - the repo has Linux and Windows release runners enabled for the fork
   - npm trusted publishing is configured for the fork, or you explicitly plan
     to test the fallback token path from phase `028`
   - you accept that the alpha npm path intentionally skips macOS assets/signing
     on the fork

Example commands:

```bash
git tag -a rust-v0.1.0-beta.1 -m "fork dry run beta"
git push origin rust-v0.1.0-beta.1
```

Later, for the npm path:

```bash
git tag -a rust-v0.1.0-alpha.12 -m "fork dry run alpha"
git push origin rust-v0.1.0-alpha.12
```

## External infrastructure required for the prerelease path

These cannot be proven locally:

- GitHub Actions access to the fork repository
- self-hosted Linux release runners referenced by the upstream release jobs
- Windows ARM64 and Linux ARM64 release coverage for alpha prereleases
- macOS codesigning and notarization environment required by the full/stable
  release jobs
- Azure Trusted Signing or equivalent Windows signing environment
- npm trusted publisher configuration for the fork repository, or an explicit
  fallback `NPM_TOKEN` secret plus `NPM_PUBLISH_USE_TOKEN_FALLBACK=true`

If any of those are unavailable, stop at branch/PR CI validation and do not push
release tags yet. For the fork alpha npm path specifically, macOS release/signing
is no longer a hard prerequisite; Linux/Windows runner availability and npm auth
still are.

## Explicit stop conditions before a real release

Do not proceed from dry run to real release if any of the following are still
true:

- the audited branch still contains `@openai` npm scope references in
  `.github/workflows/rust-release.yml` or shipped installer scripts
- `scripts/stage_npm_packages.py` still falls all the way through to the `openai/codex` default because no explicit or inferred fork repo context was supplied
- `repo-checks.yml` is enabled for fork validation without also providing `CODEX_REPO_CHECKS_NPM_STAGING_WORKFLOW_RUN_ID`
- the repo lacks the Linux/Windows release runner and signing environment
  required for the fork alpha path, or the full release runner/signing
  environment required for stable/full desktop releases
- the npm auth path is ambiguous; the recommended default is OIDC trusted
  publishing, with the token fallback used only when explicitly enabled

Treat those as hard stops, not soft warnings.

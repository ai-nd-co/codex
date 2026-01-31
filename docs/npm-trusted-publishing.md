# npm Trusted Publishing (OIDC) for ai-nd-co/codex

This repo is set up to publish npm packages via GitHub Actions using **Trusted Publishing (OIDC)** so no long-lived npm tokens are required.

## What changed in this fork

- NPM scope changed to `@ai-nd-co` for the Codex packages.
- GitHub Actions publish job uses OIDC (`id-token: write`) with npm CLI >= 11.5.1.

## Packages published

- `@ai-nd-co/codex`
- `@ai-nd-co/codex-responses-api-proxy`
- `@ai-nd-co/codex-sdk`

## One-time setup on npmjs.com (per package)

1. Open the package settings on npmjs.com and go to **Trusted Publishers**.
2. Choose **GitHub Actions** and set:
   - **Organization/User**: `ai-nd-co`
   - **Repository**: `codex`
   - **Workflow filename**: `rust-release.yml`
   - **Environment name**: (optional; leave blank unless you use a GitHub Environment)
3. Save the trusted publisher.
4. (Recommended) Under **Publishing access**, select **Require two-factor authentication and disallow tokens** to enforce tokenless publishing.

Notes:
- npm trusted publishing currently supports **GitHub-hosted runners only** (self-hosted runners are not supported).
- Trusted publishing requires **npm CLI >= 11.5.1**.
- The workflow filename must match exactly (including `.yml`) and the file must live in `.github/workflows/`.
- When using trusted publishing, npm automatically generates provenance; no `--provenance` flag is required.

### If the package is brand new

npm’s UI only lets you configure trusted publishing on an **existing package**, so you may need a one-time bootstrap publish to create the package entry. This is a common limitation reported by the community; once the package exists, you can switch to OIDC-only and revoke tokens.

## GitHub Actions configuration (already in this repo)

The release workflow already contains the required permissions and OIDC flow:

- `permissions: id-token: write` on the publish job.
- `actions/setup-node` configured for the npm registry and the `@ai-nd-co` scope.
- `npm publish` is executed on the release tarballs without `NODE_AUTH_TOKEN`.

Trusted publishing is now GA and explicitly supports tokenless publishes from GitHub Actions.

## Publishing flow in this repo

1. Tag a release in the format `rust-vX.Y.Z` and push it.
2. The `rust-release.yml` workflow builds artifacts and stages npm tarballs.
3. The publish job downloads those tarballs and publishes via OIDC.

## Troubleshooting

- **“Unable to authenticate”**: verify the trusted publisher settings match the exact workflow filename and that the job has `id-token: write`.
- **Self-hosted runners**: switch to GitHub-hosted runners, since self-hosted is not yet supported.
- **Provenance missing**: ensure the repo is public; provenance is only generated for public repos + public packages.

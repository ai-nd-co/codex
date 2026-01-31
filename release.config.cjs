module.exports = {
  branches: ["main"],
  tagFormat: "rust-v${version}",
  plugins: [
    ["@semantic-release/commit-analyzer", { preset: "conventionalcommits" }],
    ["@semantic-release/release-notes-generator", { preset: "conventionalcommits" }],
    ["@semantic-release/exec", {
      prepareCmd: "python3 scripts/bump_rust_version.py ${nextRelease.version}",
    }],
    ["@semantic-release/git", {
      assets: ["codex-rs/Cargo.toml"],
      message: "chore(release): ${nextRelease.version}",
    }],
  ],
};

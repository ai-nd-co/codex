# Implementation Summary (2026-04-05)

This document records the main work implemented during the April 4-5, 2026 port and release-repair session.

## Implemented in this session

### 1. Release pipeline restoration and repair

The ai-nd-co release pipeline was repaired on top of the new `main` branch until it successfully published again.

Implemented changes included:

- restoring fork-compatible release workflows
- restoring the ai-nd-co npm packaging path expected by the release job
- switching Linux release artifacts from musl to GNU because the musl `rusty_v8` archive was no longer available
- increasing the release build timeout so Windows release jobs were not cancelled mid-build
- adding repo-aware and target-aware native artifact download support in the release staging scripts
- adding GNU Linux target support for native dependency installation and ripgrep staging
- removing the broken runner-local `npm install -g npm@latest` self-update step
- switching npm publish to `npx -y npm@11.6.2`
- adding an npm token fallback while keeping the OIDC trusted-publishing path

### 2. TUI context token footer port

The TUI footer/statusline context display was ported so the footer can show token totals instead of only percentages.

Implemented behavior included:

- carrying context window total token counts through the TUI state pipeline
- rendering richer footer context text such as `258K / 592K tokens`
- preserving `% context left` when that information is also available
- updating status-line and right-side footer layout logic so the context indicator still appears when statusline content is active
- updating footer and composer snapshots to match the new rendering

Relevant commit:

- `0a940dbef` - `feat(tui): show context token totals in footer`

### 3. `.claude` skill-loading compatibility

Claude-style skill roots were added to the modern `codex-core-skills` loader.

Implemented behavior included:

- loading user skills from `~/.claude/skills`
- loading repo skills from `.claude/skills`
- preserving the existing `.agents/skills` and `.codex/skills` behavior
- extending loader tests to cover `.claude` home and repo discovery

Relevant PR and merge:

- PR `#34` - `feat(skills): load .claude skill roots`
- merge commit `fbb6c6ceefd3c46d78264507f1f5ca9bc92d602c`

## Verified in this session, but not newly implemented

These features were checked and confirmed to already exist on new `main` before any new code was added:

- `CLAUDE.md` project prompt loading
- `CODE.md` fallback project-doc loading
- `.claude/config.toml` project config loading

## Release outcome from this session

After the release pipeline fixes above, the following packages were successfully published at `3.0.7`:

- `@ai-nd-co/codex`
- `@ai-nd-co/codex-sdk`
- `@ai-nd-co/codex-responses-api-proxy`

## Notes

- This file is intended as a static historical summary of what was implemented during this session.
- It does not attempt to describe unrelated changes that landed on `main` before or after this work.

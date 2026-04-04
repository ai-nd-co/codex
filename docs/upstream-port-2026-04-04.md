# Upstream Port Ledger - 2026-04-04

## Source refs

- `origin/main`: `c4681dbbd`
- `upstream/main`: `1d4b5f130`
- merge base: `3241c1c6`
- port branch: `port/upstream-main-full-parity-20260404`

## Migration ledger

| source_sha  | theme                                        | resolution          | new_sha | notes                                                                                                                                              |
| ----------- | -------------------------------------------- | ------------------- | ------- | -------------------------------------------------------------------------------------------------------------------------------------------------- |
| `73ace1628` | default-mode `request_user_input`            | upstream-equivalent |         | Current upstream already ships `default_mode_request_user_input`; docs restored in `docs/config.md`.                                               |
| `9846ed76f` | `.claude` project-doc/config compatibility   | ported-manually     |         | Restored default `CLAUDE.md`/`CODE.md` project-doc fallbacks and `.claude/config.toml` project-layer discovery on top of current upstream loaders. |
| `942832345` | forced approval regexes                      | ported-manually     |         | Restored `[approvals].always_prompt_regex` via `ConfigToml` + `ExecPolicyManager`, with schema/docs/tests updated.                                 |
| `e696a2f49` | system-prompt compatibility                  | ported-manually     |         | Restored `disable_system_prompt` feature flag and base-instruction emptying behavior on current upstream session setup.                            |
| `bb1c09ec7` | Windows Git Bash overrides                   | ported-manually     |         | Restored Git Bash preference docs and env override support via `CODEX_SHELL_PATH` / `CODEX_GIT_BASH_PATH`.                                         |
| `e5fa29e75` | prefer Git Bash over WSL on Windows          | ported-manually     |         | Restored WSL shim detection and Git Bash preference in `codex-rs/core/src/shell.rs`.                                                               |
| `34aef95d8` | prefer Git Bash by default                   | ported-manually     |         | Windows default shell now prefers Git Bash, falling back to PowerShell when Git Bash is unavailable.                                               |
| `137d011ce` | manual smart compact command                 | ported-manually     |         | Added `Op::SmartCompact`, `TaskKind::SmartCompact`, smart compact task, prompt template, and core compact/rebuild logic.                           |
| `62b3560ef` | smart compact bypass                         | upstream-equivalent |         | Current upstream task/orchestrator path does not require the removed bypass line; no extra port needed.                                            |
| `818d5220e` | smart compact prompt                         | ported-manually     |         | Restored `codex-rs/core/templates/smart_compact/prompt.md` and `TurnContext::smart_compact_prompt()`.                                              |
| `5731366ec` | compaction summary/protocol                  | ported-manually     |         | Smart compact now emits compaction summaries and protocol/task kinds, with minimal app-server compatibility updates.                               |
| `2b86e7220` | disable compaction flag                      | ported-manually     |         | Added `disable_compaction` feature flag that skips automatic compaction while leaving manual compaction available.                                 |
| `3d5b97163` | manual compaction while auto disabled        | ported-manually     |         | `/compact` and `/smart-compact` remain manual compaction paths even when auto-compaction is disabled.                                              |
| `66400853b` | preserve whole recent turns in smart compact | ported-manually     |         | Smart compact rebuild keeps the recent half of the conversation intact and summarizes the older half.                                              |
| `8f5178b9d` | `/auto-rename` slash command                 | ported-manually     |         | Restored `/auto-rename`, `Op::GenerateThreadName`, and the core helper/template needed for end-to-end thread auto-renaming.                        |
| `99b9fab81` | verbose tool call output                     | ported-manually     |         | Restored `verbose_tool_calls` feature wiring for explored/read/search/list output previews in exec cells.                                          |
| `e6136b89f` | disable explored compaction                  | ported-manually     |         | Restored `disable_explored_compaction` feature wiring so explored tool calls no longer collapse when the flag is enabled.                          |
| `4b51e7fa0` | unicode markdown tables                      | ported-manually     |         | Restored unicode/box-rendered markdown tables in the TUI renderer/streaming path.                                                                  |
| `a5e19a53b` | unicode table width cap                      | ported-manually     |         | Added width capping so rendered table lines respect available width.                                                                               |
| `4f7532f96` | unicode table terminal sizing                | ported-manually     |         | Added width sizing behavior in the markdown table renderer.                                                                                        |
| `e61e69e8a` | markdown tables on startup                   | upstream-equivalent |         | Current upstream markdown/TUI wiring already supported startup rendering once the renderer was restored.                                           |
| `1513527b1` | markdown tables on resume                    | upstream-equivalent |         | Current upstream resume/render flow already supported resumed rendering once the renderer was restored.                                            |
| `c4ff6e7d3` | table separator synthesis                    | ported-manually     |         | Added table normalization/parsing support for malformed/empty-header separators.                                                                   |
| `d93ab6ce5` | resume picker search and counts              | upstream-equivalent |         | Search, counts, and associated picker behavior were already present in current upstream.                                                           |
| `197e5caac` | resume picker ranking                        | upstream-equivalent |         | Ranking/stale-load handling already existed in current upstream picker.                                                                            |
| `8a86b6427` | fork release branding                        | ported-manually     |         | Rebranded package scopes, repo URLs, README/install instructions, and release workflow guards to `ai-nd-co`.                                       |
| `ee179d8dd` | Windows release workflow adjustments         | partially-ported    |         | Kept current upstream workflow structure, but retained fork-specific repo/scope guards where they still apply on this branch.                      |
| `e92a08d26` | workflow dispatch fixes                      | partially-ported    |         | Preserved current upstream workflow layout; only fork-specific repo/scope constants were updated in this pass.                                     |
| `719750ab3` | action reference normalization               | upstream-equivalent |         | Current upstream already uses the modern action references; no extra port required.                                                                |
| `8253f2570` | id-token permission for Windows build        | not-ported          |         | Deferred; current upstream workflow layout differs and this pass focused on canonical repo/scope behavior.                                         |
| `c90845c4a` | id-token permission for shell-tool MCP       | not-applicable      |         | `shell-tool-mcp/` is not present in this upstream-based branch.                                                                                    |
| `48c0915a2` | release dispatch tag handling                | partially-ported    |         | Current upstream workflow structure retained; fork repo/scope constants updated instead of replaying the older workflow shape.                     |
| `1ad005129` | rust-release fork structure alignment        | partially-ported    |         | Retained current upstream release layout while applying fork scope/repo adjustments.                                                               |
| `fd985f0f4` | npm staging helper flags                     | upstream-equivalent |         | Current upstream helper already exposes the needed `--repo`/`--target`-style packaging flow on this branch.                                        |
| `95088681c` | non-blocking release publish                 | not-ported          |         | Deferred; current upstream release workflow structure diverges substantially and was not rewritten in this pass.                                   |

## Deferred follow-ups

- `45adf2f65` - background terminal UX: remove `/ps` and keep background terminals always visible
- `7782ffdc9` - Windows terminal attention notifications

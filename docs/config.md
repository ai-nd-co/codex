# Configuration

For basic configuration instructions, see [this documentation](https://developers.openai.com/codex/config-basic).

For advanced configuration instructions, see [this documentation](https://developers.openai.com/codex/config-advanced).

For a full configuration reference, see [this documentation](https://developers.openai.com/codex/config-reference).

## Connecting to MCP servers

Codex can connect to MCP servers configured in `~/.codex/config.toml`. See the configuration reference for the latest MCP server options:

- https://developers.openai.com/codex/config-reference

## Apps (Connectors)

Use `$` in the composer to insert a ChatGPT connector; the popover lists accessible
apps. The `/apps` command lists available and installed apps. Connected apps appear first
and are labeled as connected; others are marked as can be installed.

## Notify

Codex can run a notification hook when the agent finishes a turn. See the configuration reference for the latest notification settings:

- https://developers.openai.com/codex/config-reference

When Codex knows which client started the turn, the legacy notify JSON payload also includes a top-level `client` field. The TUI reports `codex-tui`, and the app server reports the `clientInfo.name` value from `initialize`.

## JSON Schema

The generated JSON Schema for `config.toml` lives at `codex-rs/core/config.schema.json`.

## SQLite State DB

Codex stores the SQLite-backed state DB under `sqlite_home` (config key) or the
`CODEX_SQLITE_HOME` environment variable. When unset, WorkspaceWrite sandbox
sessions default to a temp directory; other modes default to `CODEX_HOME`.

## TUI experimental feature flags

Some TUI behavior is gated behind feature flags that can be toggled via `/experimental` or in
`~/.codex/config.toml` under `[features]`.

### Windows terminal attention

On Windows, you can extend the existing unfocused TUI notification behavior with:

```toml
[features]
focus_terminal_window = true
move_terminal_window_to_primary_monitor = true
```

These flags apply to the same approval-request and turn-complete notifications the TUI already
emits. `focus_terminal_window` is best-effort because Windows may deny foreground activation.
`move_terminal_window_to_primary_monitor` keeps the current window size and centers it on the
primary monitor.

### Default mode `request_user_input`

To allow `request_user_input` in Default collaboration mode, enable:

```toml
[features]
default_mode_request_user_input = true
```

Older ai-nd-co configs may still use `request_user_input_in_default_mode = true`. Current builds
accept that older key as a compatibility alias, but `default_mode_request_user_input` is the
canonical name.

### Disable system prompt

To send empty base instructions to the model, enable:

```toml
[features]
disable_system_prompt = true
```

This is an advanced compatibility feature intended for ai-nd-co-style setups that want Codex to
run without the normal base instruction bundle.

### Smart compact and compaction disable

On ai-nd-co builds, `smart_compact` changes `/compact` to use model-driven smart compaction.

```toml
[features]
smart_compact = true
```

If `disable_compaction = true`, Codex skips automatic compaction and blocks `/compact`.
The explicit `/smart-compact` command remains the manual compaction path.

### Markdown table compatibility

Current ai-nd-co builds use:

```toml
[features]
markdown_tables = true
```

Older ai-nd-co configs may still use `enable_markdown_tables = true`. Current builds accept that
older key as a compatibility alias, but `markdown_tables` is the canonical name.

### Disable explored compaction

If you want the transcript to show each individual `Explored`/`Exploring` tool-call line (no
collapsing of reads/searches/lists into summaries), enable:

```toml
[features]
disable_explored_compaction = true
```

This does **not** change output truncation behavior (max-lines + ellipsis).

## Notices

Codex stores "do not show again" flags for some UI prompts under the `[notice]` table.

## Plan mode defaults

`plan_mode_reasoning_effort` lets you set a Plan-mode-specific default reasoning
effort override. When unset, Plan mode uses the built-in Plan preset default
(currently `medium`). When explicitly set (including `none`), it overrides the
Plan preset. The string value `none` means "no reasoning" (an explicit Plan
override), not "inherit the global default". There is currently no separate
config value for "follow the global default in Plan mode".

Ctrl+C/Ctrl+D quitting uses a ~1 second double-press hint (`ctrl + c again to quit`).

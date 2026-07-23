# Configuration

For basic configuration instructions, see [this documentation](https://developers.openai.com/codex/config-basic).

For advanced configuration instructions, see [this documentation](https://developers.openai.com/codex/config-advanced).

For a full configuration reference, see [this documentation](https://developers.openai.com/codex/config-reference).

## Approval overrides

Use `[approvals].always_prompt_regex` to force approval prompts for matching
shell-like commands when the underlying exec-policy outcome would otherwise be
allowed or approvable, even when `approval_policy = "never"`.

Codex matches the rendered command string it is about to run. For wrapped shell
commands, that means the regex must match the wrapper form that Codex renders,
including the shell path and wrapper arguments, not just the inner command.

Example:

```toml
[approvals]
# Unanchored patterns like this can still match the inner command text, but
# Codex evaluates the full rendered wrapper on this machine. The wrapper path
# and flags can vary by shell and platform (for example bash, zsh, sh,
# PowerShell, or cmd).
always_prompt_regex = ["git push origin main"]
```

Behavior notes:

- Explicit forbidden exec-policy outcomes remain forbidden; this override never
  converts them into approval requests.
- Invalid regex patterns are ignored with a warning and do not stop startup.
- This is a shell-approval override only; it does not affect non-shell tools.

## Lifecycle hooks

Admins can set top-level `allow_managed_hooks_only = true` in
`requirements.toml` to ignore user, project, and session hook configs while
still allowing managed hooks from requirements and managed config layers. This
setting is only supported in `requirements.toml`; putting it in `config.toml`
does not enable managed-hooks-only mode.
